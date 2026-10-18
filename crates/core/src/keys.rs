use crate::config::{Config, ConfigWarning};

/// A key independent of the Zellij key types; the plugin shell converts into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Tab,
    Backspace,
    Delete,
    PageUp,
    PageDown,
    Home,
    End,
    F(u8),
}

/// A key press with its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
}

impl KeyPress {
    pub fn plain(key: Key) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
        }
    }

    /// Parses Zellij-style key names such as `j`, `Up`, `Enter`, `Ctrl n` or `Alt w`.
    pub fn parse(spec: &str) -> Option<Self> {
        let mut parts: Vec<&str> = spec.split_whitespace().collect();
        let name = parts.pop()?;
        let mut press = Self::plain(parse_key(name)?);
        for modifier in parts {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" => press.ctrl = true,
                "alt" => press.alt = true,
                _ => return None,
            }
        }
        Some(press)
    }
}

fn parse_key(name: &str) -> Option<Key> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(Key::Char(c));
    }
    let key = match name.to_ascii_lowercase().as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "enter" => Key::Enter,
        "esc" => Key::Esc,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "home" => Key::Home,
        "end" => Key::End,
        "space" => Key::Char(' '),
        other => {
            let n = other.strip_prefix('f')?.parse().ok()?;
            Key::F(n)
        }
    };
    Some(key)
}

/// Picker actions that can be bound to keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Select,
    Close,
    Filter,
    JumpWaiting,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::from_config(&Config::default()).0
    }
}

/// Configured picker key bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<(KeyPress, Action)>,
}

impl Keymap {
    /// Builds the keymap from the `key_*` config entries. Unparsable key names
    /// are skipped and reported.
    pub fn from_config(config: &Config) -> (Self, Vec<ConfigWarning>) {
        let groups = [
            ("key_up", &config.key_up, Action::Up),
            ("key_down", &config.key_down, Action::Down),
            ("key_select", &config.key_select, Action::Select),
            ("key_close", &config.key_close, Action::Close),
            ("key_filter", &config.key_filter, Action::Filter),
            (
                "key_jump_waiting",
                &config.key_jump_waiting,
                Action::JumpWaiting,
            ),
        ];
        let mut bindings = Vec::new();
        let mut warnings = Vec::new();
        for (config_key, specs, action) in groups {
            for spec in specs {
                match KeyPress::parse(spec) {
                    Some(press) => bindings.push((press, action)),
                    None => warnings.push(ConfigWarning::InvalidValue {
                        key: config_key.to_string(),
                        value: spec.clone(),
                        expected: "a key name like `j`, `Up`, `Enter` or `Ctrl n`",
                    }),
                }
            }
        }
        (Self { bindings }, warnings)
    }

    /// The action bound to `press`; the first matching binding wins.
    pub fn action(&self, press: KeyPress) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(bound, _)| *bound == press)
            .map(|(_, action)| *action)
    }

    /// First key bound to `action`, for the help line.
    pub fn first_key(&self, action: Action) -> Option<KeyPress> {
        self.bindings
            .iter()
            .find(|(_, a)| *a == action)
            .map(|(press, _)| *press)
    }
}

/// Keybinding KDL for `reconfigure` that opens the picker on `key`. Both values are
/// validated by config parsing to contain no quotes or backslashes.
pub fn open_picker_binding(key: &str, plugin_location: &str) -> String {
    format!(
        r#"keybinds {{
    shared_among "normal" "locked" {{
        bind "{key}" {{
            LaunchOrFocusPlugin "{plugin_location}" {{
                floating true
                move_to_focused_tab true
                mode "picker"
            }}
        }}
    }}
}}"#
    )
}

/// Short display name for a key press in the help line.
pub fn display(press: KeyPress) -> String {
    let key = match press.key {
        Key::Char(' ') => "Space".to_string(),
        Key::Char(c) => c.to_string(),
        Key::Up => "↑".into(),
        Key::Down => "↓".into(),
        Key::Left => "←".into(),
        Key::Right => "→".into(),
        Key::Enter => "Enter".into(),
        Key::Esc => "Esc".into(),
        Key::Tab => "Tab".into(),
        Key::Backspace => "Bksp".into(),
        Key::Delete => "Del".into(),
        Key::PageUp => "PgUp".into(),
        Key::PageDown => "PgDn".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::F(n) => format!("F{n}"),
    };
    let mut out = String::new();
    if press.ctrl {
        out.push_str("Ctrl ");
    }
    if press.alt {
        out.push_str("Alt ");
    }
    out.push_str(&key);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mode;

    #[test]
    fn parses_key_names_and_modifiers() {
        assert_eq!(KeyPress::parse("j"), Some(KeyPress::plain(Key::Char('j'))));
        assert_eq!(KeyPress::parse("Up"), Some(KeyPress::plain(Key::Up)));
        assert_eq!(KeyPress::parse("F5"), Some(KeyPress::plain(Key::F(5))));
        assert_eq!(
            KeyPress::parse("Ctrl n"),
            Some(KeyPress {
                key: Key::Char('n'),
                ctrl: true,
                alt: false
            })
        );
        assert_eq!(KeyPress::parse("Hyper x"), None);
        assert_eq!(KeyPress::parse("Nope"), None);
        assert_eq!(KeyPress::parse(""), None);
    }

    #[test]
    fn default_keymap_resolves_actions() {
        let (keymap, warnings) = Keymap::from_config(&Config::defaults(Mode::Picker));

        assert!(warnings.is_empty());
        assert_eq!(
            keymap.action(KeyPress::plain(Key::Char('k'))),
            Some(Action::Up)
        );
        assert_eq!(
            keymap.action(KeyPress::plain(Key::Down)),
            Some(Action::Down)
        );
        assert_eq!(
            keymap.action(KeyPress::plain(Key::Enter)),
            Some(Action::Select)
        );
        assert_eq!(
            keymap.action(KeyPress::plain(Key::Char('q'))),
            Some(Action::Close)
        );
        assert_eq!(
            keymap.action(KeyPress::plain(Key::Char('/'))),
            Some(Action::Filter)
        );
        assert_eq!(
            keymap.action(KeyPress::plain(Key::Char('w'))),
            Some(Action::JumpWaiting)
        );
        assert_eq!(keymap.action(KeyPress::plain(Key::Char('x'))), None);
    }

    #[test]
    fn invalid_key_names_warn() {
        let mut config = Config::defaults(Mode::Picker);
        config.key_up = vec!["Bogus".into(), "u".into()];

        let (keymap, warnings) = Keymap::from_config(&config);

        assert_eq!(warnings.len(), 1);
        assert_eq!(
            keymap.action(KeyPress::plain(Key::Char('u'))),
            Some(Action::Up)
        );
    }

    #[test]
    fn open_picker_binding_embeds_key_and_location() {
        let kdl = open_picker_binding("Alt c", "file:/p.wasm");

        assert!(kdl.contains(r#"bind "Alt c""#));
        assert!(kdl.contains(r#"LaunchOrFocusPlugin "file:/p.wasm""#));
        assert!(kdl.contains(r#"mode "picker""#));
    }

    #[test]
    fn displays_keys() {
        assert_eq!(display(KeyPress::parse("Ctrl n").unwrap()), "Ctrl n");
        assert_eq!(display(KeyPress::plain(Key::Up)), "↑");
    }
}
