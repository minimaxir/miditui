//! Track list rendering.
//!
//! Displays all tracks with their names, instruments, volume, pan, and mute/solo states.
//! Supports both compact (single-line) and expanded (two-line) views.
//! Includes a "Remove Track" button and rename input functionality.

use crate::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// Height reserved for the control hints at the bottom.
const CONTROLS_HEIGHT: u16 = 2;

/// Returns the display color for a volume value.
///
/// Red for clipping (>100), yellow for hot (>80), green otherwise.
#[inline]
fn volume_color(volume: u8) -> Color {
    if volume > 100 {
        Color::Red
    } else if volume > 80 {
        Color::Yellow
    } else {
        Color::Green
    }
}

/// Formats a pan value (0-127) as a display string.
///
/// Returns "L##" for left, "R##" for right, "C  " for center.
#[inline]
fn format_pan(pan: u8) -> String {
    if pan < 64 {
        format!("L{:2}", 64 - pan)
    } else if pan > 64 {
        format!("R{:2}", pan - 64)
    } else {
        "C  ".to_string()
    }
}

/// Renders the track list panel on the left side.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `area` - The area to render in
/// * `app` - Application state
/// * `focused` - Whether this panel is focused
pub fn render_track_list(frame: &mut Frame, area: Rect, app: &App, focused: bool) {
    let block = Block::default()
        .title(" Tracks ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { Color::Cyan } else { Color::Gray }));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split the inner area into track list and controls
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                  // Track list
            Constraint::Length(CONTROLS_HEIGHT), // Control hints
        ])
        .split(inner);

    // Build list items from tracks
    let items: Vec<ListItem> = app
        .project()
        .tracks()
        .iter()
        .enumerate()
        .map(|(i, track)| {
            // Check if this track is currently being renamed
            let is_renaming = app.renaming_track && i == app.selected_track_index;

            // Check if track is currently playing audio
            let is_active = app.active_tracks.contains(&i);

            // Build status indicators
            let mute_indicator = if track.muted {
                Span::styled(
                    "M",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(".", Style::default().fg(Color::DarkGray))
            };

            let solo_indicator = if track.solo {
                Span::styled(
                    "S",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(".", Style::default().fg(Color::DarkGray))
            };

            // Activity indicator (shows when track is playing audio)
            let activity_indicator = if is_active {
                Span::styled(
                    "*",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(" ", Style::default().fg(Color::DarkGray))
            };

            // Determine name style based on selection and activity
            let name_style = if i == app.selected_track_index {
                if is_active {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                }
            } else if is_active {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            };

            if app.expanded_tracks {
                // Expanded view: two lines per track
                // Line 1: indicators + track name
                // Line 2: volume + pan + instrument
                let max_name_len = area.width.saturating_sub(6) as usize;

                let line1 = if is_renaming {
                    let display_name = if app.rename_buffer.len() > max_name_len {
                        format!(
                            "{}...",
                            &app.rename_buffer[..max_name_len.saturating_sub(3)]
                        )
                    } else {
                        format!("{}_", app.rename_buffer.clone())
                    };
                    Line::from(vec![
                        mute_indicator,
                        solo_indicator,
                        activity_indicator,
                        Span::raw(" "),
                        Span::styled(
                            display_name,
                            Style::default()
                                .fg(Color::Yellow)
                                .bg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ])
                } else {
                    let name = if track.name.len() > max_name_len {
                        format!("{}...", &track.name[..max_name_len.saturating_sub(3)])
                    } else {
                        track.name.clone()
                    };
                    Line::from(vec![
                        mute_indicator,
                        solo_indicator,
                        activity_indicator,
                        Span::raw(" "),
                        Span::styled(name, name_style),
                    ])
                };

                // Line 2: volume, pan, instrument
                let vol_str = format!("V{:3}", track.volume);
                let pan_str = format_pan(track.pan);

                let instrument = app.get_instrument_name(track.program);
                let max_inst_len = area.width.saturating_sub(14) as usize;
                let instrument_display = if instrument.len() > max_inst_len {
                    format!("{}...", &instrument[..max_inst_len.saturating_sub(3)])
                } else {
                    instrument.to_string()
                };

                let line2 = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(vol_str, Style::default().fg(volume_color(track.volume))),
                    Span::raw(" "),
                    Span::styled(pan_str, Style::default().fg(Color::Cyan)),
                    Span::raw(" "),
                    Span::styled(instrument_display, Style::default().fg(Color::DarkGray)),
                ]);

                ListItem::new(vec![line1, line2])
            } else {
                // Compact view: single line per track
                let volume_span = if track.volume != 100 {
                    let vol_bar = format!("V{:3}", track.volume);
                    Some(Span::styled(
                        vol_bar,
                        Style::default().fg(volume_color(track.volume)),
                    ))
                } else {
                    None
                };

                let pan_span = if track.pan != 64 {
                    Some(Span::styled(
                        format_pan(track.pan),
                        Style::default().fg(Color::Cyan),
                    ))
                } else {
                    None
                };

                // Calculate max name length based on whether vol/pan are shown
                let extra_chars =
                    volume_span.as_ref().map_or(0, |_| 5) + pan_span.as_ref().map_or(0, |_| 4);
                let max_name_len = area.width.saturating_sub(10 + extra_chars as u16) as usize;

                // Build spans list dynamically based on what's shown
                let mut spans = vec![
                    mute_indicator,
                    solo_indicator,
                    activity_indicator,
                    Span::raw(" "),
                ];

                if let Some(vol) = volume_span {
                    spans.push(vol);
                    spans.push(Span::raw(" "));
                }

                if let Some(pan) = pan_span {
                    spans.push(pan);
                    spans.push(Span::raw(" "));
                }

                if is_renaming {
                    // Show rename buffer with cursor
                    let display_name = if app.rename_buffer.len() > max_name_len {
                        format!(
                            "{}...",
                            &app.rename_buffer[..max_name_len.saturating_sub(3)]
                        )
                    } else {
                        format!("{}_", app.rename_buffer.clone())
                    };

                    spans.push(Span::styled(
                        display_name,
                        Style::default()
                            .fg(Color::Yellow)
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    // Track name (truncated if needed)
                    let name = if track.name.len() > max_name_len {
                        format!("{}...", &track.name[..max_name_len.saturating_sub(3)])
                    } else {
                        track.name.clone()
                    };

                    spans.push(Span::styled(name, name_style));
                }

                ListItem::new(Line::from(spans))
            }
        })
        .collect();

    // Create list widget with selection highlight
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 40, 40))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    // Track list state for selection
    let mut state = ListState::default();
    state.select(Some(app.selected_track_index));

    frame.render_stateful_widget(list, chunks[0], &mut state);

    // Render control hints
    let key_style = Style::default().fg(Color::Yellow);
    let desc_style = Style::default().fg(Color::DarkGray);

    let line1 = Line::from(vec![
        Span::styled("[", desc_style),
        Span::styled(";/'", key_style),
        Span::styled("]Vol ", desc_style),
        Span::styled("[", desc_style),
        Span::styled("(/)", key_style),
        Span::styled("]Pan ", desc_style),
        Span::styled("[", desc_style),
        Span::styled("</>", key_style),
        Span::styled("]Inst", desc_style),
    ]);

    let line2 = Line::from(vec![
        Span::styled("[", desc_style),
        Span::styled("m", key_style),
        Span::styled("]Mute ", desc_style),
        Span::styled("[", desc_style),
        Span::styled("s", key_style),
        Span::styled("]Solo ", desc_style),
        Span::styled("[", desc_style),
        Span::styled("x", key_style),
        Span::styled("]Del", desc_style),
    ]);

    let controls = Paragraph::new(vec![line1, line2]);
    frame.render_widget(controls, chunks[1]);
}
