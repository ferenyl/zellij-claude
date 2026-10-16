use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

/// Every configuration key the plugin understands.
pub const KEYS: &[&str] = &[
    "mode",
    "icons",
    "icon_working",
    "icon_waiting",
    "icon_idle",
    "icon_session",
    "color_working",
    "color_waiting",
    "color_idle",
    "all_sessions",
    "orientation",
    "show_zero",
    "blink_waiting",
    "status_dir",
    "poll_interval_secs",
    "stale_after_secs",
    "key_up",
    "key_down",
    "key_select",
    "key_close",
    "key_filter",
    "key_jump_waiting",
    "show_state_text",
    "show_cwd",
];

/// Which UI the plugin instance renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Picker,
    Bar,
}

/// Built-in icon set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSet {
    Nerd,
    Unicode,
}

/// Bar layout direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Auto,
    Horizontal,
    Vertical,
}

/// Parsed plugin configuration with defaults applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub mode: Mode,
    pub icons: IconSet,
    pub icon_working: Option<String>,
    pub icon_waiting: Option<String>,
    pub icon_idle: Option<String>,
    pub icon_session: Option<String>,
    pub color_working: Option<String>,
    pub color_waiting: Option<String>,
    pub color_idle: Option<String>,
    pub all_sessions: bool,
    pub orientation: Orientation,
    pub show_zero: bool,
    pub blink_waiting: bool,
    /// `None` means the default `$XDG_CACHE_HOME/claude-zellij`, see [`resolve_status_dir`].
    pub status_dir: Option<PathBuf>,
    pub poll_interval_secs: f64,
    pub stale_after_secs: u64,
    pub key_up: Vec<String>,
    pub key_down: Vec<String>,
    pub key_select: Vec<String>,
    pub key_close: Vec<String>,
    pub key_filter: Vec<String>,
    pub key_jump_waiting: Vec<String>,
    pub show_state_text: bool,
    pub show_cwd: bool,
}

impl Config {
    /// Defaults for the given mode (`all_sessions` differs between picker and bar).
    pub fn defaults(mode: Mode) -> Self {
        Self {
            mode,
            icons: IconSet::Nerd,
            icon_working: None,
            icon_waiting: None,
            icon_idle: None,
            icon_session: None,
            color_working: None,
            color_waiting: None,
            color_idle: None,
            all_sessions: mode == Mode::Picker,
            orientation: Orientation::Auto,
            show_zero: true,
            blink_waiting: false,
            status_dir: None,
            poll_interval_secs: 2.0,
            stale_after_secs: 3600,
            key_up: list(&["k", "Up"]),
            key_down: list(&["j", "Down"]),
            key_select: list(&["Enter"]),
            key_close: list(&["Esc", "q"]),
            key_filter: list(&["/"]),
            key_jump_waiting: list(&["w"]),
            show_state_text: true,
            show_cwd: true,
        }
    }

