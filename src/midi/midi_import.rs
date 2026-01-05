//! Standard MIDI File (SMF) import functionality.
//!
//! Imports .mid and .midi files into the internal project representation.
//! Supports SMF Format 0 (single track) and Format 1 (multi-track) files.
//!
//! # Limitations
//!
//! - Only note on/off events are imported as notes
//! - Tempo and time signature are read from the first track (or global events)
//! - Program changes set the track instrument
//! - Volume (CC7) and Pan (CC10) are imported
//! - Other MIDI events (pitch bend, aftertouch, etc.) are ignored

use super::{Note, Project, Track, TICKS_PER_BEAT};
use midly::{Format, Smf, Timing, TrackEventKind};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Errors that can occur during MIDI import.
#[derive(Debug)]
pub enum MidiImportError {
    /// File could not be read
    IoError(std::io::Error),
    /// MIDI parsing failed
    ParseError(String),
    /// Unsupported MIDI format or timing
    UnsupportedFormat(String),
}

impl std::fmt::Display for MidiImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MidiImportError::IoError(e) => write!(f, "IO error: {}", e),
            MidiImportError::ParseError(e) => write!(f, "MIDI parse error: {}", e),
            MidiImportError::UnsupportedFormat(e) => write!(f, "Unsupported format: {}", e),
        }
    }
}

impl std::error::Error for MidiImportError {}

impl From<std::io::Error> for MidiImportError {
    fn from(e: std::io::Error) -> Self {
        MidiImportError::IoError(e)
    }
}

/// State for tracking active notes during import.
/// Key is (channel, pitch), value is (start_tick, velocity).
type ActiveNotes = HashMap<(u8, u8), (u32, u8)>;

/// Result type for parsing a single MIDI track.
/// Contains: (Vec of Tracks split by channel, optional tempo, optional time signature).
type ParseTrackResult = Result<(Vec<Track>, Option<u32>, Option<(u8, u8)>), MidiImportError>;

/// Imports a MIDI file and creates a Project.
///
/// # Arguments
///
/// * `path` - Path to the .mid or .midi file
///
/// # Returns
///
/// A Project containing the imported MIDI data
///
/// # Errors
///
/// Returns error if file cannot be read or parsed
pub fn import_from_midi<P: AsRef<Path>>(path: P) -> Result<Project, MidiImportError> {
    let path = path.as_ref();
    let data = fs::read(path)?;

    let smf = Smf::parse(&data).map_err(|e| MidiImportError::ParseError(e.to_string()))?;

    // Get ticks per beat from header
    let source_ticks_per_beat = match smf.header.timing {
        Timing::Metrical(tpb) => tpb.as_int() as u32,
        Timing::Timecode(_, _) => {
            return Err(MidiImportError::UnsupportedFormat(
                "SMPTE timecode timing not supported".to_string(),
            ))
        }
    };

    // Create project with filename as name
    let project_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported MIDI")
        .to_string();

    let mut project = Project::new(&project_name);

    // Remove the default track that Project::new creates
    // We need to get the track ID first since remove_track expects a TrackId
    while project.track_count() > 0 {
        if let Some(track) = project.track_at(0) {
            let id = track.id;
            project.remove_track(id);
        } else {
            break;
        }
    }

    // Default tempo and time signature (will be overwritten if found in MIDI)
    let mut tempo: u32 = 120;
    let mut time_sig_num: u8 = 4;
    let mut time_sig_denom: u8 = 4;

    // Process tracks based on format
    match smf.header.format {
        Format::SingleTrack | Format::Parallel => {
            // Format 0: Single track with all channels
            // Format 1: First track is usually tempo/meta, rest are music
            let is_format_1 = smf.header.format == Format::Parallel;

            for (track_idx, track) in smf.tracks.iter().enumerate() {
                // For Format 1, first track is typically tempo/meta only
                let is_tempo_track = is_format_1 && track_idx == 0;

                // Parse the track
                let (track_data, track_tempo, track_time_sig) =
                    parse_track(track, track_idx, source_ticks_per_beat, is_tempo_track)?;

                // Update global tempo/time sig from tempo track or first occurrence
                if let Some(t) = track_tempo {
                    tempo = t;
                }
                if let Some((num, denom)) = track_time_sig {
                    time_sig_num = num;
                    time_sig_denom = denom;
                }

                if !is_tempo_track || !track_data.is_empty() {
                    for imported_track in track_data {
                        project.add_track(imported_track);
                    }
                }
            }
        }
        Format::Sequential => {
            return Err(MidiImportError::UnsupportedFormat(
                "Format 2 (sequential) MIDI files not supported".to_string(),
            ))
        }
    }

    project.tempo = tempo;
    project.time_sig_numerator = time_sig_num;
    project.time_sig_denominator = time_sig_denom;

    // If no tracks were created, add an empty default track
    if project.track_count() == 0 {
        project.add_track(Track::new("Track 1", 0));
    }

    Ok(project)
}

