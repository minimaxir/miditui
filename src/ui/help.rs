//! Help overlay rendering.
//!
//! Displays keyboard shortcuts and commands in a modal overlay.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::centered_rect;

/// Key binding entry for the help display.
struct KeyBinding {
    key: &'static str,
    description: &'static str,
}

const GENERAL_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "?",
        description: "Toggle this help",
    },
    KeyBinding {
        key: "q / Esc",
        description: "Quit",
    },
    KeyBinding {
        key: "Ctrl+C",
        description: "Force quit",
    },
    KeyBinding {
        key: "Tab",
        description: "Cycle focus between panels",
    },
    KeyBinding {
        key: "Space",
        description: "Play / Pause",
    },
    KeyBinding {
        key: "Shift+Space",
        description: "Restart playback from beginning",
    },
    KeyBinding {
        key: ".",
        description: "Stop (reset to start)",
    },
];

const MODE_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "i",
        description: "Enter INSERT mode",
    },
    KeyBinding {
        key: "v",
        description: "Enter SELECT mode",
    },
    KeyBinding {
        key: "Esc",
        description: "Return to NORMAL mode",
    },
];

const NAVIGATION_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "h / Left",
        description: "Move cursor left",
    },
    KeyBinding {
        key: "l / Right",
        description: "Move cursor right",
    },
    KeyBinding {
        key: "k / Up",
        description: "Move cursor up (higher pitch)",
    },
    KeyBinding {
        key: "j / Down",
        description: "Move cursor down (lower pitch)",
    },
    KeyBinding {
        key: "H",
        description: "Jump left by measure",
    },
    KeyBinding {
        key: "L",
        description: "Jump right by measure",
    },
    KeyBinding {
        key: "0",
        description: "Go to start",
    },
    KeyBinding {
        key: "$",
        description: "Go to end",
    },
];

const EDIT_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "Enter / n",
        description: "Place note at cursor",
    },
    KeyBinding {
        key: "Delete",
        description: "Delete note at cursor",
    },
    KeyBinding {
        key: "W/A/S/D",
        description: "Move selected notes (up/left/down/right)",
    },
    KeyBinding {
        key: "Shift+A/D",
        description: "Shrink/expand note duration",
    },
];

const TRACK_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "a",
        description: "Add new track",
    },
    KeyBinding {
        key: "x / d",
        description: "Delete selected track",
    },
    KeyBinding {
        key: "r",
        description: "Rename selected track",
    },
    KeyBinding {
        key: "m",
        description: "Toggle mute on selected track",
    },
    KeyBinding {
        key: "s",
        description: "Toggle solo on selected track",
    },
    KeyBinding {
        key: "J / K",
        description: "Select next/previous track",
    },
    KeyBinding {
        key: "< / >",
        description: "Change instrument (GM)",
    },
    KeyBinding {
        key: "; / '",
        description: "Decrease/increase volume",
    },
    KeyBinding {
        key: "( / )",
        description: "Pan left/right",
    },
];

const KEYBOARD_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "Z-M / Q-I",
        description: "Play notes (piano layout)",
    },
    KeyBinding {
        key: ",",
        description: "Octave down",
    },
    KeyBinding {
        key: "/",
        description: "Octave up",
    },
];

const VIEW_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "g",
        description: "Cycle views (Combined/Piano/Timeline)",
    },
    KeyBinding {
        key: "t",
        description: "Toggle track list view (compact/expanded)",
    },
    KeyBinding {
        key: "W",
        description: "Toggle active track highlighting",
    },
    KeyBinding {
        key: "= / -",
        description: "Zoom in/out",
    },
    KeyBinding {
        key: "[ / ]",
        description: "Decrease/increase tempo (BPM)",
    },
    KeyBinding {
        key: "{ / }",
        description: "Decrease/increase time sig numerator",
    },
    KeyBinding {
        key: "|",
        description: "Cycle time sig denominator (2/4/8/16)",
    },
];

const FILE_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "Ctrl+n",
        description: "New project (with confirmation)",
    },
    KeyBinding {
        key: "Ctrl+s",
        description: "Save project",
    },
    KeyBinding {
        key: "Ctrl+o",
        description: "Open project",
    },
    KeyBinding {
        key: "Ctrl+l",
        description: "Load SoundFont (.sf2)",
    },
    KeyBinding {
        key: "e / Ctrl+e",
        description: "Export to WAV",
    },
    KeyBinding {
        key: "Ctrl+m",
        description: "Export to MIDI (.mid)",
    },
];

const MOUSE_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "Click",
        description: "Focus panel / select item",
    },
    KeyBinding {
        key: "Double-click",
        description: "Toggle note / rename track",
    },
    KeyBinding {
        key: "Right-click",
        description: "Toggle Insert mode",
    },
    KeyBinding {
        key: "Drag",
        description: "Pan/scroll the view",
    },
    KeyBinding {
        key: "Shift+Click",
        description: "Multi-select notes",
    },
    KeyBinding {
        key: "Scroll",
        description: "Navigate pitch (vert) or time (horiz)",
    },
    KeyBinding {
        key: "Ctrl+Scroll",
        description: "Zoom in/out",
    },
    KeyBinding {
        key: "Piano keys",
        description: "Click to play notes",
    },
];

/// Renders the help overlay.
///
/// # Arguments
///
/// * `frame` - The frame to render to
/// * `scroll` - Vertical scroll offset
pub fn render_help(frame: &mut Frame, scroll: u16) {
    let area = centered_rect(70, 80, frame.area());

    // Clear the area behind the popup
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help - Keyboard Shortcuts ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split inner area into content and fixed footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Scrollable content
            Constraint::Length(1), // Fixed footer
        ])
        .split(inner);

    // Build help content (without footer)
    let mut lines: Vec<Line<'static>> = Vec::new();

    let section_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let key_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::White);

    // Helper to add a section
    fn add_section(
        lines: &mut Vec<Line<'static>>,
        title: &'static str,
        bindings: &[KeyBinding],
        section_style: Style,
        key_style: Style,
        desc_style: Style,
    ) {
        lines.push(Line::from(Span::styled(title, section_style)));
        for binding in bindings {
            lines.push(Line::from(vec![
                Span::styled(format!("{:15}", binding.key), key_style),
                Span::styled(binding.description, desc_style),
            ]));
        }
        lines.push(Line::from(""));
    }

    add_section(
        &mut lines,
        "General",
        GENERAL_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );
    add_section(
        &mut lines,
        "Modes",
        MODE_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );
    add_section(
        &mut lines,
        "Navigation",
        NAVIGATION_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );
    add_section(
        &mut lines,
        "Editing",
        EDIT_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );
    add_section(
        &mut lines,
        "Tracks",
        TRACK_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );
    add_section(
        &mut lines,
        "Keyboard",
        KEYBOARD_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );
    add_section(
        &mut lines,
        "View",
        VIEW_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );
    add_section(
        &mut lines,
        "File & Export",
        FILE_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );
    add_section(
        &mut lines,
        "Mouse Controls",
        MOUSE_BINDINGS,
        section_style,
        key_style,
        desc_style,
    );

    // Render scrollable content
    let help_text = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(help_text, chunks[0]);

    // Render fixed footer (always visible at bottom)
    let footer = Paragraph::new(Line::from(Span::styled(
        "Scroll: Up/Down/j/k/Mouse  |  Close: ?/Esc/Click",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));
    frame.render_widget(footer, chunks[1]);
}
