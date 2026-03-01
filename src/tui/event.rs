use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self as ct_event, KeyEvent};

pub enum Event {
    Key(KeyEvent),
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
        let (tx, rx) = mpsc::channel();
        let tick_rate = Duration::from_millis(tick_rate_ms);

        let handle = thread::spawn(move || loop {
            if ct_event::poll(tick_rate).unwrap_or(false) {
                match ct_event::read() {
                    Ok(ct_event::Event::Key(key)) => {
                        if tx.send(Event::Key(key)).is_err() {
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
            } else if tx.send(Event::Tick).is_err() {
                return;
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
