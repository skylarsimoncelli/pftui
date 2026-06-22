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
    // Install a panic hook BEFORE entering raw mode. Without this, a panic
    // (most often on a background worker thread — price service, data refresh)
    // prints "thread panicked at …" straight to stderr ON TOP OF the ratatui
    // alt-screen buffer while raw mode is still on, leaving the terminal
    // corrupted (no nav bar, garbled status line — the bug operators hit).
    //
    // Policy:
    //   • Background worker threads (named "pftui-bg…"): NEVER touch the
    //     terminal — main is still rendering. Route the panic to a log file so
    //     the TUI keeps running and the failed refresh is just a silent miss.
    //   • The main/TUI thread: restore the terminal first so the panic prints
    //     cleanly to a normal screen instead of over the alt-screen buffer.
    install_panic_hook();

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

/// Install a process-wide panic hook tailored for the TUI lifetime.
///
/// Background worker threads (named with the `pftui-bg` prefix via
/// [`std::thread::Builder`]) route their panic to `~/.local/share/pftui/panic.log`
/// and leave the terminal untouched, so a failed background refresh degrades to a
/// silent miss instead of corrupting the live screen. A panic on any other thread
/// (the main/TUI thread) restores the terminal — disable raw mode, leave the alt
/// screen, re-enable the cursor — before delegating to the previous hook so the
/// message renders on a clean normal screen.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let is_background = std::thread::current()
            .name()
            .is_some_and(|n| n.starts_with("pftui-bg"));

        if is_background {
            log_background_panic(info);
            return;
        }

        // Main/TUI thread: best-effort terminal restore, then default report.
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        previous(info);
    }));
}

/// Append a background-thread panic to the pftui data dir's `panic.log`. Never
/// writes to stdout/stderr — that would corrupt the live alt-screen TUI.
fn log_background_panic(info: &std::panic::PanicHookInfo<'_>) {
    use std::io::Write;
    let Some(dir) = dirs::data_local_dir() else {
        return;
    };
    let path = dir.join("pftui").join("panic.log");
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("<unnamed>");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "[bg-panic thread={name}] {info}");
    }
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
