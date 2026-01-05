//! miditui - A terminal-based MIDI sequencer and player.
//!
//! This application provides a DAW-like interface for composing and playing
//! MIDI music in the terminal, using SoundFonts for synthesis.
//!
//! # Features
//!
//! - Multi-track MIDI composition with unlimited tracks
//! - Real-time playback using rustysynth and rodio
//! - Piano roll editor for precise note placement
//! - Interactive keyboard for live note input
//! - WAV export functionality
//! - Project save/load (JSON format)
//! - Autosave with automatic recovery on startup
//!
//! # Usage
//!
//! ```bash
//! cargo run           # Start with autosave recovery (if available)
//! cargo run -- --new  # Start with a fresh project
//! ```
//!
//! Press `?` for help with keyboard shortcuts.

mod app;
mod audio;
mod history;
mod midi;
mod ui;

use app::{App, EditMode, FocusedPanel};
use audio::export_to_wav;
use midi::TICKS_PER_BEAT;

use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Command-line options for the application.
struct CliOptions {
    /// Start with a new project instead of loading autosave.
    new_project: bool,
    /// Path to a custom SoundFont file.
    soundfont: Option<PathBuf>,
}

impl CliOptions {
    /// Parses command-line arguments.
    ///
    /// Supports:
    /// - `--new` or `-n`: Start with a fresh project (skip autosave recovery)
    /// - `--soundfont <path>` or `-sf <path>`: Specify a custom SoundFont file
    /// - `--help` or `-h`: Print help and exit
    fn parse() -> Result<Self> {
        let args: Vec<String> = std::env::args().collect();
        let mut new_project = false;
        let mut soundfont: Option<PathBuf> = None;
        let mut i = 1;

        while i < args.len() {
            match args[i].as_str() {
                "--new" | "-n" => new_project = true,
                "--soundfont" | "-sf" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("Error: --soundfont requires a path argument");
                        std::process::exit(1);
                    }
                    soundfont = Some(PathBuf::from(&args[i]));
                }
                "--help" | "-h" => {
                    eprintln!("miditui - Terminal-based MIDI sequencer");
                    eprintln!();
                    eprintln!(
                        "Usage: {} [OPTIONS]",
                        args.first().unwrap_or(&"miditui".to_string())
                    );
                    eprintln!();
                    eprintln!("Options:");
                    eprintln!("  -n, --new              Start with a new project (skip autosave recovery)");
                    eprintln!("  -sf, --soundfont PATH  Load a specific SoundFont file (.sf2)");
                    eprintln!("  -h, --help             Print this help message");
                    eprintln!();
                    eprintln!("If no soundfont is specified, you will be prompted to select one.");
                    std::process::exit(0);
                }
                other => {
                    // Check if it might be a SoundFont file (positional argument)
                    if other.ends_with(".sf2") {
                        soundfont = Some(PathBuf::from(other));
                    } else {
                        eprintln!("Unknown option: {}", other);
                        eprintln!("Use --help for usage information");
                        std::process::exit(1);
                    }
                }
            }
            i += 1;
        }

        Ok(Self {
            new_project,
            soundfont,
        })
    }
}

const AUTOSAVE_PATH: &str = ".autosave.oxm";

/// Attempts to read the SoundFont path from the autosave file.
/// Returns Some(path) if a valid SoundFont path was found, None otherwise.
fn get_soundfont_from_autosave() -> Option<PathBuf> {
    use crate::midi::Project;

    let autosave_path = PathBuf::from(AUTOSAVE_PATH);
    if !autosave_path.exists() {
        return None;
    }

    // Try to load the autosave and extract SoundFont path
    match Project::load_from_binary(&autosave_path) {
        Ok(project) => {
            if let Some(sf_path_str) = project.get_soundfont_path() {
                let sf_path = PathBuf::from(sf_path_str);
                if sf_path.exists() {
                    return Some(sf_path);
                }
            }
            None
        }
        Err(_) => None,
    }
}

