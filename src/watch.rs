use std::sync::{Arc, Mutex};

use crate::errors::CsvlensResult;

/// A file watcher that keeps track of the file state and can check for changes. A thin wrapper
/// around a shared `Watcher` for easier usage.
pub struct FileWatcher {
    pub file_state: FileState,
    pub watcher: Arc<Watcher>,
}

impl From<Arc<Watcher>> for FileWatcher {
    fn from(watcher: Arc<Watcher>) -> Self {
        let file_state = watcher.get_file_state();
        FileWatcher {
            file_state,
            watcher,
        }
    }
}

impl FileWatcher {
    /// Check if the file has changed since the last check.
    pub fn check(&mut self) -> bool {
        let current_file_state = self.watcher.get_file_state();
        if self.file_state != current_file_state {
            self.file_state = current_file_state;
            true
        } else {
            false
        }
    }
}

/// A file watcher that monitors a file for changes in a separate thread.
pub struct Watcher {
    internal: Arc<Mutex<WatcherInternal>>,
}

impl Watcher {
    pub fn new(filename: &str) -> CsvlensResult<Watcher> {
        let internal = WatcherInternal::init(filename)?;

        Ok(Watcher { internal })
    }

    pub fn get_file_state(&self) -> FileState {
        let internal = self.internal.lock().unwrap();
        internal.file_state
    }

    pub fn terminate(&self) {
        let mut internal = self.internal.lock().unwrap();
        internal.terminate();
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FileState {
    pub modified_time: std::time::SystemTime,
    pub size: u64,
}

impl From<std::fs::Metadata> for FileState {
    fn from(metadata: std::fs::Metadata) -> Self {
        FileState {
            modified_time: metadata
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            size: metadata.len(),
        }
    }
}

struct WatcherInternal {
    should_terminate: bool,
    file_state: FileState,
}

impl WatcherInternal {
    pub fn init(filename: &str) -> CsvlensResult<Arc<Mutex<WatcherInternal>>> {
        let file_state = std::fs::metadata(filename)?;

        let internal = WatcherInternal {
            should_terminate: false,
            file_state: file_state.into(),
        };

        let m_internal = Arc::new(Mutex::new(internal));

        let _handle = {
            let filename = filename.to_string();
            let m_internal = Arc::clone(&m_internal);
            std::thread::spawn(move || {
                loop {
                    if m_internal.lock().unwrap().should_terminate {
                        break;
                    }
                    match std::fs::metadata(&filename) {
                        Ok(metadata) => {
                            let mut internal = m_internal.lock().unwrap();
                            let new_file_state = FileState::from(metadata);
                            internal.file_state = new_file_state;
                        }
                        Err(_) => {
                            // File might be temporarily unavailable, skip for now
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            })
        };

        Ok(m_internal)
    }

    pub fn terminate(&mut self) {
        self.should_terminate = true;
    }
}
