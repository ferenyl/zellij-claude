use std::collections::{BTreeMap, BTreeSet};

use crate::hook_map::{event_to_state, SessionState, StateChange};
use crate::status_file::StatusRecord;

/// A pane missing from a live session is kept this long after its last update,
/// since `SessionUpdate` can lag behind a newly opened pane.
const NEW_PANE_GRACE_SECS: u64 = 10;

/// Longer grace for other sessions, whose metadata arrives via the session scan.
const OTHER_SESSION_GRACE_SECS: u64 = 30;

/// Identifies a Claude session by its Zellij session name and terminal pane id.
pub type SessionKey = (String, u32);

/// A Zellij session as reported by `SessionUpdate`, converted by the plugin shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub name: String,
    pub is_current: bool,
    pub tabs: Vec<LiveTab>,
    pub panes: Vec<LivePane>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTab {
    pub position: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePane {
    pub id: u32,
    pub tab_position: usize,
    pub title: String,
    pub is_plugin: bool,
    pub is_floating: bool,
    pub exited: bool,
    /// Command of a command pane; `None` for shell panes.
    pub command: Option<String>,
}

/// A tracked Claude session joined with live Zellij data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSession {
    pub zellij_session: String,
    pub pane_id: u32,
    pub state: SessionState,
    pub cwd: Option<String>,
    pub claude_session_id: Option<String>,
    pub updated_at: u64,
    /// The Zellij session is running; otherwise it is gone, renamed or only resurrectable.
    pub is_live: bool,
    pub is_current_session: bool,
    pub tab_position: Option<usize>,
    pub tab_name: Option<String>,
    pub title: Option<String>,
    pub is_floating: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    state: SessionState,
    cwd: Option<String>,
    claude_session_id: Option<String>,
    updated_at: u64,
}

/// Tracks Claude sessions from pipe messages and status files.
#[derive(Debug, Default)]
pub struct Store {
    entries: BTreeMap<SessionKey, Entry>,
    /// `SessionEnd` timestamps, so an older status file read cannot revive an ended session.
    ended: BTreeMap<SessionKey, u64>,
}

impl Store {
    /// Applies one update (pipe message or status file). The newest `updated_at` wins;
    /// on a tie the update applied last wins, except that a tool event never replaces
    /// idle or waiting from the same second: pipe messages are sent in the background and can
    /// arrive after the `Stop` that followed them.
    pub fn apply(&mut self, record: StatusRecord) {
        let Some(change) = event_to_state(&record.hook_event, record.notification_type.as_deref())
        else {
            return;
        };
        let key = (record.zellij_session, record.pane_id);
        let is_tool_event = matches!(
            record.hook_event.as_str(),
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
        );
        if is_tool_event
            && self.entries.get(&key).is_some_and(|e| {
                e.updated_at == record.updated_at
                    && matches!(e.state, SessionState::Idle | SessionState::Waiting)
            })
        {
            return;
        }
        let newest_known = self
            .entries
            .get(&key)
            .map(|e| e.updated_at)
            .into_iter()
            .chain(self.ended.get(&key).copied())
            .max();
        if newest_known.is_some_and(|t| record.updated_at < t) {
            return;
        }

        match change {
            StateChange::Ended => {
                self.entries.remove(&key);
                self.ended.insert(key, record.updated_at);
            }
            StateChange::Set(state) => {
                self.ended.remove(&key);
                self.entries.insert(
                    key,
                    Entry {
                        state,
                        cwd: record.cwd,
                        claude_session_id: record.claude_session_id,
                        updated_at: record.updated_at,
                    },
                );
            }
        }
    }

    /// Applies a read of the status directory. Sessions without a file are dropped,
    /// since the hook writes the file before sending the pipe message. A partial read
    /// (`complete == false`) only applies the records.
    pub fn apply_snapshot(&mut self, records: Vec<StatusRecord>, complete: bool) {
        if !complete {
            for record in records {
                self.apply(record);
            }
            return;
        }
        let present: BTreeSet<SessionKey> = records
            .iter()
            .map(|r| (r.zellij_session.clone(), r.pane_id))
            .collect();
        self.entries.retain(|key, _| present.contains(key));
        self.ended.retain(|key, _| present.contains(key));
        for record in records {
            self.apply(record);
        }
    }

