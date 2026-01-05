//! Dialog overlays for save and load operations.
//!
//! Provides modal dialogs for saving projects with filename/format selection,
//! browsing files for loading, and selecting SoundFont.

use crate::app::{App, SaveFormat};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;
use std::path::Path;

use super::centered_rect;

/// Truncates a path string to fit within max_width, adding "..." prefix if needed.
#[inline]
fn truncate_path(path_str: &str, max_width: usize) -> String {
    if path_str.len() > max_width {
        format!(
            "...{}",
            &path_str[path_str.len().saturating_sub(max_width - 3)..]
        )
    } else {
        path_str.to_string()
    }
}

/// Extracts the display name from a path, returning "?" if extraction fails.
#[inline]
fn path_display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string()
}

/// Renders the save dialog overlay.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `app` - Application state
pub fn render_save_dialog(frame: &mut Frame, app: &App) {
    if !app.save_dialog.open {
        return;
    }

    let area = centered_rect(50, 30, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Save Project ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into filename input and format selection
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Label
            Constraint::Length(1), // Filename input
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Format label
            Constraint::Length(1), // Format selection
            Constraint::Length(1), // Spacer
            Constraint::Min(1),    // Instructions
        ])
        .split(inner);

    // Filename label
    frame.render_widget(
        Paragraph::new(Span::styled("Filename:", Style::default().fg(Color::White))),
        chunks[0],
    );

    // Filename input with cursor
    let extension = match app.save_dialog.format {
        SaveFormat::Json => ".json",
        SaveFormat::Oxm => ".oxm",
        SaveFormat::Midi => ".mid",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                &app.save_dialog.filename,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "_",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::RAPID_BLINK),
            ),
            Span::styled(extension, Style::default().fg(Color::DarkGray)),
        ])),
        chunks[1],
    );

    // Format label
    frame.render_widget(
        Paragraph::new(Span::styled("Format:", Style::default().fg(Color::White))),
        chunks[3],
    );

    // Format selection - helper to create styled checkbox
    let format_style = |selected: bool| {
        if selected {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };

    let is_json = app.save_dialog.format == SaveFormat::Json;
    let is_oxm = app.save_dialog.format == SaveFormat::Oxm;
    let is_midi = app.save_dialog.format == SaveFormat::Midi;

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[", Style::default().fg(Color::DarkGray)),
            Span::styled(if is_json { "X" } else { " " }, format_style(is_json)),
            Span::styled("] JSON  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[", Style::default().fg(Color::DarkGray)),
            Span::styled(if is_oxm { "X" } else { " " }, format_style(is_oxm)),
            Span::styled("] OXM  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[", Style::default().fg(Color::DarkGray)),
            Span::styled(if is_midi { "X" } else { " " }, format_style(is_midi)),
            Span::styled("] MIDI", Style::default().fg(Color::DarkGray)),
        ])),
        chunks[4],
    );

    // Instructions
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[Tab]", Style::default().fg(Color::Yellow)),
            Span::styled(" Toggle format  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Enter]", Style::default().fg(Color::Yellow)),
            Span::styled(" Save  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
        ])),
        chunks[6],
    );
}

/// Renders the file browser dialog overlay.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `app` - Application state
pub fn render_file_browser(frame: &mut Frame, app: &App) {
    if !app.file_browser.open {
        return;
    }

    let area = centered_rect(60, 70, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Open Project ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into path display and file list
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Current path
            Constraint::Length(1), // Separator
            Constraint::Min(5),    // File list
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

    // Current directory
    let path_str = app.file_browser.current_dir.display().to_string();
    let max_width = chunks[0].width.saturating_sub(2) as usize;
    let display_path = truncate_path(&path_str, max_width);

    frame.render_widget(
        Paragraph::new(Span::styled(display_path, Style::default().fg(Color::Cyan))),
        chunks[0],
    );

    // File list
    let visible_height = chunks[2].height as usize;
    let start_idx = app.file_browser.scroll;
    let end_idx = (start_idx + visible_height).min(app.file_browser.entries.len());

    let items: Vec<ListItem> = app.file_browser.entries[start_idx..end_idx]
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let idx = start_idx + i;
            let is_selected = idx == app.file_browser.selected;

            let (icon, name, style) = if path == &std::path::PathBuf::from("..") {
                (
                    "[..]",
                    "Parent Directory".to_string(),
                    Style::default().fg(Color::Blue),
                )
            } else if path.is_dir() {
                (
                    "[D]",
                    path_display_name(path),
                    Style::default().fg(Color::Blue),
                )
            } else {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let (icon, color) = match ext {
                    "oxm" => ("[B]", Color::White),
                    "mid" | "midi" => ("[M]", Color::Magenta),
                    _ => ("[J]", Color::White),
                };
                (icon, path_display_name(path), Style::default().fg(color))
            };

            let display_style = if is_selected {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", icon), Style::default().fg(Color::DarkGray)),
                Span::styled(name, display_style),
            ]))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, chunks[2]);

    // Instructions
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[Up/Down]", Style::default().fg(Color::Yellow)),
            Span::styled(" Navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Enter]", Style::default().fg(Color::Yellow)),
            Span::styled(" Open  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
        ])),
        chunks[3],
    );
}

