//! MIDI track representation.
//!
//! A track contains a collection of notes assigned to a specific MIDI channel
//! and instrument (program). Tracks can be muted, soloed, and have adjustable volume.

use super::note::{Note, NoteId};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for generating unique track IDs.
static TRACK_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for a track within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackId(u64);

impl TrackId {
    /// Generates a new unique track ID.
    pub fn new() -> Self {
        Self(TRACK_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw ID value.
    #[allow(dead_code)]
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for TrackId {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a single MIDI track containing notes.
///
/// Each track has its own instrument (program), channel, and mixing settings.
/// Notes within a track are sorted by start time for efficient playback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    /// Unique identifier for this track.
    pub id: TrackId,

    /// Human-readable name for the track.
    pub name: String,

    /// MIDI channel (0-15). Channel 9 is reserved for drums in General MIDI.
    pub channel: u8,

    /// MIDI program number (0-127). Determines the instrument sound.
    pub program: u8,

    /// Track volume (0-127). Applied during playback.
    pub volume: u8,

    /// Pan position (0=left, 64=center, 127=right).
    pub pan: u8,

    /// Whether this track is muted (not played during playback).
    pub muted: bool,

    /// Whether this track is soloed (only soloed tracks play when any track is soloed).
    pub solo: bool,

    /// Collection of notes in this track, sorted by start_tick.
    notes: Vec<Note>,
}

impl Track {
    /// Creates a new track with default settings.
    ///
    /// # Arguments
    ///
    /// * `name` - Display name for the track
    /// * `channel` - MIDI channel (0-15)
    ///
    /// # Returns
    ///
    /// A new Track with default instrument (piano), centered pan, and full volume
    pub fn new(name: impl Into<String>, channel: u8) -> Self {
        Self {
            id: TrackId::new(),
            name: name.into(),
            channel: channel.min(15),
            program: 0, // Piano
            volume: 100,
            pan: 64, // Center
            muted: false,
            solo: false,
            notes: Vec::new(),
        }
    }

    /// Creates a drum track on channel 9.
    ///
    /// # Arguments
    ///
    /// * `name` - Display name for the track
    #[allow(dead_code)]
    pub fn new_drum_track(name: impl Into<String>) -> Self {
        Self {
            id: TrackId::new(),
            name: name.into(),
            channel: 9, // Drum channel in General MIDI
            program: 0,
            volume: 100,
            pan: 64,
            muted: false,
            solo: false,
            notes: Vec::new(),
        }
    }

    /// Adds a note to the track, maintaining sorted order by start_tick.
    ///
    /// # Arguments
    ///
    /// * `note` - The note to add
    ///
    /// # Returns
    ///
    /// The NoteId of the added note
    pub fn add_note(&mut self, note: Note) -> NoteId {
        let id = note.id;
        // Binary search insertion to maintain sorted order by start_tick
        // This enables O(log n) insertion and efficient range queries.
        let pos = self
            .notes
            .binary_search_by_key(&note.start_tick, |n| n.start_tick)
            .unwrap_or_else(|pos| pos);
        self.notes.insert(pos, note);
        id
    }

    /// Creates and adds a new note to the track.
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
    /// The NoteId of the created note
    pub fn create_note(
        &mut self,
        pitch: u8,
        velocity: u8,
        start_tick: u32,
        duration_ticks: u32,
    ) -> NoteId {
        let note = Note::new(pitch, velocity, start_tick, duration_ticks);
        self.add_note(note)
    }

    /// Removes a note by its ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The NoteId of the note to remove
    ///
    /// # Returns
    ///
    /// The removed note, or None if not found
    pub fn remove_note(&mut self, id: NoteId) -> Option<Note> {
        // Linear search is acceptable here as note removal is relatively infrequent.
        // Could be optimized with a HashMap<NoteId, usize> index if needed.
        let pos = self.notes.iter().position(|n| n.id == id)?;
        Some(self.notes.remove(pos))
    }

    /// Returns a reference to a note by its ID.
    #[allow(dead_code)]
    pub fn get_note(&self, id: NoteId) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// Returns a mutable reference to a note by its ID.
    #[allow(dead_code)]
    pub fn get_note_mut(&mut self, id: NoteId) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    /// Returns all notes in the track (sorted by start_tick).
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Returns mutable access to all notes in the track.
    pub fn notes_mut(&mut self) -> &mut [Note] {
        &mut self.notes
    }

    /// Returns notes within a given tick range.
    ///
    /// # Arguments
    ///
    /// * `start` - Start tick (inclusive)
    /// * `end` - End tick (exclusive)
    ///
    /// # Returns
    ///
    /// Iterator over notes that overlap with the range
    #[allow(dead_code)]
    pub fn notes_in_range(&self, start: u32, end: u32) -> impl Iterator<Item = &Note> {
        // Use binary search to find the first note that could possibly overlap
        let first_idx = self
            .notes
            .binary_search_by_key(&start.saturating_sub(u32::MAX / 2), |n| n.start_tick)
            .unwrap_or_else(|pos| pos);

        self.notes[first_idx..]
            .iter()
            .take_while(move |n| n.start_tick < end)
            .filter(move |n| n.overlaps_range(start, end))
    }

    /// Returns notes that are active at a specific tick.
    #[allow(dead_code)]
    pub fn notes_at_tick(&self, tick: u32) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(move |n| n.is_active_at(tick))
    }

    /// Returns the total duration of the track in ticks.
    /// This is the end tick of the last note.
    pub fn duration_ticks(&self) -> u32 {
        self.notes.iter().map(|n| n.end_tick()).max().unwrap_or(0)
    }

    /// Returns the number of notes in the track.
    #[allow(dead_code)]
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// Clears all notes from the track.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.notes.clear();
    }

    /// Quantizes all notes to a grid.
    ///
    /// # Arguments
    ///
    /// * `grid_ticks` - The quantization grid size in ticks
    #[allow(dead_code)]
    pub fn quantize(&mut self, grid_ticks: u32) {
        if grid_ticks == 0 {
            return;
        }
        for note in &mut self.notes {
            // Round start_tick to nearest grid position
            let remainder = note.start_tick % grid_ticks;
            if remainder > grid_ticks / 2 {
                note.start_tick += grid_ticks - remainder;
            } else {
                note.start_tick -= remainder;
            }
        }
        // Re-sort after quantization (notes may have moved)
        self.notes.sort_by_key(|n| n.start_tick);
    }

    /// Transposes all notes by a number of semitones.
    ///
    /// # Arguments
    ///
    /// * `semitones` - Number of semitones to transpose
    ///
    /// # Returns
    ///
    /// Number of notes that couldn't be transposed (out of range)
    #[allow(dead_code)]
    pub fn transpose_all(&mut self, semitones: i8) -> usize {
        let mut failed = 0;
        for note in &mut self.notes {
            if !note.transpose(semitones) {
                failed += 1;
            }
        }
        failed
    }
}

impl Default for Track {
    fn default() -> Self {
        Self::new("Track 1", 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_creation() {
        let track = Track::new("Piano", 0);
        assert_eq!(track.name, "Piano");
        assert_eq!(track.channel, 0);
        assert_eq!(track.program, 0);
        assert!(!track.muted);
        assert!(!track.solo);
    }

    #[test]
    fn test_add_notes_sorted() {
        let mut track = Track::new("Test", 0);
        track.create_note(60, 100, 480, 240); // Beat 2
        track.create_note(62, 100, 0, 240); // Beat 1
        track.create_note(64, 100, 960, 240); // Beat 3

        let notes = track.notes();
        assert_eq!(notes[0].start_tick, 0);
        assert_eq!(notes[1].start_tick, 480);
        assert_eq!(notes[2].start_tick, 960);
    }

    #[test]
    fn test_notes_in_range() {
        let mut track = Track::new("Test", 0);
        track.create_note(60, 100, 0, 480); // 0-480
        track.create_note(62, 100, 480, 480); // 480-960
        track.create_note(64, 100, 960, 480); // 960-1440

        let in_range: Vec<_> = track.notes_in_range(240, 720).collect();
        assert_eq!(in_range.len(), 2); // First two notes overlap
    }

    #[test]
    fn test_duration() {
        let mut track = Track::new("Test", 0);
        assert_eq!(track.duration_ticks(), 0);

        track.create_note(60, 100, 0, 480);
        assert_eq!(track.duration_ticks(), 480);

        track.create_note(62, 100, 960, 480);
        assert_eq!(track.duration_ticks(), 1440);
    }
}
