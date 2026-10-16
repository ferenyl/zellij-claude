//! Status bar rendering.

use claude_sessions_core::bar::Span;
use claude_sessions_core::color::Color;
use claude_sessions_core::config::Config;
use claude_sessions_core::hook_map::SessionState;

const RESET: &str = "\u{1b}[0m";

pub fn print_lines(lines: &[Vec<Span>], config: &Config) {
    let rendered: Vec<String> = lines
        .iter()
        .map(|line| line.iter().map(|span| styled(span, config)).collect())
        .collect();
    // print! without a trailing newline so a one-row pane does not scroll.
    print!("{}", rendered.join("\n"));
}

fn styled(span: &Span, config: &Config) -> String {
    let Some(state) = span.state else {
        return span.text.clone();
    };
    let custom = match state {
        SessionState::Working => config.color_working,
        SessionState::Waiting => config.color_waiting,
        SessionState::Idle => config.color_idle,
    };
    let color = custom.unwrap_or_else(|| Color::default_for(state));
    format!("{}{}{RESET}", color.fg(), span.text)
}
