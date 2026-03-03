pub mod event;
pub mod theme;
pub mod ui;
pub mod views;
pub mod widgets;

use std::io;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use crate::app::App;
use crate::tui::event::EventHandler;

pub fn run(app: &mut App) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let events = EventHandler::new(16); // ~60fps

    // Set initial terminal height for half-page scroll
    if let Ok(size) = crossterm::terminal::size() {
        app.set_terminal_size(size.0, size.1);
    }

    let result = run_loop(&mut terminal, app, &events);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events: &EventHandler,
) -> Result<()> {
    loop {
        terminal.draw(|frame| {
            ui::render(frame, app);
        })?;

        match events.next()? {
            event::Event::Key(key) => {
                app.handle_key(key);
            }
            event::Event::Mouse(mouse) => {
                app.handle_mouse(mouse);
            }
            event::Event::Tick => {
                app.tick();
            }
            event::Event::Resize(w, h) => {
                app.set_terminal_size(w, h);
            }
        }

        // Write clipboard via OSC 52 if pending
        if let Some(encoded) = app.clipboard_osc52.take() {
            let _ = crossterm::execute!(
                terminal.backend_mut(),
                crossterm::style::Print(format!("\x1b]52;c;{encoded}\x07"))
            );
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
