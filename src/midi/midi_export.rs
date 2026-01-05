//! Standard MIDI File (SMF) export functionality.
//!
//! Exports the internal project representation to .mid files compatible
//! with any MIDI sequencer or player.
//!
//! # Limitations (Information Degradation)
//!
//! The internal format supports features that cannot be fully represented in SMF:
//! - Track mute/solo states are not exported (all tracks exported as-is)
//! - Project-specific metadata (custom names beyond track names) may be simplified
//! - Binary note IDs are not preserved
//!
//! # Format Details
//!
//! Exports as SMF Format 1 (multi-track) with:
//! - Track 0: Tempo and time signature meta events
//! - Tracks 1-N: MIDI note data with program changes

use super::{Project, TICKS_PER_BEAT};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Writes a variable-length quantity (VLQ) used for delta times in MIDI.
///
/// VLQ encodes values using 7 bits per byte, with the MSB indicating
/// whether more bytes follow (1 = more bytes, 0 = last byte).
///
/// # Arguments
///
/// * `value` - The value to encode (max 0x0FFFFFFF for MIDI)
/// * `buffer` - Output buffer to write to
fn write_vlq(value: u32, buffer: &mut Vec<u8>) {
    // VLQ can be 1-4 bytes for MIDI delta times
    // Each byte uses 7 bits for data, MSB indicates continuation
    if value == 0 {
        buffer.push(0);
        return;
    }

    let mut temp = value;
    let mut bytes = Vec::with_capacity(4);

    while temp > 0 {
        bytes.push((temp & 0x7F) as u8);
        temp >>= 7;
    }

    // Write bytes in reverse order with continuation bits
    for (i, &byte) in bytes.iter().rev().enumerate() {
        if i < bytes.len() - 1 {
            buffer.push(byte | 0x80); // Set continuation bit
        } else {
            buffer.push(byte); // Last byte, no continuation
        }
    }
}