/// Renders the new project confirmation dialog overlay.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `app` - Application state
pub fn render_new_project_dialog(frame: &mut Frame, app: &App) {
    if !app.new_project_dialog.open {
        return;
    }

    let area = centered_rect(45, 25, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" New Project ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into message and buttons
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Warning message
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Buttons
            Constraint::Length(1), // Spacer
            Constraint::Min(1),    // Instructions
        ])
        .split(inner);

    // Warning message
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Create a new project?",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Unsaved changes will be lost.",
                Style::default().fg(Color::Red),
            )),
        ]),
        chunks[1],
    );

    // Button styles
    let yes_style = if app.new_project_dialog.selected == 0 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };

    let no_style = if app.new_project_dialog.selected == 1 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };

    // Buttons - center them
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("       ", Style::default()), // Left padding
            Span::styled(" Yes ", yes_style),
            Span::styled("     ", Style::default()), // Spacing between buttons
            Span::styled(" No ", no_style),
        ])),
        chunks[3],
    );

    // Instructions
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[Left/Right]", Style::default().fg(Color::Yellow)),
            Span::styled(" Select  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Enter]", Style::default().fg(Color::Yellow)),
            Span::styled(" Confirm  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
        ])),
        chunks[5],
    );
}

/// Renders the SoundFont browser dialog overlay.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `app` - Application state
pub fn render_soundfont_dialog(frame: &mut Frame, app: &App) {
    if !app.soundfont_dialog.open {
        return;
    }

    let area = centered_rect(65, 75, frame.area());
    frame.render_widget(Clear, area);

    // Use different title/style for first-load modal
    let (title, border_color) = if app.soundfont_dialog.is_first_load {
        (" Select a SoundFont to Continue ", Color::Yellow)
    } else {
        (" Load SoundFont ", Color::Cyan)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into header, path display, file list, and instructions
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if app.soundfont_dialog.is_first_load {
                3
            } else {
                0
            }),
            Constraint::Length(1), // Current path
            Constraint::Length(1), // Separator
            Constraint::Min(5),    // File list
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

    // Show explanation for first-load modal
    if app.soundfont_dialog.is_first_load {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "A SoundFont (.sf2) is required for audio playback.",
                    Style::default().fg(Color::White),
                )),
                Line::from(Span::styled(
                    "Browse to select a SoundFont file.",
                    Style::default().fg(Color::DarkGray),
                )),
            ]),
            chunks[0],
        );
    }

    // Current directory
    let path_str = app.soundfont_dialog.current_dir.display().to_string();
    let max_width = chunks[1].width.saturating_sub(2) as usize;
    let display_path = truncate_path(&path_str, max_width);

    frame.render_widget(
        Paragraph::new(Span::styled(display_path, Style::default().fg(Color::Cyan))),
        chunks[1],
    );

    // File list
    let visible_height = chunks[3].height as usize;
    let start_idx = app.soundfont_dialog.scroll;
    let end_idx = (start_idx + visible_height).min(app.soundfont_dialog.entries.len());

    let items: Vec<ListItem> = if app.soundfont_dialog.entries.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No SoundFont files found in this directory",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )))]
    } else {
        app.soundfont_dialog.entries[start_idx..end_idx]
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let idx = start_idx + i;
                let is_selected = idx == app.soundfont_dialog.selected;

                let (icon, name, style) = if path == &std::path::PathBuf::from("..") {
                    (
                        "[..]",
                        "Parent Directory".to_string(),
                        Style::default().fg(Color::Blue),
                    )
                } else if path.is_dir() {
                    (
                        "[D]",
                        path_display_name(path),
                        Style::default().fg(Color::Blue),
                    )
                } else {
                    (
                        "[SF2]",
                        path_display_name(path),
                        Style::default().fg(Color::Green),
                    )
                };

                let display_style = if is_selected {
                    style.add_modifier(Modifier::REVERSED)
                } else {
                    style
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", icon), Style::default().fg(Color::DarkGray)),
                    Span::styled(name, display_style),
                ]))
            })
            .collect()
    };

    let list = List::new(items);
    frame.render_widget(list, chunks[3]);

    // Instructions - show different message for first-load modal
    let instructions = if app.soundfont_dialog.is_first_load {
        Line::from(vec![
            Span::styled("[Up/Down]", Style::default().fg(Color::Yellow)),
            Span::styled(" Navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Enter]", Style::default().fg(Color::Yellow)),
            Span::styled(" Select", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled("[Up/Down]", Style::default().fg(Color::Yellow)),
            Span::styled(" Navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Enter]", Style::default().fg(Color::Yellow)),
            Span::styled(" Select  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
        ])
    };

    frame.render_widget(Paragraph::new(instructions), chunks[4]);
}
