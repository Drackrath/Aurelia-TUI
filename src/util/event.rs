use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use crate::util::log::log;

use crossterm::event::{
    read, Event as CrossEvent, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
};

pub enum Event<I> {
    Input(I),
    Tick,
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
///
/// # Input contract
///
/// Every distinct keypress is forwarded to the channel as an [`Event::Input`];
/// the input thread never drops or debounces user keys, so fast typing is
/// lossless and the consumer can simply call [`Events::next`] in a loop without
/// any release/re-arm handshake. Tick events are produced independently by the
/// tick thread.
pub struct Events {
    rx: mpsc::Receiver<Event<KeyCode>>,
    stop: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub tick_rate: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            // ~4 redraws/second when idle: fast enough for the smooth list-name
            // marquee and snappy adoption of async (friends/workshop) results,
            // while still cheap (tui only writes the cells that actually change).
            tick_rate: Duration::from_millis(250),
        }
    }
}

impl Events {
    pub fn new() -> Events {
        Events::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> Events {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));

        let _input_handle = {
            let tx = tx.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                //matching the key
                loop {
                    // Map the raw terminal event to a key code we act on. Mouse
                    // wheel scrolling is folded into Down/Up so the list handlers
                    // work for both keyboard and wheel. (Shift+drag selection is
                    // handled by the terminal itself and never reaches here.)
                    let event = match read() {
                        Ok(event) => event,
                        Err(err) => {
                            // A read error (e.g. the terminal going away on
                            // shutdown) must not panic and silently kill input;
                            // log it and stop the thread cleanly.
                            log!(err);
                            return;
                        }
                    };
                    let code = match event {
                        CrossEvent::Key(KeyEvent {
                            code: kc,
                            modifiers,
                            ..
                        }) => {
                            // Let CTRL-c just be q
                            if kc == KeyCode::Char('c')
                                && modifiers.contains(KeyModifiers::CONTROL)
                            {
                                Some(KeyCode::Char('q'))
                            } else {
                                Some(kc)
                            }
                        }
                        CrossEvent::Mouse(MouseEvent { kind, .. }) => match kind {
                            MouseEventKind::ScrollDown => Some(KeyCode::Down),
                            MouseEventKind::ScrollUp => Some(KeyCode::Up),
                            _ => None,
                        },
                        _ => None,
                    };

                    // Forward every keypress; no debounce so no user key is lost.
                    if let Some(kc) = code {
                        if let Err(err) = tx.send(Event::Input(kc)) {
                            log!(err);
                            return;
                        }
                    }
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                }
            })
        };
        let _tick_handle = {
            let stop = stop.clone();
            thread::spawn(move || loop {
                if tx.send(Event::Tick).is_err() {
                    break;
                }
                thread::sleep(config.tick_rate);
                if stop.load(Ordering::Relaxed) {
                    return;
                }
            })
        };
        Events { rx, stop }
    }

    /// Block for the next input or tick event. Every keypress is delivered
    /// exactly once, in order, with no release handshake required.
    pub fn next(&self) -> Result<Event<KeyCode>, mpsc::RecvError> {
        self.rx.recv()
    }
}

impl Default for Events {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
