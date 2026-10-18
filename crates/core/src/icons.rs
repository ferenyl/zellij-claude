use crate::config::{Config, IconSet};
use crate::hook_map::SessionState;

/// Resolved icons: the configured set with per-icon overrides applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icons {
    pub working: String,
    pub waiting: String,
    pub idle: String,
    pub session: String,
}

impl Default for Icons {
    fn default() -> Self {
        Self::from_config(&Config::default())
    }
}

impl Icons {
    pub fn from_config(config: &Config) -> Self {
        let (working, waiting, idle, session) = match config.icons {
            IconSet::Nerd => ("\u{f110}", "\u{f059}", "\u{f00c}", "\u{f06a9}"),
            IconSet::Unicode => ("●", "▲", "✔", "◆"),
        };
        let pick = |custom: &Option<String>, default: &str| {
            custom.clone().unwrap_or_else(|| default.to_string())
        };
        Self {
            working: pick(&config.icon_working, working),
            waiting: pick(&config.icon_waiting, waiting),
            idle: pick(&config.icon_idle, idle),
            session: pick(&config.icon_session, session),
        }
    }

    pub fn for_state(&self, state: SessionState) -> &str {
        match state {
            SessionState::Working => &self.working,
            SessionState::Waiting => &self.waiting,
            SessionState::Idle => &self.idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mode;

    #[test]
    fn overrides_replace_set_defaults() {
        let mut config = Config::defaults(Mode::Picker);
        config.icons = IconSet::Unicode;
        config.icon_idle = Some("ok".into());

        let icons = Icons::from_config(&config);

        assert_eq!(icons.for_state(SessionState::Working), "●");
        assert_eq!(icons.for_state(SessionState::Idle), "ok");
    }
}
