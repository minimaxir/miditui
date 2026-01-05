//! Audio engine for real-time MIDI synthesis.
//!
//! Provides a high-level interface for playing MIDI notes using
//! rustysynth for synthesis and rodio for audio output.

use crate::midi::{ticks_to_seconds, Track};
use anyhow::{Context, Result};
use rodio::{OutputStream, OutputStreamHandle, Source};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Sample rate for audio synthesis (44.1 kHz standard).
pub const SAMPLE_RATE: u32 = 44100;

/// Audio buffer size for low-latency playback.
/// Smaller = lower latency but higher CPU usage.
const BUFFER_SIZE: usize = 256;

/// Represents the current playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Not playing, position reset to start.
    Stopped,
    /// Currently playing.
    Playing,
    /// Paused at current position.
    Paused,
}

/// Shared state between the audio engine and the audio source.
/// Uses atomics for lock-free access from the audio thread.
struct SharedState {
    /// Current playback state.
    playing: AtomicBool,
    /// Current playback position in ticks.
    position_ticks: AtomicU32,
}

/// Audio source that generates samples from the synthesizer.
/// Implements rodio's Source trait for playback.
struct SynthSource {
    /// The synthesizer instance.
    synth: Arc<Mutex<Synthesizer>>,
    /// Left channel buffer.
    left_buf: Vec<f32>,
    /// Right channel buffer.
    right_buf: Vec<f32>,
    /// Current position in the buffer.
    buf_pos: usize,
    /// Current channel (0 = left, 1 = right).
    channel: usize,
}

impl SynthSource {
    fn new(synth: Arc<Mutex<Synthesizer>>) -> Self {
        Self {
            synth,
            left_buf: vec![0.0; BUFFER_SIZE],
            right_buf: vec![0.0; BUFFER_SIZE],
            buf_pos: BUFFER_SIZE, // Start at end to trigger first render
            channel: 0,
        }
    }
}

impl Iterator for SynthSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // Render a new buffer when we've exhausted the current one
        if self.buf_pos >= BUFFER_SIZE {
            // Always render from the synthesizer - it will output silence if no notes
            // are playing, but will properly render preview notes triggered via note_on
            // even when sequence playback is stopped.
            if let Ok(mut synth) = self.synth.lock() {
                synth.render(&mut self.left_buf, &mut self.right_buf);
            } else {
                // Only fill with silence if we can't get the lock
                self.left_buf.fill(0.0);
                self.right_buf.fill(0.0);
            }
            self.buf_pos = 0;
        }

        // Interleave stereo samples: L, R, L, R, ...
        let sample = if self.channel == 0 {
            self.left_buf[self.buf_pos]
        } else {
            self.right_buf[self.buf_pos]
        };

        // Advance to next channel/sample
        self.channel = 1 - self.channel;
        if self.channel == 0 {
            self.buf_pos += 1;
        }

        Some(sample)
    }
}

impl Source for SynthSource {
    fn current_frame_len(&self) -> Option<usize> {
        None // Continuous stream
    }

    fn channels(&self) -> u16 {
        2 // Stereo
    }

    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None // Infinite stream
    }
}

/// The main audio engine for MIDI synthesis and playback.
///
/// Manages the synthesizer, audio output, and playback state.
/// Supports real-time note playback and project sequencing.
pub struct AudioEngine {
    /// The synthesizer (wrapped for sharing with audio thread).
    synth: Arc<Mutex<Synthesizer>>,
    /// Shared playback state.
    state: Arc<SharedState>,
    /// Audio output stream (must be kept alive).
    _stream: OutputStream,
    /// Audio output handle for playback.
    _stream_handle: OutputStreamHandle,
    /// Current playback state.
    playback_state: PlaybackState,
    /// Current tempo for tick calculations.
    tempo: u32,
    /// Instrument names extracted from the loaded SoundFont.
    /// Indexed by program number (0-127). Falls back to "Program N" if not found.
    instrument_names: [String; 128],
}

