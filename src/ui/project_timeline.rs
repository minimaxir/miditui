//! Project timeline view rendering.
//!
//! Displays all tracks on a combined timeline, showing note blocks for each track
//! and visual feedback for tracks that are currently playing audio.

use crate::app::App;
use crate::midi::{contains_beat, contains_measure};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Track row height in the project timeline.
const TRACK_ROW_HEIGHT: u16 = 2;

/// Default width reserved for track labels on the left.
const DEFAULT_LABEL_WIDTH: u16 = 12;

/// Compact label width for combined view (matches piano key width).
pub const COMPACT_LABEL_WIDTH: u16 = 5;

/// Renders the project timeline view showing all tracks.
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
pub fn render_project_timeline(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    focused: bool,
) -> Option<Rect> {
    render_project_timeline_with_label_width(frame, area, app, focused, DEFAULT_LABEL_WIDTH)
}

/// Renders the project timeline with a compact label width for combined view.
///
/// This version uses a narrower label width to align with the piano roll's key column.
///
/// # Returns
///
/// The time ruler region for mouse hit testing, or None if too small to render.
pub fn render_project_timeline_compact(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    focused: bool,
) -> Option<Rect> {
    render_project_timeline_with_label_width(frame, area, app, focused, COMPACT_LABEL_WIDTH)
}

/// Internal render function with configurable label width.
fn render_project_timeline_with_label_width(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    focused: bool,
    label_width: u16,
) -> Option<Rect> {
    let block = Block::default()
        .title(" Project Timeline - All Tracks ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { Color::Cyan } else { Color::Gray }));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 20 || inner.height < 3 {
        return None; // Too small to render
    }

    // Calculate timeline dimensions
    let timeline_width = inner.width.saturating_sub(label_width);
    let max_tracks_visible = (inner.height / TRACK_ROW_HEIGHT) as usize;

    // Calculate which tracks to show (scrolled view if many tracks)
    let track_count = app.project().track_count();
    let start_track = if track_count > max_tracks_visible {
        app.selected_track_index
            .saturating_sub(max_tracks_visible / 2)
            .min(track_count.saturating_sub(max_tracks_visible))
    } else {
        0
    };
    let end_track = (start_track + max_tracks_visible).min(track_count);

    // Render time ruler at the top
    let ruler_rect = Rect::new(inner.x + label_width, inner.y, timeline_width, 1);
    super::render_time_ruler(frame, ruler_rect, app.scroll_x, app.zoom);

    // Render each visible track
    for (display_idx, track_idx) in (start_track..end_track).enumerate() {
        let track = &app.project().tracks()[track_idx];
        let track_y = inner.y + 1 + (display_idx as u16 * TRACK_ROW_HEIGHT);

        if track_y + TRACK_ROW_HEIGHT > inner.y + inner.height {
            break;
        }

        // Determine if track is selected, active, muted, or soloed
        let is_selected = track_idx == app.selected_track_index;
        let is_active = app.active_tracks.contains(&track_idx);
        let is_muted = track.muted;
        let is_solo = track.solo;

        // Render track label
        let label_style = if is_active {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else if is_muted {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Gray)
        };

        // Build label with indicators - adapt to label width
        let label_text = build_track_label(track, is_active, is_muted, is_solo, label_width);

        let label = Paragraph::new(label_text).style(label_style);
        frame.render_widget(
            label,
            Rect::new(inner.x, track_y, label_width, TRACK_ROW_HEIGHT),
        );

        // Render track content (note blocks on timeline)
        render_track_content(
            frame,
            Rect::new(
                inner.x + label_width,
                track_y,
                timeline_width,
                TRACK_ROW_HEIGHT,
            ),
            app,
            track_idx,
            is_selected,
            is_active,
            is_muted,
        );
    }

    // Render playhead if playing
    // The Piano Roll uses: tick / zoom == cursor_tick / zoom
    // So we calculate the screen column as: (cursor_tick / zoom) - (scroll_x / zoom)
    // This ensures the playhead appears in the same column as the Piano Roll
    if app.audio.is_playing() {
        let cursor_col = app.cursor_tick / app.zoom;
        let start_col = app.scroll_x / app.zoom;
        // Check if playhead is in visible range
        if cursor_col >= start_col {
            let screen_col = (cursor_col - start_col) as u16;
            if screen_col < timeline_width {
                let playhead_x = inner.x + label_width + screen_col;
                for row in 0..inner.height.saturating_sub(1) {
                    frame.render_widget(
                        Paragraph::new("|")
                            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                        Rect::new(playhead_x, inner.y + 1 + row, 1, 1),
                    );
                }
            }
        }
    }

    Some(ruler_rect)
}

