use crate::hook_map::SessionState;
use crate::icons::Icons;
use crate::keys::{display, Action, Key, KeyPress, Keymap};
use crate::state::{ClaudeSession, SessionKey};
use crate::truncate::{fit, pad, truncate_end, truncate_start, width};

const MAX_TAB_WIDTH: usize = 20;
const STATE_WIDTH: usize = 7;

/// Sorts sessions for display: current Zellij session first, then by session name,
/// tab and pane. Drops other sessions unless `all_sessions`.
pub fn order(mut sessions: Vec<ClaudeSession>, all_sessions: bool) -> Vec<ClaudeSession> {
    sessions.retain(|s| all_sessions || s.is_current_session);
    sessions.sort_by(|a, b| {
        (
            !a.is_current_session,
            &a.zellij_session,
            a.tab_position,
            a.pane_id,
        )
            .cmp(&(
                !b.is_current_session,
                &b.zellij_session,
                b.tab_position,
                b.pane_id,
            ))
    });
    sessions
}

/// What a rendered line represents, so the plugin shell can style it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Warning,
    Header,
    Row { state: SessionState, selected: bool },
    Message,
    Filter,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    pub text: String,
}

/// Display options for picker rows.
#[derive(Debug, Clone)]
pub struct RowOptions<'a> {
    pub icons: &'a Icons,
    pub show_state_text: bool,
    pub show_cwd: bool,
    /// Home directory, shown as `~` in paths.
    pub home: Option<&'a str>,
}

/// Result of handling a key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    None,
    Redraw,
    Close,
    Jump(ClaudeSession),
}

/// Picker UI state: cursor, scroll offset and type-to-filter.
#[derive(Debug, Default, Clone)]
pub struct Picker {
    cursor: usize,
    scroll: usize,
    selected: Option<SessionKey>,
    filter: String,
    filtering: bool,
}

impl Picker {
    /// Clears the filter; called when the picker is shown again.
    pub fn reset_filter(&mut self) {
        self.filter.clear();
        self.filtering = false;
    }