/// Parses a single MIDI track and returns track data plus any tempo/time sig info.
fn parse_track(
    track: &[midly::TrackEvent],
    track_idx: usize,
    source_ticks_per_beat: u32,
    is_tempo_track: bool,
) -> ParseTrackResult {
    // Track state per channel
    let mut channel_tracks: HashMap<u8, Track> = HashMap::new();
    let mut active_notes: ActiveNotes = HashMap::new();
    let mut tempo: Option<u32> = None;
    let mut time_sig: Option<(u8, u8)> = None;
    let mut track_name: Option<String> = None;

    // Current absolute tick position
    let mut current_tick: u32 = 0;

    for event in track {
        // Advance tick by delta time, scaling to our internal resolution
        let delta_scaled = scale_ticks(event.delta.as_int(), source_ticks_per_beat);
        current_tick += delta_scaled;

        match event.kind {
            TrackEventKind::Meta(meta) => {
                match meta {
                    midly::MetaMessage::TrackName(name_bytes) => {
                        if let Ok(name) = std::str::from_utf8(name_bytes) {
                            track_name = Some(name.to_string());
                        }
                    }
                    midly::MetaMessage::Tempo(tempo_val) => {
                        // tempo_val is microseconds per beat
                        let usec_per_beat = tempo_val.as_int();
                        if usec_per_beat > 0 {
                            tempo = Some(60_000_000 / usec_per_beat);
                        }
                    }
                    midly::MetaMessage::TimeSignature(num, denom_power, _, _) => {
                        // denom_power is power of 2 (e.g., 2 means quarter note)
                        let denom = 1u8 << denom_power;
                        time_sig = Some((num, denom));
                    }
                    _ => {} // Ignore other meta events
                }
            }
            TrackEventKind::Midi { channel, message } => {
                let ch = channel.as_int();

                // Ensure we have a track for this channel, using entry API
                channel_tracks.entry(ch).or_insert_with(|| {
                    let name = track_name
                        .clone()
                        .unwrap_or_else(|| format!("Track {}", track_idx + 1));
                    let mut new_track = Track::new(&name, ch);
                    new_track.channel = ch;
                    new_track
                });

                match message {
                    midly::MidiMessage::NoteOn { key, vel } => {
                        let pitch = key.as_int();
                        let velocity = vel.as_int();

                        if velocity > 0 {
                            // Note on - record start
                            active_notes.insert((ch, pitch), (current_tick, velocity));
                        } else {
                            // Note on with velocity 0 = note off
                            if let Some((start_tick, note_vel)) = active_notes.remove(&(ch, pitch))
                            {
                                let duration = current_tick.saturating_sub(start_tick).max(1);
                                if let Some(track) = channel_tracks.get_mut(&ch) {
                                    track
                                        .add_note(Note::new(pitch, note_vel, start_tick, duration));
                                }
                            }
                        }
                    }
                    midly::MidiMessage::NoteOff { key, vel: _ } => {
                        let pitch = key.as_int();
                        if let Some((start_tick, velocity)) = active_notes.remove(&(ch, pitch)) {
                            let duration = current_tick.saturating_sub(start_tick).max(1);
                            if let Some(track) = channel_tracks.get_mut(&ch) {
                                track.add_note(Note::new(pitch, velocity, start_tick, duration));
                            }
                        }
                    }
                    midly::MidiMessage::ProgramChange { program } => {
                        if let Some(track) = channel_tracks.get_mut(&ch) {
                            track.program = program.as_int();
                        }
                    }
                    midly::MidiMessage::Controller { controller, value } => {
                        let cc = controller.as_int();
                        let val = value.as_int();

                        if let Some(track) = channel_tracks.get_mut(&ch) {
                            match cc {
                                7 => track.volume = val, // Volume
                                10 => track.pan = val,   // Pan
                                _ => {}                  // Ignore other CCs
                            }
                        }
                    }
                    _ => {} // Ignore other MIDI messages
                }
            }
            _ => {} // Ignore SysEx and other events
        }
    }

    // Close any remaining active notes (in case MIDI file is incomplete)
    for ((ch, pitch), (start_tick, velocity)) in active_notes {
        if let Some(track) = channel_tracks.get_mut(&ch) {
            // Use a default duration of 1 beat for unclosed notes
            let duration = TICKS_PER_BEAT;
            track.add_note(Note::new(pitch, velocity, start_tick, duration));
        }
    }

    // Convert HashMap to Vec, sorted by channel
    let mut tracks: Vec<Track> = channel_tracks.into_values().collect();
    tracks.sort_by_key(|t| t.channel);

    // For tempo-only tracks in Format 1, we might have no notes
    // Only return empty tracks if it's not supposed to be a tempo track
    if is_tempo_track && tracks.iter().all(|t| t.notes().is_empty()) {
        tracks.clear();
    }

    Ok((tracks, tempo, time_sig))
}

/// Scales ticks from source resolution to our internal resolution (TICKS_PER_BEAT).
fn scale_ticks(source_ticks: u32, source_tpb: u32) -> u32 {
    if source_tpb == TICKS_PER_BEAT {
        source_ticks
    } else {
        // Scale: (source_ticks * TICKS_PER_BEAT) / source_tpb
        // Use u64 to avoid overflow
        ((source_ticks as u64 * TICKS_PER_BEAT as u64) / source_tpb as u64) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_ticks() {
        // Same resolution
        assert_eq!(scale_ticks(480, 480), 480);

        // Double resolution source
        assert_eq!(scale_ticks(960, 960), 480);

        // Half resolution source
        assert_eq!(scale_ticks(240, 240), 480);

        // Different resolution
        assert_eq!(scale_ticks(120, 120), 480);
    }
}