/// Builds track label text adapted to the available width.
fn build_track_label(
    track: &crate::midi::Track,
    is_active: bool,
    is_muted: bool,
    is_solo: bool,
    label_width: u16,
) -> String {
    let active_char = if is_active { '*' } else { ' ' };

    if label_width <= COMPACT_LABEL_WIDTH {
        // Compact mode: just show activity indicator and truncated name
        let max_name = (label_width as usize).saturating_sub(2);
        let name = if track.name.len() > max_name {
            format!("{}.", &track.name[..max_name.saturating_sub(1)])
        } else {
            track.name.clone()
        };
        format!("{} {}", active_char, name)
    } else {
        // Full mode: show all indicators and longer name
        let mute_char = if is_muted { 'M' } else { ' ' };
        let solo_char = if is_solo { 'S' } else { ' ' };
        let max_name = (label_width as usize).saturating_sub(4);
        let name = if track.name.len() > max_name {
            format!("{}...", &track.name[..max_name.saturating_sub(3)])
        } else {
            track.name.clone()
        };
        format!("{}{}{} {}", mute_char, solo_char, active_char, name)
    }
}

/// Renders the content of a single track (note blocks).
fn render_track_content(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    track_idx: usize,
    is_selected: bool,
    is_active: bool,
    is_muted: bool,
) {
    let track = &app.project().tracks()[track_idx];

    // Create a representation of notes in the visible range
    // Use different colors for different tracks for visual distinction
    let track_colors = [
        Color::Blue,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Cyan,
        Color::Red,
        Color::LightBlue,
        Color::LightGreen,
    ];
    let base_color = track_colors[track_idx % track_colors.len()];

    // Determine note color: white when active (and highlighting enabled), else track color
    let note_color = if is_muted {
        Color::DarkGray
    } else if is_active && app.highlight_timeline() {
        Color::White
    } else {
        base_color
    };

    // Build the track content line by line
    for row in 0..area.height {
        let mut line_spans: Vec<Span> = Vec::with_capacity(area.width as usize);

        for col in 0..area.width {
            let tick = app.scroll_x + (col as u32 * app.zoom);
            let tick_end = tick + app.zoom;

            // Check if any note is active at this position
            let has_note = track
                .notes()
                .iter()
                .any(|n| n.start_tick < tick_end && n.end_tick() > tick);

            let is_cursor = is_selected && (tick / app.zoom == app.cursor_tick / app.zoom);

            let (ch, style) = if has_note {
                let bg = if is_cursor { Color::Cyan } else { note_color };
                ('=', Style::default().fg(Color::Black).bg(bg))
            } else if is_cursor && row == 0 {
                ('_', Style::default().fg(Color::Cyan))
            } else {
                // Grid background - use range-based detection for unaligned scroll
                let is_measure = contains_measure(tick, app.zoom);
                let is_beat = contains_beat(tick, app.zoom);

                let ch = if is_measure {
                    '|'
                } else if is_beat && row == 0 {
                    ':'
                } else {
                    ' '
                };

                let fg = if is_measure {
                    Color::DarkGray
                } else {
                    Color::Rgb(40, 40, 40)
                };

                (ch, Style::default().fg(fg))
            };

            line_spans.push(Span::styled(ch.to_string(), style));
        }

        frame.render_widget(
            Paragraph::new(Line::from(line_spans)),
            Rect::new(area.x, area.y + row, area.width, 1),
        );
    }
}
