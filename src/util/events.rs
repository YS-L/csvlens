use std::time::{Duration, Instant};

use crossterm::event::{poll, read, Event, KeyEvent, KeyEventKind};

pub enum CsvlensEvent<I> {
    Input(I),
    Tick,
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct CsvlensEvents {
    tick_rate: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub tick_rate: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            tick_rate: Duration::from_millis(250),
        }
    }
}

impl CsvlensEvents {
    pub fn new() -> CsvlensEvents {
        CsvlensEvents::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> CsvlensEvents {
        CsvlensEvents {
            tick_rate: config.tick_rate,
        }
    }

    pub fn next(&self) -> std::io::Result<CsvlensEvent<KeyEvent>> {
        let now = Instant::now();
        match poll(self.tick_rate) {
            Ok(true) => match read()? {
                Event::Key(event) if event.kind == KeyEventKind::Press => {
                    Ok(CsvlensEvent::Input(event))
                }
                _ => {
                    let time_spent = now.elapsed();
                    let rest = self.tick_rate.saturating_sub(time_spent);

                    Self { tick_rate: rest }.next()
                }
            },
            Ok(false) => Ok(CsvlensEvent::Tick),
            Err(_) => todo!(),
        }
    }
}
