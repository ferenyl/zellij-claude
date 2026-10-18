mod bar;
mod host;
mod picker;

use std::collections::BTreeMap;
use std::path::PathBuf;

use claude_sessions_core::bar::{self as core_bar, BarOptions, Counts};
use claude_sessions_core::config::{resolve_status_dir, Config, Mode};
use claude_sessions_core::floating::{
    became_visible, moved_tab, panes_to_isolate, record_layers, shown_picker_position, Isolation,
    LayerStates,
};
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
    /// Defaults until `load` has parsed the configuration.
    config: Config,
    keymap: Keymap,
    icons: Icons,
    warnings: Vec<String>,
    home: Option<String>,
    status_dir: Option<PathBuf>,
    host_folder_ready: bool,
    store: Store,
    /// `None` until the first `SessionUpdate`; pruning waits for it.
    live: Option<Vec<LiveSession>>,
    /// Set once `get_session_list` has returned a full scan. Before that `SessionUpdate`
    /// lists the current session only, so pruning (and file removal) must wait.
    session_list_complete: bool,
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
    /// Latest `TabUpdate`. `PaneUpdate` comes first, so this is the state before a change.
    /// Zellij sends neither event to a hidden plugin, so this is cleared on close.
    last_tabs: Vec<TabInfo>,
    /// The picker pane's tab position and `is_suppressed` flag from the last `PaneUpdate`;
    /// `None` while hidden.
    own_pane: Option<(usize, bool)>,
    /// Floating panes hidden while the picker is open.
    isolation: Option<Isolation>,
    /// Bar: layer states written for the picker.
    layers: LayerStates,
    /// Bar: tab position of a shown picker, from the last `PaneUpdate`.
    shown_picker: Option<usize>,
    /// The session environment must be read, but not before the permission is live.
    env_pending: bool,
    /// Set on `Granted`. Host calls needing permissions also wait for the first `SessionUpdate`.
    permissions_granted: bool,
    /// Bar: what was last rendered, to skip redraws that would look the same.
    last_bar_view: Option<BarView>,
}