    /// Parses Zellij's string key/value plugin configuration.
    ///
    /// Never fails: invalid values keep their default and unknown keys are ignored,
    /// both reported as warnings to show in the plugin pane.
    pub fn parse(raw: &BTreeMap<String, String>) -> (Self, Vec<ConfigWarning>) {
        let mut warnings = Vec::new();
        let mode = match raw.get("mode") {
            Some(v) => parse_mode(v).unwrap_or_else(|| {
                warnings.push(ConfigWarning::invalid("mode", v, "picker | bar"));
                Mode::Picker
            }),
            None => Mode::Picker,
        };
        let mut config = Self::defaults(mode);

        for (key, value) in raw {
            let value = value.trim();
            let result = match key.as_str() {
                "mode" => Ok(()),
                "icons" => parse_icons(value)
                    .map(|v| config.icons = v)
                    .ok_or("nerd | unicode"),
                "icon_working" => set_text(&mut config.icon_working, value),
                "icon_waiting" => set_text(&mut config.icon_waiting, value),
                "icon_idle" => set_text(&mut config.icon_idle, value),
                "icon_session" => set_text(&mut config.icon_session, value),
                "color_working" => set_text(&mut config.color_working, value),
                "color_waiting" => set_text(&mut config.color_waiting, value),
                "color_idle" => set_text(&mut config.color_idle, value),
                "all_sessions" => parse_bool(value).map(|v| config.all_sessions = v),
                "orientation" => parse_orientation(value)
                    .map(|v| config.orientation = v)
                    .ok_or("auto | horizontal | vertical"),
                "show_zero" => parse_bool(value).map(|v| config.show_zero = v),
                "blink_waiting" => parse_bool(value).map(|v| config.blink_waiting = v),
                "status_dir" => {
                    if value.is_empty() {
                        Err("a directory path")
                    } else {
                        config.status_dir = Some(PathBuf::from(value));
                        Ok(())
                    }
                }
                "poll_interval_secs" => match value.parse::<f64>() {
                    Ok(v) if v.is_finite() && v > 0.0 => {
                        config.poll_interval_secs = v;
                        Ok(())
                    }
                    _ => Err("a positive number"),
                },
                "stale_after_secs" => value
                    .parse::<u64>()
                    .map(|v| config.stale_after_secs = v)
                    .map_err(|_| "a non-negative integer"),
                "key_up" => set_keys(&mut config.key_up, value),
                "key_down" => set_keys(&mut config.key_down, value),
                "key_select" => set_keys(&mut config.key_select, value),
                "key_close" => set_keys(&mut config.key_close, value),
                "key_filter" => set_keys(&mut config.key_filter, value),
                "key_jump_waiting" => set_keys(&mut config.key_jump_waiting, value),
                "show_state_text" => parse_bool(value).map(|v| config.show_state_text = v),
                "show_cwd" => parse_bool(value).map(|v| config.show_cwd = v),
                _ => {
                    warnings.push(ConfigWarning::UnknownKey(key.clone()));
                    Ok(())
                }
            };
            if let Err(expected) = result {
                warnings.push(ConfigWarning::invalid(key, value, expected));
            }
        }

        (config, warnings)
    }
}

/// Resolves the status file directory: explicit config, else `$XDG_CACHE_HOME/claude-zellij`,
/// else `$HOME/.cache/claude-zellij`. `env` is the Zellij session environment.
pub fn resolve_status_dir(config: &Config, env: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(dir) = &config.status_dir {
        return Some(expand_home(dir, env));
    }
    let non_empty = |k: &str| env.get(k).filter(|v| !v.is_empty());
    non_empty("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| non_empty("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .map(|base| base.join("claude-zellij"))
}

fn expand_home(dir: &std::path::Path, env: &BTreeMap<String, String>) -> PathBuf {
    match (dir.strip_prefix("~"), env.get("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => dir.to_path_buf(),
    }
}

/// A problem found while parsing the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigWarning {
    UnknownKey(String),
    InvalidValue {
        key: String,
        value: String,
        expected: &'static str,
    },
}

impl ConfigWarning {
    fn invalid(key: &str, value: &str, expected: &'static str) -> Self {
        Self::InvalidValue {
            key: key.to_string(),
            value: value.to_string(),
            expected,
        }
    }
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(key) => write!(f, "unknown config key `{key}` ignored"),
            Self::InvalidValue {
                key,
                value,
                expected,
            } => write!(
                f,
                "invalid value `{value}` for `{key}` (expected {expected}), using default"
            ),
        }
    }
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn parse_mode(v: &str) -> Option<Mode> {
    match v.trim().to_ascii_lowercase().as_str() {
        "picker" => Some(Mode::Picker),
        "bar" => Some(Mode::Bar),
        _ => None,
    }
}

fn parse_icons(v: &str) -> Option<IconSet> {
    match v.to_ascii_lowercase().as_str() {
        "nerd" => Some(IconSet::Nerd),
        "unicode" => Some(IconSet::Unicode),
        _ => None,
    }
}

fn parse_orientation(v: &str) -> Option<Orientation> {
    match v.to_ascii_lowercase().as_str() {
        "auto" => Some(Orientation::Auto),
        "horizontal" => Some(Orientation::Horizontal),
        "vertical" => Some(Orientation::Vertical),
        _ => None,
    }
}

fn parse_bool(v: &str) -> Result<bool, &'static str> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err("true | false"),
    }
}

