use serde::Deserialize;

/// One status update written by the hook script, both as a status file
/// (`<zellij_session>-<pane_id>.json`) and as the `claude-status` pipe payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StatusRecord {
    pub zellij_session: String,
    pub pane_id: u32,
    /// Raw Claude Code hook event name, mapped by [`crate::hook_map::event_to_state`].
    pub hook_event: String,
    /// `notification_type` of a `Notification` event, e.g. `idle_prompt`.
    #[serde(default)]
    pub notification_type: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub claude_session_id: Option<String>,
    /// Unix seconds.
    pub updated_at: u64,
}

impl StatusRecord {
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }
}

/// File name used for a pane's status file.
pub fn file_name(zellij_session: &str, pane_id: u32) -> String {
    format!("{zellij_session}-{pane_id}.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_record() {
        let json = r#"{
            "zellij_session": "work",
            "pane_id": 12,
            "hook_event": "Notification",
            "cwd": "/home/user/project",
            "claude_session_id": "abc123",
            "updated_at": 1760558400
        }"#;

        let record = StatusRecord::parse(json).unwrap();

        assert_eq!(record.zellij_session, "work");
        assert_eq!(record.pane_id, 12);
        assert_eq!(record.hook_event, "Notification");
        assert_eq!(record.cwd.as_deref(), Some("/home/user/project"));
        assert_eq!(record.updated_at, 1760558400);
    }

    #[test]
    fn optional_fields_and_unknown_fields_are_tolerated() {
        let json =
            r#"{"zellij_session":"s","pane_id":1,"hook_event":"Stop","updated_at":1,"extra":true}"#;

        let record = StatusRecord::parse(json).unwrap();

        assert_eq!(record.notification_type, None);
        assert_eq!(record.cwd, None);
        assert_eq!(record.claude_session_id, None);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(StatusRecord::parse("{").is_err());
        assert!(StatusRecord::parse(r#"{"pane_id":1}"#).is_err());
    }

    #[test]
    fn file_name_combines_session_and_pane() {
        assert_eq!(file_name("work", 12), "work-12.json");
    }
}
