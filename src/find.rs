use crate::csv;
use crate::errors::CsvlensResult;
use crate::sort;
use crate::sort::SortOrder;
use regex::Regex;
use sorted_vec::SortedVec;
use std::cmp::min;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self};
use std::time::{Duration, Instant};

struct FinderCursor {
    row: usize,
    column: usize,
}

pub struct Finder {
    internal: Arc<Mutex<FinderInternalState>>,
    cursor: Option<FinderCursor>,
    row_hint: usize,
    target: Regex,
    column_index: Option<usize>,
    sorter: Option<Arc<sort::Sorter>>,
    pub sort_order: SortOrder,
}

#[derive(Clone, Debug)]
pub struct FoundEntry {
    row_index: usize,
    row_order: usize,
    column_index: usize,
}

impl FoundEntry {
    pub fn row_index(&self) -> usize {
        self.row_index
    }

    pub fn row_order(&self) -> usize {
        self.row_order
    }

    pub fn column_index(&self) -> usize {
        self.column_index
    }
}

#[derive(Clone, Debug)]
pub struct FoundRecord {
    row_index: usize,
    row_order: usize,
    column_indices: Vec<usize>,
}

impl FoundRecord {
    pub fn row_index(&self) -> usize {
        self.row_index
    }

    pub fn row_order(&self) -> usize {
        self.row_order
    }

    pub fn column_indices(&self) -> &Vec<usize> {
        &self.column_indices
    }

    pub fn get_entry(&self, entry_index: usize) -> Option<FoundEntry> {
        self.column_indices
            .get(entry_index)
            .map(|column_index| FoundEntry {
                row_index: self.row_index,
                row_order: self.row_order,
                column_index: *column_index,
            })
    }
}

impl Ord for FoundRecord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.row_order.cmp(&other.row_order)
    }
}

impl PartialOrd for FoundRecord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.row_order.cmp(&other.row_order))
    }
}

impl PartialEq for FoundRecord {
    fn eq(&self, other: &Self) -> bool {
        self.row_order == other.row_order
    }
}

impl Eq for FoundRecord {}

impl Finder {
    pub fn new(
        config: Arc<csv::CsvConfig>,
        target: Regex,
        column_index: Option<usize>,
        sorter: Option<Arc<sort::Sorter>>,
        sort_order: SortOrder,
    ) -> CsvlensResult<Self> {
        let internal = FinderInternalState::init(
            config,
            target.clone(),
            column_index,
            sorter.clone(),
            sort_order,
        );
        let finder = Finder {
            internal,
            cursor: None,
            row_hint: 0,
            target,
            column_index,
            sorter: sorter.clone(),
            sort_order,
        };
        Ok(finder)
    }

    pub fn count(&self) -> usize {
        (self.internal.lock().unwrap()).count
    }

    pub fn count_and_max_row_index(&self) -> (usize, Option<u64>) {
        let g = self.internal.lock().unwrap();
        (g.count, g.founds.last().map(|x| x.row_index() as u64))
    }

    pub fn done(&self) -> bool {
        (self.internal.lock().unwrap()).done
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor.as_ref().map(|x| x.row)
    }

    pub fn cursor_row_order(&self) -> Option<usize> {
        let m_guard = self.internal.lock().unwrap();
        self.get_found_record_at_cursor(&m_guard)
            .map(|x| x.row_order())
    }

    pub fn target(&self) -> Regex {
        self.target.clone()
    }

    pub fn column_index(&self) -> Option<usize> {
        self.column_index
    }

    pub fn sorter(&self) -> &Option<Arc<sort::Sorter>> {
        &self.sorter
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = None;
    }

    pub fn set_row_hint(&mut self, row_hint: usize) {
        self.row_hint = row_hint;
    }

