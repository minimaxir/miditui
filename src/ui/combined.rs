//! Combined view rendering.
//!
//! Displays both the Piano Roll and Project Timeline simultaneously,
//! split horizontally. This provides a comprehensive view for editing.
//! The Project Timeline uses a compact label width to align with the
//! Piano Roll's key column for visual consistency of playhead indicators.

use super::{render_piano_roll, render_project_timeline_compact};
use crate::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

/// Renders the combined view with Piano Roll on top and Project Timeline below.
///
/// The view is split horizontally with roughly equal space for each component.
/// Both views share the same horizontal scroll position (`scroll_x`) for
/// synchronized navigation through the timeline. The Project Timeline uses
/// a compact label width (5 chars) to align playhead indicators with the
/// Piano Roll's key column.
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
/// A tuple of (piano_roll_ruler, project_timeline_ruler) regions for mouse hit testing.
pub fn render_combined(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    focused: bool,
) -> (Option<Rect>, Option<Rect>) {
    // Split the area horizontally: Piano Roll on top, Project Timeline below
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55), // Piano Roll gets slightly more space for note editing
            Constraint::Percentage(45), // Project Timeline for track overview
        ])
        .split(area);

    // Render Piano Roll in the top section
    let piano_roll_ruler = render_piano_roll(frame, chunks[0], app, focused);

    // Render Project Timeline in the bottom section with compact labels
    // to align playhead indicators with the Piano Roll's key column
    let project_timeline_ruler = render_project_timeline_compact(frame, chunks[1], app, focused);

    (piano_roll_ruler, project_timeline_ruler)
}
