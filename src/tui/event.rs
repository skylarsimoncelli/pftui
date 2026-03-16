use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self as ct_event, KeyEvent, MouseEvent};

pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Tick,
    #[allow(dead_code)]
    Resize(u16, u16),
}

pub struct EventHandler {
    rx: mpsc::Receiver<Event>,
    _handle: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        // Keep the queue bounded so periodic ticks cannot run far ahead of
        // input handling on slower terminals such as remote SSH sessions.
        let (tx, rx) = mpsc::sync_channel(8);
        let tick_rate = Duration::from_millis(tick_rate_ms);

        let handle = thread::spawn(move || loop {
            if ct_event::poll(tick_rate).unwrap_or(false) {
                match ct_event::read() {
                    Ok(ct_event::Event::Key(key)) => {
                        if tx.send(Event::Key(key)).is_err() {
                            return;
                        }
                    }
                    Ok(ct_event::Event::Mouse(mouse)) => {
                        if tx.send(Event::Mouse(mouse)).is_err() {
                            return;
                        }
                    }
                    Ok(ct_event::Event::Resize(w, h)) => {
                        if tx.send(Event::Resize(w, h)).is_err() {
                            return;
                        }
                    }
                    _ => {}
                }
            } else {
                // Dropping an occasional tick is fine; dropping key or mouse
                // input is not. This prevents lag from compounding when draw
                // speed falls behind the requested tick rate.
                match tx.try_send(Event::Tick) {
                    Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                    Err(mpsc::TrySendError::Disconnected(_)) => return,
                }
            }
        });

        EventHandler {
            rx,
            _handle: handle,
        }
    }

    pub fn next(&self) -> Result<Event> {
        Ok(self.rx.recv()?)
    }
}
