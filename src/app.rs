//! Application state and event handling.
//!
//! This module defines the main application state that coordinates
//! between the MIDI project, audio engine, and TUI interface.

use crate::audio::{engine::AudioEngine, engine::PlaybackState};
use crate::history::{HistoryManager, StateSnapshot};
use crate::midi::{note_to_name, NoteId, Project, TICKS_PER_BEAT};
use anyhow::Result;
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Autosave delay in seconds after last modification.
const AUTOSAVE_DELAY_SECS: u64 = 5;

/// Save file format options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveFormat {
    /// JSON project file (human-readable).
    #[default]
    Json,
    /// Binary project file (compact).
    Oxm,
    /// Standard MIDI file (portable, but loses project-specific metadata).
    Midi,
}

/// State for the save dialog.
#[derive(Debug, Clone, Default)]
pub struct SaveDialogState {
    /// Whether the dialog is open.
    pub open: bool,
    /// The filename being edited (without extension).
    pub filename: String,
    /// Selected save format.
    pub format: SaveFormat,
}

/// State for the file browser dialog.
#[derive(Debug, Clone)]
pub struct FileBrowserState {
    /// Whether the browser is open.
    pub open: bool,
    /// Current directory path.
    pub current_dir: std::path::PathBuf,
    /// List of entries in current directory.
    pub entries: Vec<std::path::PathBuf>,
    /// Currently selected index.
    pub selected: usize,
    /// Scroll offset for long lists.
    pub scroll: usize,
}

impl Default for FileBrowserState {
    fn default() -> Self {
        Self {
            open: false,
            current_dir: std::env::current_dir().unwrap_or_default(),
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
        }
    }
}

/// State for the new project confirmation dialog.
#[derive(Debug, Clone, Default)]
pub struct NewProjectDialogState {
    /// Whether the dialog is open.
    pub open: bool,
    /// Currently selected option (0 = Yes, 1 = No).
    pub selected: usize,
}

/// State for the SoundFont browser dialog.
/// Similar to FileBrowserState but filters for .sf2 files.
#[derive(Debug, Clone)]
pub struct SoundfontDialogState {
    /// Whether the browser is open.
    pub open: bool,
    /// Whether this is the initial "first load" modal (blocks other interactions).
    pub is_first_load: bool,
    /// Current directory path.
    pub current_dir: std::path::PathBuf,
    /// List of entries in current directory.
    pub entries: Vec<std::path::PathBuf>,
    /// Currently selected index.
    pub selected: usize,
    /// Scroll offset for long lists.
    pub scroll: usize,
}

impl Default for SoundfontDialogState {
    fn default() -> Self {
        Self {
            open: false,
            is_first_load: false,
            current_dir: std::env::current_dir().unwrap_or_default(),
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
        }
    }
}

/// Width of the piano key labels in the piano roll.
pub const PIANO_KEY_WIDTH: u16 = 5;

/// Height of the time ruler at the top of the piano roll grid (in rows).
/// This offset must be subtracted from mouse Y coordinates when converting
/// to pitch, since the ruler occupies the first row of the grid area.
const TIME_RULER_HEIGHT: u16 = 1;

/// Layout regions for mouse hit testing.
/// Stores the screen coordinates of each UI panel.
#[derive(Debug, Clone, Default)]
pub struct LayoutRegions {
    /// The timeline/transport bar at the top.
    pub timeline: Rect,
    /// The track list on the left side.
    pub track_list: Rect,
    /// The piano roll editor (main area).
    pub piano_roll: Rect,
    /// The piano roll grid area (excluding key labels).
    pub piano_roll_grid: Rect,
    /// The keyboard display at the bottom.
    pub keyboard: Rect,
    /// The Piano Roll time ruler area (set during rendering).
    pub piano_roll_ruler: Rect,
    /// The Project Timeline time ruler area (set during rendering).
    pub project_timeline_ruler: Rect,
    /// Number of visible pitch rows in the piano roll grid.
    /// Dynamically calculated based on terminal height.
    pub visible_pitches: u8,
}

impl LayoutRegions {
    /// Determines which panel contains the given screen coordinates.
    ///
    /// # Arguments
    ///
    /// * `x` - Screen X coordinate
    /// * `y` - Screen Y coordinate
    ///
    /// # Returns
    ///
    /// The panel at the given coordinates, or None if outside all panels
    pub fn panel_at(&self, x: u16, y: u16) -> Option<FocusedPanel> {
        if self.contains(self.timeline, x, y) {
            Some(FocusedPanel::Timeline)
        } else if self.contains(self.track_list, x, y) {
            Some(FocusedPanel::TrackList)
        } else if self.contains(self.piano_roll, x, y) {
            Some(FocusedPanel::PianoRoll)
        } else if self.contains(self.keyboard, x, y) {
            Some(FocusedPanel::Keyboard)
        } else {
            None
        }
    }

    /// Checks if a point is within a rectangle.
    fn contains(&self, rect: Rect, x: u16, y: u16) -> bool {
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    }

    /// Checks if a point is within the piano roll grid area.
    pub fn is_in_piano_roll_grid(&self, x: u16, y: u16) -> bool {
        self.contains(self.piano_roll_grid, x, y)
    }

    /// Checks if a point is within any time ruler and returns the relative X position.
    ///
    /// Returns `Some((relative_x, ruler_width))` if clicking on a ruler, `None` otherwise.
    /// The relative_x is the distance from the left edge of the ruler's grid area.
    pub fn ruler_hit_test(&self, x: u16, y: u16) -> Option<(u16, u16)> {
        // Check Piano Roll ruler first
        if self.contains(self.piano_roll_ruler, x, y) {
            let relative_x = x.saturating_sub(self.piano_roll_ruler.x);
            return Some((relative_x, self.piano_roll_ruler.width));
        }
        // Check Project Timeline ruler
        if self.contains(self.project_timeline_ruler, x, y) {
            let relative_x = x.saturating_sub(self.project_timeline_ruler.x);
            return Some((relative_x, self.project_timeline_ruler.width));
        }
        None
    }
}

/// Mouse drag state for selection operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    /// Not currently dragging.
    None,
    /// Dragging to select notes in piano roll.
    SelectingNotes { start_x: u16, start_y: u16 },
    /// Dragging to scroll the view.
    Scrolling { last_x: u16, last_y: u16 },
    /// Dragging selected notes to move them.
    MovingNotes {
        /// Last mouse X position for delta calculation.
        last_x: u16,
        /// Last mouse Y position for delta calculation.
        last_y: u16,
        /// Original tick position when drag started (for snapping).
        start_tick: u32,
        /// Original pitch when drag started.
        start_pitch: u8,
    },
}

/// Default note velocity for new notes.
pub const DEFAULT_VELOCITY: u8 = 100;

/// Default note duration in ticks (quarter note).
pub const DEFAULT_NOTE_DURATION: u32 = TICKS_PER_BEAT;

/// The currently focused UI panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    /// Track list on the left side.
    TrackList,
    /// Timeline/arrangement view.
    Timeline,
    /// Piano roll editor for selected track.
    PianoRoll,
    /// Interactive keyboard at the bottom.
    Keyboard,
}

/// The current editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// Normal mode - navigation and selection.
    Normal,
    /// Insert mode - placing new notes.
    Insert,
    /// Select mode - selecting notes for editing.
    Select,
}

/// The current view mode for the main content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Combined view - shows both piano roll and project timeline split horizontally.
    /// This is the default view for comprehensive editing.
    #[default]
    Combined,
    /// Piano roll view - edit notes for selected track.
    PianoRoll,
    /// Project timeline view - shows all tracks on a combined timeline.
    ProjectTimeline,
}

/// Highlight mode for active notes during playback.
/// Controls which views show white highlighting for notes being played.
/// Cycled with Shift+W in the order: PianoRollOnly -> Both -> Off -> TimelineOnly -> repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HighlightMode {
    /// Only highlight active notes in the Piano Roll view (default).
    #[default]
    PianoRollOnly,
    /// Highlight active notes in both Piano Roll and Project Timeline views.
    Both,
    /// No highlighting in either view.
    Off,
    /// Only highlight active tracks in the Project Timeline view.
    TimelineOnly,
}

/// Keyboard key to MIDI note mapping for the computer keyboard.
/// Uses a piano-like layout on QWERTY keyboards.
pub const KEYBOARD_MAP: [(char, u8); 25] = [
    // Lower row (Z-M) = C3 to B3
    ('z', 48), // C3
    ('s', 49), // C#3
    ('x', 50), // D3
    ('d', 51), // D#3
    ('c', 52), // E3
    ('v', 53), // F3
    ('g', 54), // F#3
    ('b', 55), // G3
    ('h', 56), // G#3
    ('n', 57), // A3
    ('j', 58), // A#3
    ('m', 59), // B3
    // Upper row (Q-U) = C4 to B4
    ('q', 60), // C4 (Middle C)
    ('2', 61), // C#4
    ('w', 62), // D4
    ('3', 63), // D#4
    ('e', 64), // E4
    ('r', 65), // F4
    ('5', 66), // F#4
    ('t', 67), // G4
    ('6', 68), // G#4
    ('y', 69), // A4
    ('7', 70), // A#4
    ('u', 71), // B4
    ('i', 72), // C5
];

/// Main application state.
pub struct App {
    /// The MIDI project being edited.
    project: Project,
    /// The audio engine for playback and preview.
    pub audio: AudioEngine,
    /// Path to the loaded SoundFont.
    pub soundfont_path: PathBuf,
    /// Currently focused UI panel.
    pub focused_panel: FocusedPanel,
    /// Current editing mode.
    pub edit_mode: EditMode,
    /// Current view mode (piano roll or project timeline).
    pub view_mode: ViewMode,
    /// Index of the selected track in the track list.
    pub selected_track_index: usize,
    /// Currently selected notes (for multi-select editing).
    pub selected_notes: HashSet<NoteId>,
    /// Current cursor position in the piano roll (tick).
    pub cursor_tick: u32,
    /// Current cursor pitch in the piano roll.
    pub cursor_pitch: u8,
    /// Horizontal scroll position in ticks.
    pub scroll_x: u32,
    /// Vertical scroll position (lowest visible pitch).
    pub scroll_y: u8,
    /// Zoom level for the timeline (ticks per column).
    pub zoom: u32,
    /// Notes currently being held down via keyboard.
    held_notes: HashSet<u8>,
    /// Octave offset for keyboard input.
    pub octave_offset: i8,
    /// Status message to display.
    pub status_message: Option<(String, Instant)>,
    /// Last tick where notes were triggered for sequencer.
    /// None means we haven't processed any notes yet (first frame after play).
    last_sequencer_tick: Option<u32>,
    /// Time when playback started (for position calculation).
    playback_start_time: Option<Instant>,
    /// Tick position when playback started.
    playback_start_tick: u32,
    /// Whether we're currently exporting.
    pub exporting: bool,
    /// Layout regions for mouse hit testing (updated each frame).
    pub layout: LayoutRegions,
    /// Current mouse drag state.
    pub drag_state: DragState,
    /// Whether we're currently renaming a track.
    pub renaming_track: bool,
    /// Buffer for track rename input.
    pub rename_buffer: String,
    /// Whether to show expanded track view (two lines per track).
    pub expanded_tracks: bool,
    /// Tracks currently playing audio (track indices with active notes).
    /// Updated during sequencer playback for visual feedback.
    pub active_tracks: HashSet<usize>,
    /// Path to the current project file (None if unsaved).
    pub project_path: Option<PathBuf>,
    /// Last time the project was modified (for autosave).
    last_modified: Option<Instant>,
    /// Last time autosave was performed.
    last_autosave: Option<Instant>,
    /// Path to the autosave file.
    autosave_path: PathBuf,
    /// Save dialog state.
    pub save_dialog: SaveDialogState,
    /// File browser state for loading.
    pub file_browser: FileBrowserState,
    /// New project confirmation dialog state.
    pub new_project_dialog: NewProjectDialogState,
    /// Soundfont browser dialog state.
    pub soundfont_dialog: SoundfontDialogState,
    /// Highlight mode for active notes during playback.
    /// Controls which views show white highlighting for notes being played.
    pub highlight_mode: HighlightMode,
    /// Display offset in ticks to compensate for rendering latency.
    /// The visual playhead is advanced by this many ticks to appear synchronized
    /// with the audio output. Set to 0 to disable.
    pub display_offset_ticks: u32,
    /// Help menu scroll offset (for viewing all shortcuts).
    pub help_scroll: u16,

