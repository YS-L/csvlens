use std::time::Duration;

use crossterm::event::{Event, KeyEvent, KeyEventKind, poll, read};

use crate::watch::FileWatcher;

pub enum CsvlensEvent<I> {
    Input(I),
    FileChanged,
    Tick,
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct CsvlensEvents {
    tick_rate: Duration,
    file_watcher: Option<FileWatcher>,
}

impl CsvlensEvents {
    pub fn new(file_watcher: Option<FileWatcher>) -> CsvlensEvents {
        CsvlensEvents {
            tick_rate: Duration::from_millis(250),
            file_watcher,
        }
    }

    pub fn next(&mut self) -> std::io::Result<CsvlensEvent<KeyEvent>> {
        // let now = Instant::now();
        match poll(self.tick_rate) {
            Ok(true) => match read()? {
                Event::Key(event) if event.kind == KeyEventKind::Press => {
                    Ok(CsvlensEvent::Input(event))
                }
                _ => Ok(CsvlensEvent::Tick),
            },
            Ok(false) => {
                if let Some(file_watcher) = &mut self.file_watcher {
                    if file_watcher.check() {
                        return Ok(CsvlensEvent::FileChanged);
                    }
                    return Ok(CsvlensEvent::Tick);
                }
                Ok(CsvlensEvent::Tick)
            }
            Err(_) => todo!(),
        }
    }
}