    /// Sessions matching the filter (case-insensitive on session, tab, title and cwd).
    pub fn visible<'a>(&self, sessions: &'a [ClaudeSession]) -> Vec<&'a ClaudeSession> {
        let needle = self.filter.to_lowercase();
        sessions
            .iter()
            .filter(|s| {
                needle.is_empty()
                    || [
                        Some(s.zellij_session.as_str()),
                        s.tab_name.as_deref(),
                        s.title.as_deref(),
                        s.cwd.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|field| field.to_lowercase().contains(&needle))
            })
            .collect()
    }

    /// Keeps the cursor on the same session when the list changes, else clamps it.
    pub fn sync(&mut self, sessions: &[ClaudeSession]) {
        let visible = self.visible(sessions);
        if let Some(index) = self
            .selected
            .as_ref()
            .and_then(|key| visible.iter().position(|s| key_of(s) == *key))
        {
            self.cursor = index;
        } else {
            self.cursor = self.cursor.min(visible.len().saturating_sub(1));
        }
        self.selected = visible.get(self.cursor).map(|s| key_of(s));
    }

    pub fn handle_key(
        &mut self,
        press: KeyPress,
        keymap: &Keymap,
        sessions: &[ClaudeSession],
    ) -> Command {
        if self.filtering {
            if let Some(command) = self.handle_filter_key(press, sessions) {
                return command;
            }
        }
        let command = match keymap.action(press) {
            Some(Action::Up) => self.move_cursor(-1, sessions),
            Some(Action::Down) => self.move_cursor(1, sessions),
            Some(Action::Select) => self.selected_jump(sessions),
            Some(Action::Close) => Command::Close,
            Some(Action::Filter) => {
                self.filtering = true;
                Command::Redraw
            }
            Some(Action::JumpWaiting) => self
                .visible(sessions)
                .into_iter()
                .find(|s| s.state == SessionState::Waiting)
                .map_or(Command::None, |s| Command::Jump(s.clone())),
            None => Command::None,
        };
        self.sync(sessions);
        command
    }

    /// Handles typing while filtering. `None` falls through to the keymap
    /// (navigation and select keep working).
    fn handle_filter_key(
        &mut self,
        press: KeyPress,
        sessions: &[ClaudeSession],
    ) -> Option<Command> {
        if press.ctrl || press.alt {
            return None;
        }
        match press.key {
            Key::Esc => self.reset_filter(),
            Key::Backspace => {
                self.filter.pop();
            }
            Key::Char(c) => self.filter.push(c),
            _ => return None,
        }
        self.cursor = 0;
        self.selected = None;
        self.sync(sessions);
        Some(Command::Redraw)
    }

    fn move_cursor(&mut self, delta: isize, sessions: &[ClaudeSession]) -> Command {
        let len = self.visible(sessions).len();
        if len == 0 {
            return Command::None;
        }
        self.cursor = self.cursor.saturating_add_signed(delta).min(len - 1);
        self.selected = None;
        Command::Redraw
    }

    fn selected_jump(&self, sessions: &[ClaudeSession]) -> Command {
        self.visible(sessions)
            .get(self.cursor)
            .map_or(Command::None, |s| Command::Jump((*s).clone()))
    }

    /// Renders the picker into exactly `rows` or fewer lines, none wider than `cols`.
    pub fn render(
        &mut self,
        sessions: &[ClaudeSession],
        keymap: &Keymap,
        options: &RowOptions,
        warnings: &[String],
        rows: usize,
        cols: usize,
    ) -> Vec<Line> {
        self.sync(sessions);
        let mut top: Vec<Line> = warnings
            .iter()
            .map(|w| line(LineKind::Warning, &format!("! {w}"), cols))
            .collect();
        let mut footer = Vec::new();
        if self.filtering || !self.filter.is_empty() {
            footer.push(line(LineKind::Filter, &format!("/{}", self.filter), cols));
        }
        if rows >= 4 {
            footer.push(line(LineKind::Help, &help_text(keymap), cols));
        }

        let visible = self.visible(sessions);
        let body_height = rows.saturating_sub(top.len() + footer.len());
        let (body, selected_line) = if visible.is_empty() {
            let message = if self.filter.is_empty() {
                "No Claude sessions found".to_string()
            } else {
                format!("No sessions match \"{}\"", self.filter)
            };
            (vec![line(LineKind::Message, &message, cols)], None)
        } else {
            self.body_lines(&visible, options, cols)
        };

        if let Some(selected) = selected_line {
            // Keep the group header above the cursor visible when scrolling up.
            let top_line = match selected.checked_sub(1).map(|i| &body[i].kind) {
                Some(LineKind::Header) => selected - 1,
                _ => selected,
            };
            if top_line < self.scroll {
                self.scroll = top_line;
            } else if selected >= self.scroll + body_height {
                self.scroll = (selected + 1).saturating_sub(body_height);
            }
        }
        self.scroll = self.scroll.min(body.len().saturating_sub(body_height));

        top.extend(body.into_iter().skip(self.scroll).take(body_height));
        top.extend(footer);
        top.truncate(rows);
        top
    }

    fn body_lines(
        &self,
        visible: &[&ClaudeSession],
        options: &RowOptions,
        cols: usize,
    ) -> (Vec<Line>, Option<usize>) {
        let tab_width = visible
            .iter()
            .map(|s| width(s.tab_name.as_deref().unwrap_or("?")))
            .max()
            .unwrap_or(0)
            .min(MAX_TAB_WIDTH);
        let mut lines = Vec::new();
        let mut selected_line = None;
        let mut current_group: Option<&str> = None;
        for (index, session) in visible.iter().enumerate() {
            if current_group != Some(session.zellij_session.as_str()) {
                current_group = Some(&session.zellij_session);
                let suffix = if session.is_current_session {
                    " (current)"
                } else {
                    ""
                };
                lines.push(line(
                    LineKind::Header,
                    &format!("{}{suffix}", session.zellij_session),
                    cols,
                ));
            }
            let selected = index == self.cursor;
            if selected {
                selected_line = Some(lines.len());
            }
            lines.push(Line {
                kind: LineKind::Row {
                    state: session.state,
                    selected,
                },
                text: row_text(session, selected, tab_width, options, cols),
            });
        }
        (lines, selected_line)
    }
}

fn row_text(
    session: &ClaudeSession,
    selected: bool,
    tab_width: usize,
    options: &RowOptions,
    cols: usize,
) -> String {
    let mut prefix = String::from(if selected { "> " } else { "  " });
    prefix.push_str(options.icons.for_state(session.state));
    prefix.push(' ');
    if options.show_state_text {
        prefix.push_str(&pad(session.state.as_str(), STATE_WIDTH));
        prefix.push(' ');
    }
    let tab = session.tab_name.as_deref().unwrap_or("?");
    let detail = if options.show_cwd {
        session
            .cwd
            .as_deref()
            .map(|cwd| shorten_home(cwd, options.home))
            .or_else(|| session.title.clone())
    } else {
        session.title.clone()
    }
    .unwrap_or_default();

    // Truncation priority: the detail column shrinks first, then the tab name.
    let remaining = cols.saturating_sub(width(&prefix));
    let tab_cells = tab_width.min(remaining);
    let detail_cells = remaining.saturating_sub(tab_cells + 2);
    let mut text = prefix;
    if detail_cells >= 2 && !detail.is_empty() {
        text.push_str(&pad(&truncate_end(tab, tab_cells), tab_cells));
        text.push_str("  ");
        text.push_str(&truncate_start(&detail, detail_cells));
    } else {
        text.push_str(&truncate_end(tab, tab_cells));
    }
    fit(text.trim_end(), cols)
}

