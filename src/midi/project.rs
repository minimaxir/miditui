//! MIDI project container.
//!
//! A project represents a complete musical composition with multiple tracks,
//! tempo settings, and time signature information.

use super::note::NoteId;
use super::track::{Track, TrackId};
use super::{ticks_to_seconds, DEFAULT_TEMPO, TICKS_PER_BEAT};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Represents a complete MIDI project with multiple tracks.
///
/// The project maintains a list of tracks and global settings like tempo.
/// Supports unlimited tracks - memory is the only constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Project name.
    pub name: String,

    /// Tempo in beats per minute.
    pub tempo: u32,

    /// Time signature numerator (beats per measure).
    pub time_sig_numerator: u8,

    /// Time signature denominator (beat unit, as power of 2).
    /// 4 means quarter note, 8 means eighth note, etc.
    pub time_sig_denominator: u8,

    /// Collection of tracks in the project.
    tracks: Vec<Track>,

    /// Next available MIDI channel for auto-assignment.
    /// Skips channel 9 (drums) for melodic tracks.
    next_channel: u8,

    /// Path to the SoundFont file used for playback.
    /// Stored as a string for cross-platform serialization compatibility.
    /// None means no SoundFont is explicitly associated (use default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soundfont_path: Option<String>,
}

impl Project {
    /// Creates a new empty project with default settings.
    ///
    /// # Arguments
    ///
    /// * `name` - Project name
    ///
    /// # Returns
    ///
    /// A new Project with 120 BPM tempo and 4/4 time signature
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tempo: DEFAULT_TEMPO,
            time_sig_numerator: 4,
            time_sig_denominator: 4,
            tracks: Vec::new(),
            next_channel: 0,
            soundfont_path: None,
        }
    }

    /// Creates a new project with a single default track.
    pub fn with_default_track(name: impl Into<String>) -> Self {
        let mut project = Self::new(name);
        project.add_track(Track::new("Track 1", 0));
        project
    }

    /// Sets the SoundFont path for this project.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the SoundFont file, or None to clear
    pub fn set_soundfont_path(&mut self, path: Option<impl AsRef<Path>>) {
        self.soundfont_path = path.map(|p| p.as_ref().to_string_lossy().into_owned());
    }

    /// Returns the SoundFont path for this project, if set.
    pub fn get_soundfont_path(&self) -> Option<&str> {
        self.soundfont_path.as_deref()
    }

    /// Returns the number of ticks per measure based on time signature.
    pub fn ticks_per_measure(&self) -> u32 {
        // Calculate based on time signature
        // For 4/4: 4 * 480 = 1920 ticks per measure
        // For 6/8: 6 * 240 = 1440 ticks per measure (eighth note = 240 ticks)
        let beat_ticks = TICKS_PER_BEAT * 4 / self.time_sig_denominator as u32;
        beat_ticks * self.time_sig_numerator as u32
    }

    /// Returns the total duration of the project in ticks.
    /// This is the maximum duration across all tracks.
    pub fn duration_ticks(&self) -> u32 {
        self.tracks
            .iter()
            .map(|t| t.duration_ticks())
            .max()
            .unwrap_or(0)
    }

    /// Returns the total duration of the project in seconds.
    #[allow(dead_code)]
    pub fn duration_seconds(&self) -> f64 {
        ticks_to_seconds(self.duration_ticks(), self.tempo)
    }

    /// Adds a track to the project.
    ///
    /// # Arguments
    ///
    /// * `track` - The track to add
    ///
    /// # Returns
    ///
    /// The TrackId of the added track
    pub fn add_track(&mut self, track: Track) -> TrackId {
        let id = track.id;
        self.tracks.push(track);
        id
    }

    /// Creates and adds a new track with auto-assigned channel.
    ///
    /// # Arguments
    ///
    /// * `name` - Display name for the track
    ///
    /// # Returns
    ///
    /// The TrackId of the created track
    pub fn create_track(&mut self, name: impl Into<String>) -> TrackId {
        let channel = self.next_channel;
        // Skip drum channel (9) for melodic tracks
        self.next_channel = if self.next_channel == 8 {
            10
        } else if self.next_channel >= 15 {
            0 // Wrap around (multiple tracks can share channels)
        } else {
            self.next_channel + 1
        };

        let track = Track::new(name, channel);
        self.add_track(track)
    }

    /// Creates and adds a drum track.
    ///
    /// # Arguments
    ///
    /// * `name` - Display name for the track
    ///
    /// # Returns
    ///
    /// The TrackId of the created drum track
    #[allow(dead_code)]
    pub fn create_drum_track(&mut self, name: impl Into<String>) -> TrackId {
        let track = Track::new_drum_track(name);
        self.add_track(track)
    }

    /// Removes a track by its ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The TrackId of the track to remove
    ///
    /// # Returns
    ///
    /// The removed track, or None if not found
    pub fn remove_track(&mut self, id: TrackId) -> Option<Track> {
        let pos = self.tracks.iter().position(|t| t.id == id)?;
        Some(self.tracks.remove(pos))
    }

    /// Returns a reference to a track by its ID.
    #[allow(dead_code)]
    pub fn get_track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Returns a mutable reference to a track by its ID.
    #[allow(dead_code)]
    pub fn get_track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Returns a reference to a track by index.
    pub fn track_at(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    /// Returns a mutable reference to a track by index.
    pub fn track_at_mut(&mut self, index: usize) -> Option<&mut Track> {
        self.tracks.get_mut(index)
    }

    /// Returns all tracks in the project.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Returns an iterator over mutable track references.
    #[allow(dead_code)]
    pub fn tracks_mut(&mut self) -> impl Iterator<Item = &mut Track> {
        self.tracks.iter_mut()
    }

    /// Returns the number of tracks in the project.
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Moves a track to a new position in the track list.
    ///
    /// # Arguments
    ///
    /// * `from` - Current index of the track
    /// * `to` - Target index for the track
    ///
    /// # Returns
    ///
    /// true if the move was successful
    #[allow(dead_code)]
    pub fn move_track(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tracks.len() || to >= self.tracks.len() {
            return false;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        true
    }

    /// Returns tracks that should be played (considering mute/solo states).
    ///
    /// If any track is soloed, only soloed tracks play.
    /// Otherwise, all non-muted tracks play.
    #[allow(dead_code)]
    pub fn playable_tracks(&self) -> impl Iterator<Item = &Track> {
        let any_solo = self.tracks.iter().any(|t| t.solo);
        self.tracks.iter().filter(move |t| {
            if any_solo {
                t.solo && !t.muted
            } else {
                !t.muted
            }
        })
    }

    /// Finds a note by its ID across all tracks.
    ///
    /// # Arguments
    ///
    /// * `note_id` - The NoteId to search for
    ///
    /// # Returns
    ///
    /// Tuple of (TrackId, &Note) if found
    #[allow(dead_code)]
    pub fn find_note(&self, note_id: NoteId) -> Option<(TrackId, &super::note::Note)> {
        for track in &self.tracks {
            if let Some(note) = track.get_note(note_id) {
                return Some((track.id, note));
            }
        }
        None
    }

    /// Calculates the measure and beat for a given tick position.
    ///
    /// # Arguments
    ///
    /// * `tick` - Tick position
    ///
    /// # Returns
    ///
    /// Tuple of (measure, beat, tick_within_beat), all 1-indexed
    pub fn tick_to_position(&self, tick: u32) -> (u32, u32, u32) {
        let ticks_per_measure = self.ticks_per_measure();
        let ticks_per_beat = TICKS_PER_BEAT;

        let measure = tick / ticks_per_measure + 1;
        let tick_in_measure = tick % ticks_per_measure;
        let beat = tick_in_measure / ticks_per_beat + 1;
        let tick_in_beat = tick_in_measure % ticks_per_beat;

        (measure, beat, tick_in_beat)
    }

    /// Converts a measure/beat position to ticks.
    ///
    /// # Arguments
    ///
    /// * `measure` - Measure number (1-indexed)
    /// * `beat` - Beat number (1-indexed)
    ///
    /// # Returns
    ///
    /// Tick position
    #[allow(dead_code)]
    pub fn position_to_tick(&self, measure: u32, beat: u32) -> u32 {
        let ticks_per_measure = self.ticks_per_measure();
        (measure - 1) * ticks_per_measure + (beat - 1) * TICKS_PER_BEAT
    }

    /// Saves the project to JSON.
    ///
    /// # Returns
    ///
    /// JSON string representation of the project
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails
    #[allow(dead_code)]
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Loads a project from JSON.
    ///
    /// # Arguments
    ///
    /// * `json` - JSON string to parse
    ///
    /// # Returns
    ///
    /// Parsed Project
    ///
    /// # Errors
    ///
    /// Returns error if parsing fails
    #[allow(dead_code)]
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Saves the project to a JSON file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the output file
    ///
    /// # Errors
    ///
    /// Returns error if serialization or file writing fails
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, json)
    }

    /// Loads a project from a JSON file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the input file
    ///
    /// # Returns
    ///
    /// Loaded Project
    ///
    /// # Errors
    ///
    /// Returns error if file reading or parsing fails
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let json = fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Saves the project to binary format (.oxm).
    ///
    /// Uses bincode for efficient serialization of numeric data.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the output file
    ///
    /// # Errors
    ///
    /// Returns error if serialization or file writing fails
    pub fn save_to_binary<P: AsRef<Path>>(&self, path: P) -> Result<(), std::io::Error> {
        let data = bincode::serialize(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, data)
    }

    /// Loads a project from binary format (.oxm).
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the input file
    ///
    /// # Returns
    ///
    /// Loaded Project
    ///
    /// # Errors
    ///
    /// Returns error if file reading or parsing fails
    pub fn load_from_binary<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let data = fs::read(path)?;
        bincode::deserialize(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Exports the project to a Standard MIDI File (.mid).
    ///
    /// Creates a Format 1 MIDI file with tempo, time signature, and all tracks.
    /// Note that some project metadata (mute/solo states, custom note IDs) cannot
    /// be represented in the MIDI format and will not be preserved.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the output file
    ///
    /// # Errors
    ///
    /// Returns error if file creation or writing fails
    pub fn export_to_midi<P: AsRef<Path>>(&self, path: P) -> Result<(), std::io::Error> {
        super::export_to_midi(self, path)
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::with_default_track("Untitled Project")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_creation() {
        let project = Project::new("Test");
        assert_eq!(project.name, "Test");
        assert_eq!(project.tempo, 120);
        assert_eq!(project.track_count(), 0);
    }

    #[test]
    fn test_add_tracks() {
        let mut project = Project::new("Test");
        project.create_track("Track 1");
        project.create_track("Track 2");
        assert_eq!(project.track_count(), 2);
    }

    #[test]
    fn test_channel_assignment() {
        let mut project = Project::new("Test");
        for i in 0..16 {
            project.create_track(format!("Track {}", i + 1));
        }
        // Check that channel 9 was skipped (it's for drums)
        let channels: Vec<_> = project.tracks().iter().map(|t| t.channel).collect();
        assert!(!channels[..10].contains(&9)); // First 10 tracks skip channel 9
    }

    #[test]
    fn test_tick_position_conversion() {
        let project = Project::new("Test"); // 4/4 time

        // Tick 0 = Measure 1, Beat 1
        assert_eq!(project.tick_to_position(0), (1, 1, 0));

        // Tick 480 = Measure 1, Beat 2
        assert_eq!(project.tick_to_position(480), (1, 2, 0));

        // Tick 1920 = Measure 2, Beat 1
        assert_eq!(project.tick_to_position(1920), (2, 1, 0));
    }

    #[test]
    fn test_serialization() {
        let mut project = Project::new("Test");
        project.create_track("Piano");
        project
            .track_at_mut(0)
            .unwrap()
            .create_note(60, 100, 0, 480);

        let json = project.to_json().unwrap();
        let loaded = Project::from_json(&json).unwrap();

        assert_eq!(loaded.name, "Test");
        assert_eq!(loaded.track_count(), 1);
        assert_eq!(loaded.track_at(0).unwrap().note_count(), 1);
    }
}