/// Main entry point.
fn main() -> Result<()> {
    // Parse CLI options first (before any terminal setup)
    let cli = CliOptions::parse()?;

    // Initialize logging (optional, for debugging)
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Determine which SoundFont to use:
    // 1. CLI-specified SoundFont takes priority
    // 2. Check autosave for a saved SoundFont path (unless --new flag)
    // 3. Prompt user to select a SoundFont
    let soundfont_path = if let Some(ref sf_path) = cli.soundfont {
        if sf_path.exists() {
            Some(sf_path.clone())
        } else {
            eprintln!(
                "Warning: Specified soundfont not found: {}",
                sf_path.display()
            );
            eprintln!("Will prompt for soundfont selection.");
            None
        }
    } else if !cli.new_project {
        get_soundfont_from_autosave()
    } else {
        None
    };

    let mut terminal = setup_terminal().context("Failed to setup terminal")?;

    // If no SoundFont found, show selection dialog before creating App
    let soundfont_path = match soundfont_path {
        Some(path) => path,
        None => {
            // Show SoundFont selection dialog
            match run_soundfont_selector(&mut terminal)? {
                Some(path) => path,
                None => {
                    // User cancelled - exit cleanly
                    restore_terminal(&mut terminal)?;
                    std::process::exit(0);
                }
            }
        }
    };

    // Create application with the selected SoundFont
    let mut app = App::new(soundfont_path).context("Failed to initialize application")?;

    // Attempt to load autosave unless --new flag was used
    if !cli.new_project {
        app.try_load_autosave();

        // If autosave loaded a project with a different SoundFont path, try to load it
        if let Some(saved_sf_path) = app.project().get_soundfont_path() {
            let saved_path = PathBuf::from(saved_sf_path);
            if saved_path.exists() && saved_path != app.soundfont_path {
                // Load the project's SoundFont
                if app.load_soundfont(saved_path) {
                    app.set_status("Loaded soundfont from saved project");
                }
            }
        }
    }

    // Run main loop
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    restore_terminal(&mut terminal).context("Failed to restore terminal")?;

    // Handle any errors from the main loop
    result
}

/// State for the standalone SoundFont selector (before App is created).
struct SoundfontSelectorState {
    current_dir: PathBuf,
    entries: Vec<PathBuf>,
    selected: usize,
    scroll: usize,
}

impl SoundfontSelectorState {
    fn new() -> Self {
        let mut state = Self {
            current_dir: std::env::current_dir().unwrap_or_default(),
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
        };
        state.refresh_entries();
        state
    }

    fn refresh_entries(&mut self) {
        self.entries.clear();

        // Add parent directory entry if not at root
        if self.current_dir.parent().is_some() {
            self.entries.push(PathBuf::from(".."));
        }

        // Read directory entries
        if let Ok(entries) = std::fs::read_dir(&self.current_dir) {
            let mut dirs: Vec<PathBuf> = Vec::new();
            let mut files: Vec<PathBuf> = Vec::new();

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if ext_lower == "sf2" {
                        files.push(path);
                    }
                }
            }

            dirs.sort();
            files.sort();

            self.entries.extend(dirs);
            self.entries.extend(files);
        }

        if self.selected >= self.entries.len() {
            self.selected = 0;
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll {
                self.scroll = self.selected;
            }
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
            if self.selected >= self.scroll + 20 {
                self.scroll = self.selected.saturating_sub(19);
            }
        }
    }

    /// Returns Some(path) if a SoundFont was selected, None to continue browsing
    fn select(&mut self) -> Option<PathBuf> {
        if self.entries.is_empty() {
            return None;
        }

        let selected_path = &self.entries[self.selected];

        if selected_path == &PathBuf::from("..") {
            if let Some(parent) = self.current_dir.parent() {
                self.current_dir = parent.to_path_buf();
                self.selected = 0;
                self.scroll = 0;
                self.refresh_entries();
            }
            None
        } else if selected_path.is_dir() {
            self.current_dir = selected_path.clone();
            self.selected = 0;
            self.scroll = 0;
            self.refresh_entries();
            None
        } else {
            Some(selected_path.clone())
        }
    }
}

