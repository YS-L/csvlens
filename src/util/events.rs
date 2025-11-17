use std::{path::Path, time::Duration};

use crossterm::event::{Event, KeyEvent, KeyEventKind, poll, read};
use notify::RecommendedWatcher;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::errors::CsvlensResult;

pub enum CsvlensEvent<I> {
    Input(I),
    FileChanged,
    Tick,
}

struct FileWatcher {
    watch_filename: String,
    rx: std::sync::mpsc::Receiver<DebounceEventResult>,
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct CsvlensEvents {
    tick_rate: Duration,
    file_watcher: Option<FileWatcher>,
}

impl CsvlensEvents {
    pub fn new(watch_filename: Option<&str>) -> CsvlensResult<CsvlensEvents> {
        let file_watcher = if let Some(filename) = watch_filename {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut debouncer = new_debouncer(Duration::from_millis(100), None, tx)?;
            debouncer.watch(Path::new(filename), notify::RecursiveMode::NonRecursive)?;
            Some(FileWatcher {
                watch_filename: filename.to_string(),
                rx,
                _debouncer: debouncer,
            })
        } else {
            None
        };
        Ok(CsvlensEvents {
            tick_rate: Duration::from_millis(250),
            file_watcher,
        })
    }

    pub fn next(&self) -> std::io::Result<CsvlensEvent<KeyEvent>> {
        // let now = Instant::now();
        match poll(self.tick_rate) {
            Ok(true) => match read()? {
                Event::Key(event) if event.kind == KeyEventKind::Press => {
                    Ok(CsvlensEvent::Input(event))
                }
                _ => Ok(CsvlensEvent::Tick),
            },
            Ok(false) => {
                if let Some(file_watcher) = &self.file_watcher {
                    // Drain the file watcher channel
                    let mut file_changed = false;
                    while let Ok(debounced_event) = file_watcher.rx.try_recv() {
                        match debounced_event {
                            Ok(events) => {
                                for ev in events {
                                    if let notify::EventKind::Modify(_) = ev.event.kind {
                                        file_changed = true;
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "watch error for file {:?}: {:?}",
                                    file_watcher.watch_filename, e
                                );
                            }
                        }
                    }
                    if file_changed {
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
