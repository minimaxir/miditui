//! Audio engine for MIDI synthesis and playback.
//!
//! This module provides real-time MIDI synthesis using rustysynth
//! and audio output via rodio. It supports:
//! - Loading SoundFont files for instrument sounds
//! - Real-time note playback with low latency
//! - Multi-track synthesis with mixing
//! - WAV export functionality

pub mod engine;
pub mod export;

pub use engine::PlaybackState;
pub use export::export_to_wav;
