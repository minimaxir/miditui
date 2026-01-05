//! miditui - A terminal-based MIDI composer and player.
//!
//! This library provides the core functionality for the MIDI composer app.

pub mod app;
pub mod audio;
pub mod history;
pub mod midi;
pub mod ui;

// Re-export commonly used types
pub use app::{App, EditMode, FocusedPanel, ViewMode};
pub use audio::{engine::AudioEngine, export::export_to_wav};
pub use midi::{Note, NoteId, Project, Track, TrackId, TICKS_PER_BEAT};
