//! Snapshot-based undo/redo.
//!
//! The editor reports every change with a key. Changes that share a key and arrive
//! close together (typing in one field, dragging one node) merge into a single undo
//! step; every other change is its own step.

use std::time::{Duration, Instant};

use arcweave_rust::project::Project;

use super::layout::LayoutStore;
use crate::covers::Covers;

const MAX_STEPS: usize = 200;
const MERGE_WINDOW: Duration = Duration::from_millis(1200);

/// Everything an undo has to bring back.
#[derive(Clone)]
pub struct Snapshot {
    pub project: Project,
    pub covers: Covers,
    pub layout: LayoutStore,
}

pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The state as of the last committed change.
    committed: Snapshot,
    pending: Option<String>,
    last_key: Option<String>,
    last_change: Instant,
    next_action: u64,
}

impl History {
    pub fn new(initial: Snapshot) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            committed: initial,
            pending: None,
            last_key: None,
            last_change: Instant::now(),
            next_action: 0,
        }
    }

    /// A change that may merge with neighbouring changes carrying the same key.
    pub fn changed(&mut self, key: impl Into<String>) {
        self.pending = Some(key.into());
    }

    /// A discrete change that is always its own undo step.
    pub fn action(&mut self) {
        self.next_action += 1;
        self.pending = Some(format!("action:{}", self.next_action));
    }

    /// Call once per frame after the UI ran; records the change reported this frame.
    pub fn commit(&mut self, current: impl FnOnce() -> Snapshot) {
        let Some(key) = self.pending.take() else {
            return;
        };
        let now = Instant::now();
        let merge = self.last_key.as_deref() == Some(key.as_str())
            && now.duration_since(self.last_change) < MERGE_WINDOW;
        if !merge {
            self.undo.push(self.committed.clone());
            if self.undo.len() > MAX_STEPS {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.committed = current();
        self.last_key = Some(key);
        self.last_change = now;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Returns the snapshot to restore, if there is anything to undo.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.committed = previous.clone();
        self.last_key = None;
        Some(previous)
    }

    /// Returns the snapshot to restore, if there is anything to redo.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.committed = next.clone();
        self.last_key = None;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model::new_project;

    fn snap(marker: &str) -> Snapshot {
        let (mut project, _, _) = new_project("x");
        project.name = marker.to_owned();
        Snapshot {
            project,
            covers: Covers::new(),
            layout: LayoutStore::default(),
        }
    }

    fn name(s: &Snapshot) -> &str {
        &s.project.name
    }

    #[test]
    fn same_key_changes_merge_into_one_step() {
        let mut h = History::new(snap("a"));
        h.changed("title:1");
        h.commit(|| snap("b"));
        h.changed("title:1");
        h.commit(|| snap("c"));
        h.changed("title:1");
        h.commit(|| snap("d"));

        let back = h.undo(snap("d")).unwrap();
        assert_eq!(name(&back), "a", "typing burst undoes in one step");
        assert!(!h.can_undo());
    }

    #[test]
    fn different_keys_and_actions_are_separate_steps() {
        let mut h = History::new(snap("a"));
        h.changed("title:1");
        h.commit(|| snap("b"));
        h.changed("body:1");
        h.commit(|| snap("c"));
        h.action();
        h.commit(|| snap("d"));
        h.action();
        h.commit(|| snap("e"));

        assert_eq!(name(&h.undo(snap("e")).unwrap()), "d");
        assert_eq!(name(&h.undo(snap("d")).unwrap()), "c");
        assert_eq!(name(&h.undo(snap("c")).unwrap()), "b");
        assert_eq!(name(&h.undo(snap("b")).unwrap()), "a");
        assert!(h.undo(snap("a")).is_none());
    }

    #[test]
    fn redo_replays_and_new_change_clears_it() {
        let mut h = History::new(snap("a"));
        h.action();
        h.commit(|| snap("b"));
        h.action();
        h.commit(|| snap("c"));

        assert_eq!(name(&h.undo(snap("c")).unwrap()), "b");
        assert!(h.can_redo());
        assert_eq!(name(&h.redo(snap("b")).unwrap()), "c");
        assert!(!h.can_redo());

        h.undo(snap("c"));
        h.action();
        h.commit(|| snap("z"));
        assert!(!h.can_redo(), "a fresh change drops the redo stack");
    }

    #[test]
    fn nothing_recorded_without_a_change() {
        let mut h = History::new(snap("a"));
        h.commit(|| snap("b"));
        assert!(!h.can_undo());
    }
}
