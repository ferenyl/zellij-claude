use crate::config::{Align, Orientation, VerticalAlign};
use crate::hook_map::SessionState;
use crate::icons::Icons;
use crate::state::ClaudeSession;
use crate::truncate::{fit, width};

const SEPARATOR: &str = "  ";

/// Number of sessions in total and per state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub total: usize,
    pub working: usize,
    pub waiting: usize,
    pub idle: usize,
}

impl Counts {
    pub fn from_sessions(sessions: &[ClaudeSession]) -> Self {
        let mut counts = Self {
            total: sessions.len(),
            ..Self::default()
        };
        for session in sessions {
            match session.state {
                SessionState::Working => counts.working += 1,
                SessionState::Waiting => counts.waiting += 1,
                SessionState::Idle => counts.idle += 1,
            }
        }
        counts
    }
}

/// A piece of bar text; `state` is `None` for the total count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub state: Option<SessionState>,
}

#[derive(Debug, Clone)]
pub struct BarOptions<'a> {
    pub icons: &'a Icons,
    pub orientation: Orientation,
    pub align: Align,
    pub vertical_align: VerticalAlign,
    pub show_zero: bool,
    /// Blink "off" phase: the waiting icon is replaced by blanks of the same width,
    /// only while at least one session is waiting.
    pub hide_waiting_icon: bool,
}

/// Renders the bar as lines of spans. Never more than `rows` lines or `cols` cells per line.
pub fn render(counts: Counts, options: &BarOptions, rows: usize, cols: usize) -> Vec<Vec<Span>> {
    if rows == 0 || cols == 0 {
        return Vec::new();
    }
    let items = items(counts, options);
    let horizontal_width = items.iter().map(|s| width(&s.text)).sum::<usize>()
        + SEPARATOR.len() * items.len().saturating_sub(1);
    let vertical = match options.orientation {
        Orientation::Horizontal => false,
        Orientation::Vertical => true,
        Orientation::Auto => rows >= 2 && (horizontal_width > cols || rows > cols),
    };

    let mut lines: Vec<Vec<Span>> = if vertical {
        items
            .into_iter()
            .take(rows)
            .map(|span| vec![fit_span(span, cols)])
            .collect()
    } else {
        vec![horizontal_line(items, cols)]
    };

    if options.align == Align::Right {
        for line in &mut lines {
            let used: usize = line.iter().map(|s| width(&s.text)).sum();
            if used < cols {
                line.insert(
                    0,
                    Span {
                        text: " ".repeat(cols - used),
                        state: None,
                    },
                );
            }
        }
    }
    if options.vertical_align == VerticalAlign::Bottom {
        let blank = rows - lines.len();
        lines.splice(0..0, std::iter::repeat_n(Vec::new(), blank));
    }
    lines
}

fn items(counts: Counts, options: &BarOptions) -> Vec<Span> {
    let icons = options.icons;
    let mut items = vec![Span {
        text: format!("{} {}", icons.session, counts.total),
        state: None,
    }];
    for (state, count) in [
        (SessionState::Working, counts.working),
        (SessionState::Waiting, counts.waiting),
        (SessionState::Idle, counts.idle),
    ] {
        if count == 0 && !options.show_zero {
            continue;
        }
        let icon = icons.for_state(state);
        let icon = if state == SessionState::Waiting && options.hide_waiting_icon && count > 0 {
            " ".repeat(width(icon))
        } else {
            icon.to_string()
        };
        items.push(Span {
            text: format!("{icon} {count}"),
            state: Some(state),
        });
    }
    items
}

/// Adds whole items while they fit; only a lone first item is cut.
fn horizontal_line(items: Vec<Span>, cols: usize) -> Vec<Span> {
    let mut line = Vec::new();
    let mut used = 0;
    for span in items {
        let separator = if line.is_empty() { 0 } else { SEPARATOR.len() };
        let w = width(&span.text);
        if used + separator + w > cols {
            if line.is_empty() {
                line.push(fit_span(span, cols));
            }
            break;
        }
        if separator > 0 {
            line.push(Span {
                text: SEPARATOR.to_string(),
                state: None,
            });
        }
        used += separator + w;
        line.push(span);
    }
    line
}

