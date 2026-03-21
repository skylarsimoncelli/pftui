pub mod event;
pub mod theme;
pub mod ui;
pub mod views;
pub mod widgets;

use std::env;
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

const LOCAL_TICK_RATE_MS: u64 = 16;
const REMOTE_TICK_RATE_MS: u64 = 100;
const TICK_RATE_ENV: &str = "PFTUI_TICK_RATE_MS";

pub fn run(app: &mut App) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let events = EventHandler::new(detect_tick_rate_ms());

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

fn detect_tick_rate_ms() -> u64 {
    if let Ok(raw) = env::var(TICK_RATE_ENV) {
        if let Ok(parsed) = raw.parse::<u64>() {
            return parsed.max(1);
        }
    }

    if is_remote_terminal_session() {
        REMOTE_TICK_RATE_MS
    } else {
        LOCAL_TICK_RATE_MS
    }
}

fn is_remote_terminal_session() -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|key| env::var_os(key).is_some())
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

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::{
        detect_tick_rate_ms, is_remote_terminal_session, LOCAL_TICK_RATE_MS, REMOTE_TICK_RATE_MS,
        TICK_RATE_ENV,
    };

    fn with_env_vars<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();

        let saved: Vec<(String, Option<std::ffi::OsString>)> = vars
            .iter()
            .map(|(key, _)| ((*key).to_string(), std::env::var_os(key)))
            .collect();

        for (key, value) in vars {
            match value {
                Some(v) => unsafe { std::env::set_var(key, v) },
                None => unsafe { std::env::remove_var(key) },
            }
        }

        f();

        for (key, value) in saved {
            match value {
                Some(v) => unsafe { std::env::set_var(&key, v) },
                None => unsafe { std::env::remove_var(&key) },
            }
        }
    }

    #[test]
    fn detects_remote_session_from_ssh_env() {
        with_env_vars(
            &[
                ("SSH_CONNECTION", Some("1 2 3 4")),
                ("SSH_CLIENT", None),
                ("SSH_TTY", None),
                (TICK_RATE_ENV, None),
            ],
            || {
                assert!(is_remote_terminal_session());
                assert_eq!(detect_tick_rate_ms(), REMOTE_TICK_RATE_MS);
            },
        );
    }

    #[test]
    fn uses_local_rate_without_ssh_env() {
        with_env_vars(
            &[
                ("SSH_CONNECTION", None),
                ("SSH_CLIENT", None),
                ("SSH_TTY", None),
                (TICK_RATE_ENV, None),
            ],
            || {
                assert!(!is_remote_terminal_session());
                assert_eq!(detect_tick_rate_ms(), LOCAL_TICK_RATE_MS);
            },
        );
    }

    #[test]
    fn env_override_wins() {
        with_env_vars(
            &[
                ("SSH_CONNECTION", Some("1 2 3 4")),
                ("SSH_CLIENT", None),
                ("SSH_TTY", None),
                (TICK_RATE_ENV, Some("42")),
            ],
            || {
                assert_eq!(detect_tick_rate_ms(), 42);
            },
        );
    }
}
