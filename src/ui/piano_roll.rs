//! Piano roll editor rendering.
//!
//! Displays notes on a grid with pitch on the Y-axis and time on the X-axis.
//! Similar to a DAW piano roll interface. Includes visual indicators for
//! notes that are scrolled off-screen.

use crate::app::{App, EditMode};
use crate::midi::{contains_beat, contains_measure, note_to_name, Note};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

// Note: visible_pitches is now dynamically calculated based on terminal height.
// See App::layout.visible_pitches for the actual value used in mouse handling.

/// Tracks which edges of the piano roll have notes scrolled off-screen.
///
/// This struct is populated by scanning all notes in the selected track
/// and checking if any fall outside the currently visible viewport.
#[derive(Debug, Default, Clone, Copy)]
struct OffScreenIndicators {
    /// Notes exist above the visible pitch range.
    above: bool,
    /// Notes exist below the visible pitch range.
    below: bool,
    /// Notes extend to the left of the visible tick range.
    left: bool,
    /// Notes extend to the right of the visible tick range.
    right: bool,
}

impl OffScreenIndicators {
    /// Calculates which edges have off-screen notes based on the current viewport.
    ///
    /// # Arguments
    ///
    /// * `notes` - All notes in the selected track
    /// * `scroll_x` - Horizontal scroll position in ticks
    /// * `scroll_y` - Vertical scroll position (lowest visible pitch)
    /// * `visible_ticks` - Number of ticks visible horizontally
    /// * `visible_pitches` - Number of pitches visible vertically
    ///
    /// # Returns
    ///
    /// Indicators showing which edges have off-screen notes
    fn calculate(
        notes: &[Note],
        scroll_x: u32,
        scroll_y: u8,
        visible_ticks: u64,
        visible_pitches: u8,
    ) -> Self {
        let mut indicators = Self::default();

        // Calculate viewport boundaries
        let pitch_min = scroll_y;
        let pitch_max = scroll_y.saturating_add(visible_pitches).min(128);
        let tick_min = scroll_x;
        let tick_max = scroll_x.saturating_add(visible_ticks as u32);

        for note in notes {
            // Check if note is above visible range
            if note.pitch >= pitch_max {
                indicators.above = true;
            }

            // Check if note is below visible range
            if note.pitch < pitch_min {
                indicators.below = true;
            }

            // Check if note extends to the left of visible range
            if note.start_tick < tick_min && note.end_tick() > tick_min {
                indicators.left = true;
            }

            // Check if note extends to the right of visible range
            if note.start_tick < tick_max && note.end_tick() > tick_max {
                indicators.right = true;
            }

            // Early exit if all indicators are set
            if indicators.above && indicators.below && indicators.left && indicators.right {
                break;
            }
        }

        indicators
    }
}

/// Builds a compact indicator string for the title showing off-screen notes.
///
/// # Arguments
///
/// * `indicators` - The calculated off-screen note indicators
///
/// # Returns
///
/// A string like "[^v<>]" showing which directions have off-screen notes.
/// Empty string if no off-screen notes exist.
fn build_title_indicator(indicators: &OffScreenIndicators) -> String {
    if !indicators.above && !indicators.below && !indicators.left && !indicators.right {
        return String::new();
    }

    let mut parts = Vec::with_capacity(4);
    if indicators.above {
        parts.push('^');
    }
    if indicators.below {
        parts.push('v');
    }
    if indicators.left {
        parts.push('<');
    }
    if indicators.right {
        parts.push('>');
    }

    format!("[{}]", parts.into_iter().collect::<String>())
}

