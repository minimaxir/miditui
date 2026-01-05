//! Piano keyboard display.
//!
//! Shows the computer keyboard to MIDI note mapping and currently held keys.
//! Also displays contextual key bindings based on the current edit mode.

use crate::app::{App, EditMode, KEYBOARD_MAP};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Builds a keyboard row from a slice of key characters.
///
/// Maps each key to its corresponding MIDI note and applies appropriate styling
/// based on whether the note is a black key or was recently added.
fn build_keyboard_row(keys: &[char], app: &App) -> Vec<Span<'static>> {
    keys.iter()
        .map(|&key| {
            let base_note = KEYBOARD_MAP
                .iter()
                .find(|(k, _)| k.to_ascii_uppercase() == key)
                .map(|(_, n)| *n);

            if let Some(base) = base_note {
                let note = (base as i16 + app.octave_offset as i16 * 12) as u8;
                let is_black = matches!(note % 12, 1 | 3 | 6 | 8 | 10);
                let is_recently_added = app.is_recently_added_pitch(note);

                let style = if is_recently_added {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else if is_black {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD)
                };

                Span::styled(format!(" {} ", key), style)
            } else {
                Span::raw(format!(" {} ", key))
            }
        })
        .collect()
}

/// Renders the piano keyboard at the bottom of the screen.
///
/// Shows the keyboard mapping and highlights currently pressed keys.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `area` - The area to render in
/// * `app` - Application state
/// * `focused` - Whether this panel is focused
pub fn render_keyboard(frame: &mut Frame, area: Rect, app: &App, focused: bool) {
    let octave_str = if app.octave_offset >= 0 {
        format!("+{}", app.octave_offset)
    } else {
        format!("{}", app.octave_offset)
    };

    let block = Block::default()
        .title(format!(" Keyboard (Octave: {}) ", octave_str))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { Color::Cyan } else { Color::Gray }));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 {
        return;
    }

    // Keyboard layout: upper row (Q-I) and lower row (Z-M)
    const UPPER_KEYS: &[char] = &[
        'Q', '2', 'W', '3', 'E', 'R', '5', 'T', '6', 'Y', '7', 'U', 'I',
    ];
    const LOWER_KEYS: &[char] = &['Z', 'S', 'X', 'D', 'C', 'V', 'G', 'B', 'H', 'N', 'J', 'M'];

    let upper_row = build_keyboard_row(UPPER_KEYS, app);
    let lower_row = build_keyboard_row(LOWER_KEYS, app);

    // Contextual help text based on current mode
    let help_line = build_contextual_help(app.edit_mode);

    // Render rows
    if inner.height >= 1 {
        frame.render_widget(
            Paragraph::new(Line::from(upper_row)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    }
    if inner.height >= 2 {
        frame.render_widget(
            Paragraph::new(Line::from(lower_row)),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
    if inner.height >= 3 {
        frame.render_widget(
            Paragraph::new(help_line),
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
        );
    }
}

/// Builds the contextual help line based on the current edit mode.
///
/// Different modes show different relevant key bindings to guide the user.
fn build_contextual_help(mode: EditMode) -> Line<'static> {
    let key_style = Style::default().fg(Color::Yellow);
    let bracket_style = Style::default().fg(Color::DarkGray);
    let desc_style = Style::default().fg(Color::DarkGray);

    match mode {
        EditMode::Normal => {
            // Normal mode: show mode switching, file ops, and quit hint
            Line::from(vec![
                Span::styled("[", bracket_style),
                Span::styled("i", key_style),
                Span::styled("]Ins ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("v", key_style),
                Span::styled("]Sel ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("Space", key_style),
                Span::styled("]Play ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("^S", key_style),
                Span::styled("]Save ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("?", key_style),
                Span::styled("]Help ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("q", key_style),
                Span::styled("]Quit", desc_style),
            ])
        }
        EditMode::Insert => {
            // Insert mode: show how to play notes and exit
            Line::from(vec![
                Span::styled(
                    "INSERT MODE  ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("[", bracket_style),
                Span::styled("Z-M", key_style),
                Span::styled("] Play+Add  ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled(",/", key_style),
                Span::styled("] Octave  ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("Arrows", key_style),
                Span::styled("] Move  ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("Esc", key_style),
                Span::styled("] Exit", desc_style),
            ])
        }
        EditMode::Select => {
            // Select mode: show selection and editing hints
            Line::from(vec![
                Span::styled(
                    "SELECT MODE  ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("[", bracket_style),
                Span::styled("Enter", key_style),
                Span::styled("] Toggle  ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("x", key_style),
                Span::styled("] Delete  ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("hjkl", key_style),
                Span::styled("] Move  ", desc_style),
                Span::styled("[", bracket_style),
                Span::styled("Esc", key_style),
                Span::styled("] Exit", desc_style),
            ])
        }
    }
}
