//! Audio export functionality.
//!
//! Exports MIDI projects to WAV files by rendering the entire
//! composition through the synthesizer.

use crate::audio::engine::SAMPLE_RATE;
use crate::midi::{ticks_to_seconds, Project, TICKS_PER_BEAT};
use anyhow::{Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

/// Buffer size for rendering chunks.
/// Larger buffers are more efficient but use more memory.
const RENDER_BUFFER_SIZE: usize = 4096;

/// Exports a project to a WAV file (native only).
///
/// Renders the entire project through the synthesizer and writes
/// the resulting audio to a WAV file.
///
/// # Arguments
///
/// * `project` - The project to export
/// * `soundfont_path` - Path to the SoundFont file
/// * `output_path` - Path for the output WAV file
/// * `progress_callback` - Optional callback for progress updates (0.0 to 1.0)
///
/// # Returns
///
/// Ok(()) on success
///
/// # Errors
///
/// Returns error if:
/// - SoundFont cannot be loaded
/// - Output file cannot be created
/// - Rendering fails
pub fn export_to_wav<P1, P2, F>(
    project: &Project,
    soundfont_path: P1,
    output_path: P2,
    mut progress_callback: Option<F>,
) -> Result<()>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
    F: FnMut(f32),
{
    let mut sf_file = BufReader::new(File::open(soundfont_path.as_ref()).with_context(|| {
        format!(
            "Failed to open SoundFont for export: {}",
            soundfont_path.as_ref().display()
        )
    })?);
    let soundfont = Arc::new(
        SoundFont::new(&mut sf_file)
            .map_err(|e| anyhow::anyhow!("Failed to load SoundFont: {:?}", e))?,
    );

    let settings = SynthesizerSettings::new(SAMPLE_RATE as i32);
    let mut synth = Synthesizer::new(&soundfont, &settings)
        .map_err(|e| anyhow::anyhow!("Failed to create synthesizer: {:?}", e))?;

    // Calculate total duration with a small buffer at the end for note release
    let duration_ticks = project.duration_ticks();
    let duration_seconds = ticks_to_seconds(duration_ticks, project.tempo) + 2.0; // 2 sec buffer
    let total_samples = (duration_seconds * SAMPLE_RATE as f64) as usize;

    let spec = WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(output_path.as_ref(), spec).with_context(|| {
        format!(
            "Failed to create output WAV file: {}",
            output_path.as_ref().display()
        )
    })?;

    // Configure channels for each track
    for track in project.tracks() {
        if track.muted {
            continue;
        }
        // Set program (instrument) for each track's channel
        synth.process_midi_message(
            track.channel as i32,
            0xC0, // Program change
            track.program as i32,
            0,
        );
        // Set volume
        synth.process_midi_message(
            track.channel as i32,
            0xB0, // Control change
            7,    // Volume controller
            track.volume as i32,
        );
        // Set pan
        synth.process_midi_message(
            track.channel as i32,
            0xB0,
            10, // Pan controller
            track.pan as i32,
        );
    }

    // Collect and sort all note events across all playable tracks
    // An event is (tick, is_note_on, channel, pitch, velocity)
    let mut events: Vec<(u32, bool, u8, u8, u8)> = Vec::new();

    let any_solo = project.tracks().iter().any(|t| t.solo);

    for track in project.tracks() {
        // Skip muted tracks, or non-solo tracks when any track is soloed
        if track.muted || (any_solo && !track.solo) {
            continue;
        }

        for note in track.notes() {
            events.push((
                note.start_tick,
                true,
                track.channel,
                note.pitch,
                note.velocity,
            ));
            events.push((note.end_tick(), false, track.channel, note.pitch, 0));
        }
    }

    // Sort events by tick (stable sort preserves note-off before note-on at same tick)
    events.sort_by_key(|(tick, is_on, _, _, _)| (*tick, !*is_on));

    let mut left_buf = vec![0.0f32; RENDER_BUFFER_SIZE];
    let mut right_buf = vec![0.0f32; RENDER_BUFFER_SIZE];

    let mut current_sample = 0usize;
    let mut event_idx = 0usize;
    let samples_per_tick =
        SAMPLE_RATE as f64 * 60.0 / (project.tempo as f64 * TICKS_PER_BEAT as f64);

    while current_sample < total_samples {
        // Process any events that should occur before this buffer
        let current_tick = (current_sample as f64 / samples_per_tick) as u32;

        while event_idx < events.len() && events[event_idx].0 <= current_tick {
            let (_, is_note_on, channel, pitch, velocity) = events[event_idx];
            if is_note_on {
                synth.note_on(channel as i32, pitch as i32, velocity as i32);
            } else {
                synth.note_off(channel as i32, pitch as i32);
            }
            event_idx += 1;
        }

        // Determine buffer size for this iteration
        let samples_to_render = (total_samples - current_sample).min(RENDER_BUFFER_SIZE);

        // Render audio
        synth.render(
            &mut left_buf[..samples_to_render],
            &mut right_buf[..samples_to_render],
        );

        // Write to WAV (interleaved stereo, 16-bit)
        for i in 0..samples_to_render {
            // Convert f32 (-1.0 to 1.0) to i16
            let left_sample = (left_buf[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
            let right_sample = (right_buf[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(left_sample)?;
            writer.write_sample(right_sample)?;
        }

        current_sample += samples_to_render;

        if let Some(ref mut callback) = progress_callback {
            callback(current_sample as f32 / total_samples as f32);
        }
    }

    writer.finalize().context("Failed to finalize WAV file")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::Project;
    use std::path::PathBuf;

    #[test]
    #[ignore] // Requires SoundFont file
    fn test_export_simple_project() {
        let mut project = Project::new("Test");
        let track_id = project.create_track("Piano");
        let track = project.get_track_mut(track_id).unwrap();
        track.create_note(60, 100, 0, 480);
        track.create_note(64, 100, 480, 480);
        track.create_note(67, 100, 960, 480);

        let sf_path = PathBuf::from("assets/TimGM6mb.sf2");
        let output_path = PathBuf::from("test_output/test_export.wav");

        std::fs::create_dir_all("test_output").unwrap();

        export_to_wav(&project, sf_path, output_path, None::<fn(f32)>).unwrap();
    }
}
