use crate::midi::{NoteId, Project};
use std::collections::HashSet;

/// Maximum number of undo/redo states to keep.
const MAX_HISTORY_SIZE: usize = 8;

/// A snapshot of the application state at a point in time.
///
/// Contains all data needed to restore the application to a previous state.
/// This includes the full project data plus UI selection state that directly
/// relates to editing operations.
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    /// The complete project state (tracks, notes, tempo, etc.).
    pub project: Project,

    /// Currently selected track index.
    pub selected_track_index: usize,

    /// Currently selected notes (by ID).
    /// Stored to maintain selection context across undo/redo.
    pub selected_notes: HashSet<NoteId>,

    /// A brief description of what operation created this snapshot.
    /// Used for status messages when undoing/redoing.
    pub description: String,
}

impl StateSnapshot {
    /// Creates a new snapshot from the current state.
    ///
    /// # Arguments
    ///
    /// * `project` - The current project state
    /// * `selected_track_index` - The currently selected track
    /// * `selected_notes` - The currently selected notes
    /// * `description` - A brief description of the operation
    pub fn new(
        project: &Project,
        selected_track_index: usize,
        selected_notes: &HashSet<NoteId>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            project: project.clone(),
            selected_track_index,
            selected_notes: selected_notes.clone(),
            description: description.into(),
        }
    }

    /// Validates that the snapshot can be safely restored.
    ///
    /// Checks invariants like:
    /// - Selected track index is within bounds
    /// - Selected notes exist in the project
    ///
    /// # Returns
    ///
    /// true if the snapshot is valid and can be restored
    pub fn is_valid(&self) -> bool {
        // If there are no tracks, index 0 is still valid (will be clamped)
        if self.project.track_count() > 0 && self.selected_track_index >= self.project.track_count()
        {
            return false;
        }

        // Selected notes will be filtered during restoration via valid_selected_notes()
        // so we don't need to validate them here - just ensure the project structure is sound

        true
    }

    /// Returns a copy of selected_notes filtered to only include notes that
    /// still exist in the project.
    ///
    /// This handles cases where notes were deleted in a redo operation
    /// but the selection still references them.
    pub fn valid_selected_notes(&self) -> HashSet<NoteId> {
        if let Some(track) = self.project.track_at(self.selected_track_index) {
            let track_note_ids: HashSet<NoteId> = track.notes().iter().map(|n| n.id).collect();
            self.selected_notes
                .intersection(&track_note_ids)
                .copied()
                .collect()
        } else {
            HashSet::new()
        }
    }
}

/// Manages undo/redo history using a snapshot-based approach.
///
/// The manager maintains two stacks:
/// - `undo_stack`: Past states that can be reverted to
/// - `redo_stack`: Future states that can be restored after undoing
///
/// When a new action is performed, the current state is pushed to the
/// undo stack and the redo stack is cleared (branching creates a new timeline).
#[derive(Debug, Default)]
pub struct HistoryManager {
    /// Stack of states to undo to (most recent last).
    undo_stack: Vec<StateSnapshot>,

    /// Stack of states to redo to (most recent last).
    redo_stack: Vec<StateSnapshot>,
}

impl HistoryManager {
    /// Creates a new empty history manager.
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::with_capacity(MAX_HISTORY_SIZE),
            redo_stack: Vec::with_capacity(MAX_HISTORY_SIZE),
        }
    }

    /// Records a snapshot before an operation.
    ///
    /// Call this BEFORE making any changes to capture the current state.
    /// The redo stack is cleared since we're starting a new branch of history.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - The current state snapshot
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Before placing a note:
    /// history.push_undo(StateSnapshot::new(&project, selected_idx, &selected_notes, "Place note"));
    /// // Now make the change:
    /// track.create_note(...);
    /// ```
    pub fn push_undo(&mut self, snapshot: StateSnapshot) {
        // Clear redo stack - we're branching to a new timeline
        self.redo_stack.clear();

        self.push_undo_preserve_redo(snapshot);
    }

    /// Pushes a state to the undo stack WITHOUT clearing the redo stack.
    ///
    /// This is used internally during redo operations. When the user redoes,
    /// we need to push the current state to undo for potential future undos,
    /// but we must NOT clear the remaining redo states.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - The state to push to the undo stack
    pub fn push_undo_preserve_redo(&mut self, snapshot: StateSnapshot) {
        // Add to undo stack without clearing redo
        self.undo_stack.push(snapshot);

        // Enforce maximum history size by removing oldest entries
        while self.undo_stack.len() > MAX_HISTORY_SIZE {
            self.undo_stack.remove(0);
        }
    }

    /// Pops the most recent undo state.
    ///
    /// This should be called to get the state to restore to.
    /// The caller should push the CURRENT state to redo before applying
    /// the returned snapshot.
    ///
    /// # Returns
    ///
    /// The most recent undo snapshot, or None if undo stack is empty
    pub fn pop_undo(&mut self) -> Option<StateSnapshot> {
        self.undo_stack.pop()
    }

    /// Pushes a state to the redo stack.
    ///
    /// Called when undoing to save the current state for potential redo.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - The state before the undo was applied
    pub fn push_redo(&mut self, snapshot: StateSnapshot) {
        self.redo_stack.push(snapshot);

        // Enforce maximum history size
        while self.redo_stack.len() > MAX_HISTORY_SIZE {
            self.redo_stack.remove(0);
        }
    }

    /// Pops the most recent redo state.
    ///
    /// The caller should push the CURRENT state to undo before applying
    /// the returned snapshot.
    ///
    /// # Returns
    ///
    /// The most recent redo snapshot, or None if redo stack is empty
    pub fn pop_redo(&mut self) -> Option<StateSnapshot> {
        self.redo_stack.pop()
    }

    /// Clears all history.
    ///
    /// Called when:
    /// - Loading a new project
    /// - Creating a new project
    /// - Encountering an invalid state that can't be recovered
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