    /// Undo/redo history manager.
    /// Stores up to 8 snapshots of the project state for undo/redo functionality.
    history: HistoryManager,

    // ==================== Insert Mode Recording State ====================
    // These fields manage the real-time recording behavior in Insert Mode,
    // where the indicator line moves and notes are placed at the current time.
    /// Whether Insert Mode recording is active (indicator line is moving).
    /// Becomes true when first piano key is pressed in Insert Mode,
    /// becomes false after 2 measures pass with no note input.
    pub insert_recording_active: bool,
    /// Time when Insert Mode recording started.
    insert_recording_start_time: Option<Instant>,
    /// Tick position when Insert Mode recording started.
    insert_recording_start_tick: u32,
    /// Time when the last note was inserted in Insert Mode recording.
    /// Used to detect 2 measures of silence to stop recording.
    last_insert_note_time: Option<Instant>,

    // ==================== Recently Added Note State ====================
    // Tracks the single most recently added note for visual highlighting.
    // The note is highlighted blue until a new note is added in a different beat.
    /// The beat (tick / TICKS_PER_BEAT) where the note was most recently added.
    /// Used to determine when to clear the highlighting.
    recently_added_beat: Option<u32>,
    /// NoteId and tick of the most recently added note (for blue highlighting in piano roll).
    /// Stores (NoteId, start_tick) to verify the note is at the expected position.
    pub recently_added_note: Option<(NoteId, u32)>,
    /// Pitch of the most recently added note (for blue highlighting on keyboard).
    pub recently_added_pitch: Option<u8>,
}

impl App {
    /// Creates a new application with the specified SoundFont (native only).
    ///
    /// # Arguments
    ///
    /// * `soundfont_path` - Path to the SoundFont file
    ///
    /// # Returns
    ///
    /// A new App ready for use
    ///
    /// # Errors
    ///
    /// Returns error if the audio engine cannot be initialized
    pub fn new(soundfont_path: PathBuf) -> Result<Self> {
        let audio = AudioEngine::new(&soundfont_path)?;

        Ok(Self {
            project: Project::with_default_track("New Project"),
            audio,
            soundfont_path,
            focused_panel: FocusedPanel::PianoRoll,
            edit_mode: EditMode::Normal,
            view_mode: ViewMode::default(),
            selected_track_index: 0,
            selected_notes: HashSet::new(),
            cursor_tick: 0,
            cursor_pitch: 60, // Middle C
            scroll_x: 0,
            scroll_y: 48,             // Start viewing from C3
            zoom: TICKS_PER_BEAT / 4, // 4 columns per beat
            held_notes: HashSet::new(),
            octave_offset: 0,
            status_message: None,
            last_sequencer_tick: None,
            playback_start_time: None,
            playback_start_tick: 0,
            exporting: false,
            layout: LayoutRegions::default(),
            drag_state: DragState::None,
            renaming_track: false,
            rename_buffer: String::new(),
            expanded_tracks: true, // Two-line track view enabled by default
            active_tracks: HashSet::new(),
            project_path: None,
            last_modified: None,
            last_autosave: None,
            autosave_path: PathBuf::from(".autosave.oxm"),
            save_dialog: SaveDialogState::default(),
            file_browser: FileBrowserState::default(),
            new_project_dialog: NewProjectDialogState::default(),
            soundfont_dialog: SoundfontDialogState::default(),
            highlight_mode: HighlightMode::default(), // Piano roll highlighting on by default
            display_offset_ticks: 12, // ~25ms at 120 BPM to compensate for display latency
            help_scroll: 0,
            history: HistoryManager::new(),
            // Insert Mode recording state
            insert_recording_active: false,
            insert_recording_start_time: None,
            insert_recording_start_tick: 0,
            last_insert_note_time: None,
            // Recently added note state
            recently_added_beat: None,
            recently_added_note: None,
            recently_added_pitch: None,
        })
    }

    // ==================== Accessor methods ====================
    // These methods provide a stable public API for the App struct.
    // Some are not called internally but are part of the public interface.

    /// Returns a reference to the project.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Returns a mutable reference to the project.
    pub fn project_mut(&mut self) -> &mut Project {
        &mut self.project
    }

    /// Adjusts the duration of all selected notes.
    ///
    /// # Arguments
    ///
    /// * `delta` - Amount to add to duration (negative to reduce)
    pub fn adjust_selected_notes_duration(&mut self, delta: i32) {
        if self.selected_notes.is_empty() {
            return;
        }
        self.save_state("Adjust note duration");
        let ids: Vec<_> = self.selected_notes.iter().copied().collect();
        if let Some(track) = self.project.track_at_mut(self.selected_track_index) {
            for note in track.notes_mut() {
                if ids.contains(&note.id) {
                    let new_duration = (note.duration_ticks as i32 + delta).max(1) as u32;
                    note.duration_ticks = new_duration;
                }
            }
        }
        self.mark_modified();
    }

    /// Transposes all selected notes by a number of semitones.
    ///
    /// # Arguments
    ///
    /// * `semitones` - Amount to transpose (positive = up, negative = down)
    pub fn transpose_selected_notes(&mut self, semitones: i8) {
        if self.selected_notes.is_empty() {
            return;
        }
        self.save_state("Transpose notes");
        let ids: Vec<_> = self.selected_notes.iter().copied().collect();
        if let Some(track) = self.project.track_at_mut(self.selected_track_index) {
            for note in track.notes_mut() {
                if ids.contains(&note.id) {
                    let new_pitch = (note.pitch as i16 + semitones as i16).clamp(0, 127) as u8;
                    note.pitch = new_pitch;
                }
            }
        }
        self.mark_modified();
    }

    /// Moves all selected notes horizontally by a number of ticks.
    ///
    /// # Arguments
    ///
    /// * `ticks` - Amount to move (positive = right/later, negative = left/earlier)
    pub fn move_selected_notes_horizontal(&mut self, ticks: i32) {
        if self.selected_notes.is_empty() {
            return;
        }
        self.save_state("Move notes");
        let ids: Vec<_> = self.selected_notes.iter().copied().collect();
        if let Some(track) = self.project.track_at_mut(self.selected_track_index) {
            for note in track.notes_mut() {
                if ids.contains(&note.id) {
                    if ticks < 0 {
                        note.start_tick = note.start_tick.saturating_sub((-ticks) as u32);
                    } else {
                        note.start_tick = note.start_tick.saturating_add(ticks as u32);
                    }
                }
            }
        }
        self.mark_modified();
    }

    /// Moves all selected notes horizontally without saving undo state.
    /// Used during drag operations where undo is saved at drag start/end.
    fn move_selected_notes_horizontal_no_undo(&mut self, ticks: i32) {
        if self.selected_notes.is_empty() {
            return;
        }
        let ids: Vec<_> = self.selected_notes.iter().copied().collect();
        if let Some(track) = self.project.track_at_mut(self.selected_track_index) {
            for note in track.notes_mut() {
                if ids.contains(&note.id) {
                    if ticks < 0 {
                        note.start_tick = note.start_tick.saturating_sub((-ticks) as u32);
                    } else {
                        note.start_tick = note.start_tick.saturating_add(ticks as u32);
                    }
                }
            }
        }
    }

    /// Transposes all selected notes without saving undo state.
    /// Used during drag operations where undo is saved at drag start/end.
    fn transpose_selected_notes_no_undo(&mut self, semitones: i8) {
        if self.selected_notes.is_empty() {
            return;
        }
        let ids: Vec<_> = self.selected_notes.iter().copied().collect();
        if let Some(track) = self.project.track_at_mut(self.selected_track_index) {
            for note in track.notes_mut() {
                if ids.contains(&note.id) {
                    let new_pitch = (note.pitch as i16 + semitones as i16).clamp(0, 127) as u8;
                    note.pitch = new_pitch;
                }
            }
        }
    }

    /// Updates the layout regions based on current terminal size.
    /// Called by the UI module during rendering.
    pub fn update_layout(&mut self, layout: LayoutRegions) {
        self.layout = layout;
    }

    /// Returns the currently selected track, if any.
    pub fn selected_track(&self) -> Option<&crate::midi::Track> {
        self.project.track_at(self.selected_track_index)
    }

    /// Returns a mutable reference to the currently selected track.
    pub fn selected_track_mut(&mut self) -> Option<&mut crate::midi::Track> {
        self.project.track_at_mut(self.selected_track_index)
    }

