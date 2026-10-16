use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use claude_sessions_core::config::{resolve_status_dir, Config, ConfigWarning, Mode};
use claude_sessions_core::state::{LivePane, LiveSession, LiveTab, Store};
use claude_sessions_core::status_file::StatusRecord;
use zellij_tile::prelude::*;

const PIPE_NAME: &str = "claude-status";
/// `change_host_folder` remaps this WASI mount to the status directory.
const HOST_MOUNT: &str = "/host";

#[derive(Default)]
struct State {
    config: Option<Config>,
    warnings: Vec<ConfigWarning>,
    permissions: Option<PermissionStatus>,
    status_dir: Option<PathBuf>,
    host_folder_ready: bool,
    host_folder_error: Option<String>,
    store: Store,
    /// `None` until the first `SessionUpdate`; pruning waits for it.
    live: Option<Vec<LiveSession>>,
    last_error: Option<String>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        let (config, warnings) = Config::parse(&configuration);
        self.config = Some(config);
        self.warnings = warnings;

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadCliPipes,
            PermissionType::FullHdAccess,
            PermissionType::ReadSessionEnvironmentVariables,
        ]);
        subscribe(&[
            EventType::PaneUpdate,
            EventType::TabUpdate,
            EventType::SessionUpdate,
            EventType::Key,
            EventType::Timer,
            EventType::PermissionRequestResult,
            EventType::HostFolderChanged,
            EventType::FailedToChangeHostFolder,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(status) => {
                self.permissions = Some(status);
                if status == PermissionStatus::Granted {
                    self.init_status_dir();
                }
                true
            }
            Event::HostFolderChanged(_) => {
                self.host_folder_ready = true;
                self.poll_status_files();
                set_timeout(self.config().poll_interval_secs);
                true
            }
            Event::FailedToChangeHostFolder(error) => {
                self.host_folder_error = Some(error.unwrap_or_else(|| "unknown error".into()));
                true
            }
            Event::Timer(_) => {
                self.poll_status_files();
                set_timeout(self.config().poll_interval_secs);
                true
            }
            Event::SessionUpdate(sessions, _) => {
                self.live = Some(sessions.iter().map(to_live_session).collect());
                self.prune();
                true
            }
            Event::Key(key)
                if key.bare_key == BareKey::Esc && self.config().mode == Mode::Picker =>
            {
                hide_self();
                false
            }
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

    fn render(&mut self, _rows: usize, _cols: usize) {
        let mode = match self.config().mode {
            Mode::Picker => "picker",
            Mode::Bar => "bar",
        };
        println!("claude-sessions [{mode}]");
        for warning in &self.warnings {
            println!("warning: {warning}");
        }
        println!("permissions: {:?}", self.permissions);
        println!(
            "status dir: {}",
            self.status_dir
                .as_deref()
                .map_or("<unresolved>".into(), |p| p.display().to_string())
        );
        if let Some(error) = &self.host_folder_error {
            println!("host folder error: {error}");
        }
        if let Some(error) = &self.last_error {
            println!("error: {error}");
        }
        println!("host folder ready: {}", self.host_folder_ready);
        let live = self.live.as_deref().unwrap_or_default();
        for session in self.store.sessions(live) {
            if !self.config().all_sessions && !session.is_current_session {
                continue;
            }
            println!(
                "{} pane {} tab {} {} {}",
                session.zellij_session,
                session.pane_id,
                session.tab_name.as_deref().unwrap_or("?"),
                session.state.as_str(),
                session.cwd.as_deref().unwrap_or("")
            );
        }
    }
}

impl State {
    fn config(&self) -> &Config {
        self.config.as_ref().expect("config is set in load")
    }

    fn init_status_dir(&mut self) {
        // get_session_environment_variables: needs ReadSessionEnvironmentVariables.
        let env = get_session_environment_variables();
        self.status_dir = resolve_status_dir(self.config(), &env);
        if let Some(dir) = &self.status_dir {
            // change_host_folder: needs FullHdAccess; answered by HostFolderChanged.
            change_host_folder(dir.clone());
        }
    }

    fn poll_status_files(&mut self) {
        if !self.host_folder_ready {
            return;
        }
        let entries = match std::fs::read_dir(Path::new(HOST_MOUNT)) {
            Ok(entries) => entries,
            Err(error) => {
                self.last_error = Some(format!("cannot read status dir: {error}"));
                return;
            }
        };
        let records = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .filter_map(|json| StatusRecord::parse(&json).ok())
            .collect();
        self.store.apply_snapshot(records);
        self.prune();
    }

    fn prune(&mut self) {
        if let Some(live) = &self.live {
            self.store.prune(
                live,
                unix_now(),
                self.config.as_ref().map_or(3600, |c| c.stale_after_secs),
            );
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn to_live_session(session: &SessionInfo) -> LiveSession {
    LiveSession {
        name: session.name.clone(),
        is_current: session.is_current_session,
        tabs: session
            .tabs
            .iter()
            .map(|t| LiveTab {
                position: t.position,
                name: t.name.clone(),
            })
            .collect(),
        panes: session
            .panes
            .panes
            .iter()
            .flat_map(|(tab_position, panes)| {
                panes.iter().map(move |p| LivePane {
                    id: p.id,
                    tab_position: *tab_position,
                    title: p.title.clone(),
                    is_plugin: p.is_plugin,
                    is_floating: p.is_floating,
                    exited: p.exited,
                })
            })
            .collect(),
    }
}