impl AudioEngine {
    /// Creates a new audio engine with the specified SoundFont.
    ///
    /// # Arguments
    ///
    /// * `soundfont_path` - Path to the SoundFont file (.sf2)
    ///
    /// # Returns
    ///
    /// A new AudioEngine ready for playback
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - The SoundFont file cannot be read
    /// - The SoundFont is invalid
    /// - Audio output cannot be initialized
    pub fn new<P: AsRef<Path>>(soundfont_path: P) -> Result<Self> {
        // Load the SoundFont
        let mut file = BufReader::new(File::open(soundfont_path.as_ref()).with_context(|| {
            format!(
                "Failed to open SoundFont: {}",
                soundfont_path.as_ref().display()
            )
        })?);
        let soundfont = Arc::new(
            SoundFont::new(&mut file)
                .map_err(|e| anyhow::anyhow!("Failed to load SoundFont: {:?}", e))?,
        );

        let instrument_names = Self::extract_instrument_names(&soundfont);

        let settings = SynthesizerSettings::new(SAMPLE_RATE as i32);
        let synth = Synthesizer::new(&soundfont, &settings)
            .map_err(|e| anyhow::anyhow!("Failed to create synthesizer: {:?}", e))?;
        let synth = Arc::new(Mutex::new(synth));

        let state = Arc::new(SharedState {
            playing: AtomicBool::new(false),
            position_ticks: AtomicU32::new(0),
        });

        let (stream, stream_handle) =
            OutputStream::try_default().context("Failed to open audio output")?;

        let source = SynthSource::new(Arc::clone(&synth));
        stream_handle
            .play_raw(source)
            .context("Failed to start audio playback")?;

        Ok(Self {
            synth,
            state,
            _stream: stream,
            _stream_handle: stream_handle,
            playback_state: PlaybackState::Stopped,
            tempo: 120,
            instrument_names,
        })
    }

    /// Extracts instrument names from the SoundFont's presets.
    ///
    /// Maps program numbers (0-127) to preset names from bank 0 (General MIDI bank).
    /// If a program number has no preset in the SoundFont, falls back to "Program N".
    fn extract_instrument_names(soundfont: &SoundFont) -> [String; 128] {
        // Initialize with fallback names
        let mut names: [String; 128] = std::array::from_fn(|i| format!("Program {}", i));

        // Iterate through presets and map bank 0 presets to their program numbers
        for preset in soundfont.get_presets() {
            let bank = preset.get_bank_number();
            let program = preset.get_patch_number();

            // Only use presets from bank 0 (General MIDI) for the main instrument list
            if bank == 0 && (0..128).contains(&program) {
                names[program as usize] = preset.get_name().to_string();
            }
        }

        names
    }

    /// Returns the instrument name for a given program number.
    ///
    /// # Arguments
    ///
    /// * `program` - MIDI program number (0-127)
    ///
    /// # Returns
    ///
    /// The instrument name from the loaded SoundFont, or a fallback name.
    pub fn get_instrument_name(&self, program: u8) -> &str {
        &self.instrument_names[program as usize]
    }

    /// Plays a single note immediately.
    ///
    /// # Arguments
    ///
    /// * `channel` - MIDI channel (0-15)
    /// * `note` - MIDI note number (0-127)
    /// * `velocity` - Note velocity (0-127)
    pub fn note_on(&self, channel: u8, note: u8, velocity: u8) {
        if let Ok(mut synth) = self.synth.lock() {
            synth.note_on(channel as i32, note as i32, velocity as i32);
        }
    }

    /// Stops a playing note.
    ///
    /// # Arguments
    ///
    /// * `channel` - MIDI channel (0-15)
    /// * `note` - MIDI note number (0-127)
    pub fn note_off(&self, channel: u8, note: u8) {
        if let Ok(mut synth) = self.synth.lock() {
            synth.note_off(channel as i32, note as i32);
        }
    }

    /// Stops all playing notes.
    ///
    /// # Arguments
    ///
    /// * `immediate` - If true, notes stop immediately without release
    pub fn all_notes_off(&self, immediate: bool) {
        if let Ok(mut synth) = self.synth.lock() {
            synth.note_off_all(immediate);
        }
    }