    /// Sets a status message to display temporarily.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some((message.into(), Instant::now()));
    }

    /// Clears expired status messages.
    pub fn clear_expired_status(&mut self) {
        if let Some((_, time)) = &self.status_message {
            if time.elapsed() > Duration::from_secs(3) {
                self.status_message = None;
            }
        }
    }

    /// Handles a keyboard key press for note input (native only).
    ///
    /// In Insert Mode, this implements a real-time recording system:
    /// - First key press starts the recording (indicator line starts moving)
    /// - Notes are placed at the current recording position based on elapsed time
    /// - Multiple simultaneous key presses add notes at the same tick position
    /// - Recording stops after 2 measures of no input (handled in update_insert_recording)
    ///
    /// # Arguments
    ///
    /// * `key` - The character key pressed
    ///
    /// # Returns
    ///
    /// true if the key was handled as a note
    pub fn handle_note_key(&mut self, key: char) -> bool {
        let key_lower = key.to_ascii_lowercase();

        // Find the note for this key
        for (k, base_note) in KEYBOARD_MAP.iter() {
            if *k == key_lower {
                let note = (*base_note as i16 + self.octave_offset as i16 * 12) as u8;
                if note > 127 {
                    return false;
                }

                let channel = self.selected_track().map(|t| t.channel).unwrap_or(0);
                let already_held = self.held_notes.contains(&note);

                // In Insert Mode, allow repeated presses of the same key by re-triggering
                // the note. This works around terminals that don't send key release events.
                if self.edit_mode == EditMode::Insert {
                    // If note is already held, send note_off first to create clean attack
                    if already_held {
                        self.audio.note_off(channel, note);
                    } else {
                        self.held_notes.insert(note);
                    }

                    // Play the note
                    self.audio.note_on(channel, note, DEFAULT_VELOCITY);

                    let now = Instant::now();

                    // Start recording if not already active
                    if !self.insert_recording_active {
                        self.insert_recording_active = true;
                        self.insert_recording_start_time = Some(now);
                        self.insert_recording_start_tick = self.cursor_tick;
                    }

                    // Calculate the current tick position based on elapsed time
                    // This allows simultaneous notes to be placed at the same position
                    let insert_tick = self.get_insert_recording_tick();

                    self.save_state("Insert note");
                    let note_id = self.selected_track_mut().map(|track| {
                        track.create_note(
                            note,
                            DEFAULT_VELOCITY,
                            insert_tick,
                            DEFAULT_NOTE_DURATION,
                        )
                    });

                    // Register the note for blue highlighting and auto-scroll
                    if let Some(id) = note_id {
                        self.register_added_note(id, note, insert_tick);
                    }

                    // Update last note time for timeout detection
                    self.last_insert_note_time = Some(now);

                    // Update cursor to follow recording position
                    self.cursor_tick = insert_tick;

                    self.mark_modified();
                    return true;
                }

                // Normal/Select mode: only trigger if not already held
                if !already_held {
                    self.held_notes.insert(note);
                    self.audio.note_on(channel, note, DEFAULT_VELOCITY);
                    return true;
                }
            }
        }
        false
    }

    /// Calculates the current tick position for Insert Mode recording.
    ///
    /// Based on elapsed time since recording started and the project tempo,
    /// determines where new notes should be placed. This allows multiple
    /// simultaneous key presses to add notes at the same position.
    ///
    /// # Returns
    ///
    /// The tick position where new notes should be inserted
    fn get_insert_recording_tick(&self) -> u32 {
        if let Some(start_time) = self.insert_recording_start_time {
            let elapsed = start_time.elapsed();
            let elapsed_secs = elapsed.as_secs_f64();

            // Convert elapsed time to ticks based on tempo
            // beats_per_second = tempo / 60
            // ticks_per_second = beats_per_second * TICKS_PER_BEAT
            let tempo = self.project.tempo as f64;
            let ticks_per_second = (tempo / 60.0) * TICKS_PER_BEAT as f64;
            let elapsed_ticks = (elapsed_secs * ticks_per_second) as u32;

            self.insert_recording_start_tick + elapsed_ticks
        } else {
            self.cursor_tick
        }
    }

    /// Returns the current Insert Mode recording position for display.
    ///
    /// This is used by the piano roll to show the moving indicator line.
    /// If recording is active, returns the calculated position based on elapsed time.
    /// Otherwise returns None (indicator should be static at cursor position).
    pub fn get_insert_indicator_tick(&self) -> Option<u32> {
        if self.insert_recording_active && self.edit_mode == EditMode::Insert {
            Some(self.get_insert_recording_tick())
        } else {
            None
        }
    }

    /// Updates Insert Mode recording state, checking for timeout.
    ///
    /// If recording is active and 2 measures have passed with no note input,
    /// stops the recording. This should be called in the main update loop.
    pub fn update_insert_recording(&mut self) {
        if !self.insert_recording_active || self.edit_mode != EditMode::Insert {
            return;
        }

        if let Some(last_note_time) = self.last_insert_note_time {
            // Calculate duration of 2 measures in seconds based on tempo and time signature
            // A measure is (time_sig_numerator) beats, so 2 measures = 2 * numerator beats
            let tempo = self.project.tempo as f64;
            let beats_per_measure = self.project.time_sig_numerator as f64;
            let beats_for_timeout = 2.0 * beats_per_measure;
            let seconds_per_beat = 60.0 / tempo;
            let timeout_duration = Duration::from_secs_f64(beats_for_timeout * seconds_per_beat);

            if last_note_time.elapsed() > timeout_duration {
                // Stop recording, update cursor to final position
                self.cursor_tick = self.get_insert_recording_tick();
                self.insert_recording_active = false;
                self.insert_recording_start_time = None;
                self.last_insert_note_time = None;
                self.set_status("Recording stopped (2 measures idle)");
            }
        }
    }

    /// Stops Insert Mode recording if active.
    ///
    /// Called when exiting Insert Mode or when the user explicitly stops recording.
    pub fn stop_insert_recording(&mut self) {
        if self.insert_recording_active {
            // Update cursor to final position before stopping
            self.cursor_tick = self.get_insert_recording_tick();
        }
        self.insert_recording_active = false;
        self.insert_recording_start_time = None;
        self.last_insert_note_time = None;
    }

    // ==================== Recently Added Note Tracking ====================

    /// Registers a newly added note for visual highlighting.
    ///
    /// This method tracks the most recently added note so it can be highlighted
    /// blue in the piano roll and keyboard. Only the single most recently added
    /// note is highlighted. If the new note is in a different beat than the
    /// previous note, the old highlighting is cleared first.
    ///
    /// Also scrolls the viewport to ensure the note is visible.
    ///
    /// # Arguments
    ///
    /// * `note_id` - The ID of the newly added note
    /// * `pitch` - The MIDI pitch of the note (for keyboard highlighting)
    /// * `tick` - The tick position where the note was added
    pub fn register_added_note(&mut self, note_id: NoteId, pitch: u8, tick: u32) {
        let new_beat = tick / TICKS_PER_BEAT;

        // Clear old highlighting if we're in a different beat
        if let Some(prev_beat) = self.recently_added_beat {
            if prev_beat != new_beat {
                self.recently_added_note = None;
                self.recently_added_pitch = None;
            }
        }

        // Track this note (replacing any previous note in the same beat)
        self.recently_added_beat = Some(new_beat);
        self.recently_added_note = Some((note_id, tick));
        self.recently_added_pitch = Some(pitch);

        // Scroll viewport to show the note if it's not visible
        self.scroll_to_note(tick, pitch);
    }

    /// Scrolls the piano roll viewport to ensure a note at the given position is visible.
    ///
    /// # Arguments
    ///
    /// * `tick` - The tick position of the note
    /// * `pitch` - The MIDI pitch of the note
    fn scroll_to_note(&mut self, tick: u32, pitch: u8) {
        // Get viewport dimensions from layout (dynamically calculated based on terminal size)
        let visible_pitches = self.layout.visible_pitches.max(1);

        // Estimate visible ticks based on current zoom
        // A reasonable estimate is 60-80 columns for the grid
        let estimated_grid_width = 70u32;
        let visible_ticks = self.zoom * estimated_grid_width;

        // Check horizontal visibility
        let tick_visible = tick >= self.scroll_x && tick < self.scroll_x + visible_ticks;
        if !tick_visible {
            // Scroll horizontally to center the note
            let half_visible = visible_ticks / 2;
            self.scroll_x = tick.saturating_sub(half_visible);
        }

        // Check vertical visibility
        let pitch_min = self.scroll_y;
        let pitch_max = self.scroll_y.saturating_add(visible_pitches);
        let pitch_visible = pitch >= pitch_min && pitch < pitch_max;
        if !pitch_visible {
            // Scroll vertically to center the note
            let half_visible = visible_pitches / 2;
            self.scroll_y = pitch.saturating_sub(half_visible);
        }

        // Also update cursor to the note position
        self.cursor_tick = tick;
        self.cursor_pitch = pitch;
    }

    /// Checks if a note matches the recently added note.
    ///
    /// Verifies both the NoteId AND the tick position match to ensure
    /// the correct note is highlighted even after viewport scrolling.
    ///
    /// # Arguments
    ///
    /// * `note_id` - The note ID to check
    /// * `start_tick` - The start tick of the note to verify position
    ///
    /// # Returns
    ///
    /// true if this is the recently added note
    pub fn is_recently_added_note(&self, note_id: NoteId, start_tick: u32) -> bool {
        if let Some((recent_id, recent_tick)) = self.recently_added_note {
            recent_id == note_id && recent_tick == start_tick
        } else {
            false
        }
    }

    /// Checks if a pitch matches the recently added note's pitch.
    ///
    /// # Arguments
    ///
    /// * `pitch` - The MIDI pitch to check
    ///
    /// # Returns
    ///
    /// true if this pitch matches the recently added note
    pub fn is_recently_added_pitch(&self, pitch: u8) -> bool {
        self.recently_added_pitch == Some(pitch)
    }

    /// Handles a keyboard key release (native only).
    ///
    /// # Arguments
    ///
    /// * `key` - The character key released
    pub fn handle_note_key_release(&mut self, key: char) {
        let key_lower = key.to_ascii_lowercase();

        for (k, base_note) in KEYBOARD_MAP.iter() {
            if *k == key_lower {
                let note = (*base_note as i16 + self.octave_offset as i16 * 12) as u8;
                if self.held_notes.remove(&note) {
                    let channel = self.selected_track().map(|t| t.channel).unwrap_or(0);
                    self.audio.note_off(channel, note);
                }
            }
        }
    }

    /// Releases all held notes (native only).
    pub fn release_all_notes(&mut self) {
        // Get channel first, then drain notes
        let channel = self.selected_track().map(|t| t.channel).unwrap_or(0);
        for note in self.held_notes.drain() {
            self.audio.note_off(channel, note);
        }
    }

    /// Toggles play/pause state (native only).
    pub fn toggle_playback(&mut self) {
        match self.audio.playback_state() {
            PlaybackState::Playing => {
                self.audio.set_playing(false);
                self.audio.all_notes_off(false);
                self.playback_start_time = None;
                self.set_status("Paused");
            }
            PlaybackState::Paused | PlaybackState::Stopped => {
                // Configure all tracks before playing
                for track in self.project.tracks() {
                    self.audio.configure_track(track);
                }
                self.audio.set_tempo(self.project.tempo);
                self.playback_start_time = Some(Instant::now());
                let current_position = self.audio.position_ticks();
                self.playback_start_tick = current_position;
                // Use Some(current_position) when resuming from non-zero position
                // to prevent re-triggering notes that already played.
                // Use None only when starting from the beginning (position 0)
                // so notes at tick 0 will be triggered.
                self.last_sequencer_tick = if current_position == 0 {
                    None
                } else {
                    Some(current_position)
                };
                self.audio.set_playing(true);
                self.set_status("Playing");
            }
        }
    }

    /// Stops playback and resets to beginning (native version with audio engine).
    pub fn stop_playback(&mut self) {
        self.audio.stop();
        self.playback_start_time = None;
        self.cursor_tick = 0;
        self.scroll_x = 0;
        self.set_status("Stopped");
    }

    /// Restarts playback from the beginning of the song (native version with audio engine).
    /// Stops current playback, resets position, and immediately starts playing.
    pub fn restart_playback(&mut self) {
        // Stop and reset position
        self.audio.stop();
        self.cursor_tick = 0;
        self.scroll_x = 0;

        // Configure all tracks before playing
        for track in self.project.tracks() {
            self.audio.configure_track(track);
        }
        self.audio.set_tempo(self.project.tempo);
        self.playback_start_time = Some(Instant::now());
        self.playback_start_tick = 0;
        self.last_sequencer_tick = None;
        self.audio.set_playing(true);
        self.set_status("Restarting from beginning");
    }

    /// Updates the sequencer, triggering notes at the current position (native only).
    /// Should be called regularly during playback.
    /// Also updates the active_tracks set for visual feedback in the project view.
    pub fn update_sequencer(&mut self) {
        // Clear active tracks when not playing
        if !self.audio.is_playing() {
            self.active_tracks.clear();
            return;
        }

        // Calculate current tick based on elapsed time
        if let Some(start_time) = self.playback_start_time {
            let elapsed = start_time.elapsed().as_secs_f64();
            let ticks_elapsed =
                (elapsed * self.project.tempo as f64 / 60.0 * TICKS_PER_BEAT as f64) as u32;
            let current_tick = self.playback_start_tick + ticks_elapsed;

            // Update position
            self.audio.set_position_ticks(current_tick);
            self.cursor_tick = current_tick;

            // Clear active tracks and recalculate
            self.active_tracks.clear();

            // Trigger notes between last_sequencer_tick and current_tick
            // If last_sequencer_tick is None, this is the first frame - trigger notes at start
            let any_solo = self.project.tracks().iter().any(|t| t.solo);

            for (track_idx, track) in self.project.tracks().iter().enumerate() {
                if track.muted || (any_solo && !track.solo) {
                    continue;
                }

                // Check if any note is currently active for this track
                let has_active_note = track.notes().iter().any(|n| n.is_active_at(current_tick));
                if has_active_note {
                    self.active_tracks.insert(track_idx);
                }

                for note in track.notes() {
                    // Note on: trigger if in range (last_tick, current_tick]
                    // On first frame (None), trigger all notes with start_tick <= current_tick
                    let should_note_on = match self.last_sequencer_tick {
                        None => note.start_tick <= current_tick,
                        Some(last) => note.start_tick > last && note.start_tick <= current_tick,
                    };
                    if should_note_on {
                        self.audio.note_on(track.channel, note.pitch, note.velocity);
                    }

                    // Note off: trigger if in range (last_tick, current_tick]
                    let should_note_off = match self.last_sequencer_tick {
                        None => note.end_tick() <= current_tick && note.end_tick() > 0,
                        Some(last) => note.end_tick() > last && note.end_tick() <= current_tick,
                    };
                    if should_note_off {
                        self.audio.note_off(track.channel, note.pitch);
                    }
                }
            }

            self.last_sequencer_tick = Some(current_tick);

            // Auto-scroll to follow playback
            // Use actual layout width if available, accounting for view mode
            let visible_cols = if self.layout.piano_roll_grid.width > 0 {
                // Actual grid width from layout (already excludes borders and piano keys)
                self.layout.piano_roll_grid.width as u32
            } else {
                60 // Reasonable fallback before first render
            };
            let visible_ticks = self.zoom * visible_cols;
            if current_tick > self.scroll_x + visible_ticks * 3 / 4 {
                self.scroll_x = current_tick.saturating_sub(visible_ticks / 4);
            }

            // Check if we've reached the end
            let end_tick = self.project.duration_ticks();
            if current_tick > end_tick + TICKS_PER_BEAT * 2 {
                self.stop_playback();
            }
        }
    }

    /// Adds a new track to the project.
    pub fn add_track(&mut self) {
        self.save_state("Add track");
        let track_num = self.project.track_count() + 1;
        self.project.create_track(format!("Track {}", track_num));
        self.selected_track_index = self.project.track_count() - 1;
        self.set_status(format!("Added Track {}", track_num));
        self.mark_modified();
    }

    /// Deletes the currently selected track.
    pub fn delete_selected_track(&mut self) {
        if self.project.track_count() <= 1 {
            self.set_status("Cannot delete the last track");
            return;
        }

        if let Some(track) = self.selected_track() {
            let name = track.name.clone();
            let id = track.id;
            self.save_state("Delete track");
            self.project.remove_track(id);
            if self.selected_track_index >= self.project.track_count() {
                self.selected_track_index = self.project.track_count() - 1;
            }
            self.set_status(format!("Deleted {}", name));
            self.mark_modified();
        }
    }

    /// Starts renaming the currently selected track.
    /// Initializes the rename buffer with the current track name.
    pub fn start_rename_track(&mut self) {
        if let Some(track) = self.selected_track() {
            self.rename_buffer = track.name.clone();
            self.renaming_track = true;
            self.set_status("Renaming track - Enter to confirm, Esc to cancel");
        }
    }

    /// Handles a character input during track rename.
    ///
    /// # Arguments
    ///
    /// * `c` - The character to add to the rename buffer
    pub fn rename_track_input(&mut self, c: char) {
        if self.renaming_track && self.rename_buffer.len() < 32 {
            self.rename_buffer.push(c);
        }
    }

    /// Handles backspace during track rename.
    pub fn rename_track_backspace(&mut self) {
        if self.renaming_track {
            self.rename_buffer.pop();
        }
    }

    /// Confirms the track rename and applies the new name.
    pub fn confirm_rename_track(&mut self) {
        if self.renaming_track {
            let new_name = self.rename_buffer.trim().to_string();
            if !new_name.is_empty() {
                self.save_state("Rename track");
                if let Some(track) = self.selected_track_mut() {
                    track.name = new_name.clone();
                }
                self.set_status(format!("Renamed to: {}", new_name));
                self.mark_modified();
            } else {
                self.set_status("Rename cancelled - name cannot be empty");
            }
            self.renaming_track = false;
            self.rename_buffer.clear();
        }
    }

    /// Cancels the track rename operation.
    pub fn cancel_rename_track(&mut self) {
        if self.renaming_track {
            self.renaming_track = false;
            self.rename_buffer.clear();
            self.set_status("Rename cancelled");
        }
    }

    /// Marks the project as modified, triggering autosave after delay.
    pub fn mark_modified(&mut self) {
        self.last_modified = Some(Instant::now());
    }

    // ==================== Undo/Redo Methods ====================
    // These methods manage the undo/redo history for user-initiated changes.

    /// Saves the current state to the undo history.
    ///
    /// Call this BEFORE making any user-initiated change to the project.
    /// This captures a snapshot of the current state that can be restored later.
    ///
    /// # Arguments
    ///
    /// * `description` - Brief description of the operation (e.g., "Place note")
    ///
    /// # Example
    ///
    /// ```ignore
    /// self.save_state("Add track");
    /// self.project.create_track("Track 2");
    /// ```
    pub fn save_state(&mut self, description: impl Into<String>) {
        let snapshot = StateSnapshot::new(
            &self.project,
            self.selected_track_index,
            &self.selected_notes,
            description,
        );
        self.history.push_undo(snapshot);
    }

    /// Undoes the last user-initiated change.
    ///
    /// Restores the project, track selection, and note selection to
    /// their previous state. The current state is saved for potential redo.
    ///
    /// If the undo state is invalid (e.g., due to external changes),
    /// the history is cleared to prevent cascading errors.
    ///
    /// # Returns
    ///
    /// true if undo was successful, false if nothing to undo or state was invalid
    pub fn undo(&mut self) -> bool {
        if let Some(prev_state) = self.history.pop_undo() {
            // Validate the snapshot before applying
            if !prev_state.is_valid() {
                // State is invalid - clear history as per requirements
                self.history.clear();
                self.set_status("Undo failed: history cleared due to invalid state");
                return false;
            }

            // Extract data from prev_state before moving project out
            let description = prev_state.description.clone();
            let selected_track_index = prev_state.selected_track_index;
            let valid_notes = prev_state.valid_selected_notes();

            // Save current state to redo stack before restoring
            let current_snapshot = StateSnapshot::new(
                &self.project,
                self.selected_track_index,
                &self.selected_notes,
                description.clone(),
            );
            self.history.push_redo(current_snapshot);

            // Restore the previous state
            self.project = prev_state.project;
            self.selected_track_index =
                selected_track_index.min(self.project.track_count().saturating_sub(1));
            self.selected_notes = valid_notes;

            // Re-sync audio engine with restored tracks
            self.sync_audio_after_restore();

            self.set_status(format!("Undo: {}", description));
            self.mark_modified();

            true
        } else {
            self.set_status("Nothing to undo");
            false
        }
    }

    /// Redoes the last undone change.
    ///
    /// Restores the project to the state before the last undo operation.
    /// The current state is saved for potential undo.
    ///
    /// If the redo state is invalid, the history is cleared.
    ///
    /// # Returns
    ///
    /// true if redo was successful, false if nothing to redo or state was invalid
    pub fn redo(&mut self) -> bool {
        if let Some(next_state) = self.history.pop_redo() {
            // Validate the snapshot before applying
            if !next_state.is_valid() {
                // State is invalid - clear history as per requirements
                self.history.clear();
                self.set_status("Redo failed: history cleared due to invalid state");
                return false;
            }

            // Extract data from next_state before moving project out
            let description = next_state.description.clone();
            let selected_track_index = next_state.selected_track_index;
            let valid_notes = next_state.valid_selected_notes();

            // Save current state to undo stack before restoring.
            // IMPORTANT: Use push_undo_preserve_redo to avoid clearing remaining redo states.
            // This allows multiple consecutive redos (e.g., undo 4x then redo 4x).
            let current_snapshot = StateSnapshot::new(
                &self.project,
                self.selected_track_index,
                &self.selected_notes,
                description.clone(),
            );
            self.history.push_undo_preserve_redo(current_snapshot);

            // Restore the next state
            self.project = next_state.project;
            self.selected_track_index =
                selected_track_index.min(self.project.track_count().saturating_sub(1));
            self.selected_notes = valid_notes;

            // Re-sync audio engine with restored tracks
            self.sync_audio_after_restore();

            self.set_status(format!("Redo: {}", description));
            self.mark_modified();

            true
        } else {
            self.set_status("Nothing to redo");
            false
        }
    }

    /// Re-syncs the audio engine after restoring state.
    ///
    /// This ensures the audio engine has the correct instrument/volume/pan
    /// settings for all tracks after an undo/redo operation.
    fn sync_audio_after_restore(&mut self) {
        // Stop any notes that might be playing
        self.audio.all_notes_off(true);

        // Reconfigure all tracks
        for track in self.project.tracks() {
            self.audio.configure_track(track);
        }
    }

    /// Clears the undo/redo history.
    ///
    /// Called when loading a new project, creating a new project, or
    /// when encountering an unrecoverable error in history state.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Checks if autosave should be performed and does it if needed.
    /// Should be called periodically (e.g., in the main loop).
    pub fn check_autosave(&mut self) {
        if let Some(modified_time) = self.last_modified {
            let should_autosave = modified_time.elapsed()
                >= Duration::from_secs(AUTOSAVE_DELAY_SECS)
                && self.last_autosave.is_none_or(|t| t < modified_time);

            if should_autosave {
                self.force_autosave();
            }
        }
    }

    /// Forces an immediate autosave, bypassing the delay timer.
    /// Useful when critical state changes (like SoundFont selection) should be persisted immediately.
    pub fn force_autosave(&mut self) {
        // Save SoundFont path before autosaving
        self.project.set_soundfont_path(Some(&self.soundfont_path));

        if let Err(e) = self.project.save_to_binary(&self.autosave_path) {
            tracing::error!("Autosave failed: {}", e);
        } else {
            self.last_autosave = Some(Instant::now());
        }
    }

    /// Returns the instrument name for a given program number.
    ///
    /// The name is derived from the currently loaded SoundFont's presets.
    /// Falls back to "Program N" if the preset is not defined in the SoundFont.
    ///
    /// # Arguments
    ///
    /// * `program` - MIDI program number (0-127)
    pub fn get_instrument_name(&self, program: u8) -> &str {
        self.audio.get_instrument_name(program)
    }

    /// Adjusts the volume of the selected track.
    ///
    /// # Arguments
    ///
    /// * `delta` - Amount to change volume (-127 to 127)
    pub fn adjust_track_volume(&mut self, delta: i16) {
        if self.selected_track().is_some() {
            self.save_state("Adjust volume");
        }
        if let Some(track) = self.selected_track_mut() {
            let new_volume = (track.volume as i16 + delta).clamp(0, 127) as u8;
            track.volume = new_volume;
            let name = track.name.clone();
            let channel = track.channel;
            self.audio.set_volume(channel, new_volume);
            self.set_status(format!("{}: Volume {}", name, new_volume));
            self.mark_modified();
        }
    }

    /// Adjusts the pan (L/R balance) of the selected track.
    ///
    /// # Arguments
    ///
    /// * `delta` - Amount to change pan (-127 to 127, 64 = center)
    pub fn adjust_track_pan(&mut self, delta: i16) {
        if self.selected_track().is_some() {
            self.save_state("Adjust pan");
        }
        if let Some(track) = self.selected_track_mut() {
            let new_pan = (track.pan as i16 + delta).clamp(0, 127) as u8;
            track.pan = new_pan;
            let name = track.name.clone();
            let channel = track.channel;
            self.audio.set_pan(channel, new_pan);
            let pan_str = if new_pan < 54 {
                format!("L{}", 64 - new_pan)
            } else if new_pan > 74 {
                format!("R{}", new_pan - 64)
            } else {
                "C".to_string()
            };
            self.set_status(format!("{}: Pan {}", name, pan_str));
            self.mark_modified();
        }
    }

    /// Adjusts the time signature numerator (beats per measure).
    ///
    /// # Arguments
    ///
    /// * `delta` - Amount to change numerator
    pub fn adjust_time_sig_numerator(&mut self, delta: i8) {
        self.save_state("Adjust time signature");
        let new_num = (self.project.time_sig_numerator as i16 + delta as i16).clamp(1, 16) as u8;
        self.project.time_sig_numerator = new_num;
        self.set_status(format!(
            "Time signature: {}/{}",
            self.project.time_sig_numerator, self.project.time_sig_denominator
        ));
        self.mark_modified();
    }

    /// Cycles through common time signature denominators (2, 4, 8, 16).
    pub fn cycle_time_sig_denominator(&mut self) {
        self.save_state("Adjust time signature");
        self.project.time_sig_denominator = match self.project.time_sig_denominator {
            2 => 4,
            4 => 8,
            8 => 16,
            _ => 2,
        };
        self.set_status(format!(
            "Time signature: {}/{}",
            self.project.time_sig_numerator, self.project.time_sig_denominator
        ));
        self.mark_modified();
    }

    /// Cycles through view modes: Combined -> PianoRoll -> ProjectTimeline -> Combined.
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Combined => {
                self.set_status("Piano Roll View");
                ViewMode::PianoRoll
            }
            ViewMode::PianoRoll => {
                self.set_status("Project Timeline View");
                ViewMode::ProjectTimeline
            }
            ViewMode::ProjectTimeline => {
                self.set_status("Combined View");
                ViewMode::Combined
            }
        };
    }

    /// Toggles between compact and expanded track list view.
    pub fn toggle_expanded_tracks(&mut self) {
        self.expanded_tracks = !self.expanded_tracks;
        if self.expanded_tracks {
            self.set_status("Expanded track view");
        } else {
            self.set_status("Compact track view");
        }
    }

    /// Cycles the highlight mode for active notes during playback.
    ///
    /// Cycles through: PianoRollOnly -> Both -> Off -> TimelineOnly -> repeat.
    /// This controls which views show white highlighting for notes being played.
    pub fn cycle_highlight_mode(&mut self) {
        self.highlight_mode = match self.highlight_mode {
            HighlightMode::PianoRollOnly => {
                self.set_status("Highlight: Piano Roll + Timeline");
                HighlightMode::Both
            }
            HighlightMode::Both => {
                self.set_status("Highlight: OFF");
                HighlightMode::Off
            }
            HighlightMode::Off => {
                self.set_status("Highlight: Timeline only");
                HighlightMode::TimelineOnly
            }
            HighlightMode::TimelineOnly => {
                self.set_status("Highlight: Piano Roll only");
                HighlightMode::PianoRollOnly
            }
        };
    }

    /// Returns true if piano roll should highlight active notes during playback.
    #[inline]
    pub fn highlight_piano_roll(&self) -> bool {
        matches!(
            self.highlight_mode,
            HighlightMode::PianoRollOnly | HighlightMode::Both
        )
    }

    /// Returns true if project timeline should highlight active tracks during playback.
    #[inline]
    pub fn highlight_timeline(&self) -> bool {
        matches!(
            self.highlight_mode,
            HighlightMode::TimelineOnly | HighlightMode::Both
        )
    }

    /// Returns the display position in ticks, adjusted for visual latency compensation.
    /// This is the playhead position advanced by `display_offset_ticks` to appear
    /// synchronized with the audio output.
    #[inline]
    pub fn display_position_ticks(&self) -> u32 {
        self.audio
            .position_ticks()
            .saturating_add(self.display_offset_ticks)
    }

    /// Opens the save dialog with a default filename.
    pub fn open_save_dialog(&mut self) {
        let default_name = self
            .project_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(String::from)
            .unwrap_or_else(|| {
                self.project
                    .name
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
                    .collect::<String>()
                    .replace(' ', "_")
            });

        let default_name = if default_name.is_empty() {
            "project".to_string()
        } else {
            default_name
        };

        self.save_dialog.filename = default_name;
        self.save_dialog.format = SaveFormat::Json;
        self.save_dialog.open = true;
    }

    /// Handles character input in the save dialog.
    pub fn save_dialog_input(&mut self, c: char) {
        if self.save_dialog.open && !c.is_control() {
            self.save_dialog.filename.push(c);
        }
    }

    /// Handles backspace in the save dialog.
    pub fn save_dialog_backspace(&mut self) {
        if self.save_dialog.open {
            self.save_dialog.filename.pop();
        }
    }

    /// Toggles the save format in the dialog (cycles: Json -> Oxm -> Midi -> Json).
    pub fn save_dialog_toggle_format(&mut self) {
        if self.save_dialog.open {
            self.save_dialog.format = match self.save_dialog.format {
                SaveFormat::Json => SaveFormat::Oxm,
                SaveFormat::Oxm => SaveFormat::Midi,
                SaveFormat::Midi => SaveFormat::Json,
            };
        }
    }

    /// Confirms and executes the save.
    pub fn save_dialog_confirm(&mut self) -> bool {
        if !self.save_dialog.open || self.save_dialog.filename.is_empty() {
            return false;
        }

        let extension = match self.save_dialog.format {
            SaveFormat::Json => "json",
            SaveFormat::Oxm => "oxm",
            SaveFormat::Midi => "mid",
        };
        let path = PathBuf::from(format!("{}.{}", self.save_dialog.filename, extension));

        // Save the current SoundFont path to the project before saving (not applicable for MIDI)
        if self.save_dialog.format != SaveFormat::Midi {
            self.project.set_soundfont_path(Some(&self.soundfont_path));
        }

        let result = match self.save_dialog.format {
            SaveFormat::Json => self.project.save_to_file(&path),
            SaveFormat::Oxm => self.project.save_to_binary(&path),
            SaveFormat::Midi => crate::midi::export_to_midi(&self.project, &path),
        };

        self.save_dialog.open = false;

        match result {
            Ok(()) => {
                self.project_path = Some(path.clone());
                self.set_status(format!("Saved: {}", path.display()));
                true
            }
            Err(e) => {
                self.set_status(format!("Save failed: {}", e));
                false
            }
        }
    }

    /// Cancels the save dialog.
    pub fn save_dialog_cancel(&mut self) {
        self.save_dialog.open = false;
        self.set_status("Save cancelled");
    }

    /// Loads a project from a file (JSON or OXM based on extension).
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the project file
    ///
    /// # Returns
    ///
    /// true if load was successful
    pub fn load_project(&mut self, path: PathBuf) -> bool {
        let result = match path.extension().and_then(|e| e.to_str()) {
            Some("oxm") => Project::load_from_binary(&path),
            Some("mid") | Some("midi") => {
                // Import MIDI file using the midi_import module
                crate::midi::import_from_midi(&path).map_err(std::io::Error::other)
            }
            _ => Project::load_from_file(&path),
        };

        match result {
            Ok(project) => {
                // Stop any current playback and reset position
                self.audio.stop();
                self.playback_start_time = None;
                self.last_sequencer_tick = None;
                self.playback_start_tick = 0;
                self.active_tracks.clear();

                // Check if project has a SoundFont path and try to load it
                let should_load_soundfont = project.get_soundfont_path().is_some_and(|sf_path| {
                    let sf_pathbuf = PathBuf::from(sf_path);
                    sf_pathbuf.exists() && sf_pathbuf != self.soundfont_path
                });

                self.project = project;
                self.project_path = Some(path.clone());
                self.selected_track_index = 0;
                self.selected_notes.clear();
                self.cursor_tick = 0;
                self.scroll_x = 0;

                // Clear undo/redo history when loading a new project
                self.clear_history();

                // Load the project's SoundFont if different from current
                if should_load_soundfont {
                    if let Some(sf_path) = self.project.get_soundfont_path() {
                        let sf_pathbuf = PathBuf::from(sf_path);
                        if self.load_soundfont(sf_pathbuf) {
                            self.set_status(format!("Loaded: {} (with soundfont)", path.display()));
                            return true;
                        }
                    }
                }

                // Configure audio engine for all tracks
                for track in self.project.tracks() {
                    self.audio.configure_track(track);
                }

                self.set_status(format!("Loaded: {}", path.display()));
                true
            }
            Err(e) => {
                self.set_status(format!("Load failed: {}", e));
                false
            }
        }
    }

    /// Opens the file browser for loading a project (native only).
    pub fn open_file_browser(&mut self) {
        self.file_browser.open = true;
        self.file_browser.current_dir = std::env::current_dir().unwrap_or_default();
        self.file_browser.selected = 0;
        self.file_browser.scroll = 0;
        self.refresh_file_browser();
    }

    /// Refreshes the file browser entries (native only).
    fn refresh_file_browser(&mut self) {
        self.file_browser.entries.clear();

        // Add parent directory entry if not at root
        if self.file_browser.current_dir.parent().is_some() {
            self.file_browser.entries.push(PathBuf::from(".."));
        }

        // Read directory entries
        if let Ok(entries) = std::fs::read_dir(&self.file_browser.current_dir) {
            let mut dirs: Vec<PathBuf> = Vec::new();
            let mut files: Vec<PathBuf> = Vec::new();

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    // Support native formats (.json, .oxm) and MIDI files (.mid, .midi)
                    if ext == "json" || ext == "oxm" || ext == "mid" || ext == "midi" {
                        files.push(path);
                    }
                }
            }

            // Sort directories and files alphabetically
            dirs.sort();
            files.sort();

            self.file_browser.entries.extend(dirs);
            self.file_browser.entries.extend(files);
        }

        // Reset selection if out of bounds
        if self.file_browser.selected >= self.file_browser.entries.len() {
            self.file_browser.selected = 0;
        }
    }

    /// Moves selection up in the file browser (native only).
    pub fn file_browser_up(&mut self) {
        if self.file_browser.open && self.file_browser.selected > 0 {
            self.file_browser.selected -= 1;
            if self.file_browser.selected < self.file_browser.scroll {
                self.file_browser.scroll = self.file_browser.selected;
            }
        }
    }

    /// Moves selection down in the file browser (native only).
    pub fn file_browser_down(&mut self) {
        if self.file_browser.open
            && self.file_browser.selected + 1 < self.file_browser.entries.len()
        {
            self.file_browser.selected += 1;
            // Scroll if needed (assuming ~10 visible entries)
            if self.file_browser.selected >= self.file_browser.scroll + 10 {
                self.file_browser.scroll = self.file_browser.selected.saturating_sub(9);
            }
        }
    }

    /// Selects the current entry in the file browser (native only).
    pub fn file_browser_select(&mut self) -> bool {
        if !self.file_browser.open || self.file_browser.entries.is_empty() {
            return false;
        }

        let selected_path = &self.file_browser.entries[self.file_browser.selected];

        if selected_path == &PathBuf::from("..") {
            // Go to parent directory
            if let Some(parent) = self.file_browser.current_dir.parent() {
                self.file_browser.current_dir = parent.to_path_buf();
                self.file_browser.selected = 0;
                self.file_browser.scroll = 0;
                self.refresh_file_browser();
            }
            false
        } else if selected_path.is_dir() {
            // Enter directory
            self.file_browser.current_dir = selected_path.clone();
            self.file_browser.selected = 0;
            self.file_browser.scroll = 0;
            self.refresh_file_browser();
            false
        } else {
            // Load the file
            let path = selected_path.clone();
            self.file_browser.open = false;
            self.load_project(path)
        }
    }

    /// Cancels the file browser (native only).
    pub fn file_browser_cancel(&mut self) {
        self.file_browser.open = false;
        self.set_status("Load cancelled");
    }

    // ========== SOUNDFONT DIALOG METHODS ==========

    /// Opens the SoundFont browser dialog.
    ///
    /// # Arguments
    ///
    /// * `is_first_load` - If true, this is the initial load modal that blocks other UI
    pub fn open_soundfont_dialog(&mut self, is_first_load: bool) {
        self.soundfont_dialog.open = true;
        self.soundfont_dialog.is_first_load = is_first_load;
        self.soundfont_dialog.current_dir = std::env::current_dir().unwrap_or_default();
        self.soundfont_dialog.selected = 0;
        self.soundfont_dialog.scroll = 0;
        self.refresh_soundfont_browser();
    }

    /// Refreshes the SoundFont browser entries.
    fn refresh_soundfont_browser(&mut self) {
        self.soundfont_dialog.entries.clear();

        // Add parent directory entry if not at root
        if self.soundfont_dialog.current_dir.parent().is_some() {
            self.soundfont_dialog.entries.push(PathBuf::from(".."));
        }

        // Read directory entries
        if let Ok(entries) = std::fs::read_dir(&self.soundfont_dialog.current_dir) {
            let mut dirs: Vec<PathBuf> = Vec::new();
            let mut files: Vec<PathBuf> = Vec::new();

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    // Filter for SoundFont files (.sf2)
                    let ext_lower = ext.to_lowercase();
                    if ext_lower == "sf2" {
                        files.push(path);
                    }
                }
            }

            // Sort directories and files alphabetically
            dirs.sort();
            files.sort();

            self.soundfont_dialog.entries.extend(dirs);
            self.soundfont_dialog.entries.extend(files);
        }

        // Reset selection if out of bounds
        if self.soundfont_dialog.selected >= self.soundfont_dialog.entries.len() {
            self.soundfont_dialog.selected = 0;
        }
    }

    /// Moves selection up in the SoundFont browser.
    pub fn soundfont_dialog_up(&mut self) {
        if self.soundfont_dialog.open && self.soundfont_dialog.selected > 0 {
            self.soundfont_dialog.selected -= 1;
            if self.soundfont_dialog.selected < self.soundfont_dialog.scroll {
                self.soundfont_dialog.scroll = self.soundfont_dialog.selected;
            }
        }
    }

    /// Moves selection down in the SoundFont browser.
    pub fn soundfont_dialog_down(&mut self) {
        if self.soundfont_dialog.open
            && self.soundfont_dialog.selected + 1 < self.soundfont_dialog.entries.len()
        {
            self.soundfont_dialog.selected += 1;
            // Scroll if needed (assuming ~10 visible entries)
            if self.soundfont_dialog.selected >= self.soundfont_dialog.scroll + 10 {
                self.soundfont_dialog.scroll = self.soundfont_dialog.selected.saturating_sub(9);
            }
        }
    }

    /// Selects the current entry in the SoundFont browser.
    ///
    /// # Returns
    ///
    /// true if a SoundFont was successfully loaded
    pub fn soundfont_dialog_select(&mut self) -> bool {
        if !self.soundfont_dialog.open || self.soundfont_dialog.entries.is_empty() {
            return false;
        }

        let selected_path = &self.soundfont_dialog.entries[self.soundfont_dialog.selected];

        if selected_path == &PathBuf::from("..") {
            // Go to parent directory
            if let Some(parent) = self.soundfont_dialog.current_dir.parent() {
                self.soundfont_dialog.current_dir = parent.to_path_buf();
                self.soundfont_dialog.selected = 0;
                self.soundfont_dialog.scroll = 0;
                self.refresh_soundfont_browser();
            }
            false
        } else if selected_path.is_dir() {
            // Enter directory
            self.soundfont_dialog.current_dir = selected_path.clone();
            self.soundfont_dialog.selected = 0;
            self.soundfont_dialog.scroll = 0;
            self.refresh_soundfont_browser();
            false
        } else {
            // Load the SoundFont file
            let path = selected_path.clone();
            self.soundfont_dialog.open = false;
            self.load_soundfont(path)
        }
    }

    /// Cancels the SoundFont browser.
    /// If this is the first load modal, the app cannot proceed without a SoundFont.
    ///
    /// # Returns
    ///
    /// true if the dialog was closed, false if it cannot be closed (first load modal)
    pub fn soundfont_dialog_cancel(&mut self) -> bool {
        if self.soundfont_dialog.is_first_load {
            self.set_status("A soundfont is required to continue");
            false
        } else {
            self.soundfont_dialog.open = false;
            self.set_status("Soundfont selection cancelled");
            true
        }
    }

    /// Loads a new SoundFont and reinitializes the audio engine.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the SoundFont file
    ///
    /// # Returns
    ///
    /// true if the SoundFont was loaded successfully
    pub fn load_soundfont(&mut self, path: PathBuf) -> bool {
        match AudioEngine::new(&path) {
            Ok(new_audio) => {
                // Stop current playback
                self.audio.stop();
                self.playback_start_time = None;
                self.last_sequencer_tick = None;
                self.playback_start_tick = 0;
                self.active_tracks.clear();
                self.held_notes.clear();

                // Replace audio engine
                self.audio = new_audio;
                self.soundfont_path = path.clone();

                // Update project's SoundFont path
                self.project.set_soundfont_path(Some(&path));

                // Reconfigure all tracks with the new audio engine
                for track in self.project.tracks() {
                    self.audio.configure_track(track);
                }

                self.set_status(format!(
                    "Loaded soundfont: {}",
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                ));

                // Force immediate autosave so SoundFont selection persists across restarts
                self.force_autosave();

                true
            }
            Err(e) => {
                tracing::error!("Failed to load SoundFont {:?}: {}", path, e);
                self.set_status(format!("Failed to load SoundFont: {}", e));
                false
            }
        }
    }

    // ========== AUTOSAVE RECOVERY METHODS ==========

    /// Attempts to load the autosave file on startup (native only).
    /// If the autosave file exists and loads successfully, shows a status message.
    /// If it fails or doesn't exist, silently continues with a new project.
    pub fn try_load_autosave(&mut self) {
        if self.autosave_path.exists() {
            match Project::load_from_binary(&self.autosave_path) {
                Ok(project) => {
                    self.project = project;
                    self.selected_track_index = 0;
                    self.selected_notes.clear();
                    self.cursor_tick = 0;
                    self.scroll_x = 0;

                    // Configure audio engine for all tracks
                    for track in self.project.tracks() {
                        self.audio.configure_track(track);
                    }

                    self.set_status("Recovered from autosave");
                    tracing::info!("Loaded autosave from {:?}", self.autosave_path);
                }
                Err(e) => {
                    tracing::warn!("Failed to load autosave: {}", e);
                    // Continue with default new project
                }
            }
        }
    }

    // ========== NEW PROJECT DIALOG METHODS ==========

    /// Opens the new project confirmation dialog.
    pub fn open_new_project_dialog(&mut self) {
        self.new_project_dialog.open = true;
        self.new_project_dialog.selected = 1; // Default to "No" for safety
    }

    /// Moves selection left in the new project dialog (selects Yes).
    pub fn new_project_dialog_left(&mut self) {
        if self.new_project_dialog.open {
            self.new_project_dialog.selected = 0;
        }
    }

    /// Moves selection right in the new project dialog (selects No).
    pub fn new_project_dialog_right(&mut self) {
        if self.new_project_dialog.open {
            self.new_project_dialog.selected = 1;
        }
    }

    /// Confirms the new project dialog selection.
    /// If "Yes" is selected, resets to a new project.
    /// Returns true if a new project was created.
    pub fn new_project_dialog_confirm(&mut self) -> bool {
        if !self.new_project_dialog.open {
            return false;
        }

        self.new_project_dialog.open = false;

        if self.new_project_dialog.selected == 0 {
            // "Yes" was selected - create new project
            self.reset_to_new_project();
            true
        } else {
            // "No" was selected - cancel
            self.set_status("New project cancelled");
            false
        }
    }

    /// Cancels the new project dialog.
    pub fn new_project_dialog_cancel(&mut self) {
        self.new_project_dialog.open = false;
        self.set_status("New project cancelled");
    }

    /// Resets the application to a new, empty project.
    /// Clears all tracks and notes, resets cursor and view state.
    /// Maintains current edit mode and octave settings.
    pub fn reset_to_new_project(&mut self) {
        // Stop any playback
        self.audio.stop();
        self.playback_start_time = None;

        // Create fresh project
        self.project = Project::with_default_track("New Project");

        // Reset position and view state (keep edit_mode and octave_offset unchanged)
        self.project_path = None;
        self.selected_track_index = 0;
        self.selected_notes.clear();
        self.cursor_tick = 0;
        self.cursor_pitch = 60; // Middle C
        self.scroll_x = 0;
        self.scroll_y = 48;
        // Note: edit_mode and octave_offset are intentionally preserved
        self.last_modified = None;
        self.last_autosave = None;
        self.active_tracks.clear();
        self.held_notes.clear();

        // Reset Insert Mode recording state (seek position back to 0:00:000)
        self.insert_recording_active = false;
        self.insert_recording_start_time = None;
        self.insert_recording_start_tick = 0;
        self.last_insert_note_time = None;
        self.recently_added_beat = None;
        self.recently_added_note = None;
        self.recently_added_pitch = None;

        // Clear undo/redo history when creating a new project
        self.clear_history();

        // Configure audio for the default track
        if let Some(track) = self.project.track_at(0) {
            self.audio.configure_track(track);
        }

        self.set_status("New project created");
    }

    /// Seeks playback to a specific tick position.
    ///
    /// Updates the cursor position and, if playing, adjusts the playback
    /// position to the new tick. This is called when clicking on time rulers.
    ///
    /// # Arguments
    ///
    /// * `tick` - The tick position to seek to
    pub fn seek_to_tick(&mut self, tick: u32) {
        // Update cursor position
        self.cursor_tick = tick;

        // If playing, update the playback position
        if self.audio.is_playing() {
            // Stop all currently playing notes to avoid hanging notes
            self.audio.all_notes_off(true);

            // Reset playback timing to the new position
            self.playback_start_time = Some(Instant::now());
            self.playback_start_tick = tick;
            self.last_sequencer_tick = Some(tick);
            self.audio.set_position_ticks(tick);
        } else {
            // Even when stopped, update the audio position so playback
            // will start from this position
            self.audio.set_position_ticks(tick);
        }

        // Adjust scroll to keep cursor in view
        let visible_cols = if self.layout.piano_roll_grid.width > 0 {
            self.layout.piano_roll_grid.width as u32
        } else {
            60
        };
        let visible_ticks = self.zoom * visible_cols;

        // Center the cursor if it's out of view
        if tick < self.scroll_x || tick >= self.scroll_x + visible_ticks {
            self.scroll_x = tick.saturating_sub(visible_ticks / 2);
        }

        // Show position in status
        let (measure, beat, sub_tick) = self.project.tick_to_position(tick);
        self.set_status(format!("Seek to {}:{:02}:{:03}", measure, beat, sub_tick));
    }

    /// Moves the cursor by a number of ticks.
    pub fn move_cursor_horizontal(&mut self, ticks: i32) {
        if ticks < 0 {
            self.cursor_tick = self.cursor_tick.saturating_sub((-ticks) as u32);
        } else {
            self.cursor_tick = self.cursor_tick.saturating_add(ticks as u32);
        }

        // Scroll if cursor is out of view
        let visible_ticks = self.zoom * 80;
        if self.cursor_tick < self.scroll_x {
            self.scroll_x = self.cursor_tick;
        } else if self.cursor_tick > self.scroll_x + visible_ticks {
            self.scroll_x = self.cursor_tick.saturating_sub(visible_ticks);
        }
    }

    /// Moves the cursor pitch up or down.
    pub fn move_cursor_vertical(&mut self, semitones: i8) {
        let new_pitch = self.cursor_pitch as i16 + semitones as i16;
        if (0..=127).contains(&new_pitch) {
            self.cursor_pitch = new_pitch as u8;

            // Scroll if cursor is out of view (use dynamic visible_pitches)
            let visible = self.layout.visible_pitches.max(1);
            if self.cursor_pitch < self.scroll_y {
                self.scroll_y = self.cursor_pitch;
            } else if self.cursor_pitch > self.scroll_y + visible {
                self.scroll_y = self.cursor_pitch.saturating_sub(visible);
            }
        }
    }

    /// Places a note at the current cursor position.
    pub fn place_note(&mut self) {
        // Copy values to avoid borrow checker issues
        let cursor_pitch = self.cursor_pitch;
        let cursor_tick = self.cursor_tick;

        // Get channel before mutable borrow
        let channel = self.selected_track().map(|t| t.channel).unwrap_or(0);

        self.save_state("Place note");
        let note_id = self.selected_track_mut().map(|track| {
            track.create_note(
                cursor_pitch,
                DEFAULT_VELOCITY,
                cursor_tick,
                DEFAULT_NOTE_DURATION,
            )
        });

        // Register the note for blue highlighting and auto-scroll
        if let Some(id) = note_id {
            self.register_added_note(id, cursor_pitch, cursor_tick);
        }

        // Play the note audio as feedback (short preview)
        self.audio.note_on(channel, cursor_pitch, DEFAULT_VELOCITY);
        // Schedule note off after a short duration (handled by held_notes system isn't
        // ideal here, so we'll just trigger a short note - the audio engine handles it)

        self.set_status(format!(
            "Added {} at {}",
            note_to_name(cursor_pitch),
            cursor_tick / TICKS_PER_BEAT
        ));
        self.mark_modified();
    }

    /// Deletes the note under the cursor.
    pub fn delete_note_at_cursor(&mut self) {
        // Copy values to avoid borrow checker issues
        let cursor_pitch = self.cursor_pitch;
        let cursor_tick = self.cursor_tick;

        let note_id = self.selected_track().and_then(|track| {
            track
                .notes()
                .iter()
                .find(|n| n.pitch == cursor_pitch && n.is_active_at(cursor_tick))
                .map(|n| n.id)
        });

        if let Some(id) = note_id {
            self.delete_note_by_id(id);
        }
    }

    /// Deletes a note by its ID.
    ///
    /// # Arguments
    ///
    /// * `note_id` - The ID of the note to delete
    pub fn delete_note_by_id(&mut self, note_id: NoteId) {
        self.save_state("Delete note");
        if let Some(track) = self.selected_track_mut() {
            track.remove_note(note_id);
        }
        // Remove from selection if selected
        self.selected_notes.remove(&note_id);
        self.set_status("Deleted note");
        self.mark_modified();
    }

    /// Changes the octave offset for keyboard input.
    pub fn change_octave(&mut self, delta: i8) {
        let new_offset = self.octave_offset + delta;
        if (-4..=4).contains(&new_offset) {
            self.octave_offset = new_offset;
            self.set_status(format!("Octave: {}", new_offset));
        }
    }

    /// Zooms in or out on the timeline.
    pub fn zoom(&mut self, factor: f32) {
        let new_zoom = (self.zoom as f32 * factor) as u32;
        self.zoom = new_zoom.clamp(TICKS_PER_BEAT / 16, TICKS_PER_BEAT * 4);
    }

    /// Cycles the instrument (program) for the selected track (native only).
    ///
    /// # Arguments
    ///
    /// * `delta` - Direction to cycle (+1 for next, -1 for previous)
    ///
    /// Changes the MIDI program number and updates the audio engine in real-time.
    pub fn cycle_instrument(&mut self, delta: i8) {
        if self.selected_track().is_some() {
            self.save_state("Change instrument");
        }

        // Silence all currently playing notes before switching instruments.
        // This prevents notes from playing indefinitely with the old instrument.
        // release_all_notes() handles keyboard-held notes tracked in held_notes,
        // while all_notes_off(true) immediately silences any sequencer-triggered notes.
        self.release_all_notes();
        self.audio.all_notes_off(true);

        // Get current program and calculate new program with wrapping
        let (channel, new_program) = {
            if let Some(track) = self.selected_track_mut() {
                let current = track.program as i16;
                // Wrap around: 0-127 (128 instruments in General MIDI)
                let new_program = ((current + delta as i16).rem_euclid(128)) as u8;
                track.program = new_program;
                (track.channel, new_program)
            } else {
                return;
            }
        };

        // Update the audio engine with the new program
        self.audio.set_program(channel, new_program);

        // Get the instrument name from the SoundFont (after mutable borrow is released)
        let instrument_name = self.get_instrument_name(new_program);

        // Show status with instrument name
        self.set_status(format!("Instrument: {} ({})", instrument_name, new_program));
        self.mark_modified();
    }

    /// Returns the current position formatted as "measure:beat:tick".
    pub fn position_string(&self) -> String {
        let (measure, beat, tick) = self.project.tick_to_position(self.cursor_tick);
        format!("{}:{:02}:{:03}", measure, beat, tick)
    }

    // ========== MOUSE HANDLING METHODS (NATIVE ONLY) ==========

    /// Handles a mouse click event (native only).
    ///
    /// # Arguments
    ///
    /// * `x` - Screen X coordinate
    /// * `y` - Screen Y coordinate
    /// * `modifiers` - Keyboard modifiers (Ctrl, Shift, etc.)
    ///
    /// # Returns
    ///
    /// true if the click was handled
    pub fn handle_mouse_click(&mut self, x: u16, y: u16, shift_held: bool) -> bool {
        // Determine which panel was clicked
        if let Some(panel) = self.layout.panel_at(x, y) {
            self.focused_panel = panel;

            match panel {
                FocusedPanel::TrackList => self.handle_track_list_click(x, y),
                FocusedPanel::PianoRoll => self.handle_piano_roll_click(x, y, shift_held),
                FocusedPanel::Timeline => self.handle_timeline_click(x, y),
                FocusedPanel::Keyboard => self.handle_keyboard_click(x, y),
            }

            true
        } else {
            false
        }
    }

    /// Handles a click in the track list (native only).
    fn handle_track_list_click(&mut self, x: u16, y: u16) {
        let region = self.layout.track_list;

        // Check if click is within the track list region (accounting for borders)
        if y > region.y && y < region.y + region.height - 1 {
            let relative_y = y - region.y - 1; // -1 for top border
            let inner_height = region.height.saturating_sub(2); // -2 for borders

            // Control hints take up 2 rows at the bottom
            let controls_height: u16 = 2;
            let list_height = inner_height.saturating_sub(controls_height);

            // Check if clicked in the control hints area (bottom 2 rows)
            if relative_y >= list_height {
                // Clicked in control hints area - ignore
                return;
            }

            // Calculate track index based on view mode
            // In expanded mode, each track takes 2 rows; in compact mode, 1 row
            let rows_per_track = if self.expanded_tracks { 2 } else { 1 };

            // Calculate the scroll offset that ratatui's List uses
            // The List scrolls to keep the selected item visible
            let visible_rows = list_height as usize;
            let visible_items = visible_rows / rows_per_track;
            let selected = self.selected_track_index;

            // Calculate scroll offset using same algorithm as ratatui
            let scroll_offset = if selected >= visible_items {
                selected - visible_items + 1
            } else {
                0
            };

            // Apply scroll offset when calculating track index from click
            let track_index = scroll_offset + (relative_y as usize) / rows_per_track;

            if track_index < self.project.track_count() {
                // Check if clicking on mute/solo indicators (only on first row of track)
                let row_within_track = (relative_y as usize) % rows_per_track;
                let relative_x = x.saturating_sub(region.x + 1); // +1 for left border

                if row_within_track == 0 && relative_x == 0 {
                    // Clicked on mute indicator
                    self.save_state("Toggle mute");
                    if let Some(track) = self.project.track_at_mut(track_index) {
                        track.muted = !track.muted;
                        let status = if track.muted { "Muted" } else { "Unmuted" };
                        let name = track.name.clone();
                        self.set_status(format!("{} {}", status, name));
                    }
                    // Silence all notes - the sequencer will restart appropriate ones
                    self.audio.all_notes_off(true);
                    self.mark_modified();
                } else if row_within_track == 0 && relative_x == 1 {
                    // Clicked on solo indicator
                    self.save_state("Toggle solo");
                    if let Some(track) = self.project.track_at_mut(track_index) {
                        track.solo = !track.solo;
                        let status = if track.solo { "Solo on" } else { "Solo off" };
                        let name = track.name.clone();
                        self.set_status(format!("{} {}", status, name));
                    }
                    // Silence all notes - the sequencer will restart appropriate ones
                    self.audio.all_notes_off(true);
                    self.mark_modified();
                } else {
                    // Clicked on track name or second row - select it
                    self.selected_track_index = track_index;
                    if let Some(track) = self.selected_track() {
                        self.set_status(format!("Selected: {}", track.name));
                    }
                }
            }
        }
    }

    /// Handles a click in the piano roll (native only).
    fn handle_piano_roll_click(&mut self, x: u16, y: u16, shift_held: bool) {
        let region = self.layout.piano_roll;
        let grid_region = self.layout.piano_roll_grid;

        // Check if clicking on any time ruler (Piano Roll or Project Timeline)
        // The ruler regions are set during rendering for accurate hit testing
        if let Some((relative_x, _)) = self.layout.ruler_hit_test(x, y) {
            let tick = self.scroll_x + (relative_x as u32 * self.zoom);
            // Snap to beat for more intuitive seeking
            let snapped_tick = (tick / TICKS_PER_BEAT) * TICKS_PER_BEAT;
            self.seek_to_tick(snapped_tick);
            return;
        }

        // Check if clicking in the grid area (not the piano keys)
        if self.layout.is_in_piano_roll_grid(x, y) {
            // Convert screen coordinates to tick/pitch
            let relative_x = x.saturating_sub(grid_region.x);
            let relative_y = y.saturating_sub(grid_region.y);

            // Calculate tick from X position
            let tick = self.scroll_x + (relative_x as u32 * self.zoom);

            // Calculate pitch from Y position (inverted - top is higher)
            // Use layout.visible_pitches to match the rendering formula in piano_roll.rs
            // The formula is: pitch = scroll_y + visible_pitches - 1 - row
            // Subtract TIME_RULER_HEIGHT because the ruler occupies the first row of grid_region
            let pitch_row = relative_y.saturating_sub(TIME_RULER_HEIGHT) as u8;
            let pitch = (self.scroll_y + self.layout.visible_pitches.max(1) - 1)
                .saturating_sub(pitch_row)
                .min(127);

            // Update cursor position
            self.cursor_tick = tick;
            self.cursor_pitch = pitch;

            // Check if there's a note at this position
            let cursor_pitch = self.cursor_pitch;
            let cursor_tick = self.cursor_tick;
            let note_at_pos = self.selected_track().and_then(|track| {
                track
                    .notes()
                    .iter()
                    .find(|n| n.pitch == cursor_pitch && n.is_active_at(cursor_tick))
                    .map(|n| n.id)
            });

            if let Some(note_id) = note_at_pos {
                // Clicked on a note
                if shift_held {
                    // Toggle selection with shift
                    if self.selected_notes.contains(&note_id) {
                        self.selected_notes.remove(&note_id);
                    } else {
                        self.selected_notes.insert(note_id);
                    }
                } else {
                    // Single select without shift
                    self.selected_notes.clear();
                    self.selected_notes.insert(note_id);
                }
                self.set_status(format!(
                    "Selected note at {} ({})",
                    note_to_name(cursor_pitch),
                    cursor_tick / TICKS_PER_BEAT
                ));
            } else if self.edit_mode == EditMode::Insert {
                // In insert mode, place a note
                self.place_note();
            } else {
                // Clear selection when clicking empty space (without shift)
                if !shift_held {
                    self.selected_notes.clear();
                }
            }
        } else if x > region.x && x < region.x + 1 + PIANO_KEY_WIDTH {
            // Clicking on piano keys - play the note
            // Subtract TIME_RULER_HEIGHT to align with pitch rows (ruler occupies first row)
            let relative_y = y.saturating_sub(region.y + 1 + TIME_RULER_HEIGHT);
            // Use layout.visible_pitches to match rendering formula: pitch = scroll_y + visible_pitches - 1 - row
            let pitch = (self.scroll_y + self.layout.visible_pitches.max(1) - 1)
                .saturating_sub(relative_y as u8)
                .min(127);

            let channel = self.selected_track().map(|t| t.channel).unwrap_or(0);
            self.audio.note_on(channel, pitch, DEFAULT_VELOCITY);

            // In Insert mode, also add the note at the current cursor position
            if self.edit_mode == EditMode::Insert {
                self.cursor_pitch = pitch;
                self.save_state("Insert note");
                let cursor_tick = self.cursor_tick;
                let note_id = self.selected_track_mut().map(|track| {
                    track.create_note(pitch, DEFAULT_VELOCITY, cursor_tick, DEFAULT_NOTE_DURATION)
                });
                // Register the note for blue highlighting and auto-scroll
                if let Some(id) = note_id {
                    self.register_added_note(id, pitch, cursor_tick);
                }
                self.set_status(format!(
                    "Added {} at {}",
                    note_to_name(pitch),
                    cursor_tick / TICKS_PER_BEAT
                ));
                self.mark_modified();
            } else {
                self.set_status(format!("Playing: {}", note_to_name(pitch)));
            }
        }
    }

    /// Handles releasing a note played by clicking piano keys (native only).
    pub fn handle_piano_key_release(&mut self, x: u16, y: u16) {
        let region = self.layout.piano_roll;

        if x > region.x && x < region.x + 1 + PIANO_KEY_WIDTH {
            // Subtract TIME_RULER_HEIGHT to align with pitch rows (ruler occupies first row)
            let relative_y = y.saturating_sub(region.y + 1 + TIME_RULER_HEIGHT);
            // Use layout.visible_pitches to match rendering formula: pitch = scroll_y + visible_pitches - 1 - row
            let pitch = (self.scroll_y + self.layout.visible_pitches.max(1) - 1)
                .saturating_sub(relative_y as u8)
                .min(127);

            let channel = self.selected_track().map(|t| t.channel).unwrap_or(0);
            self.audio.note_off(channel, pitch);
        }
    }

    /// Handles a click in the timeline (native only).
    fn handle_timeline_click(&mut self, x: u16, _y: u16) {
        let region = self.layout.timeline;

        // Check for clicks on transport controls
        // Layout: [Play status (20)] [Position (20)] [Tempo (15)] [Time sig (10)] [Mode]
        let relative_x = x.saturating_sub(region.x + 1);

        if relative_x < 15 {
            // Clicked on play/pause area - toggle playback
            self.toggle_playback();
        } else if relative_x < 35 {
            // Clicked on position - could implement seek here
            // For now, just stop and reset
            self.stop_playback();
        }
    }

    /// Handles a click in the keyboard display (native only).
    fn handle_keyboard_click(&mut self, x: u16, _y: u16) {
        let region = self.layout.keyboard;

        // The keyboard shows key mappings - clicking changes octave
        let relative_x = x.saturating_sub(region.x);
        let width = region.width;

        if relative_x < width / 2 {
            // Left half - octave down
            self.change_octave(-1);
        } else {
            // Right half - octave up
            self.change_octave(1);
        }
    }

    /// Handles mouse scroll events (native only).
    ///
    /// # Arguments
    ///
    /// * `x` - Screen X coordinate
    /// * `y` - Screen Y coordinate
    /// * `delta_x` - Horizontal scroll amount (positive = right)
    /// * `delta_y` - Vertical scroll amount (positive = up)
    /// * `ctrl_held` - Whether Ctrl/Cmd is held (for zoom)
    pub fn handle_mouse_scroll(
        &mut self,
        x: u16,
        y: u16,
        delta_x: i16,
        delta_y: i16,
        ctrl_held: bool,
    ) {
        if let Some(panel) = self.layout.panel_at(x, y) {
            match panel {
                FocusedPanel::PianoRoll => {
                    if ctrl_held {
                        // Zoom with Ctrl+scroll
                        if delta_y > 0 {
                            self.zoom(0.8); // Zoom in
                            self.set_status(format!("Zoom: {} ticks/col", self.zoom));
                        } else if delta_y < 0 {
                            self.zoom(1.25); // Zoom out
                            self.set_status(format!("Zoom: {} ticks/col", self.zoom));
                        }
                    } else {
                        // Normal scroll - vertical scrolls pitch, horizontal scrolls time
                        if delta_y != 0 {
                            // Vertical scroll - change pitch view
                            let scroll_amount = (delta_y.unsigned_abs() as u8).max(1);
                            let visible = self.layout.visible_pitches.max(1);
                            if delta_y > 0 {
                                // Scroll up - show higher pitches
                                self.scroll_y = (self.scroll_y + scroll_amount)
                                    .min(127u8.saturating_sub(visible));
                            } else {
                                // Scroll down - show lower pitches
                                self.scroll_y = self.scroll_y.saturating_sub(scroll_amount);
                            }
                        }
                        if delta_x != 0 {
                            // Horizontal scroll - change time view
                            let scroll_ticks =
                                (delta_x.unsigned_abs() as u32 * self.zoom).max(self.zoom);
                            if delta_x > 0 {
                                self.scroll_x = self.scroll_x.saturating_add(scroll_ticks);
                            } else {
                                self.scroll_x = self.scroll_x.saturating_sub(scroll_ticks);
                            }
                        }
                    }
                }
                FocusedPanel::TrackList => {
                    // Scroll track list (if we had more tracks than visible)
                    // For now, just change selected track
                    if delta_y > 0 && self.selected_track_index > 0 {
                        self.selected_track_index -= 1;
                    } else if delta_y < 0
                        && self.selected_track_index < self.project.track_count().saturating_sub(1)
                    {
                        self.selected_track_index += 1;
                    }
                }
                FocusedPanel::Timeline => {
                    // Scroll timeline adjusts tempo
                    if delta_y > 0 {
                        self.save_state("Adjust tempo");
                        self.project.tempo = (self.project.tempo + 1).min(300);
                        self.audio.set_tempo(self.project.tempo);
                        self.set_status(format!("Tempo: {} BPM", self.project.tempo));
                        self.mark_modified();
                    } else if delta_y < 0 {
                        self.save_state("Adjust tempo");
                        self.project.tempo = self.project.tempo.saturating_sub(1).max(20);
                        self.audio.set_tempo(self.project.tempo);
                        self.set_status(format!("Tempo: {} BPM", self.project.tempo));
                        self.mark_modified();
                    }
                }
                FocusedPanel::Keyboard => {
                    // Scroll on keyboard changes octave
                    if delta_y > 0 {
                        self.change_octave(1);
                    } else if delta_y < 0 {
                        self.change_octave(-1);
                    }
                }
            }
        }
    }

    /// Handles mouse drag start (native only).
    pub fn handle_drag_start(&mut self, x: u16, y: u16, shift_held: bool) {
        if self.layout.is_in_piano_roll_grid(x, y) {
            // Convert mouse coordinates to tick/pitch
            let grid_region = self.layout.piano_roll_grid;
            let relative_x = x.saturating_sub(grid_region.x);
            let relative_y = y.saturating_sub(grid_region.y);
            let tick = self.scroll_x + (relative_x as u32 * self.zoom);
            let pitch_row = relative_y.saturating_sub(TIME_RULER_HEIGHT) as u8;
            let pitch = (self.scroll_y + self.layout.visible_pitches.max(1) - 1)
                .saturating_sub(pitch_row)
                .min(127);

            // Check if clicking on a selected note - if so, start moving notes
            if !self.selected_notes.is_empty() {
                let clicking_selected_note = self.selected_track().is_some_and(|track| {
                    track.notes().iter().any(|n| {
                        self.selected_notes.contains(&n.id)
                            && n.pitch == pitch
                            && n.is_active_at(tick)
                    })
                });

                if clicking_selected_note {
                    // Save state before moving notes
                    self.save_state("Move notes");
                    self.drag_state = DragState::MovingNotes {
                        last_x: x,
                        last_y: y,
                        start_tick: tick,
                        start_pitch: pitch,
                    };
                    return;
                }
            }

            if shift_held {
                // Start selecting notes with shift+drag
                self.drag_state = DragState::SelectingNotes {
                    start_x: x,
                    start_y: y,
                };
            } else {
                // Start scrolling with drag
                self.drag_state = DragState::Scrolling {
                    last_x: x,
                    last_y: y,
                };
            }
        }
    }

    /// Handles mouse drag movement (native only).
    pub fn handle_drag_move(&mut self, x: u16, y: u16) {
        match self.drag_state {
            DragState::Scrolling { last_x, last_y } => {
                // Calculate movement delta
                let dx = x as i32 - last_x as i32;
                let dy = y as i32 - last_y as i32;

                // Scroll the view (inverted for natural scrolling feel)
                if dx != 0 {
                    let tick_delta = -dx * self.zoom as i32;
                    if tick_delta < 0 {
                        self.scroll_x = self.scroll_x.saturating_sub((-tick_delta) as u32);
                    } else {
                        self.scroll_x = self.scroll_x.saturating_add(tick_delta as u32);
                    }
                }
                if dy != 0 {
                    // Invert vertical for natural scrolling
                    let visible = self.layout.visible_pitches.max(1);
                    if dy > 0 {
                        self.scroll_y = self.scroll_y.saturating_sub(dy as u8);
                    } else {
                        self.scroll_y =
                            (self.scroll_y + (-dy) as u8).min(127u8.saturating_sub(visible));
                    }
                }

                self.drag_state = DragState::Scrolling {
                    last_x: x,
                    last_y: y,
                };
            }
            DragState::SelectingNotes {
                start_x: _,
                start_y: _,
            } => {
                // Could implement rubber-band selection here
                // For now, just update cursor to the current position
                if self.layout.is_in_piano_roll_grid(x, y) {
                    let grid_region = self.layout.piano_roll_grid;
                    let relative_x = x.saturating_sub(grid_region.x);
                    let relative_y = y.saturating_sub(grid_region.y);

                    let tick = self.scroll_x + (relative_x as u32 * self.zoom);
                    // Use layout.visible_pitches to match rendering formula: pitch = scroll_y + visible_pitches - 1 - row
                    // Subtract TIME_RULER_HEIGHT because the ruler occupies the first row of grid_region
                    let pitch_row = relative_y.saturating_sub(TIME_RULER_HEIGHT) as u8;
                    let pitch = (self.scroll_y + self.layout.visible_pitches.max(1) - 1)
                        .saturating_sub(pitch_row)
                        .min(127);

                    self.cursor_tick = tick;
                    self.cursor_pitch = pitch;
                }
            }
            DragState::MovingNotes {
                last_x,
                last_y,
                start_tick: _,
                start_pitch: _,
            } => {
                // Calculate movement delta in screen coordinates
                let dx = x as i32 - last_x as i32;
                let dy = y as i32 - last_y as i32;

                // Convert horizontal delta to ticks (positive dx = move right = later in time)
                if dx != 0 {
                    let tick_delta = dx * self.zoom as i32;
                    self.move_selected_notes_horizontal_no_undo(tick_delta);
                }

                // Convert vertical delta to pitch (negative dy = move up = higher pitch)
                if dy != 0 {
                    // Each row is 1 semitone
                    let semitone_delta = -dy as i8; // Invert because screen Y increases downward
                    self.transpose_selected_notes_no_undo(semitone_delta);
                }

                // Update last position for next delta calculation
                self.drag_state = DragState::MovingNotes {
                    last_x: x,
                    last_y: y,
                    start_tick: 0, // Not used after initial setup
                    start_pitch: 0,
                };
            }
            DragState::None => {}
        }
    }

    /// Handles mouse drag end (native only).
    pub fn handle_drag_end(&mut self) {
        // Mark modified if we were moving notes
        if matches!(self.drag_state, DragState::MovingNotes { .. }) {
            self.mark_modified();
        }
        self.drag_state = DragState::None;
    }

    /// Handles double-click events (native only).
    pub fn handle_double_click(&mut self, x: u16, y: u16) {
        if let Some(panel) = self.layout.panel_at(x, y) {
            match panel {
                FocusedPanel::PianoRoll => {
                    // Double-click in piano roll toggles note at mouse position
                    // Note: save_state is called inside delete_note_at_cursor and place_note
                    if self.layout.is_in_piano_roll_grid(x, y) {
                        // Convert mouse coordinates to tick/pitch
                        // This is essential during playback when cursor_tick follows the playhead
                        let grid_region = self.layout.piano_roll_grid;
                        let relative_x = x.saturating_sub(grid_region.x);
                        let relative_y = y.saturating_sub(grid_region.y);

                        // Calculate tick from X position
                        let tick = self.scroll_x + (relative_x as u32 * self.zoom);

                        // Calculate pitch from Y position (inverted - top is higher)
                        // Subtract TIME_RULER_HEIGHT because the ruler occupies the first row
                        let pitch_row = relative_y.saturating_sub(TIME_RULER_HEIGHT) as u8;
                        let pitch = (self.scroll_y + self.layout.visible_pitches.max(1) - 1)
                            .saturating_sub(pitch_row)
                            .min(127);

                        // Update cursor to mouse position for the note operation
                        self.cursor_tick = tick;
                        self.cursor_pitch = pitch;

                        let note_at_pos = self.selected_track().and_then(|track| {
                            track
                                .notes()
                                .iter()
                                .find(|n| n.pitch == pitch && n.is_active_at(tick))
                                .map(|n| n.id)
                        });

                        if let Some(note_id) = note_at_pos {
                            // Delete existing note using the ID we already found
                            self.delete_note_by_id(note_id);
                        } else {
                            // Create new note
                            self.place_note();
                        }
                    }
                }
                FocusedPanel::TrackList => {
                    // Double-click on track starts rename mode
                    self.start_rename_track();
                }
                FocusedPanel::Timeline => {
                    // Double-click on timeline - reset to start
                    self.stop_playback();
                }
                FocusedPanel::Keyboard => {
                    // Double-click on keyboard - reset octave
                    self.octave_offset = 0;
                    self.set_status("Octave reset to 0");
                }
            }
        }
    }
}
