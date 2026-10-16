/// Status of a Claude Code session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SessionState {
    /// Claude is processing a prompt.
    Working,
    /// Claude needs user input or a permission approval.
    Waiting,
    /// Claude finished and is ready for the next prompt.
    Idle,
}

impl SessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Idle => "idle",
        }
    }
}

/// Effect of a Claude Code hook event on a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateChange {
    Set(SessionState),
    Ended,
}

/// The single place mapping Claude Code hook event names to states.
/// Unknown events return `None` and leave the session unchanged.
pub fn event_to_state(hook_event: &str) -> Option<StateChange> {
    use SessionState::*;
    let change = match hook_event {
        "SessionStart" | "Stop" => StateChange::Set(Idle),
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
            StateChange::Set(Working)
        }
        "Notification" | "PermissionRequest" => StateChange::Set(Waiting),
        "SessionEnd" => StateChange::Ended,
        _ => return None,
    };
    Some(change)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_lifecycle_events() {
        let cases = [
            ("SessionStart", StateChange::Set(SessionState::Idle)),
            ("UserPromptSubmit", StateChange::Set(SessionState::Working)),
            ("PreToolUse", StateChange::Set(SessionState::Working)),
            ("PostToolUse", StateChange::Set(SessionState::Working)),
            ("Notification", StateChange::Set(SessionState::Waiting)),
            ("PermissionRequest", StateChange::Set(SessionState::Waiting)),
            ("Stop", StateChange::Set(SessionState::Idle)),
            ("SessionEnd", StateChange::Ended),
        ];

        for (event, expected) in cases {
            assert_eq!(event_to_state(event), Some(expected), "{event}");
        }
    }

    #[test]
    fn unknown_event_is_ignored() {
        assert_eq!(event_to_state("SubagentStop"), None);
        assert_eq!(event_to_state(""), None);
    }
}
