//! Snapshot tests of rendered picker and bar output. Each line is framed with `|`
//! at the pane width, so layout regressions are visible in review.

use claude_sessions_core::bar::{self, BarOptions, Counts};
use claude_sessions_core::config::{Align, Config, IconSet, Mode, Orientation, VerticalAlign};
use claude_sessions_core::hook_map::SessionState;
use claude_sessions_core::icons::Icons;
use claude_sessions_core::keys::{Key, KeyPress, Keymap};
use claude_sessions_core::list::{order, LineKind, Picker, RowOptions};
use claude_sessions_core::state::ClaudeSession;
use claude_sessions_core::truncate::pad;

fn icons() -> Icons {
    let mut config = Config::defaults(Mode::Picker);
    config.icons = IconSet::Unicode;
    Icons::from_config(&config)
}

fn keymap() -> Keymap {
    Keymap::from_config(&Config::defaults(Mode::Picker)).0
}

fn session(
    name: &str,
    current: bool,
    pane: u32,
    tab: &str,
    cwd: &str,
    state: SessionState,
) -> ClaudeSession {
    ClaudeSession {
        zellij_session: name.into(),
        pane_id: pane,
        state,
        cwd: Some(cwd.into()),
        claude_session_id: None,
        updated_at: 0,
        is_current_session: current,
        tab_position: Some(pane as usize),
        tab_name: Some(tab.into()),
        title: Some(format!("pane {pane}")),
        is_floating: false,
    }
}

fn sessions() -> Vec<ClaudeSession> {
    order(
        vec![
            session(
                "work",
                true,
                1,
                "backend",
                "/home/u/code/api",
                SessionState::Working,
            ),
            session(
                "work",
                true,
                2,
                "frontend",
                "/home/u/code/web",
                SessionState::Waiting,
            ),
            session(
                "personal",
                false,
                3,
                "notes",
                "/home/u/notes",
                SessionState::Idle,
            ),
            session(
                "personal",
                false,
                4,
                "a-very-long-tab-name-indeed",
                "/home/u/some/deeply/nested/project/directory",
                SessionState::Working,
            ),
        ],
        true,
    )
}

fn picker_frame(
    picker: &mut Picker,
    sessions: &[ClaudeSession],
    rows: usize,
    cols: usize,
) -> String {
    picker_frame_with(picker, sessions, &[], rows, cols)
}

fn picker_frame_with(
    picker: &mut Picker,
    sessions: &[ClaudeSession],
    warnings: &[String],
    rows: usize,
    cols: usize,
) -> String {
    let icons = icons();
    let options = RowOptions {
        icons: &icons,
        show_state_text: true,
        show_cwd: true,
        home: Some("/home/u"),
    };
    picker
        .render(sessions, &keymap(), &options, warnings, rows, cols)
        .iter()
        .map(|line| {
            let tag = match line.kind {
                LineKind::Warning => "W",
                LineKind::Header => "H",
                LineKind::Row { selected: true, .. } => "*",
                LineKind::Row { .. } => "R",
                LineKind::Message => "M",
                LineKind::Filter => "F",
                LineKind::Help => "?",
            };
            format!("{tag} |{}|", pad(&line.text, cols))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn bar_frame(counts: Counts, options: &BarOptions, rows: usize, cols: usize) -> String {
    bar::render(counts, options, rows, cols)
        .iter()
        .map(|line| {
            let text: String = line.iter().map(|s| s.text.as_str()).collect();
            format!("|{}|", pad(&text, cols))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn counts() -> Counts {
    Counts {
        total: 4,
        working: 2,
        waiting: 1,
        idle: 1,
    }
}

fn bar_options(icons: &Icons, orientation: Orientation) -> BarOptions<'_> {
    BarOptions {
        icons,
        orientation,
        align: Align::Left,
        vertical_align: VerticalAlign::Top,
        show_zero: true,
        hide_waiting_icon: false,
    }
}

#[test]
fn picker_wide() {
    insta::assert_snapshot!(picker_frame(&mut Picker::default(), &sessions(), 10, 80));
}

#[test]
fn picker_narrow() {
    insta::assert_snapshot!(picker_frame(&mut Picker::default(), &sessions(), 10, 30));
}

#[test]
fn picker_scrolled_to_last_row() {
    let sessions = sessions();
    let mut picker = Picker::default();
    for _ in 0..3 {
        picker.handle_key(KeyPress::plain(Key::Down), &keymap(), &sessions);
    }

    insta::assert_snapshot!(picker_frame(&mut picker, &sessions, 5, 60));
}

#[test]
fn picker_filtering() {
    let sessions = sessions();
    let mut picker = Picker::default();
    for c in "/note".chars() {
        picker.handle_key(KeyPress::plain(Key::Char(c)), &keymap(), &sessions);
    }

    insta::assert_snapshot!(picker_frame(&mut picker, &sessions, 8, 60));
}

#[test]
fn picker_empty_with_warning() {
    let warnings = vec!["unknown config key `colour` ignored".to_string()];

    insta::assert_snapshot!(picker_frame_with(
        &mut Picker::default(),
        &[],
        &warnings,
        6,
        50
    ));
}

#[test]
fn bar_horizontal() {
    let icons = icons();

    insta::assert_snapshot!(bar_frame(
        counts(),
        &bar_options(&icons, Orientation::Auto),
        1,
        30
    ));
}

#[test]
fn bar_horizontal_right_aligned_narrow() {
    let icons = icons();
    let mut options = bar_options(&icons, Orientation::Horizontal);
    options.align = Align::Right;

    insta::assert_snapshot!(bar_frame(counts(), &options, 1, 12));
}

#[test]
fn bar_vertical() {
    let icons = icons();

    insta::assert_snapshot!(bar_frame(
        counts(),
        &bar_options(&icons, Orientation::Auto),
        5,
        6
    ));
}

#[test]
fn bar_vertical_bottom_right() {
    let icons = icons();
    let mut options = bar_options(&icons, Orientation::Vertical);
    options.align = Align::Right;
    options.vertical_align = VerticalAlign::Bottom;
    options.show_zero = false;
    let counts = Counts {
        idle: 0,
        ..counts()
    };

    insta::assert_snapshot!(bar_frame(counts, &options, 6, 8));
}

#[test]
fn bar_tiny() {
    let icons = icons();

    insta::assert_snapshot!(bar_frame(
        counts(),
        &bar_options(&icons, Orientation::Auto),
        2,
        3
    ));
}
