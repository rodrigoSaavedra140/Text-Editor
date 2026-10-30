//! Undo/Redo stack with idle chunking and $O(1)$ Copy-On-Write Rope snapshots.

use ropey::Rope;
use std::time::{Duration, Instant};

/// Timeout to group consecutive typing into a single undo step (~700ms).
pub const CHUNK_TIMEOUT: Duration = Duration::from_millis(700);

/// Action classification to determine chunk grouping behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    Typing,
    Enter,
    Paste,
    LostFocus,
    Save,
}

/// Structural actions break the chunk immediately.
pub fn should_snapshot_immediately(action: EditAction) -> bool {
    match action {
        EditAction::Enter | EditAction::Paste | EditAction::LostFocus | EditAction::Save => true,
        EditAction::Typing => false,
    }
}

/// Tracks inactivity timeouts to chunk user typing strokes.
pub struct ChunkTracker {
    pub last_edit_time: Option<Instant>,
    pub timeout: Duration,
}

impl Default for ChunkTracker {
    fn default() -> Self {
        Self::new(CHUNK_TIMEOUT)
    }
}

impl ChunkTracker {
    pub fn new(timeout: Duration) -> Self {
        Self {
            last_edit_time: None,
            timeout,
        }
    }

    pub fn default_timeout() -> Self {
        Self::default()
    }

    pub fn on_edit(&mut self, action: EditAction, now: Instant) -> bool {
        self.last_edit_time = Some(now);
        should_snapshot_immediately(action)
    }

    pub fn check_idle_timeout(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_edit_time {
            if now.duration_since(last) >= self.timeout {
                self.last_edit_time = None;
                return true;
            }
        }
        false
    }

    pub fn remaining_time(&self, now: Instant) -> Option<Duration> {
        let last = self.last_edit_time?;
        let elapsed = now.duration_since(last);
        Some(self.timeout.saturating_sub(elapsed))
    }

    pub fn reset(&mut self) {
        self.last_edit_time = None;
    }
}

/// Maximum number of undo steps preserved in history.
pub const MAX_UNDO_EDITS: usize = 200;

/// A reversible edit step holding before and after state snapshots via $O(1)$ COW Rope trees.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub before: Rope,
    pub after: Rope,
}

impl Edit {
    pub fn new(before: Rope, after: Rope) -> Self {
        Self { before, after }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UndoStack {
    pub stack: Vec<Edit>,
    pub redo_stack: Vec<Edit>,
}

impl UndoStack {
    pub fn new() -> Self {
        UndoStack {
            stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn push(&mut self, edit: Edit) {
        self.stack.push(edit);
        let overflow = self.stack.len().saturating_sub(MAX_UNDO_EDITS);
        if overflow > 0 {
            self.stack.drain(0..overflow);
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> Option<Edit> {
        let edit = self.stack.pop()?;
        self.redo_stack.push(Edit {
            before: edit.before.clone(),
            after: edit.after.clone(),
        });
        Some(edit)
    }

    pub fn redo(&mut self) -> Option<Edit> {
        let edit = self.redo_stack.pop()?;
        self.stack.push(Edit {
            before: edit.before.clone(),
            after: edit.after.clone(),
        });
        Some(edit)
    }

    pub fn can_undo(&self) -> bool {
        !self.stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.stack.clear();
        self.redo_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(n: usize) -> Edit {
        Edit {
            before: Rope::from_str(&format!("before-{}", n)),
            after: Rope::from_str(&format!("after-{}", n)),
        }
    }

    #[test]
    fn test_undo_stack_capacity_limit() {
        let mut stack = UndoStack::new();
        for i in 0..(MAX_UNDO_EDITS + 10) {
            stack.push(edit(i));
        }
        let mut undone = Vec::new();
        while let Some(e) = stack.undo() {
            undone.push(e.before.to_string());
        }
        assert_eq!(undone.len(), MAX_UNDO_EDITS);
        assert_eq!(&undone[0], &format!("before-{}", MAX_UNDO_EDITS + 9));
        assert_eq!(undone.last().map(|s| s.as_str()), Some(format!("before-10").as_str()));
    }

    #[test]
    fn test_push_clears_redo() {
        let mut stack = UndoStack::new();
        stack.push(edit(0));
        stack.undo();
        assert!(stack.can_redo());
        stack.push(edit(1));
        assert!(!stack.can_redo());
    }

    #[test]
    fn test_undo_and_redo_roundtrip() {
        let mut stack = UndoStack::new();
        stack.push(edit(7));
        let undone = stack.undo().expect("should undo");
        assert_eq!(undone.before.to_string(), "before-7");
        let redone = stack.redo().expect("should redo");
        assert_eq!(redone.after.to_string(), "after-7");
        assert!(stack.can_undo());
    }

    #[test]
    fn test_snapshots_share_cow_rope_nodes() {
        let mut stack = UndoStack::new();
        let s1 = Rope::from_str("state 1");
        let s2 = Rope::from_str("state 2");
        stack.push(Edit {
            before: s1.clone(),
            after: s2.clone(),
        });
        let s3 = Rope::from_str("state 3");
        stack.push(Edit {
            before: s2.clone(),
            after: s3.clone(),
        });
        assert_eq!(stack.stack.len(), 2);
        assert_eq!(stack.stack[0].after, stack.stack[1].before);
    }

    #[test]
    fn test_chunk_tracker_timeout() {
        let mut tracker = ChunkTracker::default_timeout();
        let t0 = Instant::now();

        assert!(!tracker.on_edit(EditAction::Typing, t0));

        let t_early = t0 + Duration::from_millis(300);
        assert!(!tracker.check_idle_timeout(t_early));
        assert!(tracker.remaining_time(t_early).unwrap() <= Duration::from_millis(400));

        let t_late = t0 + Duration::from_millis(700);
        assert!(tracker.check_idle_timeout(t_late));
        assert!(!tracker.check_idle_timeout(t_late + Duration::from_millis(10)));
    }

    #[test]
    fn test_chunk_tracker_structural_action_immediate() {
        let mut tracker = ChunkTracker::default_timeout();
        let now = Instant::now();

        assert!(tracker.on_edit(EditAction::Enter, now));
        assert!(tracker.on_edit(EditAction::Paste, now));
        assert!(tracker.on_edit(EditAction::LostFocus, now));
        assert!(tracker.on_edit(EditAction::Save, now));
    }
}