fn shorten_home(path: &str, home: Option<&str>) -> String {
    match home.filter(|h| !h.is_empty()) {
        Some(home) if path == home => "~".to_string(),
        Some(home) => match path.strip_prefix(home) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        },
        None => path.to_string(),
    }
}

fn help_text(keymap: &Keymap) -> String {
    [
        (Action::Up, "up"),
        (Action::Down, "down"),
        (Action::Select, "jump"),
        (Action::Filter, "filter"),
        (Action::JumpWaiting, "waiting"),
        (Action::Close, "close"),
    ]
    .into_iter()
    .filter_map(|(action, label)| {
        keymap
            .first_key(action)
            .map(|k| format!("{} {label}", display(k)))
    })
    .collect::<Vec<_>>()
    .join("  ")
}

fn line(kind: LineKind, text: &str, cols: usize) -> Line {
    Line {
        kind,
        text: truncate_end(text, cols),
    }
}

fn key_of(session: &ClaudeSession) -> SessionKey {
    (session.zellij_session.clone(), session.pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IconSet, Mode};

    fn session(
        name: &str,
        current: bool,
        pane: u32,
        tab: usize,
        state: SessionState,
    ) -> ClaudeSession {
        ClaudeSession {
            zellij_session: name.into(),
            pane_id: pane,
            state,
            cwd: Some(format!("/home/u/code/{name}-{pane}")),
            claude_session_id: None,
            updated_at: 0,
            is_current_session: current,
            tab_position: Some(tab),
            tab_name: Some(format!("tab{tab}")),
            title: Some(format!("title{pane}")),
            is_floating: false,
        }
    }

    fn fixture() -> Vec<ClaudeSession> {
        order(
            vec![
                session("personal", false, 1, 0, SessionState::Idle),
                session("work", true, 5, 1, SessionState::Waiting),
                session("work", true, 2, 0, SessionState::Working),
            ],
            true,
        )
    }

    fn keymap() -> Keymap {
        Keymap::from_config(&Config::defaults(Mode::Picker)).0
    }

    fn unicode_icons() -> Icons {
        let mut config = Config::defaults(Mode::Picker);
        config.icons = IconSet::Unicode;
        Icons::from_config(&config)
    }

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.text.clone()).collect()
    }

    fn press(c: char) -> KeyPress {
        KeyPress::plain(Key::Char(c))
    }

    #[test]
    fn order_puts_current_session_first_and_filters_others() {
        let sessions = fixture();
        let keys: Vec<_> = sessions.iter().map(key_of).collect();
        assert_eq!(
            keys,
            vec![
                ("work".into(), 2),
                ("work".into(), 5),
                ("personal".into(), 1)
            ]
        );

        let current_only = order(sessions, false);
        assert_eq!(current_only.len(), 2);
    }

    #[test]
    fn navigation_is_clamped_and_select_jumps() {
        let sessions = fixture();
        let mut picker = Picker::default();
        let keymap = keymap();

        picker.handle_key(press('k'), &keymap, &sessions);
        for _ in 0..5 {
            picker.handle_key(KeyPress::plain(Key::Down), &keymap, &sessions);
        }

        assert_eq!(
            picker.handle_key(KeyPress::plain(Key::Enter), &keymap, &sessions),
            Command::Jump(sessions[2].clone())
        );
        assert_eq!(
            picker.handle_key(press('q'), &keymap, &sessions),
            Command::Close
        );
    }

    #[test]
    fn jump_waiting_selects_first_waiting() {
        let sessions = fixture();
        let mut picker = Picker::default();

        let command = picker.handle_key(press('w'), &keymap(), &sessions);

        assert_eq!(command, Command::Jump(sessions[1].clone()));
    }

    #[test]
    fn filter_mode_captures_typing() {
        let sessions = fixture();
        let mut picker = Picker::default();
        let keymap = keymap();

        picker.handle_key(press('/'), &keymap, &sessions);
        for c in "pers".chars() {
            picker.handle_key(press(c), &keymap, &sessions);
        }

        assert_eq!(picker.visible(&sessions).len(), 1);
        assert_eq!(
            picker.handle_key(KeyPress::plain(Key::Enter), &keymap, &sessions),
            Command::Jump(sessions[2].clone())
        );

        picker.handle_key(KeyPress::plain(Key::Esc), &keymap, &sessions);
        assert_eq!(picker.visible(&sessions).len(), 3);
    }

    #[test]
    fn cursor_follows_session_when_list_changes() {
        let mut sessions = fixture();
        let mut picker = Picker::default();
        picker.handle_key(KeyPress::plain(Key::Down), &keymap(), &sessions);

        sessions.remove(0);
        picker.sync(&sessions);
        let command = picker.selected_jump(&sessions);

        assert_eq!(command, Command::Jump(sessions[0].clone()));
    }

    #[test]
    fn cursor_is_clamped_when_selected_session_disappears() {
        let mut sessions = fixture();
        let mut picker = Picker::default();
        for _ in 0..2 {
            picker.handle_key(KeyPress::plain(Key::Down), &keymap(), &sessions);
        }

        sessions.pop();
        picker.sync(&sessions);

        assert_eq!(
            picker.selected_jump(&sessions),
            Command::Jump(sessions[1].clone())
        );
    }

    #[test]
    fn renders_grouped_rows() {
        let sessions = fixture();
        let icons = unicode_icons();
        let options = RowOptions {
            icons: &icons,
            show_state_text: true,
            show_cwd: true,
            home: Some("/home/u"),
        };

        let lines = Picker::default().render(&sessions, &keymap(), &options, &[], 10, 60);

        assert_eq!(
            texts(&lines),
            vec![
                "work (current)",
                "> ● working tab0  ~/code/work-2",
                "  ▲ waiting tab1  ~/code/work-5",
                "personal",
                "  ✔ idle    tab0  ~/code/personal-1",
                "k up  j down  Enter jump  / filter  w waiting  Esc close",
            ]
        );
        assert_eq!(
            lines[1].kind,
            LineKind::Row {
                state: SessionState::Working,
                selected: true
            }
        );
    }

    #[test]
    fn narrow_width_truncates_cwd_before_tab() {
        let sessions = fixture();
        let icons = unicode_icons();
        let options = RowOptions {
            icons: &icons,
            show_state_text: true,
            show_cwd: true,
            home: Some("/home/u"),
        };

        let lines = Picker::default().render(&sessions, &keymap(), &options, &[], 3, 22);
        assert_eq!(lines[1].text, "> ● working tab0  …k-2");

        let lines = Picker::default().render(&sessions, &keymap(), &options, &[], 3, 14);
        assert_eq!(lines[1].text, "> ● working t…");
        assert!(lines.iter().all(|l| width(&l.text) <= 14));
    }

    #[test]
    fn scrolls_to_keep_cursor_visible() {
        let sessions = fixture();
        let icons = unicode_icons();
        let options = RowOptions {
            icons: &icons,
            show_state_text: false,
            show_cwd: false,
            home: None,
        };
        let mut picker = Picker::default();
        for _ in 0..2 {
            picker.handle_key(KeyPress::plain(Key::Down), &keymap(), &sessions);
        }

        let lines = picker.render(&sessions, &keymap(), &options, &[], 2, 40);

        assert_eq!(texts(&lines), vec!["personal", "> ✔ tab0  title1"]);
    }

    #[test]
    fn empty_state_and_warnings() {
        let icons = unicode_icons();
        let options = RowOptions {
            icons: &icons,
            show_state_text: true,
            show_cwd: true,
            home: None,
        };

        let lines = Picker::default().render(&[], &keymap(), &options, &["bad key".into()], 2, 40);

        assert_eq!(texts(&lines), vec!["! bad key", "No Claude sessions found"]);
    }

    #[test]
    fn zero_size_renders_nothing() {
        let sessions = fixture();
        let icons = unicode_icons();
        let options = RowOptions {
            icons: &icons,
            show_state_text: true,
            show_cwd: true,
            home: None,
        };

        let lines = Picker::default().render(&sessions, &keymap(), &options, &[], 0, 0);

        assert!(lines.is_empty());
    }

    #[test]
    fn shortens_home_only_on_path_boundary() {
        assert_eq!(shorten_home("/home/u/x", Some("/home/u")), "~/x");
        assert_eq!(shorten_home("/home/u", Some("/home/u")), "~");
        assert_eq!(shorten_home("/home/user2", Some("/home/u")), "/home/user2");
    }
}