type BarView = (Option<String>, Counts, bool);

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
        self.icons = Icons::from_config(&config);
        self.keymap = keymap;
        self.config = config;

        // The same set in both modes: Zellij keeps one permission entry per plugin URL and
        // overwrites it with whatever an instance requests, so differing sets would prompt again.
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
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::PermissionRequestResult,
            EventType::HostFolderChanged,
            EventType::FailedToChangeHostFolder,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                match self.config().mode {
                    // set_selectable: only after the permission prompt, which needs focus.
                    Mode::Bar => set_selectable(false),
                    // rename_plugin_pane: the frame title would otherwise be the plugin path.
                    Mode::Picker => {
                        rename_plugin_pane(get_plugin_ids().plugin_id, "Claude sessions")
                    }
                }
                self.init_status_dir();
                self.bind_open_key();
                self.permissions_granted = true;
                true
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                self.last_error = Some("permissions denied".into());
                true
            }
            Event::HostFolderChanged(_) => {
                self.host_folder_ready = true;
                self.last_error = None;
                self.save_layers();
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
                    self.refresh_session_list();
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
                self.bar_changed()
            }
            Event::SessionUpdate(sessions, _) => {
                self.live = Some(sessions.iter().map(host::to_live_session).collect());
                if self.env_pending {
                    self.env_pending = false;
                    self.init_status_dir_from_env();
                }
                if !self.session_list_complete {
                    self.refresh_session_list();
                }
                self.prune();
                self.bar_changed()
            }
            Event::TabUpdate(tabs) => match self.config().mode {
                Mode::Picker => {
                    self.last_tabs = tabs;
                    self.fill_isolation_tab_id();
                    false
                }
                Mode::Bar => {
                    self.record_layers(&tabs);
                    false
                }
            },
            Event::PaneUpdate(manifest) => match self.config().mode {
                Mode::Picker => self.on_pane_update(&manifest),
                Mode::Bar => {
                    let panes = host::to_positioned_snapshots(&manifest);
                    self.shown_picker = shown_picker_position(get_plugin_ids().plugin_id, &panes);
                    false
                }
            },
            Event::Key(key) if self.config().mode == Mode::Picker => self.handle_key(&key),
            _ => false,
        }
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        if pipe_message.name != PIPE_NAME {
            return false;
        }
        if let PipeSource::Cli(pipe_id) = &pipe_message.source {
            if self.permissions_live() {
                unblock_cli_pipe_input(pipe_id);
            }
        }
        let Some(payload) = pipe_message.payload else {
            return false;
        };
        match StatusRecord::parse(&payload) {
            Ok(record) => {
                if !is_valid_record(&record) {
                    return false;
                }
                self.store.apply(record);
                self.prune();
            }
            Err(error) => self.last_error = Some(format!("invalid pipe payload: {error}")),
        }
        self.bar_changed()
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
        &self.config
    }

    fn icons(&self) -> &Icons {
        &self.icons
    }

    fn sessions(&self) -> Vec<ClaudeSession> {
        let live = self.live.as_deref().unwrap_or_default();
        let mut sessions = self.store.sessions(live);
        if self.config().fallback_detect {
            sessions.extend(self.store.detected(live));
        }
        order(sessions, self.config().all_sessions)
    }

    /// What the bar shows: error text, counts and whether the waiting icon is hidden.
    fn bar_view(&self) -> BarView {
        let error = self
            .last_error
            .clone()
            .or_else(|| self.warnings.first().cloned());
        let counts = Counts::from_sessions(&self.sessions());
        let hide = self.config().blink_waiting && self.blink_hidden && counts.waiting > 0;
        (error, counts, hide)
    }

    /// Whether a redraw is needed: the bar differs from what `render_bar` last drew.
    /// The picker is always redrawn.
    fn bar_changed(&self) -> bool {
        self.config().mode != Mode::Bar || self.last_bar_view.as_ref() != Some(&self.bar_view())
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
        self.last_bar_view = Some(self.bar_view());
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
        let keymap = &self.keymap;
        match self.picker.handle_key(press, keymap, &sessions) {
            Command::None => false,
            Command::Redraw => true,
            Command::Close => {
                self.close_picker();
                false
            }
            Command::Jump(session) => {
                self.close_picker();
                picker::jump(&session);
                false
            }
        }
    }

    fn record_layers(&mut self, tabs: &[TabInfo]) {
        let tabs: Vec<_> = tabs
            .iter()
            .map(|t| (t.position, t.are_floating_panes_visible))
            .collect();
        let layers = record_layers(&self.layers, &tabs, self.shown_picker);
        if layers != self.layers {
            self.layers = layers;
            self.save_layers();
        }
    }

    fn save_layers(&mut self) {
        if self.config().mode != Mode::Bar || !self.host_folder_ready {
            return;
        }
        if let Err(error) = host::write_layer_states(get_plugin_ids().zellij_pid, &self.layers) {
            self.last_error = Some(error);
        }
    }

    /// A hidden plugin gets no `PaneUpdate`/`TabUpdate`, so the next one after showing
    /// again must look like a fresh open, without stale tab state.
    fn close_picker(&mut self) {
        picker::close(self.isolation.take());
        self.own_pane = None;
        self.last_tabs.clear();
    }

    /// `Visible` is not sent when a hidden picker is shown again, so its own
    /// `is_suppressed` flag tells when it opens or closes.
    fn on_pane_update(&mut self, manifest: &PaneManifest) -> bool {
        let own_id = get_plugin_ids().plugin_id;
        let own = manifest.panes.iter().find_map(|(position, panes)| {
            panes
                .iter()
                .find(|p| p.is_plugin && p.id == own_id)
                .map(|p| (*position, p.is_suppressed))
        });
        let was = std::mem::replace(&mut self.own_pane, own);
        let was_suppressed = was.map(|(_, suppressed)| suppressed);
        let is_suppressed = own.map(|(_, suppressed)| suppressed);
        let moved = moved_tab(was, own);

        if moved {
            // Moved away from the isolated tab by `move_to_focused_tab`.
            if let Some(isolation) = self.isolation.take() {
                picker::restore(&isolation);
            }
        }
        if !moved && !became_visible(was_suppressed, is_suppressed) {
            return false;
        }
        self.picker.reset_filter();
        if let Some((position, _)) = own {
            self.isolate_floating(own_id, position, manifest);
        }
        true
    }

    fn isolate_floating(&mut self, own_id: u32, position: usize, manifest: &PaneManifest) {
        if !self.config().isolate_floating || self.isolation.is_some() {
            return;
        }
        // No TabUpdate since the picker was hidden: use the state the bar recorded. Without a
        // bar assume hidden, otherwise the other panes stay shown after close.
        let tab = self.last_tabs.iter().find(|t| t.position == position);
        let layer_was_visible = match tab {
            Some(tab) => tab.are_floating_panes_visible,
            None => host::read_layer_states(get_plugin_ids().zellij_pid)
                .and_then(|layers| layers.get(&position).copied())
                .unwrap_or(false),
        };
        let panes: Vec<_> = manifest
            .panes
            .get(&position)
            .into_iter()
            .flatten()
            .map(host::to_pane_snapshot)
            .collect();
        let hidden = panes_to_isolate(own_id, &panes, layer_was_visible);
        if hidden.is_empty() {
            return;
        }
        for pane in &hidden {
            // hide_pane_with_id: suppresses the pane; needs ChangeApplicationState.
            hide_pane_with_id(host::to_pane_id(pane.id, pane.is_plugin));
        }
        self.isolation = Some(Isolation {
            tab_id: tab.map(|t| t.tab_id),
            hidden,
        });
    }

    /// The first open has no earlier `TabUpdate`; take the tab id from the one that follows.
    fn fill_isolation_tab_id(&mut self) {
        let Some(isolation) = self.isolation.as_mut().filter(|i| i.tab_id.is_none()) else {
            return;
        };
        let Some((position, _)) = self.own_pane else {
            return;
        };
        isolation.tab_id = self
            .last_tabs
            .iter()
            .find(|t| t.position == position)
            .map(|t| t.tab_id);
    }

    fn render_picker(&mut self, rows: usize, cols: usize) {
        let sessions = self.sessions();
        let mut warnings = self.warnings.clone();
        warnings.extend(self.last_error.clone());
        warnings.extend(self.poll_warning.clone());
        let config = &self.config;
        let icons = &self.icons;
        let options = RowOptions {
            icons,
            show_state_text: config.show_state_text,
            show_cwd: config.show_cwd,
            home: self.home.as_deref(),
        };
        let keymap = &self.keymap;
        let lines = self
            .picker
            .render(&sessions, keymap, &options, &warnings, rows, cols);
        picker::print_lines(&lines, cols);
    }

    fn init_status_dir(&mut self) {
        // The environment is only needed without `status_dir`, or to expand its `~`. Reading it
        // right after `Granted` can panic in the host shim, so it waits for the next `SessionUpdate`.
        if self.config().needs_session_env() {
            self.env_pending = true;
        } else {
            self.resolve_status_dir(&BTreeMap::new());
        }
    }

    fn init_status_dir_from_env(&mut self) {
        // get_session_environment_variables: needs ReadSessionEnvironmentVariables.
        let env = get_session_environment_variables();
        self.home = env.get("HOME").cloned();
        self.resolve_status_dir(&env);
    }

    fn resolve_status_dir(&mut self, env: &BTreeMap<String, String>) {
        self.status_dir = resolve_status_dir(self.config(), env);
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
                self.store.apply_snapshot(files.records, !files.truncated);
                self.prune();
            }
            Err(error) => self.last_error = Some(error),
        }
    }

    /// Zellij 0.45 only rescans other sessions on request, so without this `SessionUpdate`
    /// would list the current session only and live sessions elsewhere would look gone.
    fn refresh_session_list(&mut self) {
        if !self.permissions_live() {
            return;
        }
        // get_session_list: needs ReadApplicationState; also pushes a fresh SessionUpdate.
        if let Ok(snapshot) = get_session_list() {
            self.live = Some(
                snapshot
                    .live_sessions
                    .iter()
                    .map(host::to_live_session)
                    .collect(),
            );
            self.session_list_complete = true;
        }
    }

    /// Calls right after `Granted` are denied by the host shim; a `SessionUpdate` proves it is live.
    fn permissions_live(&self) -> bool {
        self.permissions_granted && self.live.is_some()
    }

    fn prune(&mut self) {
        let stale_after = self.config().stale_after_secs;
        let live = self.live.as_deref().filter(|_| self.session_list_complete);
        let removed = self.store.prune(live, host::unix_now(), stale_after);
        if self.host_folder_ready {
            for (zellij_session, pane_id) in removed {
                host::remove_status_file(&zellij_session, pane_id);
            }
        }
    }
}

/// A record without a session is not a status record.
fn is_valid_record(record: &StatusRecord) -> bool {
    !record.zellij_session.is_empty()
}
