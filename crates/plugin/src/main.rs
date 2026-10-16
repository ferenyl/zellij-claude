mod bar;
mod host;
mod picker;

use std::collections::BTreeMap;
use std::path::PathBuf;

use claude_sessions_core::bar::{self as core_bar, BarOptions, Counts};
use claude_sessions_core::config::{resolve_status_dir, Config, Mode};
use claude_sessions_core::icons::Icons;
use claude_sessions_core::keys::{open_picker_binding, Keymap};
use claude_sessions_core::list::{order, Command, Picker, RowOptions};
use claude_sessions_core::state::{ClaudeSession, LiveSession, Store};
use claude_sessions_core::status_file::StatusRecord;
use claude_sessions_core::truncate::truncate_end;
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
    /// Non-fatal problem from the last status directory read.
    poll_warning: Option<String>,
    /// Seconds since the status directory was last read.
    since_poll: f64,
    /// Blink phase of the waiting icon in the bar.
    blink_hidden: bool,
    /// A `set_timeout` is pending; there must only ever be one timer chain.
    timer_armed: bool,
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

        let mut permissions = vec![
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadCliPipes,
            PermissionType::FullHdAccess,
            PermissionType::ReadSessionEnvironmentVariables,
        ];
        if self.config().bind_open_key.is_some() {
            permissions.push(PermissionType::Reconfigure);
        }
        request_permission(&permissions);
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
                if self.config().mode == Mode::Bar {
                    // set_selectable: only after the permission prompt, which needs focus.
                    set_selectable(false);
                }
                self.init_status_dir();
                self.bind_open_key();
                true
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                self.last_error = Some("permissions denied".into());
                true
            }
            Event::HostFolderChanged(_) => {
                self.host_folder_ready = true;
                self.last_error = None;
                self.poll_status_files();
                self.arm_timer();
                true
            }
            Event::FailedToChangeHostFolder(error) => {
                // Usually the directory does not exist until the hook first runs; retried on the timer.
                self.last_error = Some(format!(
                    "waiting for status dir ({}), retrying",
                    error.unwrap_or_else(|| "unknown error".into())
                ));
                self.arm_timer();
                true
            }
            Event::Timer(elapsed) => {
                self.timer_armed = false;
                self.since_poll += elapsed;
                if self.since_poll >= self.config().poll_interval_secs {
                    self.since_poll = 0.0;
                    if self.host_folder_ready {
                        self.poll_status_files();
                    } else if let Some(dir) = &self.status_dir {
                        change_host_folder(dir.clone());
                    }
                }
                if self.config().blink_waiting {
                    self.blink_hidden = !self.blink_hidden;
                }
                self.arm_timer();
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
            Mode::Bar => self.render_bar(rows, cols),
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
        let mut sessions = self.store.sessions(live);
        if self.config().fallback_detect {
            sessions.extend(self.store.detected(live));
        }
        order(sessions, self.config().all_sessions)
    }

    fn bind_open_key(&self) {
        let config = self.config();
        if let Some(key) = &config.bind_open_key {
            // reconfigure: needs Reconfigure; `false` keeps the user's config file untouched.
            reconfigure(open_picker_binding(key, &config.plugin_location), false);
        }
    }

    fn arm_timer(&mut self) {
        if !self.timer_armed {
            set_timeout(self.tick_interval());
            self.timer_armed = true;
        }
    }

    /// One timer drives both polling and blinking; blinking needs a faster tick.
    fn tick_interval(&self) -> f64 {
        let poll = self.config().poll_interval_secs;
        if self.config().blink_waiting {
            poll.min(0.5)
        } else {
            poll
        }
    }

    fn render_bar(&mut self, rows: usize, cols: usize) {
        if let Some(error) = self
            .last_error
            .as_deref()
            .or(self.warnings.first().map(String::as_str))
        {
            print!("{}", truncate_end(&format!("! {error}"), cols));
            return;
        }
        let counts = Counts::from_sessions(&self.sessions());
        let config = self.config();
        let options = BarOptions {
            icons: self.icons(),
            orientation: config.orientation,
            align: config.align,
            vertical_align: config.vertical_align,
            show_zero: config.show_zero,
            hide_waiting_icon: config.blink_waiting && self.blink_hidden,
        };
        let lines = core_bar::render(counts, &options, rows, cols);
        bar::print_lines(&lines, config);
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
        warnings.extend(self.poll_warning.clone());
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
            Ok(files) => {
                self.poll_warning = match (files.truncated, files.invalid) {
                    (true, _) => Some("too many status files, some were skipped".into()),
                    (false, 0) => None,
                    (false, n) => Some(format!("{n} unreadable status file(s) ignored")),
                };
                self.store.apply_snapshot(files.records);
                self.prune();
            }
            Err(error) => self.last_error = Some(error),
        }
    }

    fn prune(&mut self) {
        let stale_after = self.config().stale_after_secs;
        let Some(live) = &self.live else {
            return;
        };
        let removed = self.store.prune(live, host::unix_now(), stale_after);
        if self.host_folder_ready {
            for (zellij_session, pane_id) in removed {
                host::remove_status_file(&zellij_session, pane_id);
            }
        }
    }
}
