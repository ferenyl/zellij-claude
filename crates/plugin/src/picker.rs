//! Picker rendering and jump actions.

use claude_sessions_core::floating::Isolation;
use claude_sessions_core::list::{Line, LineKind};
use claude_sessions_core::state::ClaudeSession;
use claude_sessions_core::truncate::pad;
use zellij_tile::prelude::*;

use crate::host;

const RESET: &str = "\u{1b}[0m";
const BOLD: &str = "\u{1b}[1m";
const DIM: &str = "\u{1b}[2m";
const REVERSE: &str = "\u{1b}[7m";
const YELLOW: &str = "\u{1b}[33m";

pub fn print_lines(lines: &[Line], cols: usize) {
    for line in lines {
        let styled = match line.kind {
            LineKind::Header => format!("{BOLD}{}{RESET}", line.text),
            LineKind::Row { selected: true, .. } => {
                format!("{REVERSE}{}{RESET}", pad(&line.text, cols))
            }
            LineKind::Help => format!("{DIM}{}{RESET}", line.text),
            LineKind::Warning => format!("{YELLOW}{}{RESET}", line.text),
            _ => line.text.clone(),
        };
        println!("{styled}");
    }
}

/// Hides the picker and brings back the floating panes it hid while open.
pub fn close(isolation: Option<Isolation>) {
    // hide_self: Zellij hides the floating layer itself if the picker was its last pane.
    hide_self();
    if let Some(isolation) = isolation {
        restore(&isolation);
    }
}

/// Unsuppresses the hidden panes at their old geometry and hides the floating layer again.
pub fn restore(isolation: &Isolation) {
    let coordinates = isolation
        .hidden
        .iter()
        .filter_map(host::to_coordinates)
        .collect();
    // change_floating_panes_coordinates: also unsuppresses each pane as floating, without focus.
    change_floating_panes_coordinates(coordinates);
    // hide_floating_panes: blocks, so the calls above are done before a jump or session switch.
    let _ = hide_floating_panes(isolation.tab_id);
}

/// Focuses the session's pane, switching Zellij session if needed. Close the picker first.
pub fn jump(session: &ClaudeSession) {
    if session.is_current_session {
        // focus_terminal_pane: switches to the pane's tab and layer (tiled/floating).
        focus_terminal_pane(session.pane_id, false, false);
    } else {
        // switch_session_with_focus: pane id is (id, is_plugin).
        switch_session_with_focus(
            &session.zellij_session,
            session.tab_position,
            Some((session.pane_id, false)),
        );
    }
}