    /// Removes sessions whose pane is gone. In a live Zellij session the pane must exist
    /// as a running terminal (after a short grace period for new panes); in a session
    /// that is not live, entries expire after `stale_after_secs`. `live` is `None` until the
    /// session list is complete; nothing is removed then. Returns the removed keys.
    pub fn prune(
        &mut self,
        live: Option<&[LiveSession]>,
        now: u64,
        stale_after_secs: u64,
    ) -> Vec<SessionKey> {
        let Some(live) = live else {
            return Vec::new();
        };
        let mut removed = Vec::new();
        self.entries.retain(|key, entry| {
            let keep = match live.iter().find(|s| s.name == key.0) {
                Some(session) => {
                    let grace = if session.is_current {
                        NEW_PANE_GRACE_SECS
                    } else {
                        OTHER_SESSION_GRACE_SECS
                    };
                    find_terminal(session, key.1).is_some()
                        || now.saturating_sub(entry.updated_at) < grace
                }
                None => now.saturating_sub(entry.updated_at) < stale_after_secs,
            };
            if !keep {
                removed.push(key.clone());
            }
            keep
        });
        removed
    }

    /// All tracked sessions joined with live tab and pane data, ordered by key.
    pub fn sessions(&self, live: &[LiveSession]) -> Vec<ClaudeSession> {
        self.entries
            .iter()
            .map(|((zellij_session, pane_id), entry)| join(live, zellij_session, *pane_id, entry))
            .collect()
    }

    /// Fallback for setups without hooks: untracked running terminal panes whose
    /// title or command mentions Claude. Their state is unknown and shown as idle.
    pub fn detected(&self, live: &[LiveSession]) -> Vec<ClaudeSession> {
        let entry = Entry {
            state: SessionState::Idle,
            cwd: None,
            claude_session_id: None,
            updated_at: 0,
        };
        live.iter()
            .flat_map(|session| {
                session
                    .panes
                    .iter()
                    .filter(|p| !p.is_plugin && !p.exited && looks_like_claude(p))
                    .filter(|p| !self.entries.contains_key(&(session.name.clone(), p.id)))
                    .map(|p| join(live, &session.name, p.id, &entry))
            })
            .collect()
    }
}

fn looks_like_claude(pane: &LivePane) -> bool {
    [Some(pane.title.as_str()), pane.command.as_deref()]
        .into_iter()
        .flatten()
        .any(|text| text.to_lowercase().contains("claude"))
}

fn join(live: &[LiveSession], zellij_session: &str, pane_id: u32, entry: &Entry) -> ClaudeSession {
    let session = live.iter().find(|s| s.name == zellij_session);
    let pane = session.and_then(|s| find_terminal(s, pane_id));
    let tab_name = session.zip(pane).and_then(|(s, p)| {
        s.tabs
            .iter()
            .find(|t| t.position == p.tab_position)
            .map(|t| t.name.clone())
    });
    ClaudeSession {
        zellij_session: zellij_session.to_string(),
        pane_id,
        state: entry.state,
        cwd: entry.cwd.clone(),
        claude_session_id: entry.claude_session_id.clone(),
        updated_at: entry.updated_at,
        is_live: session.is_some(),
        is_current_session: session.is_some_and(|s| s.is_current),
        tab_position: pane.map(|p| p.tab_position),
        tab_name,
        title: pane.map(|p| p.title.clone()),
        is_floating: pane.is_some_and(|p| p.is_floating),
    }
}