/// Test-only helper methods for HistoryManager.
#[cfg(test)]
impl HistoryManager {
    /// Returns true if there are states available to undo to.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns true if there are states available to redo to.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Returns the number of undo states available.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Returns the number of redo states available.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_push_and_pop() {
        let mut history = HistoryManager::new();

        let project = Project::with_default_track("Test");
        let snapshot = StateSnapshot::new(&project, 0, &HashSet::new(), "Test action");

        history.push_undo(snapshot);

        assert!(history.can_undo());
        assert!(!history.can_redo());
        assert_eq!(history.undo_count(), 1);

        let restored = history.pop_undo().unwrap();
        assert_eq!(restored.description, "Test action");
        assert!(!history.can_undo());
    }

    #[test]
    fn test_history_max_size() {
        let mut history = HistoryManager::new();

        let project = Project::with_default_track("Test");

        // Push more than MAX_HISTORY_SIZE entries
        for i in 0..MAX_HISTORY_SIZE + 5 {
            let snapshot =
                StateSnapshot::new(&project, 0, &HashSet::new(), format!("Action {}", i));
            history.push_undo(snapshot);
        }

        // Should only keep MAX_HISTORY_SIZE entries
        assert_eq!(history.undo_count(), MAX_HISTORY_SIZE);

        // The oldest entries should have been removed
        // Most recent should still be there
        let last = history.pop_undo().unwrap();
        assert_eq!(last.description, format!("Action {}", MAX_HISTORY_SIZE + 4));
    }

    #[test]
    fn test_redo_cleared_on_new_action() {
        let mut history = HistoryManager::new();

        let project = Project::with_default_track("Test");

        // Create an undo state
        history.push_undo(StateSnapshot::new(&project, 0, &HashSet::new(), "Action 1"));

        // Pop it and push to redo (simulating an undo operation)
        let undone = history.pop_undo().unwrap();
        history.push_redo(undone);

        assert!(history.can_redo());

        // New action should clear redo stack
        history.push_undo(StateSnapshot::new(&project, 0, &HashSet::new(), "Action 2"));

        assert!(!history.can_redo());
    }

    #[test]
    fn test_snapshot_validation() {
        let project = Project::with_default_track("Test");

        // Valid snapshot
        let valid = StateSnapshot::new(&project, 0, &HashSet::new(), "Valid");
        assert!(valid.is_valid());

        // Invalid track index
        let invalid = StateSnapshot::new(&project, 10, &HashSet::new(), "Invalid");
        assert!(!invalid.is_valid());
    }

    #[test]
    fn test_multi_level_undo_redo() {
        // Test that if user undoes 4 changes, they can redo those same 4 changes
        let mut history = HistoryManager::new();
        let project = Project::with_default_track("Test");

        // Simulate 4 user actions
        for i in 0..4 {
            history.push_undo(StateSnapshot::new(
                &project,
                0,
                &HashSet::new(),
                format!("Action {}", i),
            ));
        }

        assert_eq!(history.undo_count(), 4);
        assert_eq!(history.redo_count(), 0);

        // Undo all 4 actions (simulating what App::undo does)
        for _ in 0..4 {
            let undone = history.pop_undo().unwrap();
            history.push_redo(undone);
        }

        assert_eq!(history.undo_count(), 0);
        assert_eq!(history.redo_count(), 4);

        // Now redo all 4 actions using push_undo_preserve_redo (as App::redo does)
        for _ in 0..4 {
            let redone = history.pop_redo().unwrap();
            // This is the key: use push_undo_preserve_redo, NOT push_undo
            history.push_undo_preserve_redo(redone);
        }

        // Should have all 4 back in undo stack, redo should be empty
        assert_eq!(history.undo_count(), 4);
        assert_eq!(history.redo_count(), 0);
    }

    #[test]
    fn test_new_action_clears_redo_after_undo() {
        // Test that a new action after undo clears the redo stack
        let mut history = HistoryManager::new();
        let project = Project::with_default_track("Test");

        // Make 3 actions
        for i in 0..3 {
            history.push_undo(StateSnapshot::new(
                &project,
                0,
                &HashSet::new(),
                format!("Action {}", i),
            ));
        }

        // Undo 2 of them
        for _ in 0..2 {
            let undone = history.pop_undo().unwrap();
            history.push_redo(undone);
        }

        assert_eq!(history.undo_count(), 1);
        assert_eq!(history.redo_count(), 2);

        // Make a NEW action (this should clear redo stack - branching timeline)
        history.push_undo(StateSnapshot::new(
            &project,
            0,
            &HashSet::new(),
            "New action after undo",
        ));

        // Redo stack should be cleared, undo should have 2 items
        assert_eq!(history.undo_count(), 2);
        assert_eq!(history.redo_count(), 0);
    }
}
