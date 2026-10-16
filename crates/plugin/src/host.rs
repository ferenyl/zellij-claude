//! Conversions from Zellij host types into core types, and status file reading.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use claude_sessions_core::keys::{Key, KeyPress};
use claude_sessions_core::state::{LivePane, LiveSession, LiveTab};
use claude_sessions_core::status_file::{file_name, StatusRecord};
use zellij_tile::prelude::*;

/// `change_host_folder` remaps this WASI mount to the status directory.
const HOST_MOUNT: &str = "/host";
/// Upper bound on status files read per poll, so a cluttered directory cannot stall the plugin.
const MAX_STATUS_FILES: usize = 500;

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Status files read from `/host`, plus problems worth showing to the user.
pub struct StatusFiles {
    pub records: Vec<StatusRecord>,
    pub invalid: usize,
    pub truncated: bool,
}

/// Reads up to [`MAX_STATUS_FILES`] `*.json` status files under `/host`.
pub fn read_status_files() -> Result<StatusFiles, String> {
    let entries = std::fs::read_dir(Path::new(HOST_MOUNT))
        .map_err(|e| format!("cannot read status dir: {e}"))?;
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    let truncated = paths.len() > MAX_STATUS_FILES;
    paths.truncate(MAX_STATUS_FILES);

    let mut records = Vec::with_capacity(paths.len());
    let mut invalid = 0;
    for path in paths {
        match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|json| StatusRecord::parse(&json))
        {
            Ok(record) => records.push(record),
            Err(_) => invalid += 1,
        }
    }
    Ok(StatusFiles {
        records,
        invalid,
        truncated,
    })
}

/// Deletes a pruned session's status file. Missing files are fine: every plugin
/// instance prunes the same entries.
pub fn remove_status_file(zellij_session: &str, pane_id: u32) {
    let _ = std::fs::remove_file(Path::new(HOST_MOUNT).join(file_name(zellij_session, pane_id)));
}

pub fn to_live_session(session: &SessionInfo) -> LiveSession {
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
                    command: p.terminal_command.clone(),
                })
            })
            .collect(),
    }
}

pub fn to_key_press(key: &KeyWithModifier) -> Option<KeyPress> {
    let mapped = match key.bare_key {
        BareKey::Char(c) => Key::Char(c),
        BareKey::Up => Key::Up,
        BareKey::Down => Key::Down,
        BareKey::Left => Key::Left,
        BareKey::Right => Key::Right,
        BareKey::Enter => Key::Enter,
        BareKey::Esc => Key::Esc,
        BareKey::Tab => Key::Tab,
        BareKey::Backspace => Key::Backspace,
        BareKey::Delete => Key::Delete,
        BareKey::PageUp => Key::PageUp,
        BareKey::PageDown => Key::PageDown,
        BareKey::Home => Key::Home,
        BareKey::End => Key::End,
        BareKey::F(n) => Key::F(n),
        _ => return None,
    };
    Some(KeyPress {
        key: mapped,
        ctrl: key.key_modifiers.contains(&KeyModifier::Ctrl),
        alt: key.key_modifiers.contains(&KeyModifier::Alt),
    })
}