    /// Sets the instrument (program) for a channel.
    ///
    /// # Arguments
    ///
    /// * `channel` - MIDI channel (0-15)
    /// * `program` - MIDI program number (0-127)
    pub fn set_program(&self, channel: u8, program: u8) {
        if let Ok(mut synth) = self.synth.lock() {
            // Program change is MIDI command 0xC0 (192)
            synth.process_midi_message(channel as i32, 0xC0, program as i32, 0);
        }
    }

    /// Sets the volume for a channel.
    ///
    /// # Arguments
    ///
    /// * `channel` - MIDI channel (0-15)
    /// * `volume` - Volume level (0-127)
    pub fn set_channel_volume(&self, channel: u8, volume: u8) {
        if let Ok(mut synth) = self.synth.lock() {
            // Control change 7 is volume
            synth.process_midi_message(channel as i32, 0xB0, 7, volume as i32);
        }
    }

    /// Sets the pan for a channel.
    ///
    /// # Arguments
    ///
    /// * `channel` - MIDI channel (0-15)
    /// * `pan` - Pan position (0=left, 64=center, 127=right)
    pub fn set_channel_pan(&self, channel: u8, pan: u8) {
        if let Ok(mut synth) = self.synth.lock() {
            // Control change 10 is pan
            synth.process_midi_message(channel as i32, 0xB0, 10, pan as i32);
        }
    }

    /// Alias for set_channel_volume.
    pub fn set_volume(&self, channel: u8, volume: u8) {
        self.set_channel_volume(channel, volume);
    }

    /// Alias for set_channel_pan.
    pub fn set_pan(&self, channel: u8, pan: u8) {
        self.set_channel_pan(channel, pan);
    }

    /// Configures the synth for a track's settings.
    ///
    /// # Arguments
    ///
    /// * `track` - The track to configure
    pub fn configure_track(&self, track: &Track) {
        self.set_program(track.channel, track.program);
        self.set_channel_volume(track.channel, track.volume);
        self.set_channel_pan(track.channel, track.pan);
    }

    /// Returns the current playback state.
    pub fn playback_state(&self) -> PlaybackState {
        self.playback_state
    }

    /// Returns whether audio is currently playing.
    pub fn is_playing(&self) -> bool {
        self.state.playing.load(Ordering::Relaxed)
    }

    /// Sets the playing state.
    pub fn set_playing(&mut self, playing: bool) {
        self.state.playing.store(playing, Ordering::Relaxed);
        self.playback_state = if playing {
            PlaybackState::Playing
        } else {
            PlaybackState::Paused
        };
    }

    /// Stops playback and resets position.
    pub fn stop(&mut self) {
        self.set_playing(false);
        self.all_notes_off(true);
        self.state.position_ticks.store(0, Ordering::Relaxed);
        self.playback_state = PlaybackState::Stopped;
    }

    /// Returns the current playback position in ticks.
    pub fn position_ticks(&self) -> u32 {
        self.state.position_ticks.load(Ordering::Relaxed)
    }

    /// Sets the playback position in ticks.
    pub fn set_position_ticks(&self, ticks: u32) {
        self.state.position_ticks.store(ticks, Ordering::Relaxed);
    }

    /// Converts the current position to seconds.
    #[allow(dead_code)]
    pub fn position_seconds(&self) -> f64 {
        ticks_to_seconds(self.position_ticks(), self.tempo)
    }

    /// Sets the tempo for position calculations.
    pub fn set_tempo(&mut self, tempo: u32) {
        self.tempo = tempo;
    }

    /// Returns the tempo.
    #[allow(dead_code)]
    pub fn tempo(&self) -> u32 {
        self.tempo
    }

    /// Resets all controllers and stops all notes.
    #[allow(dead_code)]
    pub fn reset(&self) {
        if let Ok(mut synth) = self.synth.lock() {
            synth.reset();
        }
    }

    /// Returns a reference to the synthesizer for rendering (used by export).
    #[allow(dead_code)]
    pub fn synth(&self) -> &Arc<Mutex<Synthesizer>> {
        &self.synth
    }
}
