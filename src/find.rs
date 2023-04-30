use crate::csv;
use anyhow::Result;
use regex::Regex;
use std::cmp::min;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self};
use std::time::Instant;

pub struct Finder {
    internal: Arc<Mutex<FinderInternalState>>,
    cursor: Option<usize>,
    row_hint: usize,
    target: Regex,
}

#[derive(Clone, Debug)]
pub struct FoundRecord {
    pub row_index: usize,
    column_indices: Vec<usize>,
}

impl FoundRecord {
    pub fn row_index(&self) -> usize {
        self.row_index
    }

    pub fn column_indices(&self) -> &Vec<usize> {
        &self.column_indices
    }

    pub fn first_column(&self) -> usize {
        *self.column_indices.first().unwrap()
    }
}

impl Finder {
    pub fn new(config: Arc<csv::CsvConfig>, target: Regex) -> Result<Self> {
        let internal = FinderInternalState::init(config, target.clone());
        let finder = Finder {
            internal,
            cursor: None,
            row_hint: 0,
            target,
        };
        Ok(finder)
    }

    pub fn count(&self) -> usize {
        (self.internal.lock().unwrap()).count
    }

    pub fn done(&self) -> bool {
        (self.internal.lock().unwrap()).done
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn cursor_row_index(&self) -> Option<usize> {
        let m_guard = self.internal.lock().unwrap();
        self.get_found_record_at_cursor(&m_guard)
            .map(|x| x.row_index())
    }

    pub fn target(&self) -> Regex {
        self.target.clone()
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = None;
    }

    pub fn set_row_hint(&mut self, row_hint: usize) {
        self.row_hint = row_hint;
    }

    pub fn next(&mut self) -> Option<FoundRecord> {
        let m_guard = self.internal.lock().unwrap();
        let count = m_guard.count;
        if let Some(n) = self.cursor {
            if n + 1 < count {
                self.cursor = Some(n + 1);
            }
        } else if count > 0 {
            self.cursor = Some(m_guard.next_from(self.row_hint));
        }
        self.get_found_record_at_cursor(&m_guard)
    }

    pub fn prev(&mut self) -> Option<FoundRecord> {
        let m_guard = self.internal.lock().unwrap();
        if let Some(n) = self.cursor {
            self.cursor = Some(n.saturating_sub(1));
        } else {
            let count = m_guard.count;
            if count > 0 {
                self.cursor = Some(m_guard.prev_from(self.row_hint));
            }
        }
        self.get_found_record_at_cursor(&m_guard)
    }

    pub fn current(&self) -> Option<FoundRecord> {
        let m_guard = self.internal.lock().unwrap();
        self.get_found_record_at_cursor(&m_guard)
    }

    fn get_found_record_at_cursor(
        &self,
        m_guard: &MutexGuard<FinderInternalState>,
    ) -> Option<FoundRecord> {
        if let Some(n) = self.cursor {
            // TODO: this weird ref massaging really needed?
            let res = m_guard.founds.get(n);
            res.cloned()
        } else {
            None
        }
    }

    fn terminate(&self) {
        let mut m_guard = self.internal.lock().unwrap();
        m_guard.terminate();
    }

    pub fn elapsed(&self) -> Option<u128> {
        let m_guard = self.internal.lock().unwrap();
        m_guard.elapsed()
    }

    pub fn get_subset_found(&self, offset: usize, num_rows: usize) -> Vec<u64> {
        let m_guard = self.internal.lock().unwrap();
        let founds = &m_guard.founds;
        let start = min(offset, founds.len().saturating_sub(1));
        let end = start.saturating_add(num_rows);
        let end = min(end, founds.len());
        let indices: Vec<u64> = founds[start..end]
            .iter()
            .map(|x| x.row_index() as u64)
            .collect();
        indices
    }
}

impl Drop for Finder {
    fn drop(&mut self) {
        self.terminate();
    }
}

struct FinderInternalState {
    count: usize,
    founds: Vec<FoundRecord>,
    done: bool,
    should_terminate: bool,
    elapsed: Option<u128>,
}

impl FinderInternalState {
    pub fn init(config: Arc<csv::CsvConfig>, target: Regex) -> Arc<Mutex<FinderInternalState>> {
        let internal = FinderInternalState {
            count: 0,
            founds: vec![],
            done: false,
            should_terminate: false,
            elapsed: None,
        };

        let m_state = Arc::new(Mutex::new(internal));

        let _m = m_state.clone();
        let _filename = config.filename().to_owned();

        let _handle = thread::spawn(move || {
            let mut bg_reader = config.new_reader().unwrap();

            // note that records() exludes header
            let records = bg_reader.records();

            let start = Instant::now();
            for (row_index, r) in records.enumerate() {
                let mut column_indices = vec![];
                if let Ok(valid_record) = r {
                    for (column_index, field) in valid_record.iter().enumerate() {
                        if target.is_match(field) {
                            column_indices.push(column_index);
                        }
                    }
                }
                if !column_indices.is_empty() {
                    let found = FoundRecord {
                        row_index,
                        column_indices,
                    };
                    let mut m = _m.lock().unwrap();
                    (*m).found_one(found);
                }
                let m = _m.lock().unwrap();
                if m.should_terminate {
                    break;
                }
            }

            let mut m = _m.lock().unwrap();
            m.done = true;
            m.elapsed = Some(start.elapsed().as_micros());
        });

        m_state
    }

    fn found_one(&mut self, found: FoundRecord) {
        self.founds.push(found);
        self.count += 1;
    }

    fn next_from(&self, row_hint: usize) -> usize {
        let mut index = self.founds.partition_point(|r| r.row_index() < row_hint);
        if index >= self.founds.len() {
            index -= 1;
        }
        index
    }

    fn prev_from(&self, row_hint: usize) -> usize {
        let next = self.next_from(row_hint);
        if next > 0 {
            next - 1
        } else {
            next
        }
    }

    fn terminate(&mut self) {
        self.should_terminate = true;
    }

    fn elapsed(&self) -> Option<u128> {
        self.elapsed
    }
}
