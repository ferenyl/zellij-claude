//! Picker rendering and jump actions.

use claude_sessions_core::list::{Line, LineKind};
use claude_sessions_core::state::ClaudeSession;
use claude_sessions_core::truncate::pad;
use zellij_tile::prelude::*;

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

/// Hides the picker and focuses the session's pane, switching Zellij session if needed.
pub fn jump(session: &ClaudeSession) {
    // hide_self: hides only this plugin pane, leaving other floating panes as they were.
    hide_self();
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
