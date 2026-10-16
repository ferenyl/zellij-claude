mod host;
mod picker;

use std::collections::BTreeMap;
use std::path::PathBuf;

use claude_sessions_core::config::{resolve_status_dir, Config, Mode};
use claude_sessions_core::icons::Icons;
use claude_sessions_core::keys::Keymap;
use claude_sessions_core::list::{order, Command, Picker, RowOptions};
use claude_sessions_core::state::{ClaudeSession, LiveSession, Store};
use claude_sessions_core::status_file::StatusRecord;
use zellij_tile::prelude::*;

const PIPE_NAME: &str = "claude-status";

#[derive(Default)]
struct State {
    config: Option<Config>,
    keymap: Option<Keymap>,
    icons: Option<Icons>,
    warnings: Vec<String>,
    home: Option<String>,
    status_dir: Option<PathBuf>,
    host_folder_ready: bool,
    store: Store,
    /// `None` until the first `SessionUpdate`; pruning waits for it.
    live: Option<Vec<LiveSession>>,
    picker: Picker,
    last_error: Option<String>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        let (config, config_warnings) = Config::parse(&configuration);
        let (keymap, key_warnings) = Keymap::from_config(&config);
        self.warnings = config_warnings
            .iter()
            .chain(&key_warnings)
            .map(ToString::to_string)
            .collect();
        self.icons = Some(Icons::from_config(&config));
        self.keymap = Some(keymap);
        self.config = Some(config);

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadCliPipes,
            PermissionType::FullHdAccess,
            PermissionType::ReadSessionEnvironmentVariables,
        ]);
        subscribe(&[
            EventType::SessionUpdate,
            EventType::Key,
            EventType::Timer,
            EventType::Visible,
            EventType::PermissionRequestResult,
            EventType::HostFolderChanged,
            EventType::FailedToChangeHostFolder,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                self.init_status_dir();
                true
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                self.last_error = Some("permissions denied".into());
                true
            }
            Event::HostFolderChanged(_) => {
                self.host_folder_ready = true;
                self.poll_status_files();
                set_timeout(self.config().poll_interval_secs);
                true
            }
            Event::FailedToChangeHostFolder(error) => {
                self.last_error = Some(format!(
                    "cannot open status dir: {}",
                    error.unwrap_or_else(|| "unknown error".into())
                ));
                true
            }
            Event::Timer(_) => {
                self.poll_status_files();
                set_timeout(self.config().poll_interval_secs);
                true
            }
            Event::SessionUpdate(sessions, _) => {
                self.live = Some(sessions.iter().map(host::to_live_session).collect());
                self.prune();
                true
            }
            Event::Visible(true) => {
                self.picker.reset_filter();
                true
            }
            Event::Key(key) if self.config().mode == Mode::Picker => self.handle_key(&key),
            _ => false,
        }
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        if pipe_message.name != PIPE_NAME {
            return false;
        }
        if let PipeSource::Cli(pipe_id) = &pipe_message.source {
            unblock_cli_pipe_input(pipe_id);
        }
        let Some(payload) = pipe_message.payload else {
            return false;
        };
        match StatusRecord::parse(&payload) {
            Ok(record) => {
                self.store.apply(record);
                self.prune();
            }
            Err(error) => self.last_error = Some(format!("invalid pipe payload: {error}")),
        }
        true
    }

    fn render(&mut self, rows: usize, cols: usize) {
        match self.config().mode {
            Mode::Picker => self.render_picker(rows, cols),
            Mode::Bar => {
                let sessions = self.sessions();
                println!("{} {}", self.icons().session, sessions.len());
            }
        }
    }
}

impl State {
    fn config(&self) -> &Config {
        self.config.as_ref().expect("config is set in load")
    }

    fn icons(&self) -> &Icons {
        self.icons.as_ref().expect("icons are set in load")
    }

    fn sessions(&self) -> Vec<ClaudeSession> {
        let live = self.live.as_deref().unwrap_or_default();
        order(self.store.sessions(live), self.config().all_sessions)
    }

    fn handle_key(&mut self, key: &KeyWithModifier) -> bool {
        let Some(press) = host::to_key_press(key) else {
            return false;
        };
        let sessions = self.sessions();
        let keymap = self.keymap.as_ref().expect("keymap is set in load");
        match self.picker.handle_key(press, keymap, &sessions) {
            Command::None => false,
            Command::Redraw => true,
            Command::Close => {
                hide_self();
                false
            }
            Command::Jump(session) => {
                picker::jump(&session);
                false
            }
        }
    }

    fn render_picker(&mut self, rows: usize, cols: usize) {
        let sessions = self.sessions();
        let mut warnings = self.warnings.clone();
        warnings.extend(self.last_error.clone());
        let config = self.config.as_ref().expect("config is set in load");
        let icons = self.icons.as_ref().expect("icons are set in load");
        let options = RowOptions {
            icons,
            show_state_text: config.show_state_text,
            show_cwd: config.show_cwd,
            home: self.home.as_deref(),
        };
        let keymap = self.keymap.as_ref().expect("keymap is set in load");
        let lines = self
            .picker
            .render(&sessions, keymap, &options, &warnings, rows, cols);
        picker::print_lines(&lines, cols);
    }

    fn init_status_dir(&mut self) {
        // get_session_environment_variables: needs ReadSessionEnvironmentVariables.
        let env = get_session_environment_variables();
        self.home = env.get("HOME").cloned();
        self.status_dir = resolve_status_dir(self.config(), &env);
        match &self.status_dir {
            // change_host_folder: needs FullHdAccess; answered by HostFolderChanged.
            Some(dir) => change_host_folder(dir.clone()),
            None => self.last_error = Some("cannot resolve status dir: set status_dir".into()),
        }
    }

    fn poll_status_files(&mut self) {
        if !self.host_folder_ready {
            return;
        }
        match host::read_status_files() {
            Ok(records) => {
                self.store.apply_snapshot(records);
                self.prune();
            }
            Err(error) => self.last_error = Some(error),
        }
    }

    fn prune(&mut self) {
        let stale_after = self.config().stale_after_secs;
        if let Some(live) = &self.live {
            self.store.prune(live, host::unix_now(), stale_after);
        }
    }
}
