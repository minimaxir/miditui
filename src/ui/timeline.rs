//! Timeline and transport controls rendering.
//!
//! Displays the current position, tempo, time signature, and playback status.

use crate::app::App;
use crate::audio::PlaybackState;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Renders the timeline/transport bar at the top of the screen.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `area` - The area to render in
/// * `app` - Application state
/// * `focused` - Whether this panel is focused
pub fn render_timeline(frame: &mut Frame, area: Rect, app: &App, focused: bool) {
    let block = Block::default()
        .title(" Transport ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { Color::Cyan } else { Color::Gray }));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Divide into sections
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20), // Playback controls
            Constraint::Length(20), // Position
            Constraint::Length(15), // Tempo
            Constraint::Length(10), // Time sig
            Constraint::Min(20),    // Status/mode
        ])
        .split(inner);

    // Playback controls
    let play_status = match app.audio.playback_state() {
        PlaybackState::Playing => Span::styled(
            " [>] PLAY ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        PlaybackState::Paused => Span::styled(
            " [||] PAUSE ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        PlaybackState::Stopped => Span::styled(
            " [.] STOP ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    };
    frame.render_widget(Paragraph::new(Line::from(play_status)), chunks[0]);

    // Position display (measure:beat:tick)
    let position = app.position_string();
    let position_widget = Paragraph::new(Line::from(vec![
        Span::styled("Pos: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            position,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(position_widget, chunks[1]);

    // Tempo display
    let tempo_widget = Paragraph::new(Line::from(vec![
        Span::styled("BPM: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", app.project().tempo),
            Style::default().fg(Color::White),
        ),
    ]));
    frame.render_widget(tempo_widget, chunks[2]);

    // Time signature
    let time_sig = format!(
        "{}/{}",
        app.project().time_sig_numerator,
        app.project().time_sig_denominator
    );
    let time_sig_widget = Paragraph::new(Line::from(vec![Span::styled(
        time_sig,
        Style::default().fg(Color::White),
    )]));
    frame.render_widget(time_sig_widget, chunks[3]);

    // Status message or mode indicator
    let status_line = if let Some((msg, _)) = &app.status_message {
        Line::from(Span::styled(
            msg.as_str(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::ITALIC),
        ))
    } else {
        let mode_str = match app.edit_mode {
            crate::app::EditMode::Normal => "NORMAL",
            crate::app::EditMode::Insert => "INSERT",
            crate::app::EditMode::Select => "SELECT",
        };
        let mode_color = match app.edit_mode {
            crate::app::EditMode::Normal => Color::Blue,
            crate::app::EditMode::Insert => Color::Green,
            crate::app::EditMode::Select => Color::Magenta,
        };
        Line::from(Span::styled(
            format!("-- {} --", mode_str),
            Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
        ))
    };
    frame.render_widget(Paragraph::new(status_line), chunks[4]);
}