fn find_terminal(session: &LiveSession, pane_id: u32) -> Option<&LivePane> {
    session
        .panes
        .iter()
        .find(|p| p.id == pane_id && !p.is_plugin && !p.exited)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session: &str, pane: u32, event: &str, at: u64) -> StatusRecord {
        StatusRecord {
            zellij_session: session.into(),
            pane_id: pane,
            hook_event: event.into(),
            notification_type: None,
            cwd: Some("/code".into()),
            claude_session_id: None,
            updated_at: at,
        }
    }

    fn pane(id: u32, tab: usize) -> LivePane {
        LivePane {
            id,
            tab_position: tab,
            title: format!("pane {id}"),
            is_plugin: false,
            is_floating: false,
            exited: false,
            command: None,
        }
    }

    fn live(name: &str, is_current: bool, panes: Vec<LivePane>) -> LiveSession {
        LiveSession {
            name: name.into(),
            is_current,
            tabs: vec![
                LiveTab {
                    position: 0,
                    name: "backend".into(),
                },
                LiveTab {
                    position: 1,
                    name: "frontend".into(),
                },
            ],
            panes,
        }
    }

    fn states(store: &Store) -> Vec<(String, u32, SessionState)> {
        store
            .sessions(&[])
            .into_iter()
            .map(|s| (s.zellij_session, s.pane_id, s.state))
            .collect()
    }

    #[test]
    fn newer_update_wins_and_older_is_ignored() {
        let mut store = Store::default();

        store.apply(record("work", 1, "UserPromptSubmit", 10));
        store.apply(record("work", 1, "Stop", 5));
        store.apply(record("work", 1, "Notification", 10));

        assert_eq!(
            states(&store),
            vec![("work".into(), 1, SessionState::Waiting)]
        );
    }

    #[test]
    fn tool_event_does_not_replace_idle_from_same_second() {
        let mut store = Store::default();

        store.apply(record("work", 1, "Stop", 10));
        store.apply(record("work", 1, "PreToolUse", 10));

        assert_eq!(states(&store), vec![("work".into(), 1, SessionState::Idle)]);
    }

    #[test]
    fn tool_event_does_not_replace_waiting_from_same_second() {
        let mut store = Store::default();

        store.apply(record("work", 1, "PermissionRequest", 10));
        store.apply(record("work", 1, "PreToolUse", 10));

        assert_eq!(
            states(&store),
            vec![("work".into(), 1, SessionState::Waiting)]
        );
    }

    #[test]
    fn prompt_replaces_idle_from_same_second() {
        let mut store = Store::default();

        store.apply(record("work", 1, "Stop", 10));
        store.apply(record("work", 1, "UserPromptSubmit", 10));

        assert_eq!(
            states(&store),
            vec![("work".into(), 1, SessionState::Working)]
        );
    }

    #[test]
    fn idle_prompt_notification_clears_working() {
        let mut store = Store::default();
        store.apply(record("work", 1, "PreToolUse", 10));

        store.apply(StatusRecord {
            notification_type: Some("idle_prompt".into()),
            ..record("work", 1, "Notification", 70)
        });

        assert_eq!(states(&store), vec![("work".into(), 1, SessionState::Idle)]);
    }

    #[test]
    fn unknown_event_is_ignored() {
        let mut store = Store::default();

        store.apply(record("work", 1, "SubagentStop", 10));

        assert!(states(&store).is_empty());
    }

    #[test]
    fn session_end_removes_and_blocks_older_snapshot() {
        let mut store = Store::default();
        store.apply(record("work", 1, "Stop", 10));

        store.apply(record("work", 1, "SessionEnd", 20));
        store.apply_snapshot(vec![record("work", 1, "Stop", 10)], true);

        assert!(states(&store).is_empty());
    }

    #[test]
    fn new_session_in_same_second_as_end_is_tracked() {
        let mut store = Store::default();
        store.apply(record("work", 1, "SessionEnd", 20));

        store.apply(record("work", 1, "SessionStart", 20));

        assert_eq!(states(&store), vec![("work".into(), 1, SessionState::Idle)]);
    }

    #[test]
    fn snapshot_drops_sessions_without_file() {
        let mut store = Store::default();
        store.apply(record("work", 1, "Stop", 10));
        store.apply(record("work", 2, "Stop", 10));

        store.apply_snapshot(vec![record("work", 2, "UserPromptSubmit", 11)], true);

        assert_eq!(
            states(&store),
            vec![("work".into(), 2, SessionState::Working)]
        );
    }

    #[test]
    fn prune_removes_closed_and_plugin_panes_in_live_session() {
        let mut store = Store::default();
        store.apply(record("work", 1, "Stop", 10));
        store.apply(record("work", 2, "Stop", 10));
        store.apply(record("work", 3, "Stop", 10));
        let mut plugin = pane(2, 0);
        plugin.is_plugin = true;
        let mut exited = pane(3, 0);
        exited.exited = true;

        let removed = store.prune(
            Some(&[live("work", true, vec![pane(1, 0), plugin, exited])]),
            100,
            3600,
        );

        assert_eq!(removed, vec![("work".into(), 2), ("work".into(), 3)]);
        assert_eq!(states(&store), vec![("work".into(), 1, SessionState::Idle)]);
    }

    #[test]
    fn prune_keeps_fresh_entry_whose_pane_is_not_reported_yet() {
        let mut store = Store::default();
        store.apply(record("work", 9, "SessionStart", 100));

        let removed = store.prune(Some(&[live("work", true, vec![pane(1, 0)])]), 105, 3600);

        assert!(removed.is_empty());
        assert_eq!(
            store
                .prune(Some(&[live("work", true, vec![pane(1, 0)])]), 110, 3600)
                .len(),
            1
        );
    }

    #[test]
    fn detects_untracked_claude_panes_by_title_or_command() {
        let mut store = Store::default();
        store.apply(record("work", 1, "Stop", 10));
        let mut tracked = pane(1, 0);
        tracked.title = "claude".into();
        let mut by_title = pane(2, 1);
        by_title.title = "✳ Claude Code".into();
        let mut by_command = pane(3, 0);
        by_command.command = Some("/usr/bin/claude --resume".into());
        let mut plugin = pane(4, 0);
        plugin.title = "claude-sessions".into();
        plugin.is_plugin = true;

        let detected = store.detected(&[live(
            "work",
            true,
            vec![tracked, by_title, by_command, pane(5, 0), plugin],
        )]);

        let found: Vec<_> = detected.iter().map(|s| (s.pane_id, s.state)).collect();
        assert_eq!(
            found,
            vec![(2, SessionState::Idle), (3, SessionState::Idle)]
        );
        assert_eq!(detected[0].tab_name.as_deref(), Some("frontend"));
    }

    #[test]
    fn prune_expires_entries_of_missing_sessions_after_stale_time() {
        let mut store = Store::default();
        store.apply(record("gone", 1, "Stop", 100));
        store.apply(record("gone", 2, "Stop", 50));

        let removed = store.prune(Some(&[]), 150, 60);

        assert_eq!(removed, vec![("gone".into(), 2)]);
        assert_eq!(states(&store), vec![("gone".into(), 1, SessionState::Idle)]);
    }

    #[test]
    fn sessions_join_tab_and_pane_data() {
        let mut store = Store::default();
        store.apply(record("work", 7, "Stop", 10));
        store.apply(record("other", 3, "Stop", 10));
        let mut floating = pane(7, 1);
        floating.is_floating = true;

        let sessions = store.sessions(&[live("work", true, vec![floating])]);

        let other = &sessions[0];
        assert_eq!(other.zellij_session, "other");
        assert!(!other.is_current_session);
        assert_eq!(other.tab_name, None);

        let work = &sessions[1];
        assert!(work.is_current_session);
        assert_eq!(work.tab_position, Some(1));
        assert_eq!(work.tab_name.as_deref(), Some("frontend"));
        assert_eq!(work.title.as_deref(), Some("pane 7"));
        assert!(work.is_floating);
        assert_eq!(work.cwd.as_deref(), Some("/code"));
    }

    #[test]
    fn prune_removes_nothing_until_session_list_is_complete() {
        let mut store = Store::default();
        store.apply(record("other", 1, "Stop", 1));

        let removed = store.prune(None, 100_000, 60);

        assert!(removed.is_empty());
        assert_eq!(
            states(&store),
            vec![("other".into(), 1, SessionState::Idle)]
        );
    }

    #[test]
    fn other_sessions_get_longer_grace_for_unreported_panes() {
        let mut store = Store::default();
        store.apply(record("work", 9, "SessionStart", 100));
        store.apply(record("other", 9, "SessionStart", 100));
        let sessions = [
            live("work", true, vec![pane(1, 0)]),
            live("other", false, vec![pane(1, 0)]),
        ];

        let removed = store.prune(Some(&sessions), 115, 3600);

        assert_eq!(removed, vec![("work".into(), 9)]);
    }

    #[test]
    fn partial_snapshot_keeps_entries_without_file() {
        let mut store = Store::default();
        store.apply(record("work", 1, "Stop", 10));

        store.apply_snapshot(vec![record("work", 2, "Stop", 11)], false);

        assert_eq!(store.sessions(&[]).len(), 2);
    }
}
