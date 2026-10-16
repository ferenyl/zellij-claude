use std::collections::{BTreeMap, BTreeSet};

use crate::hook_map::{event_to_state, SessionState, StateChange};
use crate::status_file::StatusRecord;

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
    /// on a tie the update applied last wins.
    pub fn apply(&mut self, record: StatusRecord) {
        let Some(change) = event_to_state(&record.hook_event) else {
            return;
        };
        let key = (record.zellij_session, record.pane_id);
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

    /// Applies a full read of the status directory. Sessions without a file are dropped,
    /// since the hook writes the file before sending the pipe message.
    pub fn apply_snapshot(&mut self, records: Vec<StatusRecord>) {
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
    /// as a running terminal; in a session that is not live, entries expire after
    /// `stale_after_secs`. Returns the removed keys.
    pub fn prune(
        &mut self,
        live: &[LiveSession],
        now: u64,
        stale_after_secs: u64,
    ) -> Vec<SessionKey> {
        let mut removed = Vec::new();
        self.entries.retain(|key, entry| {
            let keep = match live.iter().find(|s| s.name == key.0) {
                Some(session) => find_terminal(session, key.1).is_some(),
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
            .map(|((zellij_session, pane_id), entry)| {
                let session = live.iter().find(|s| &s.name == zellij_session);
                let pane = session.and_then(|s| find_terminal(s, *pane_id));
                let tab_name = session.zip(pane).and_then(|(s, p)| {
                    s.tabs
                        .iter()
                        .find(|t| t.position == p.tab_position)
                        .map(|t| t.name.clone())
                });
                ClaudeSession {
                    zellij_session: zellij_session.clone(),
                    pane_id: *pane_id,
                    state: entry.state,
                    cwd: entry.cwd.clone(),
                    claude_session_id: entry.claude_session_id.clone(),
                    updated_at: entry.updated_at,
                    is_current_session: session.is_some_and(|s| s.is_current),
                    tab_position: pane.map(|p| p.tab_position),
                    tab_name,
                    title: pane.map(|p| p.title.clone()),
                    is_floating: pane.is_some_and(|p| p.is_floating),
                }
            })
            .collect()
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
        store.apply_snapshot(vec![record("work", 1, "Stop", 10)]);

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

        store.apply_snapshot(vec![record("work", 2, "UserPromptSubmit", 11)]);

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
            &[live("work", true, vec![pane(1, 0), plugin, exited])],
            10,
            3600,
        );

        assert_eq!(removed, vec![("work".into(), 2), ("work".into(), 3)]);
        assert_eq!(states(&store), vec![("work".into(), 1, SessionState::Idle)]);
    }

    #[test]
    fn prune_expires_entries_of_missing_sessions_after_stale_time() {
        let mut store = Store::default();
        store.apply(record("gone", 1, "Stop", 100));
        store.apply(record("gone", 2, "Stop", 50));

        let removed = store.prune(&[], 150, 60);

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
}
