//! Conversions from Zellij host types into core types, and status file reading.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use claude_sessions_core::keys::{Key, KeyPress};
use claude_sessions_core::state::{LivePane, LiveSession, LiveTab};
use claude_sessions_core::status_file::StatusRecord;
use zellij_tile::prelude::*;

/// `change_host_folder` remaps this WASI mount to the status directory.
const HOST_MOUNT: &str = "/host";

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Reads every `*.json` status file under `/host`; unparsable files are skipped.
pub fn read_status_files() -> Result<Vec<StatusRecord>, String> {
    let entries = std::fs::read_dir(Path::new(HOST_MOUNT))
        .map_err(|e| format!("cannot read status dir: {e}"))?;
    Ok(entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|json| StatusRecord::parse(&json).ok())
        .collect())
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
