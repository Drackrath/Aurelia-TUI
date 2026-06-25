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
/// # Latch/debounce contract
///
/// The input thread is *latched*: after it delivers one [`Event::Input`] it
/// stops forwarding keys (folding key-repeat / held keys into a single press)
/// until the consumer calls [`Events::release`]. A consumer loop that reads a
/// key via [`Events::next`] **must** call [`Events::release`] once it has
/// finished handling that key, otherwise the input goes permanently deaf — the
/// next keypress (even `Esc`/`q`) is never delivered. Tick events are *not*
/// latched and require no release.
///
/// To keep a future consumer loop from silently reintroducing the "deaf after
/// one keypress" bug, [`Events::next`] tracks whether a latched `Input` is
/// still outstanding and `debug_assert!`s in debug builds if a second `next`
/// blocks on `recv` while the previous `Input` was never released. The
/// assertion only fires in debug builds, so release behaviour is unchanged.
pub struct Events {
    rx: mpsc::Receiver<Event<KeyCode>>,
    stop: Arc<AtomicBool>,
    debounce: Arc<AtomicBool>,
    /// True between delivering an `Input` and the consumer's `release()`. Used
    /// only to catch a missing `release()` in debug builds; never gates I/O.
    latched: std::cell::Cell<bool>,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub tick_rate: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            tick_rate: Duration::from_millis(500),
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
        let debounce = Arc::new(AtomicBool::new(true));

        let _input_handle = {
            let tx = tx.clone();
            let stop = stop.clone();
            let debounce = debounce.clone();
            thread::spawn(move || {
                //matching the key
                loop {
                    // Map the raw terminal event to a key code we act on. Mouse
                    // wheel scrolling is folded into Down/Up so the list handlers
                    // work for both keyboard and mouse.
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

                    if let Some(kc) = code {
                        if debounce.load(Ordering::Relaxed) {
                            if let Err(err) = tx.send(Event::Input(kc)) {
                                log!(err);
                                return;
                            }
                            debounce.store(false, Ordering::Relaxed);
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
        Events {
            rx,
            stop,
            debounce,
            latched: std::cell::Cell::new(false),
        }
    }

    /// Re-arm the input latch so the next keypress is delivered. Must be called
    /// once after handling each [`Event::Input`]; see the type-level docs.
    pub fn release(&self) {
        self.latched.set(false);
        self.debounce.store(true, Ordering::Relaxed);
    }

    /// Block for the next input or tick event.
    ///
    /// Each delivered [`Event::Input`] latches the input thread; the caller must
    /// call [`Events::release`] before the next `next()` that expects a fresh
    /// key, or input goes deaf. In debug builds a missing `release()` trips a
    /// `debug_assert!` here so the regression is caught in development rather
    /// than shipping as a silently unresponsive UI. Release builds are
    /// unaffected.
    pub fn next(&self) -> Result<Event<KeyCode>, mpsc::RecvError> {
        debug_assert!(
            !self.latched.get(),
            "Events::next() called while a previous Input is still latched; \
             the consumer loop must call Events::release() after handling each \
             keypress or input goes deaf"
        );
        let event = self.rx.recv()?;
        if matches!(event, Event::Input(_)) {
            self.latched.set(true);
        }
        Ok(event)
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