/// Runs a standalone SoundFont selector before the App is created.
/// Returns the selected SoundFont path, or None if the user wants to quit.
fn run_soundfont_selector(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<Option<PathBuf>> {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

    let mut state = SoundfontSelectorState::new();

    loop {
        terminal.draw(|frame| {
            let size = frame.area();

            // Center the dialog
            let popup_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(10),
                    Constraint::Percentage(80),
                    Constraint::Percentage(10),
                ])
                .split(size);

            let popup_area = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(15),
                    Constraint::Percentage(70),
                    Constraint::Percentage(15),
                ])
                .split(popup_layout[1])[1];

            frame.render_widget(Clear, popup_area);

            let block = Block::default()
                .title(" Select a SoundFont to Continue ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));

            let inner = block.inner(popup_area);
            frame.render_widget(block, popup_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Header
                    Constraint::Length(1), // Current path
                    Constraint::Length(1), // Separator
                    Constraint::Min(5),    // File list
                    Constraint::Length(1), // Instructions
                ])
                .split(inner);

            // Header
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

            // Current directory
            let path_str = state.current_dir.display().to_string();
            let max_width = chunks[1].width.saturating_sub(2) as usize;
            let display_path = if path_str.len() > max_width {
                format!(
                    "...{}",
                    &path_str[path_str.len().saturating_sub(max_width - 3)..]
                )
            } else {
                path_str
            };
            frame.render_widget(
                Paragraph::new(Span::styled(display_path, Style::default().fg(Color::Cyan))),
                chunks[1],
            );

            // File list
            let visible_height = chunks[3].height as usize;
            let start_idx = state.scroll;
            let end_idx = (start_idx + visible_height).min(state.entries.len());

            let items: Vec<ListItem> = if state.entries.is_empty() {
                vec![ListItem::new(Line::from(Span::styled(
                    "No SoundFont files found in this directory",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )))]
            } else {
                state.entries[start_idx..end_idx]
                    .iter()
                    .enumerate()
                    .map(|(i, path)| {
                        let idx = start_idx + i;
                        let is_selected = idx == state.selected;

                        let (icon, name, style) = if path == &PathBuf::from("..") {
                            (
                                "[..]",
                                "Parent Directory".to_string(),
                                Style::default().fg(Color::Blue),
                            )
                        } else if path.is_dir() {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("?")
                                .to_string();
                            ("[D]", name, Style::default().fg(Color::Blue))
                        } else {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("?")
                                .to_string();
                            ("[SF2]", name, Style::default().fg(Color::Green))
                        };

                        let display_style = if is_selected {
                            style.add_modifier(Modifier::REVERSED)
                        } else {
                            style
                        };

                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("{} ", icon),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled(name, display_style),
                        ]))
                    })
                    .collect()
            };

            frame.render_widget(List::new(items), chunks[3]);

            // Instructions
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("[Up/Down]", Style::default().fg(Color::Yellow)),
                    Span::styled(" Navigate  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[Enter]", Style::default().fg(Color::Yellow)),
                    Span::styled(" Select  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[q/Esc]", Style::default().fg(Color::Yellow)),
                    Span::styled(" Quit", Style::default().fg(Color::DarkGray)),
                ])),
                chunks[4],
            );
        })?;

        // Handle input
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => state.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => state.move_down(),
                        KeyCode::Enter => {
                            if let Some(path) = state.select() {
                                return Ok(Some(path));
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('q') => {
                            return Ok(None);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Sets up the terminal for TUI rendering.
fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("Failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).context("Failed to create terminal")?;
    Ok(terminal)
}

/// Restores the terminal to its original state.
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .context("Failed to leave alternate screen")?;
    terminal.show_cursor().context("Failed to show cursor")?;
    Ok(())
}

/// Whether the help overlay is visible.
static mut SHOW_HELP: bool = false;

/// Time threshold for detecting double-clicks (in milliseconds).
const DOUBLE_CLICK_THRESHOLD_MS: u128 = 400;

/// Tracks the last click position and time for double-click detection.
struct ClickTracker {
    last_pos: Option<(u16, u16)>,
    last_time: Option<Instant>,
}

impl ClickTracker {
    fn new() -> Self {
        Self {
            last_pos: None,
            last_time: None,
        }
    }

    /// Records a click and returns true if it's a double-click.
    fn record_click(&mut self, x: u16, y: u16) -> bool {
        let now = Instant::now();
        let is_double = if let (Some((lx, ly)), Some(lt)) = (self.last_pos, self.last_time) {
            // Check if same position and within time threshold
            lx == x && ly == y && now.duration_since(lt).as_millis() < DOUBLE_CLICK_THRESHOLD_MS
        } else {
            false
        };

        if is_double {
            // Reset after double-click
            self.last_pos = None;
            self.last_time = None;
        } else {
            self.last_pos = Some((x, y));
            self.last_time = Some(now);
        }

        is_double
    }
}

/// Main application loop.
fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    let mut click_tracker = ClickTracker::new();
    // Track the last mouse position for release events
    let mut last_mouse_pos: Option<(u16, u16)> = None;

    loop {
        // Update sequencer for playback
        app.update_sequencer();
        app.clear_expired_status();

        // Update Insert Mode recording state (checks for timeout)
        app.update_insert_recording();

        app.check_autosave();

        // Draw UI
        terminal.draw(|frame| {
            ui::render(frame, app);

            // Draw help overlay if visible
            // SAFETY: SHOW_HELP is only accessed from the main thread
            if unsafe { SHOW_HELP } {
                ui::render_help(frame, app.help_scroll);
            }

            // Draw save dialog if open
            ui::render_save_dialog(frame, app);

            // Draw file browser if open
            ui::render_file_browser(frame, app);

            // Draw new project confirmation dialog if open
            ui::render_new_project_dialog(frame, app);

            // Draw SoundFont dialog if open (highest priority since it can block)
            ui::render_soundfont_dialog(frame, app);
        })?;

        // Handle events with a short timeout to allow sequencer updates
        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => {
                    // Only handle key press events (not release)
                    if key.kind == KeyEventKind::Press {
                        // SAFETY: SHOW_HELP is only accessed from the main thread
                        if unsafe { SHOW_HELP } {
                            // Help overlay is visible - handle close and scroll
                            match key.code {
                                KeyCode::Char('?') | KeyCode::Esc => {
                                    unsafe { SHOW_HELP = false };
                                    app.help_scroll = 0; // Reset scroll on close
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    app.help_scroll = app.help_scroll.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    app.help_scroll = app.help_scroll.saturating_add(1);
                                }
                                KeyCode::PageUp => {
                                    app.help_scroll = app.help_scroll.saturating_sub(10);
                                }
                                KeyCode::PageDown => {
                                    app.help_scroll = app.help_scroll.saturating_add(10);
                                }
                                KeyCode::Home => {
                                    app.help_scroll = 0;
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // Handle SoundFont dialog input (highest priority)
                        if app.soundfont_dialog.open {
                            match key.code {
                                KeyCode::Enter => {
                                    if app.soundfont_dialog_select() {
                                        app.set_status("SoundFont loaded");
                                    }
                                }
                                KeyCode::Esc => {
                                    // Only close if not first-load modal
                                    app.soundfont_dialog_cancel();
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    app.soundfont_dialog_up();
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    app.soundfont_dialog_down();
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // Handle new project dialog input
                        if app.new_project_dialog.open {
                            match key.code {
                                KeyCode::Enter => {
                                    app.new_project_dialog_confirm();
                                }
                                KeyCode::Esc => {
                                    app.new_project_dialog_cancel();
                                }
                                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('y') => {
                                    app.new_project_dialog_left(); // Select "Yes"
                                }
                                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('n') => {
                                    app.new_project_dialog_right(); // Select "No"
                                }
                                KeyCode::Tab => {
                                    // Toggle between options
                                    if app.new_project_dialog.selected == 0 {
                                        app.new_project_dialog_right();
                                    } else {
                                        app.new_project_dialog_left();
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // Handle save dialog input
                        if app.save_dialog.open {
                            match key.code {
                                KeyCode::Enter => {
                                    if app.save_dialog_confirm() {
                                        app.set_status("Project saved");
                                    }
                                }
                                KeyCode::Esc => {
                                    app.save_dialog_cancel();
                                }
                                KeyCode::Tab => {
                                    app.save_dialog_toggle_format();
                                }
                                KeyCode::Backspace => {
                                    app.save_dialog_backspace();
                                }
                                KeyCode::Char(c) => {
                                    // Only accept valid filename characters
                                    if c.is_alphanumeric() || c == '_' || c == '-' {
                                        app.save_dialog_input(c);
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // Handle file browser input
                        if app.file_browser.open {
                            match key.code {
                                KeyCode::Enter => {
                                    if app.file_browser_select() {
                                        app.set_status("Project loaded");
                                    }
                                }
                                KeyCode::Esc => {
                                    app.file_browser_cancel();
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    app.file_browser_up();
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    app.file_browser_down();
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // Handle rename mode input
                        if app.renaming_track {
                            match key.code {
                                KeyCode::Enter => {
                                    app.confirm_rename_track();
                                }
                                KeyCode::Esc => {
                                    app.cancel_rename_track();
                                }
                                KeyCode::Backspace => {
                                    app.rename_track_backspace();
                                }
                                KeyCode::Char(c) => {
                                    // Only accept printable characters
                                    if !c.is_control() {
                                        app.rename_track_input(c);
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // Handle key based on current mode and focus
                        if handle_key(app, key.code, key.modifiers)? {
                            break;
                        }
                    } else if key.kind == KeyEventKind::Release {
                        // Handle note key releases for keyboard playing
                        if let KeyCode::Char(c) = key.code {
                            app.handle_note_key_release(c);
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    // Handle mouse events when help is visible
                    if unsafe { SHOW_HELP } {
                        match mouse.kind {
                            // Click anywhere to close help
                            MouseEventKind::Down(MouseButton::Left) => {
                                unsafe { SHOW_HELP = false };
                                app.help_scroll = 0;
                            }
                            // Mouse scroll to navigate help content
                            MouseEventKind::ScrollUp => {
                                app.help_scroll = app.help_scroll.saturating_sub(3);
                            }
                            MouseEventKind::ScrollDown => {
                                app.help_scroll = app.help_scroll.saturating_add(3);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    handle_mouse(app, mouse, &mut click_tracker, &mut last_mouse_pos);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Handles mouse events.
fn handle_mouse(
    app: &mut App,
    mouse: MouseEvent,
    click_tracker: &mut ClickTracker,
    last_mouse_pos: &mut Option<(u16, u16)>,
) {
    let x = mouse.column;
    let y = mouse.row;
    let shift_held = mouse.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl_held = mouse.modifiers.contains(KeyModifiers::CONTROL)
        || mouse.modifiers.contains(KeyModifiers::SUPER);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            *last_mouse_pos = Some((x, y));

            // Check for double-click
            if click_tracker.record_click(x, y) {
                app.handle_double_click(x, y);
            } else {
                app.handle_drag_start(x, y, shift_held);
                app.handle_mouse_click(x, y, shift_held);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.handle_drag_end();

            if let Some((lx, ly)) = *last_mouse_pos {
                app.handle_piano_key_release(lx, ly);
            }
            *last_mouse_pos = None;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.handle_drag_move(x, y);
        }
        MouseEventKind::Down(MouseButton::Right) => {
            app.edit_mode = match app.edit_mode {
                EditMode::Normal => {
                    app.set_status("Insert mode - click to place notes");
                    EditMode::Insert
                }
                EditMode::Insert => {
                    app.set_status("Normal mode");
                    EditMode::Normal
                }
                EditMode::Select => {
                    app.set_status("Normal mode");
                    EditMode::Normal
                }
            };
        }
        MouseEventKind::Down(MouseButton::Middle) => {
            // Middle-click for panning (start scroll drag)
            app.handle_drag_start(x, y, false);
        }
        MouseEventKind::Up(MouseButton::Middle) => {
            app.handle_drag_end();
        }
        MouseEventKind::Drag(MouseButton::Middle) => {
            app.handle_drag_move(x, y);
        }
        MouseEventKind::ScrollUp => {
            app.handle_mouse_scroll(x, y, 0, 1, ctrl_held);
        }
        MouseEventKind::ScrollDown => {
            app.handle_mouse_scroll(x, y, 0, -1, ctrl_held);
        }
        MouseEventKind::ScrollLeft => {
            app.handle_mouse_scroll(x, y, -1, 0, ctrl_held);
        }
        MouseEventKind::ScrollRight => {
            app.handle_mouse_scroll(x, y, 1, 0, ctrl_held);
        }
        _ => {}
    }
}

/// Handles a key press event.
///
/// # Returns
///
/// `true` if the application should quit
fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
    // Global key bindings (work in any mode/panel)
    match code {
        // Quit
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Ok(true);
        }
        KeyCode::Char('q') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Ok(true);
        }
        KeyCode::Char('q') if app.edit_mode == EditMode::Normal => {
            return Ok(true);
        }

        // Undo/Redo (Ctrl+Z / Ctrl+Y)
        KeyCode::Char('z') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.undo();
            return Ok(false);
        }
        KeyCode::Char('y') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.redo();
            return Ok(false);
        }

        // Help toggle
        KeyCode::Char('?') => {
            // SAFETY: SHOW_HELP is only accessed from the main thread
            unsafe { SHOW_HELP = !SHOW_HELP };
            return Ok(false);
        }

        // Escape - return to normal mode or quit
        KeyCode::Esc => {
            if app.edit_mode != EditMode::Normal {
                // Stop Insert Mode recording if active
                app.stop_insert_recording();
                app.edit_mode = EditMode::Normal;
                app.release_all_notes();
                app.set_status("Normal mode");
            }
            return Ok(false);
        }

        // Tab - cycle focus
        KeyCode::Tab => {
            app.focused_panel = match app.focused_panel {
                FocusedPanel::TrackList => FocusedPanel::Timeline,
                FocusedPanel::Timeline => FocusedPanel::PianoRoll,
                FocusedPanel::PianoRoll => FocusedPanel::Keyboard,
                FocusedPanel::Keyboard => FocusedPanel::TrackList,
            };
            return Ok(false);
        }

        // Playback controls
        // Shift+Space: restart playback from beginning
        KeyCode::Char(' ') if modifiers.contains(KeyModifiers::SHIFT) => {
            app.restart_playback();
            return Ok(false);
        }
        KeyCode::Char(' ') => {
            app.toggle_playback();
            return Ok(false);
        }
        KeyCode::Char('.') if app.edit_mode == EditMode::Normal => {
            app.stop_playback();
            return Ok(false);
        }

        // Export WAV (Ctrl+E)
        KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
            export_project(app)?;
            return Ok(false);
        }

        // Export MIDI (Ctrl+M)
        KeyCode::Char('m') if modifiers.contains(KeyModifiers::CONTROL) => {
            export_midi(app)?;
            return Ok(false);
        }

        // Save project (Ctrl+S) - opens save dialog
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.open_save_dialog();
            return Ok(false);
        }

        // Load project (Ctrl+O) - opens file browser
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.open_file_browser();
            return Ok(false);
        }

        // New project (Ctrl+N) - opens confirmation dialog
        KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.open_new_project_dialog();
            return Ok(false);
        }

        // Load SoundFont (Ctrl+L) - opens SoundFont browser
        KeyCode::Char('l') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.open_soundfont_dialog(false);
            return Ok(false);
        }

        _ => {}
    }

    // Mode-specific key bindings
    match app.edit_mode {
        EditMode::Normal => handle_normal_mode(app, code, modifiers),
        EditMode::Insert => handle_insert_mode(app, code, modifiers),
        EditMode::Select => handle_select_mode(app, code, modifiers),
    }
}

/// Handles keys in normal mode.
fn handle_normal_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
    match code {
        // Mode changes
        KeyCode::Char('i') => {
            app.edit_mode = EditMode::Insert;
            app.set_status("Insert mode - keys play & insert notes");
        }
        KeyCode::Char('v') => {
            app.edit_mode = EditMode::Select;
            app.set_status("Select mode");
        }

        // Navigation
        KeyCode::Char('h') | KeyCode::Left => {
            app.move_cursor_horizontal(-(app.zoom as i32));
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.move_cursor_horizontal(app.zoom as i32);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_cursor_vertical(1);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_cursor_vertical(-1);
        }
        KeyCode::Char('H') => {
            // Jump left by measure
            app.move_cursor_horizontal(-(TICKS_PER_BEAT as i32 * 4));
        }
        KeyCode::Char('L') => {
            // Jump right by measure
            app.move_cursor_horizontal(TICKS_PER_BEAT as i32 * 4);
        }
        KeyCode::Char('0') => {
            app.cursor_tick = 0;
            app.scroll_x = 0;
        }
        KeyCode::Char('$') => {
            app.cursor_tick = app.project().duration_ticks();
        }

        // Track selection
        KeyCode::Char('J') => {
            if app.selected_track_index < app.project().track_count().saturating_sub(1) {
                app.selected_track_index += 1;
            }
        }
        KeyCode::Char('K') => {
            if app.selected_track_index > 0 {
                app.selected_track_index -= 1;
            }
        }

        // Track management
        KeyCode::Char('a') => {
            app.add_track();
        }
        KeyCode::Char('d') | KeyCode::Char('x') => {
            app.delete_selected_track();
        }
        KeyCode::Char('r') => {
            // Rename selected track
            app.start_rename_track();
        }
        KeyCode::Char('g') => {
            // Toggle between piano roll and project timeline view
            app.toggle_view_mode();
        }
        KeyCode::Char('t') => {
            // Toggle expanded/compact track list view
            app.toggle_expanded_tracks();
        }
        KeyCode::Char('m') => {
            // Toggle mute and get info for status message
            if app.selected_track().is_some() {
                app.save_state("Toggle mute");
            }
            let status_msg = if let Some(track) = app.selected_track_mut() {
                track.muted = !track.muted;
                let status = if track.muted { "Muted" } else { "Unmuted" };
                Some(format!("{} {}", status, track.name.clone()))
            } else {
                None
            };
            // Silence all notes - the sequencer will restart appropriate ones
            app.audio.all_notes_off(true);
            if let Some(msg) = status_msg {
                app.set_status(msg);
                app.mark_modified();
            }
        }
        KeyCode::Char('s') => {
            // Toggle solo and get info for status message
            if app.selected_track().is_some() {
                app.save_state("Toggle solo");
            }
            let status_msg = if let Some(track) = app.selected_track_mut() {
                track.solo = !track.solo;
                let status = if track.solo { "Solo on" } else { "Solo off" };
                Some(format!("{} {}", status, track.name.clone()))
            } else {
                None
            };
            // Silence all notes - the sequencer will restart appropriate ones
            app.audio.all_notes_off(true);
            if let Some(msg) = status_msg {
                app.set_status(msg);
                app.mark_modified();
            }
        }

        // Note editing
        KeyCode::Enter | KeyCode::Char('n') => {
            app.place_note();
        }
        KeyCode::Delete => {
            app.delete_note_at_cursor();
        }

        // Zoom
        KeyCode::Char('=') | KeyCode::Char('+') => {
            app.zoom(0.5);
            app.set_status(format!("Zoom: {} ticks/col", app.zoom));
        }
        KeyCode::Char('-') if !modifiers.contains(KeyModifiers::CONTROL) => {
            app.zoom(2.0);
            app.set_status(format!("Zoom: {} ticks/col", app.zoom));
        }

        // Octave
        KeyCode::Char(',') => {
            app.change_octave(-1);
        }
        KeyCode::Char('/') => {
            app.change_octave(1);
        }

        // Tempo adjustment
        KeyCode::Char('[') => {
            app.save_state("Adjust tempo");
            app.project_mut().tempo = app.project().tempo.saturating_sub(5).max(20);
            app.audio.set_tempo(app.project().tempo);
            app.set_status(format!("Tempo: {} BPM", app.project().tempo));
            app.mark_modified();
        }
        KeyCode::Char(']') => {
            app.save_state("Adjust tempo");
            app.project_mut().tempo = (app.project().tempo + 5).min(300);
            app.audio.set_tempo(app.project().tempo);
            app.set_status(format!("Tempo: {} BPM", app.project().tempo));
            app.mark_modified();
        }

        // Time signature adjustment
        KeyCode::Char('{') => {
            app.adjust_time_sig_numerator(-1);
        }
        KeyCode::Char('}') => {
            app.adjust_time_sig_numerator(1);
        }
        KeyCode::Char('|') => {
            app.cycle_time_sig_denominator();
        }

        // Instrument cycling (< and > keys, which are Shift+, and Shift+.)
        KeyCode::Char('<') => {
            app.cycle_instrument(-1);
        }
        KeyCode::Char('>') => {
            app.cycle_instrument(1);
        }

        // Volume control (; and ' keys)
        KeyCode::Char(';') => {
            app.adjust_track_volume(-5);
        }
        KeyCode::Char('\'') => {
            app.adjust_track_volume(5);
        }

        // Pan control (( and ) keys, Shift+9 and Shift+0)
        KeyCode::Char('(') => {
            app.adjust_track_pan(-8);
        }
        KeyCode::Char(')') => {
            app.adjust_track_pan(8);
        }

        // Export to WAV directly
        KeyCode::Char('e') => {
            export_project(app)?;
        }

        // Cycle highlight mode for active notes during playback
        // Cycles: Piano Roll -> Both -> Off -> Timeline -> repeat
        KeyCode::Char('W') => {
            app.cycle_highlight_mode();
        }

        // Keyboard note playing (still works in normal mode)
        KeyCode::Char(c) => {
            if !app.handle_note_key(c) {
                // Key wasn't a note key
            }
        }

        _ => {}
    }

    Ok(false)
}

/// Handles keys in insert mode.
fn handle_insert_mode(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) -> Result<bool> {
    match code {
        // Navigation still works
        KeyCode::Left => {
            app.move_cursor_horizontal(-(app.zoom as i32));
        }
        KeyCode::Right => {
            app.move_cursor_horizontal(app.zoom as i32);
        }
        KeyCode::Up => {
            app.move_cursor_vertical(1);
        }
        KeyCode::Down => {
            app.move_cursor_vertical(-1);
        }

        // Octave
        KeyCode::Char(',') => {
            app.change_octave(-1);
        }
        KeyCode::Char('/') => {
            app.change_octave(1);
        }

        // Instrument cycling (Shift+, and Shift+. produce '<' and '>')
        // cycle_instrument() silences all playing notes before switching
        KeyCode::Char('<') => {
            app.cycle_instrument(-1);
        }
        KeyCode::Char('>') => {
            app.cycle_instrument(1);
        }

        // In insert mode, keyboard keys insert and play notes
        KeyCode::Char(c) => {
            app.handle_note_key(c);
        }

        _ => {}
    }

    Ok(false)
}

/// Handles keys in select mode.
fn handle_select_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
    let shift_held = modifiers.contains(KeyModifiers::SHIFT);

    match code {
        // Shift+A: shrink note duration
        KeyCode::Char('A') if shift_held => {
            if !app.selected_notes.is_empty() {
                app.adjust_selected_notes_duration(-(app.zoom as i32));
                app.set_status("Reduced note duration");
            }
        }
        // Shift+D: expand note duration
        KeyCode::Char('D') if shift_held => {
            if !app.selected_notes.is_empty() {
                app.adjust_selected_notes_duration(app.zoom as i32);
                app.set_status("Expanded note duration");
            }
        }

        // WASD: move selected notes (if notes selected) or navigate cursor
        KeyCode::Char('w') => {
            if !app.selected_notes.is_empty() {
                app.transpose_selected_notes(1);
                app.set_status("Moved notes up");
            } else {
                app.move_cursor_vertical(1);
            }
        }
        KeyCode::Char('s') => {
            if !app.selected_notes.is_empty() {
                app.transpose_selected_notes(-1);
                app.set_status("Moved notes down");
            } else {
                app.move_cursor_vertical(-1);
            }
        }
        KeyCode::Char('a') => {
            if !app.selected_notes.is_empty() {
                app.move_selected_notes_horizontal(-(app.zoom as i32));
                app.set_status("Moved notes left");
            } else {
                app.move_cursor_horizontal(-(app.zoom as i32));
            }
        }
        KeyCode::Char('d') => {
            if !app.selected_notes.is_empty() {
                app.move_selected_notes_horizontal(app.zoom as i32);
                app.set_status("Moved notes right");
            } else {
                app.move_cursor_horizontal(app.zoom as i32);
            }
        }

        // Navigation with hjkl and arrow keys
        KeyCode::Char('h') | KeyCode::Left => {
            app.move_cursor_horizontal(-(app.zoom as i32));
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.move_cursor_horizontal(app.zoom as i32);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_cursor_vertical(1);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_cursor_vertical(-1);
        }

        // Select note under cursor
        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(track) = app.selected_track() {
                let note_id = track
                    .notes()
                    .iter()
                    .find(|n| n.pitch == app.cursor_pitch && n.is_active_at(app.cursor_tick))
                    .map(|n| n.id);

                if let Some(id) = note_id {
                    if app.selected_notes.contains(&id) {
                        app.selected_notes.remove(&id);
                        app.set_status("Deselected note");
                    } else {
                        app.selected_notes.insert(id);
                        app.set_status("Selected note");
                    }
                }
            }
        }

        // Delete selected notes
        KeyCode::Char('x') | KeyCode::Delete => {
            if !app.selected_notes.is_empty() {
                app.save_state("Delete selected notes");
                // Collect IDs first to avoid borrow conflicts
                let ids_to_delete: Vec<_> = app.selected_notes.drain().collect();
                let count = ids_to_delete.len();
                if let Some(track) = app.selected_track_mut() {
                    for id in ids_to_delete {
                        track.remove_note(id);
                    }
                }
                app.set_status(format!("Deleted {} notes", count));
                app.mark_modified();
            }
        }

        // Clear selection
        KeyCode::Char('c') => {
            app.selected_notes.clear();
            app.set_status("Selection cleared");
        }

        _ => {}
    }

    Ok(false)
}

/// Exports the current project to a WAV file.
fn export_project(app: &mut App) -> Result<()> {
    app.set_status("Exporting to output.wav...");
    app.exporting = true;

    // Create output directory if needed
    std::fs::create_dir_all("output")?;

    let output_path = PathBuf::from("output/output.wav");
    let soundfont_path = app.soundfont_path.clone();

    // Export with progress callback
    let result = export_to_wav(
        app.project(),
        &soundfont_path,
        &output_path,
        Some(|_progress: f32| {
            // Progress updates happen but we can't easily update the UI during export
            // For a more advanced implementation, this would use channels
        }),
    );

    app.exporting = false;

    match result {
        Ok(()) => {
            app.set_status(format!("Exported to {}", output_path.display()));
        }
        Err(e) => {
            app.set_status(format!("Export failed: {}", e));
            tracing::error!("Export failed: {:?}", e);
        }
    }

    Ok(())
}

/// Exports the current project to a MIDI file.
///
/// Creates a Standard MIDI File (Format 1) with all tracks.
/// Note: Some project data (mute/solo states) cannot be represented in MIDI.
fn export_midi(app: &mut App) -> Result<()> {
    app.set_status("Exporting to MIDI...");

    // Create output directory if needed
    std::fs::create_dir_all("output")?;

    // Generate filename from project name or path
    let filename = app
        .project_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| {
            app.project()
                .name
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
                .collect::<String>()
                .replace(' ', "_")
        });

    let filename = if filename.is_empty() {
        "project".to_string()
    } else {
        filename
    };

    let output_path = PathBuf::from(format!("output/{}.mid", filename));

    match app.project().export_to_midi(&output_path) {
        Ok(()) => {
            app.set_status(format!("Exported MIDI to {}", output_path.display()));
        }
        Err(e) => {
            app.set_status(format!("MIDI export failed: {}", e));
            tracing::error!("MIDI export failed: {:?}", e);
        }
    }

    Ok(())
}