/// Renders the piano roll editor.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `area` - The area to render in
/// * `app` - Application state
/// * `focused` - Whether this panel is focused
///
/// # Returns
///
/// The time ruler region for mouse hit testing, or None if too small to render.
pub fn render_piano_roll(frame: &mut Frame, area: Rect, app: &App, focused: bool) -> Option<Rect> {
    // Get notes first to calculate off-screen indicators for the title
    let track_notes = app.selected_track().map(|t| t.notes()).unwrap_or(&[]);

    // Pre-calculate visible range for indicator detection
    // We need a rough estimate before we know the exact inner dimensions
    let estimated_grid_width = area.width.saturating_sub(7); // 5 piano + 2 borders
    let estimated_visible_ticks = app.zoom as u64 * estimated_grid_width as u64;
    // Estimate visible pitches: area height - 2 borders - 1 ruler, capped at 127
    let estimated_visible_pitches = area.height.saturating_sub(3).min(127) as u8;

    // Calculate off-screen indicators
    let indicators = OffScreenIndicators::calculate(
        track_notes,
        app.scroll_x,
        app.scroll_y,
        estimated_visible_ticks,
        estimated_visible_pitches,
    );

    // Build title with off-screen indicators and instrument name
    let track_name = app
        .selected_track()
        .map(|t| t.name.as_str())
        .unwrap_or("No Track");

    // Get instrument name for the current track
    let instrument_name = app
        .selected_track()
        .map(|t| app.get_instrument_name(t.program))
        .unwrap_or("Unknown");

    // Build indicator suffix for title (shows which edges have off-screen notes)
    let indicator_suffix = build_title_indicator(&indicators);

    let title = format!(
        " Piano Roll - {} ({}) {}",
        track_name, instrument_name, indicator_suffix
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { Color::Cyan } else { Color::Gray }));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 10 || inner.height < 6 {
        return None; // Too small to render (need room for ruler + at least 5 pitch rows)
    }

    // Calculate visible range
    // Layout: [piano keys (5 cols)] [time ruler + grid]
    // The time ruler occupies 1 row at the top, grid occupies remaining rows
    let piano_width = 5u16; // Width for note labels
    let grid_width = inner.width.saturating_sub(piano_width);
    let ruler_height = 1u16; // Time ruler takes 1 row
    let grid_height = inner.height.saturating_sub(ruler_height);
    // Calculate visible pitches for pitch calculations (capped at 127 max MIDI pitch)
    let visible_pitches = grid_height.min(127) as u8;

    let visible_ticks = app.zoom as u64 * grid_width as u64;

    // Render the time ruler at the top (above the grid, aligned with grid columns)
    let ruler_rect = Rect::new(inner.x + piano_width, inner.y, grid_width, ruler_height);
    super::render_time_ruler(frame, ruler_rect, app.scroll_x, app.zoom);

    // Render ruler label area (empty space above piano keys for alignment)
    frame.render_widget(
        Paragraph::new("     ").style(Style::default().bg(Color::Rgb(20, 20, 20))),
        Rect::new(inner.x, inner.y, piano_width, ruler_height),
    );

    // Recalculate indicators with exact dimensions for edge rendering
    let indicators = OffScreenIndicators::calculate(
        track_notes,
        app.scroll_x,
        app.scroll_y,
        visible_ticks,
        visible_pitches,
    );

    // Style for off-screen indicators (yellow on dark background for high visibility)
    let indicator_style = Style::default()
        .fg(Color::Yellow)
        .bg(Color::Rgb(60, 50, 0))
        .add_modifier(Modifier::BOLD);

    // Render each row (pitch), starting below the ruler
    for row in 0..grid_height {
        let pitch = (app.scroll_y + visible_pitches - 1 - row as u8).min(127);
        let y = inner.y + ruler_height + row; // Offset by ruler height

        // Determine if this is an edge row for vertical indicators
        let is_top_row = row == 0;
        let is_bottom_row = row == grid_height - 1;

        // Note name label (piano key column)
        let note_name = note_to_name(pitch);
        let is_black_key = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        let is_c = pitch.is_multiple_of(12);

        let show_key_indicator =
            (is_top_row && indicators.above) || (is_bottom_row && indicators.below);

        let key_style = if show_key_indicator {
            // Highlight the piano key to indicate off-screen notes
            indicator_style
        } else if pitch == app.cursor_pitch {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else if is_black_key {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else if is_c {
            Style::default().bg(Color::White).fg(Color::Black)
        } else {
            Style::default().bg(Color::Gray).fg(Color::Black)
        };

        // Build the key label with optional off-screen indicator
        let key_text = if is_top_row && indicators.above {
            format!("{:>3}^ ", note_name)
        } else if is_bottom_row && indicators.below {
            format!("{:>3}v ", note_name)
        } else {
            format!("{:>4} ", note_name)
        };

        let key_label = Paragraph::new(key_text).style(key_style);
        frame.render_widget(key_label, Rect::new(inner.x, y, piano_width, 1));

        // Grid row
        let mut grid_line: Vec<Span> = Vec::with_capacity(grid_width as usize);
        let grid_x_start = inner.x + piano_width;

        for col in 0..grid_width {
            let tick = app.scroll_x + (col as u32 * app.zoom);

            // Determine if this is an edge column for horizontal indicators
            let is_left_col = col == 0;
            let is_right_col = col == grid_width - 1;

            // Determine cell content
            let is_cursor =
                tick / app.zoom == app.cursor_tick / app.zoom && pitch == app.cursor_pitch;
            // Use range-based detection to show markers even with unaligned scroll
            let is_beat = contains_beat(tick, app.zoom);
            let is_measure = contains_measure(tick, app.zoom);
            // Playhead uses cursor_tick to stay in sync with scroll position
            let is_playhead =
                app.audio.is_playing() && tick / app.zoom == app.cursor_tick / app.zoom;

            // Insert Mode indicator - shows a red vertical line at the recording position
            // If recording is active, it shows at the dynamic position; otherwise at cursor
            let insert_indicator_tick = if app.edit_mode == EditMode::Insert {
                app.get_insert_indicator_tick().unwrap_or(app.cursor_tick)
            } else {
                u32::MAX // Never matches when not in Insert Mode
            };
            let is_insert_indicator = app.edit_mode == EditMode::Insert
                && tick / app.zoom == insert_indicator_tick / app.zoom;

            // Check if a note occupies this cell
            let note_here = track_notes
                .iter()
                .find(|n| n.pitch == pitch && n.start_tick <= tick && n.end_tick() > tick);

            // Check if this note is currently being played (for highlight)
            // Uses display_position (with offset) so highlighting appears slightly early
            // to compensate for visual latency - the note flashes white before the
            // playhead reaches it, making it appear synchronized with audio output
            let display_pos = app.display_position_ticks();
            let is_note_active = note_here
                .map(|n| n.start_tick <= display_pos && n.end_tick() > display_pos)
                .unwrap_or(false);

            let show_left_indicator = is_left_col && indicators.left;
            let show_right_indicator = is_right_col && indicators.right;
            let show_top_indicator = is_top_row && indicators.above;
            let show_bottom_indicator = is_bottom_row && indicators.below;

            let (ch, style) = if let Some(note) = note_here {
                // Note cell - notes take priority over edge indicators
                let is_start = note.start_tick <= tick && note.start_tick + app.zoom > tick;
                let is_selected = app.selected_notes.contains(&note.id);
                // Verify both ID and position to ensure correct highlighting after scrolling
                let is_recently_added = app.is_recently_added_note(note.id, note.start_tick);

                // Highlight note in white if it's currently being played and highlighting is on
                let should_highlight =
                    is_note_active && app.audio.is_playing() && app.highlight_piano_roll();

                // Determine note background color based on state
                // Priority: insert indicator > playback highlight > recently added > selected > cursor > default
                let bg = if is_insert_indicator {
                    Color::Red // Note at insert indicator position
                } else if should_highlight {
                    Color::White // Active note highlighting during playback
                } else if is_recently_added {
                    Color::Blue // Recently added note (blue highlight)
                } else if is_selected {
                    Color::Magenta
                } else if is_cursor {
                    Color::Cyan
                } else {
                    Color::Green
                };

                let ch = if is_start { '[' } else { '=' };
                (ch, Style::default().fg(Color::Black).bg(bg))
            } else if is_insert_indicator {
                // Insert Mode indicator - red vertical line
                (
                    '|',
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )
            } else if is_cursor {
                // Cursor position
                ('_', Style::default().fg(Color::Cyan).bg(Color::DarkGray))
            } else if is_playhead {
                // Playhead position (during regular playback, not Insert Mode)
                (
                    '|',
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )
            } else if show_left_indicator {
                // Left edge indicator - note extends from left
                ('<', indicator_style)
            } else if show_right_indicator {
                // Right edge indicator - note extends to right
                ('>', indicator_style)
            } else if show_top_indicator || show_bottom_indicator {
                // Top/bottom edge indicator - show arrow on grid edges
                let ch = if show_top_indicator { '^' } else { 'v' };
                (ch, indicator_style)
            } else {
                // Grid background
                let bg = if is_black_key {
                    Color::Rgb(30, 30, 30)
                } else {
                    Color::Rgb(40, 40, 40)
                };

                let ch = if is_measure {
                    '|'
                } else if is_beat {
                    ':'
                } else {
                    '.'
                };

                let fg = if is_measure {
                    Color::White
                } else if is_beat {
                    Color::DarkGray
                } else {
                    Color::Rgb(60, 60, 60)
                };

                (ch, Style::default().fg(fg).bg(bg))
            };

            grid_line.push(Span::styled(ch.to_string(), style));
        }

        frame.render_widget(
            Paragraph::new(Line::from(grid_line)),
            Rect::new(grid_x_start, y, grid_width, 1),
        );
    }

    Some(ruler_rect)
}
