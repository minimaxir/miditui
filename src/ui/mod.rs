//! Terminal user interface components.
//!
//! This module provides the visual components for the MIDI composer,
//! including the track list, piano roll, timeline, project view, and keyboard display.

mod combined;
mod dialogs;
mod help;
mod keyboard;
mod piano_roll;
mod project_timeline;
mod timeline;
mod tracks;

use crate::app::{App, FocusedPanel, LayoutRegions, ViewMode, PIANO_KEY_WIDTH};
use crate::midi::{contains_beat, contains_measure, TICKS_PER_BEAT};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub use combined::render_combined;
pub use dialogs::{
    render_file_browser, render_new_project_dialog, render_save_dialog, render_soundfont_dialog,
};
pub use help::render_help;
pub use keyboard::render_keyboard;
pub use piano_roll::render_piano_roll;
pub use project_timeline::{render_project_timeline, render_project_timeline_compact};
pub use timeline::render_timeline;
pub use tracks::render_track_list;

/// Renders a time ruler showing measure and beat markers.
///
/// Shared between Piano Roll and Project Timeline views.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `area` - The area to render the ruler in (should be 1 row high)
/// * `scroll_x` - Horizontal scroll position in ticks
/// * `zoom` - Number of ticks per display column
pub fn render_time_ruler(frame: &mut Frame, area: Rect, scroll_x: u32, zoom: u32) {
    let mut ruler_spans: Vec<Span> = Vec::with_capacity(area.width as usize);
    let mut col = 0u16;

    while col < area.width {
        let tick = scroll_x + (col as u32 * zoom);
        let is_measure = contains_measure(tick, zoom);
        let is_beat = contains_beat(tick, zoom);

        if is_measure {
            let measure_ticks = TICKS_PER_BEAT * 4;
            let measure_tick = if tick.is_multiple_of(measure_ticks) {
                tick
            } else {
                ((tick / measure_ticks) + 1) * measure_ticks
            };
            let measure_num = measure_tick / measure_ticks + 1;
            let measure_str = format!("{}", measure_num);
            let chars_remaining = (area.width - col) as usize;

            if measure_str.len() <= chars_remaining {
                ruler_spans.push(Span::styled(
                    measure_str.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
                col += measure_str.len() as u16;
                continue;
            } else {
                ruler_spans.push(Span::styled("|", Style::default().fg(Color::Yellow)));
            }
        } else if is_beat {
            ruler_spans.push(Span::styled(".", Style::default().fg(Color::DarkGray)));
        } else {
            ruler_spans.push(Span::styled(" ", Style::default().fg(Color::DarkGray)));
        }
        col += 1;
    }

    frame.render_widget(Paragraph::new(Line::from(ruler_spans)), area);
}

/// Calculates the layout regions for the given terminal size and view mode.
///
/// This is called during rendering to update the layout regions used
/// for mouse hit testing and auto-scroll calculations.
fn calculate_layout(size: Rect, view_mode: ViewMode) -> (LayoutRegions, [Rect; 3], [Rect; 2]) {
    // Main vertical layout: timeline, content, keyboard
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Timeline/transport
            Constraint::Min(10),   // Content area
            Constraint::Length(5), // Keyboard
        ])
        .split(size);

    // Content area: track list on left, piano roll on right
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(30), // Track list
            Constraint::Min(40),    // Piano roll
        ])
        .split(main_chunks[1]);

    // Calculate grid area based on view mode
    // Each view has different left-side content width:
    // - PianoRoll: 5 columns for piano keys
    // - ProjectTimeline: 12 columns for track labels
    // - Combined: use piano roll width (it's in the top half, 55% of content area)
    let piano_roll = content_chunks[1];
    let left_content_width = match view_mode {
        ViewMode::PianoRoll | ViewMode::Combined => PIANO_KEY_WIDTH,
        ViewMode::ProjectTimeline => 12, // DEFAULT_LABEL_WIDTH from project_timeline
    };

    // For Combined view, the piano roll only takes 55% of the content area height
    // We need to calculate the actual piano roll height for correct mouse hit testing
    let actual_piano_roll_height = match view_mode {
        ViewMode::Combined => {
            // Match the 55% split used in combined.rs
            (piano_roll.height as u32 * 55 / 100) as u16
        }
        _ => piano_roll.height,
    };

    let piano_roll_grid = Rect {
        x: piano_roll.x + 1 + left_content_width,
        y: piano_roll.y + 1,
        width: piano_roll.width.saturating_sub(2 + left_content_width),
        // Use actual piano roll height (accounts for Combined view's 55% split)
        height: actual_piano_roll_height.saturating_sub(2),
    };

    // Calculate visible pitches based on available grid height.
    // Subtract 1 for the time ruler row, and cap at 127 (max MIDI pitch).
    let visible_pitches = piano_roll_grid.height.saturating_sub(1).min(127) as u8;

    let layout = LayoutRegions {
        timeline: main_chunks[0],
        track_list: content_chunks[0],
        piano_roll,
        piano_roll_grid,
        keyboard: main_chunks[2],
        // Ruler regions are set during rendering
        piano_roll_ruler: Rect::default(),
        project_timeline_ruler: Rect::default(),
        visible_pitches,
    };

    // Convert to arrays for returning
    let main_arr = [main_chunks[0], main_chunks[1], main_chunks[2]];
    let content_arr = [content_chunks[0], content_chunks[1]];

    (layout, main_arr, content_arr)
}

/// Renders the complete UI layout and updates layout regions.
///
/// The layout is divided into:
/// - Top: Timeline with transport controls and position display
/// - Left: Track list with mute/solo controls
/// - Center: Piano roll editor OR project timeline (based on view mode)
/// - Bottom: Piano keyboard for live input
pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();
    let (layout, main_chunks, content_chunks) = calculate_layout(size, app.view_mode);

    // Update app's layout regions for mouse hit testing
    app.update_layout(layout);

    // Render each component with focus indication
    render_timeline(
        frame,
        main_chunks[0],
        app,
        app.focused_panel == FocusedPanel::Timeline,
    );

    render_track_list(
        frame,
        content_chunks[0],
        app,
        app.focused_panel == FocusedPanel::TrackList,
    );

    // Render based on current view mode and collect ruler regions
    let is_focused = app.focused_panel == FocusedPanel::PianoRoll;
    let (piano_roll_ruler, project_timeline_ruler) = match app.view_mode {
        ViewMode::Combined => render_combined(frame, content_chunks[1], app, is_focused),
        ViewMode::PianoRoll => {
            let ruler = render_piano_roll(frame, content_chunks[1], app, is_focused);
            (ruler, None)
        }
        ViewMode::ProjectTimeline => {
            let ruler = render_project_timeline(frame, content_chunks[1], app, is_focused);
            (None, ruler)
        }
    };

    // Update ruler regions in layout for mouse hit testing
    app.layout.piano_roll_ruler = piano_roll_ruler.unwrap_or_default();
    app.layout.project_timeline_ruler = project_timeline_ruler.unwrap_or_default();

    render_keyboard(
        frame,
        main_chunks[2],
        app,
        app.focused_panel == FocusedPanel::Keyboard,
    );
}

/// Helper function to center a rectangle within another rectangle.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
