use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use crossterm::event::{read, Event, KeyEvent, KeyCode};

pub enum CsvlensEvent<I> {
    Input(I),
    Tick,
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct CsvlensEvents {
    rx: mpsc::Receiver<CsvlensEvent<KeyEvent>>,
    input_handle: thread::JoinHandle<()>,
    ignore_exit_key: Arc<AtomicBool>,
    tick_handle: thread::JoinHandle<()>,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub exit_key: KeyCode,
    pub tick_rate: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            exit_key: KeyCode::Char('q'),
            tick_rate: Duration::from_millis(250),
        }
    }
}

impl CsvlensEvents {
    pub fn new() -> CsvlensEvents {
        CsvlensEvents::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> CsvlensEvents {
        let (tx, rx) = mpsc::channel();
        let ignore_exit_key = Arc::new(AtomicBool::new(true));
        let input_handle = {
            let tx = tx.clone();
            // TODO: not used?
            let _ignore_exit_key = ignore_exit_key.clone();
            thread::spawn(move || {
                loop {
                    let event_result = read().unwrap();
                    match event_result {
                        Event::Key(event) => {
                            if let Err(err) = tx.send(CsvlensEvent::Input(event)) {
                                eprintln!("{}", err);
                                return;
                            }
                        }
                        _ => {},
                    };
                }
            })
        };
        let tick_handle = {
            thread::spawn(move || loop {
                if tx.send(CsvlensEvent::Tick).is_err() {
                    break;
                }
                thread::sleep(config.tick_rate);
            })
        };
        CsvlensEvents {
            rx,
            ignore_exit_key,
            input_handle,
            tick_handle,
        }
    }

    pub fn next(&self) -> Result<CsvlensEvent<KeyEvent>, mpsc::RecvError> {
        self.rx.recv()
    }

    pub fn disable_exit_key(&mut self) {
        self.ignore_exit_key.store(true, Ordering::Relaxed);
    }

    pub fn enable_exit_key(&mut self) {
        self.ignore_exit_key.store(false, Ordering::Relaxed);
    }
}