fn fit_span(span: Span, cols: usize) -> Span {
    Span {
        text: fit(&span.text, cols),
        state: span.state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IconSet, Mode};

    fn icons() -> Icons {
        let mut config = Config::defaults(Mode::Bar);
        config.icons = IconSet::Unicode;
        Icons::from_config(&config)
    }

    fn counts() -> Counts {
        Counts {
            total: 3,
            working: 2,
            waiting: 1,
            idle: 0,
        }
    }

    fn options(icons: &Icons, orientation: Orientation) -> BarOptions<'_> {
        BarOptions {
            icons,
            orientation,
            align: Align::Left,
            vertical_align: VerticalAlign::Top,
            show_zero: true,
            hide_waiting_icon: false,
        }
    }

    fn texts(lines: &[Vec<Span>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn counts_sessions_per_state() {
        let session = |state| ClaudeSession {
            zellij_session: "s".into(),
            pane_id: 1,
            state,
            cwd: None,
            claude_session_id: None,
            updated_at: 0,
            is_current_session: true,
            tab_position: None,
            tab_name: None,
            title: None,
            is_floating: false,
        };
        let sessions = [
            session(SessionState::Working),
            session(SessionState::Working),
            session(SessionState::Waiting),
        ];

        assert_eq!(Counts::from_sessions(&sessions), counts());
    }

    #[test]
    fn auto_is_horizontal_on_one_row() {
        let icons = icons();

        let lines = render(counts(), &options(&icons, Orientation::Auto), 1, 80);

        assert_eq!(texts(&lines), vec!["◆ 3  ● 2  ▲ 1  ✔ 0"]);
        assert_eq!(lines[0][2].state, Some(SessionState::Working));
    }

    #[test]
    fn auto_is_vertical_when_narrow_and_tall() {
        let icons = icons();

        let lines = render(counts(), &options(&icons, Orientation::Auto), 4, 6);

        assert_eq!(texts(&lines), vec!["◆ 3", "● 2", "▲ 1", "✔ 0"]);
    }

    #[test]
    fn forced_orientation_wins() {
        let icons = icons();

        let horizontal = render(counts(), &options(&icons, Orientation::Horizontal), 4, 6);
        let vertical = render(counts(), &options(&icons, Orientation::Vertical), 1, 80);

        assert_eq!(texts(&horizontal), vec!["◆ 3"]);
        assert_eq!(texts(&vertical), vec!["◆ 3"]);
    }

    #[test]
    fn show_zero_false_hides_empty_states() {
        let icons = icons();
        let mut opts = options(&icons, Orientation::Horizontal);
        opts.show_zero = false;

        let lines = render(counts(), &opts, 1, 80);

        assert_eq!(texts(&lines), vec!["◆ 3  ● 2  ▲ 1"]);
    }

    #[test]
    fn blink_off_phase_keeps_width() {
        let icons = icons();
        let mut opts = options(&icons, Orientation::Horizontal);
        opts.hide_waiting_icon = true;

        let lines = render(counts(), &opts, 1, 80);

        assert_eq!(texts(&lines), vec!["◆ 3  ● 2    1  ✔ 0"]);
    }

    #[test]
    fn no_blink_without_waiting_sessions() {
        let icons = icons();
        let mut opts = options(&icons, Orientation::Horizontal);
        opts.hide_waiting_icon = true;
        let counts = Counts {
            waiting: 0,
            ..counts()
        };

        let lines = render(counts, &opts, 1, 80);

        assert_eq!(texts(&lines), vec!["◆ 3  ● 2  ▲ 0  ✔ 0"]);
    }

    #[test]
    fn right_and_bottom_alignment() {
        let icons = icons();
        let mut opts = options(&icons, Orientation::Vertical);
        opts.align = Align::Right;
        opts.vertical_align = VerticalAlign::Bottom;

        let lines = render(counts(), &opts, 6, 5);

        assert_eq!(
            texts(&lines),
            vec!["", "", "  ◆ 3", "  ● 2", "  ▲ 1", "  ✔ 0"]
        );
    }

    #[test]
    fn horizontal_right_alignment_pads_to_width() {
        let icons = icons();
        let mut opts = options(&icons, Orientation::Horizontal);
        opts.align = Align::Right;

        let lines = render(counts(), &opts, 1, 20);

        assert_eq!(texts(&lines), vec!["  ◆ 3  ● 2  ▲ 1  ✔ 0"]);
    }

    #[test]
    fn tiny_sizes_never_overflow() {
        let icons = icons();
        for orientation in [
            Orientation::Auto,
            Orientation::Horizontal,
            Orientation::Vertical,
        ] {
            for rows in 0..5 {
                for cols in 0..12 {
                    let mut opts = options(&icons, orientation);
                    opts.align = Align::Right;
                    opts.vertical_align = VerticalAlign::Bottom;
                    let lines = render(counts(), &opts, rows, cols);
                    assert!(lines.len() <= rows);
                    for text in texts(&lines) {
                        assert!(width(&text) <= cols, "{rows}x{cols}: {text:?}");
                    }
                }
            }
        }
        assert_eq!(
            texts(&render(
                counts(),
                &options(&icons, Orientation::Horizontal),
                1,
                2
            )),
            vec!["◆ "]
        );
    }
}