/// MIDI event types for track data.
enum MidiEvent {
    /// Note on: channel, pitch, velocity
    NoteOn {
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    /// Note off: channel, pitch, velocity (typically 0)
    NoteOff {
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    /// Program change: channel, program number
    ProgramChange { channel: u8, program: u8 },
    /// Control change: channel, controller, value
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// Set tempo: microseconds per quarter note
    SetTempo { microseconds_per_beat: u32 },
    /// Time signature: numerator, denominator (as power of 2), clocks per click, 32nds per quarter
    TimeSignature {
        numerator: u8,
        denominator_power: u8,
    },
    /// Track name (meta event)
    TrackName { name: String },
    /// End of track (meta event)
    EndOfTrack,
}

/// Represents a timed MIDI event for sorting and writing.
struct TimedEvent {
    /// Absolute tick position
    tick: u32,
    /// The MIDI event
    event: MidiEvent,
    /// Priority for sorting events at the same tick (lower = first)
    /// Used to ensure program changes come before notes, etc.
    priority: u8,
}

impl TimedEvent {
    fn new(tick: u32, event: MidiEvent, priority: u8) -> Self {
        Self {
            tick,
            event,
            priority,
        }
    }
}

/// Writes a single MIDI event to the buffer (without delta time).
fn write_event(event: &MidiEvent, buffer: &mut Vec<u8>) {
    match event {
        MidiEvent::NoteOn {
            channel,
            pitch,
            velocity,
        } => {
            buffer.push(0x90 | (channel & 0x0F));
            buffer.push(*pitch);
            buffer.push(*velocity);
        }
        MidiEvent::NoteOff {
            channel,
            pitch,
            velocity,
        } => {
            buffer.push(0x80 | (channel & 0x0F));
            buffer.push(*pitch);
            buffer.push(*velocity);
        }
        MidiEvent::ProgramChange { channel, program } => {
            buffer.push(0xC0 | (channel & 0x0F));
            buffer.push(*program);
        }
        MidiEvent::ControlChange {
            channel,
            controller,
            value,
        } => {
            buffer.push(0xB0 | (channel & 0x0F));
            buffer.push(*controller);
            buffer.push(*value);
        }
        MidiEvent::SetTempo {
            microseconds_per_beat,
        } => {
            // Meta event: FF 51 03 tt tt tt
            buffer.push(0xFF);
            buffer.push(0x51);
            buffer.push(0x03);
            buffer.push((microseconds_per_beat >> 16) as u8);
            buffer.push((microseconds_per_beat >> 8) as u8);
            buffer.push(*microseconds_per_beat as u8);
        }
        MidiEvent::TimeSignature {
            numerator,
            denominator_power,
        } => {
            // Meta event: FF 58 04 nn dd cc bb
            // nn = numerator, dd = denominator as power of 2
            // cc = MIDI clocks per metronome click (24 = quarter note)
            // bb = 32nd notes per quarter note (8)
            buffer.push(0xFF);
            buffer.push(0x58);
            buffer.push(0x04);
            buffer.push(*numerator);
            buffer.push(*denominator_power);
            buffer.push(24); // Clocks per click
            buffer.push(8); // 32nd notes per quarter
        }
        MidiEvent::TrackName { name } => {
            // Meta event: FF 03 len text
            buffer.push(0xFF);
            buffer.push(0x03);
            let name_bytes = name.as_bytes();
            write_vlq(name_bytes.len() as u32, buffer);
            buffer.extend_from_slice(name_bytes);
        }
        MidiEvent::EndOfTrack => {
            // Meta event: FF 2F 00
            buffer.push(0xFF);
            buffer.push(0x2F);
            buffer.push(0x00);
        }
    }
}

/// Builds the track chunk data from a list of timed events.
///
/// Events are sorted by tick position and converted to delta times.
fn build_track_data(events: &mut [TimedEvent]) -> Vec<u8> {
    let mut buffer = Vec::new();
    events.sort_by(|a, b| a.tick.cmp(&b.tick).then(a.priority.cmp(&b.priority)));

    let mut last_tick = 0u32;
    for timed_event in events.iter() {
        let delta = timed_event.tick.saturating_sub(last_tick);
        write_vlq(delta, &mut buffer);
        write_event(&timed_event.event, &mut buffer);
        last_tick = timed_event.tick;
    }

    buffer
}

/// Writes a track chunk to the output.
fn write_track_chunk<W: Write>(writer: &mut W, track_data: &[u8]) -> std::io::Result<()> {
    // MTrk header
    writer.write_all(b"MTrk")?;
    // Length as big-endian u32
    let length = track_data.len() as u32;
    writer.write_all(&length.to_be_bytes())?;
    // Track data
    writer.write_all(track_data)?;
    Ok(())
}

/// Calculates the power of 2 for a time signature denominator.
///
/// E.g., 4 -> 2 (2^2 = 4), 8 -> 3 (2^3 = 8)
fn denominator_to_power(denom: u8) -> u8 {
    match denom {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        16 => 4,
        32 => 5,
        _ => 2, // Default to quarter note
    }
}

/// Exports a project to a Standard MIDI File.
///
/// # Arguments
///
/// * `project` - The project to export
/// * `path` - Output file path
///
/// # Returns
///
/// Ok(()) on success, or an IO error
///
/// # Format
///
/// Creates a Format 1 MIDI file with:
/// - Track 0: Tempo and time signature
/// - Tracks 1-N: One track per project track with notes and program changes
///
/// # Errors
///
/// Returns error if file creation or writing fails
#[allow(clippy::vec_init_then_push)]
pub fn export_to_midi<P: AsRef<Path>>(project: &Project, path: P) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    // Number of tracks: 1 tempo track + N music tracks
    let num_tracks = 1 + project.track_count() as u16;

    // Write header chunk (MThd)
    writer.write_all(b"MThd")?;
    writer.write_all(&6u32.to_be_bytes())?; // Header length (always 6)
    writer.write_all(&1u16.to_be_bytes())?; // Format 1 (multi-track)
    writer.write_all(&num_tracks.to_be_bytes())?;
    writer.write_all(&(TICKS_PER_BEAT as u16).to_be_bytes())?; // Division

    // Track 0: Tempo and time signature
    {
        let mut events = Vec::new();

        // Track name
        events.push(TimedEvent::new(
            0,
            MidiEvent::TrackName {
                name: project.name.clone(),
            },
            0,
        ));

        // Time signature at tick 0
        events.push(TimedEvent::new(
            0,
            MidiEvent::TimeSignature {
                numerator: project.time_sig_numerator,
                denominator_power: denominator_to_power(project.time_sig_denominator),
            },
            1,
        ));

        // Tempo at tick 0
        // Convert BPM to microseconds per beat: 60,000,000 / BPM
        let microseconds_per_beat = 60_000_000 / project.tempo;
        events.push(TimedEvent::new(
            0,
            MidiEvent::SetTempo {
                microseconds_per_beat,
            },
            2,
        ));

        // End of track
        events.push(TimedEvent::new(
            project.duration_ticks(),
            MidiEvent::EndOfTrack,
            255,
        ));

        let track_data = build_track_data(&mut events);
        write_track_chunk(&mut writer, &track_data)?;
    }

    // Tracks 1-N: Music data
    for track in project.tracks() {
        let mut events = Vec::new();

        // Track name
        events.push(TimedEvent::new(
            0,
            MidiEvent::TrackName {
                name: track.name.clone(),
            },
            0,
        ));

        // Program change at tick 0
        events.push(TimedEvent::new(
            0,
            MidiEvent::ProgramChange {
                channel: track.channel,
                program: track.program,
            },
            1,
        ));

        // Volume (CC 7) at tick 0
        events.push(TimedEvent::new(
            0,
            MidiEvent::ControlChange {
                channel: track.channel,
                controller: 7, // Volume
                value: track.volume,
            },
            2,
        ));

        // Pan (CC 10) at tick 0
        events.push(TimedEvent::new(
            0,
            MidiEvent::ControlChange {
                channel: track.channel,
                controller: 10, // Pan
                value: track.pan,
            },
            3,
        ));

        // Note events
        for note in track.notes() {
            // Note on
            events.push(TimedEvent::new(
                note.start_tick,
                MidiEvent::NoteOn {
                    channel: track.channel,
                    pitch: note.pitch,
                    velocity: note.velocity,
                },
                10, // Notes after setup events
            ));

            // Note off
            events.push(TimedEvent::new(
                note.end_tick(),
                MidiEvent::NoteOff {
                    channel: track.channel,
                    pitch: note.pitch,
                    velocity: 0,
                },
                11, // Note offs slightly after note ons at same tick
            ));
        }

        // End of track (at the end of all notes or duration)
        let track_end = track.duration_ticks().max(1);
        events.push(TimedEvent::new(track_end, MidiEvent::EndOfTrack, 255));

        let track_data = build_track_data(&mut events);
        write_track_chunk(&mut writer, &track_data)?;
    }

    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vlq_encoding() {
        let mut buffer = Vec::new();

        // Single byte values (0-127)
        write_vlq(0, &mut buffer);
        assert_eq!(buffer, vec![0x00]);
        buffer.clear();

        write_vlq(127, &mut buffer);
        assert_eq!(buffer, vec![0x7F]);
        buffer.clear();

        // Two byte values (128-16383)
        write_vlq(128, &mut buffer);
        assert_eq!(buffer, vec![0x81, 0x00]);
        buffer.clear();

        write_vlq(0x3FFF, &mut buffer);
        assert_eq!(buffer, vec![0xFF, 0x7F]);
        buffer.clear();

        // Three byte values
        write_vlq(0x4000, &mut buffer);
        assert_eq!(buffer, vec![0x81, 0x80, 0x00]);
        buffer.clear();
    }

    #[test]
    fn test_denominator_power() {
        assert_eq!(denominator_to_power(4), 2);
        assert_eq!(denominator_to_power(8), 3);
        assert_eq!(denominator_to_power(2), 1);
        assert_eq!(denominator_to_power(16), 4);
    }
}
