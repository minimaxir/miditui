//! MIDI note representation.
//!
//! A note represents a single MIDI note-on/note-off pair with timing,
//! pitch, velocity, and duration information.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for generating unique note IDs.
/// Using atomic for thread-safety in case of parallel operations.
static NOTE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for a note within a project.
/// Allows efficient note selection and manipulation without index-based lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoteId(u64);

impl NoteId {
    /// Generates a new unique note ID.
    ///
    /// Thread-safe: uses atomic increment internally.
    pub fn new() -> Self {
        Self(NOTE_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw ID value (for serialization/debugging).
    #[allow(dead_code)]
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for NoteId {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a single MIDI note with timing and dynamics.
///
/// Notes are stored with tick-based timing for precise positioning.
/// The `id` field allows tracking notes across edits and selections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Unique identifier for this note instance.
    pub id: NoteId,

    /// MIDI note number (0-127). 60 = Middle C (C4).
    pub pitch: u8,

    /// Note velocity (0-127). Controls volume/intensity.
    /// 0 is silent, 127 is maximum.
    pub velocity: u8,

    /// Start time in ticks from the beginning of the track.
    pub start_tick: u32,

    /// Duration in ticks. Determines how long the note sounds.
    pub duration_ticks: u32,
}

impl Note {
    /// Creates a new note with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `pitch` - MIDI note number (0-127)
    /// * `velocity` - Note velocity (0-127)
    /// * `start_tick` - Start position in ticks
    /// * `duration_ticks` - Duration in ticks
    ///
    /// # Returns
    ///
    /// A new Note with a unique ID
    ///
    /// # Examples
    ///
    /// ```
    /// use miditui::midi::Note;
    ///
    /// // Create a middle C, quarter note at beat 1, medium velocity
    /// let note = Note::new(60, 100, 0, 480);
    /// ```
    pub fn new(pitch: u8, velocity: u8, start_tick: u32, duration_ticks: u32) -> Self {
        Self {
            id: NoteId::new(),
            pitch: pitch.min(127),
            velocity: velocity.min(127),
            start_tick,
            duration_ticks,
        }
    }

    /// Returns the end tick of this note (start + duration).
    pub fn end_tick(&self) -> u32 {
        self.start_tick.saturating_add(self.duration_ticks)
    }

    /// Checks if this note overlaps with a given tick range.
    ///
    /// # Arguments
    ///
    /// * `start` - Start of the range (inclusive)
    /// * `end` - End of the range (exclusive)
    ///
    /// # Returns
    ///
    /// true if any part of the note falls within the range
    #[allow(dead_code)]
    pub fn overlaps_range(&self, start: u32, end: u32) -> bool {
        self.start_tick < end && self.end_tick() > start
    }

    /// Checks if this note is sounding at a specific tick.
    ///
    /// # Arguments
    ///
    /// * `tick` - The tick to check
    ///
    /// # Returns
    ///
    /// true if the note is active at the given tick
    pub fn is_active_at(&self, tick: u32) -> bool {
        tick >= self.start_tick && tick < self.end_tick()
    }

    /// Creates a copy of this note with a new unique ID.
    /// Useful for copy/paste operations.
    #[allow(dead_code)]
    pub fn duplicate(&self) -> Self {
        Self {
            id: NoteId::new(),
            pitch: self.pitch,
            velocity: self.velocity,
            start_tick: self.start_tick,
            duration_ticks: self.duration_ticks,
        }
    }

    /// Transposes the note by a number of semitones.
    ///
    /// # Arguments
    ///
    /// * `semitones` - Number of semitones to transpose (can be negative)
    ///
    /// # Returns
    ///
    /// true if the transposition was successful (note stays in 0-127 range)
    #[allow(dead_code)]
    pub fn transpose(&mut self, semitones: i8) -> bool {
        let new_pitch = self.pitch as i16 + semitones as i16;
        if (0..=127).contains(&new_pitch) {
            self.pitch = new_pitch as u8;
            true
        } else {
            false
        }
    }

    /// Moves the note by a number of ticks.
    ///
    /// # Arguments
    ///
    /// * `ticks` - Number of ticks to move (can be negative)
    #[allow(dead_code)]
    pub fn shift(&mut self, ticks: i32) {
        if ticks < 0 {
            self.start_tick = self.start_tick.saturating_sub((-ticks) as u32);
        } else {
            self.start_tick = self.start_tick.saturating_add(ticks as u32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_creation() {
        let note = Note::new(60, 100, 0, 480);
        assert_eq!(note.pitch, 60);
        assert_eq!(note.velocity, 100);
        assert_eq!(note.start_tick, 0);
        assert_eq!(note.duration_ticks, 480);
    }

    #[test]
    fn test_note_clamping() {
        let note = Note::new(200, 200, 0, 480);
        assert_eq!(note.pitch, 127);
        assert_eq!(note.velocity, 127);
    }

    #[test]
    fn test_note_overlap() {
        let note = Note::new(60, 100, 100, 200); // 100-300
        assert!(note.overlaps_range(0, 150));
        assert!(note.overlaps_range(200, 400));
        assert!(note.overlaps_range(50, 350));
        assert!(!note.overlaps_range(0, 100));
        assert!(!note.overlaps_range(300, 400));
    }

    #[test]
    fn test_note_active() {
        let note = Note::new(60, 100, 100, 200);
        assert!(!note.is_active_at(99));
        assert!(note.is_active_at(100));
        assert!(note.is_active_at(200));
        assert!(!note.is_active_at(300));
    }

    #[test]
    fn test_transpose() {
        let mut note = Note::new(60, 100, 0, 480);
        assert!(note.transpose(12));
        assert_eq!(note.pitch, 72);

        let mut note = Note::new(120, 100, 0, 480);
        assert!(!note.transpose(12)); // Would exceed 127
        assert_eq!(note.pitch, 120); // Unchanged
    }
}
