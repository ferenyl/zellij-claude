use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use claude_sessions_core::config::{resolve_status_dir, Config, ConfigWarning, Mode};
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
    status_files: usize,
    last_pipe: Option<String>,
    zellij_sessions: usize,
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
                self.zellij_sessions = sessions.len();
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
        match pipe_message.payload {
            Some(payload) => {
                self.last_pipe = Some(payload);
                true
            }
            None => false,
        }
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
        println!(
            "status files: {} (host folder ready: {})",
            self.status_files, self.host_folder_ready
        );
        println!("zellij sessions: {}", self.zellij_sessions);
        println!("last pipe: {}", self.last_pipe.as_deref().unwrap_or("-"));
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
        self.status_files = std::fs::read_dir(Path::new(HOST_MOUNT))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                    .count()
            })
            .unwrap_or(0);
    }
}