fn set_text(slot: &mut Option<String>, v: &str) -> Result<(), &'static str> {
    if v.is_empty() {
        return Err("a non-empty string");
    }
    *slot = Some(v.to_string());
    Ok(())
}

fn set_keys(slot: &mut Vec<String>, v: &str) -> Result<(), &'static str> {
    let keys: Vec<String> = v
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_string)
        .collect();
    if keys.is_empty() {
        return Err("a comma-separated key list");
    }
    *slot = keys;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn empty_config_gives_picker_defaults() {
        let (config, warnings) = Config::parse(&BTreeMap::new());

        assert_eq!(config, Config::defaults(Mode::Picker));
        assert!(config.all_sessions);
        assert!(warnings.is_empty());
    }

    #[test]
    fn bar_mode_defaults_to_current_session_only() {
        let (config, warnings) = Config::parse(&raw(&[("mode", "bar")]));

        assert_eq!(config.mode, Mode::Bar);
        assert!(!config.all_sessions);
        assert!(warnings.is_empty());
    }

    #[test]
    fn explicit_all_sessions_overrides_mode_default() {
        let (config, _) = Config::parse(&raw(&[("mode", "bar"), ("all_sessions", "true")]));

        assert!(config.all_sessions);
    }

    #[test]
    fn parses_all_value_types() {
        let (config, warnings) = Config::parse(&raw(&[
            ("icons", "unicode"),
            ("icon_waiting", "!"),
            ("orientation", "Vertical"),
            ("show_zero", "false"),
            ("poll_interval_secs", "0.5"),
            ("stale_after_secs", "60"),
            ("key_up", " K , Up ,"),
            ("status_dir", "/tmp/x"),
        ]));

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.icons, IconSet::Unicode);
        assert_eq!(config.icon_waiting.as_deref(), Some("!"));
        assert_eq!(config.orientation, Orientation::Vertical);
        assert!(!config.show_zero);
        assert_eq!(config.poll_interval_secs, 0.5);
        assert_eq!(config.stale_after_secs, 60);
        assert_eq!(config.key_up, vec!["K", "Up"]);
        assert_eq!(config.status_dir, Some(PathBuf::from("/tmp/x")));
    }

    #[test]
    fn invalid_values_fall_back_and_warn() {
        let (config, warnings) = Config::parse(&raw(&[
            ("mode", "sidebar"),
            ("show_zero", "maybe"),
            ("poll_interval_secs", "-1"),
            ("key_close", " , "),
        ]));

        assert_eq!(config, Config::defaults(Mode::Picker));
        assert_eq!(warnings.len(), 4);
        assert!(warnings
            .iter()
            .all(|w| matches!(w, ConfigWarning::InvalidValue { .. })));
    }

    #[test]
    fn unknown_key_warns() {
        let (_, warnings) = Config::parse(&raw(&[("colour_idle", "red")]));

        assert_eq!(
            warnings,
            vec![ConfigWarning::UnknownKey("colour_idle".into())]
        );
        assert_eq!(
            warnings[0].to_string(),
            "unknown config key `colour_idle` ignored"
        );
    }

    #[test]
    fn every_known_key_is_accepted() {
        for key in KEYS {
            let (_, warnings) = Config::parse(&raw(&[(key, "x")]));
            assert!(
                !warnings.contains(&ConfigWarning::UnknownKey(key.to_string())),
                "{key} not handled"
            );
        }
    }

    #[test]
    fn status_dir_prefers_config_then_xdg_then_home() {
        let env = raw(&[("HOME", "/home/u"), ("XDG_CACHE_HOME", "/xdg")]);
        let mut config = Config::defaults(Mode::Bar);

        assert_eq!(
            resolve_status_dir(&config, &env),
            Some(PathBuf::from("/xdg/claude-zellij"))
        );
        assert_eq!(
            resolve_status_dir(&config, &raw(&[("HOME", "/home/u")])),
            Some(PathBuf::from("/home/u/.cache/claude-zellij"))
        );
        assert_eq!(resolve_status_dir(&config, &BTreeMap::new()), None);

        config.status_dir = Some(PathBuf::from("~/status"));
        assert_eq!(
            resolve_status_dir(&config, &env),
            Some(PathBuf::from("/home/u/status"))
        );
    }
}
