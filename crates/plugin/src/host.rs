//! Conversions from Zellij host types into core types, and status file reading.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use claude_sessions_core::floating::{
    decode_layers, encode_layers, layer_file_name, Geometry, HiddenPane, LayerStates, PaneSnapshot,
};
use claude_sessions_core::keys::{Key, KeyPress};
use claude_sessions_core::state::{LivePane, LiveSession, LiveTab};
use claude_sessions_core::status_file::{file_name, StatusRecord};
use zellij_tile::prelude::*;

/// `change_host_folder` remaps this WASI mount to the status directory.
const HOST_MOUNT: &str = "/host";
/// Upper bound on status files read per poll, so a cluttered directory cannot stall the plugin.
const LAYER_FILE_MAX_AGE_SECS: u64 = 7 * 24 * 3600;
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
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('-'))
        })
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
            Ok(record) if record.zellij_session.is_empty() => {}
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

pub fn write_layer_states(zellij_pid: u32, layers: &LayerStates) -> Result<(), String> {
    let path = Path::new(HOST_MOUNT).join(layer_file_name(zellij_pid));
    // Written to a temp name and renamed, so the picker never reads a half-written file.
    let tmp = Path::new(HOST_MOUNT).join(format!("{}.tmp", layer_file_name(zellij_pid)));
    std::fs::write(&tmp, encode_layers(layers))
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("cannot write layer state: {e}"))
}

/// Removes layer state files of Zellij processes that are long gone.
pub fn remove_old_layer_files() {
    let Ok(entries) = std::fs::read_dir(Path::new(HOST_MOUNT)) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("floating-") && name.ends_with(".state")) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age.as_secs() > LAYER_FILE_MAX_AGE_SECS);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// `None` if the bar has not written the file (no bar in the layout) or it is unreadable.
pub fn read_layer_states(zellij_pid: u32) -> Option<LayerStates> {
    let path = Path::new(HOST_MOUNT).join(layer_file_name(zellij_pid));
    decode_layers(&std::fs::read_to_string(path).ok()?)
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

pub fn to_pane_snapshot(pane: &PaneInfo) -> PaneSnapshot {
    PaneSnapshot {
        id: pane.id,
        is_plugin: pane.is_plugin,
        is_floating: pane.is_floating,
        is_suppressed: pane.is_suppressed,
        plugin_url: pane.plugin_url.clone(),
        geometry: Geometry {
            x: pane.pane_x,
            y: pane.pane_y,
            cols: pane.pane_columns,
            rows: pane.pane_rows,
        },
    }
}

/// All panes of all tabs, each with its tab position.
pub fn to_positioned_snapshots(manifest: &PaneManifest) -> Vec<(usize, PaneSnapshot)> {
    manifest
        .panes
        .iter()
        .flat_map(|(position, panes)| panes.iter().map(|p| (*position, to_pane_snapshot(p))))
        .collect()
}

pub fn to_pane_id(id: u32, is_plugin: bool) -> PaneId {
    if is_plugin {
        PaneId::Plugin(id)
    } else {
        PaneId::Terminal(id)
    }
}

/// Fixed coordinates of a hidden pane; pinned and borderless stay as they were.
pub fn to_coordinates(pane: &HiddenPane) -> Option<(PaneId, FloatingPaneCoordinates)> {
    let g = pane.geometry;
    let coordinates = FloatingPaneCoordinates::new(
        Some(g.x.to_string()),
        Some(g.y.to_string()),
        Some(g.cols.to_string()),
        Some(g.rows.to_string()),
        None,
        None,
    )?;
    Some((to_pane_id(pane.id, pane.is_plugin), coordinates))
}

pub fn to_key_press(key: &KeyWithModifier) -> Option<KeyPress> {
    let shift = key.key_modifiers.contains(&KeyModifier::Shift);
    if key.key_modifiers.contains(&KeyModifier::Super) {
        return None;
    }
    let mapped = match key.bare_key {
        // Kitty protocol sends `Char('j')` + Shift where legacy terminals send `Char('J')`.
        BareKey::Char(c) if shift && c.is_lowercase() => Key::Char(c.to_uppercase().next()?),
        BareKey::Char(c) => Key::Char(c),
        _ if shift => return None,
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