    pub fn next(&mut self) -> Option<FoundEntry> {
        let m_guard = self.internal.lock().unwrap();
        let count = m_guard.count;
        let founds = &m_guard.founds;
        if let Some(cursor) = &self.cursor {
            if let Some(record) = founds.get(cursor.row) {
                if cursor.column + 1 < record.column_indices().len() {
                    // Try next column first if available
                    self.cursor = Some(FinderCursor {
                        row: cursor.row,
                        column: cursor.column + 1,
                    });
                } else if cursor.row + 1 < count {
                    // Next row if available
                    self.cursor = Some(FinderCursor {
                        row: cursor.row + 1,
                        column: 0,
                    });
                }
            }
        } else if count > 0 {
            self.cursor = Some(FinderCursor {
                row: m_guard.next_from(self.row_hint),
                column: 0,
            });
        }
        self.get_found_record_at_cursor(&m_guard)
    }

    pub fn prev(&mut self) -> Option<FoundEntry> {
        let m_guard = self.internal.lock().unwrap();
        if let Some(cursor) = &self.cursor {
            if cursor.column > 0 {
                // Try previous column first if available
                self.cursor = Some(FinderCursor {
                    row: cursor.row,
                    column: cursor.column.saturating_sub(1),
                });
            } else {
                // Previous row if available
                let n = cursor.row;
                if n > 0 {
                    let prev_row = n.saturating_sub(1);
                    if let Some(prev_record) = m_guard.founds.get(prev_row) {
                        self.cursor = Some(FinderCursor {
                            row: prev_row,
                            column: prev_record.column_indices().len() - 1,
                        });
                    }
                }
            }
        } else {
            let count = m_guard.count;
            if count > 0 {
                self.cursor = Some(FinderCursor {
                    row: m_guard.prev_from(self.row_hint),
                    column: 0,
                });
            }
        }
        self.get_found_record_at_cursor(&m_guard)
    }

    pub fn current(&self) -> Option<FoundEntry> {
        let m_guard = self.internal.lock().unwrap();
        self.get_found_record_at_cursor(&m_guard)
    }

    fn get_found_record_at_cursor(
        &self,
        m_guard: &MutexGuard<FinderInternalState>,
    ) -> Option<FoundEntry> {
        if let Some(cursor) = &self.cursor {
            let res = m_guard.founds.get(cursor.row);
            res.and_then(|x| x.get_entry(cursor.column))
        } else {
            None
        }
    }

    fn terminate(&self) {
        let mut m_guard = self.internal.lock().unwrap();
        m_guard.terminate();
    }

    pub fn elapsed(&self) -> Option<Duration> {
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

    #[cfg(test)]
    pub fn wait_internal(&self) {
        loop {
            if self.internal.lock().unwrap().done {
                break;
            }
            thread::sleep(core::time::Duration::from_millis(100));
        }
    }
}

impl Drop for Finder {
    fn drop(&mut self) {
        self.terminate();
    }
}

struct FinderInternalState {
    count: usize,
    founds: SortedVec<FoundRecord>,
    done: bool,
    should_terminate: bool,
    elapsed: Option<Duration>,
}

impl FinderInternalState {
    pub fn init(
        config: Arc<csv::CsvConfig>,
        target: Regex,
        column_index: Option<usize>,
        sorter: Option<Arc<sort::Sorter>>,
        sort_order: SortOrder,
    ) -> Arc<Mutex<FinderInternalState>> {
        let internal = FinderInternalState {
            count: 0,
            founds: SortedVec::new(),
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
                    if let Some(column_index) = column_index {
                        if let Some(field) = valid_record.get(column_index) {
                            if target.is_match(field) {
                                column_indices.push(column_index);
                            }
                        }
                    } else {
                        for (column_index, field) in valid_record.iter().enumerate() {
                            if target.is_match(field) {
                                column_indices.push(column_index);
                            }
                        }
                    }
                }
                if !column_indices.is_empty() {
                    let row_order = match &sorter {
                        Some(s) => {
                            s.get_record_order(row_index as u64, sort_order).unwrap() as usize
                        }
                        _ => row_index,
                    };
                    let found = FoundRecord {
                        row_index,
                        row_order,
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
            m.elapsed = Some(start.elapsed());
        });

        m_state
    }

    fn found_one(&mut self, found: FoundRecord) {
        self.founds.push(found);
        self.count += 1;
    }

    fn next_from(&self, row_hint: usize) -> usize {
        let mut index = self.founds.partition_point(|r| r.row_order() < row_hint);
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

    fn elapsed(&self) -> Option<Duration> {
        self.elapsed
    }
}
