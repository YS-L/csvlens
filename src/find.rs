use crate::columns_filter;
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

#[derive(Debug, Clone)]
pub enum RowPos {
    Header,
    Row(usize),
}

#[derive(Debug, Clone)]
pub struct FinderCursor {
    pub row: RowPos,
    pub column: usize,
}

impl FinderCursor {
    fn next_row(&self, total_count: usize) -> FinderCursor {
        match self.row {
            RowPos::Header => FinderCursor {
                row: if total_count > 0 {
                    RowPos::Row(0)
                } else {
                    RowPos::Header
                },
                column: 0,
            },
            RowPos::Row(n) => FinderCursor {
                row: if n + 1 < total_count {
                    RowPos::Row(n + 1)
                } else {
                    RowPos::Row(n)
                },
                column: 0,
            },
        }
    }

    fn prev_row(&self, has_header_found: bool) -> FinderCursor {
        match self.row {
            RowPos::Header => FinderCursor {
                row: RowPos::Header,
                column: 0,
            },
            RowPos::Row(0) => FinderCursor {
                row: if has_header_found {
                    RowPos::Header
                } else {
                    RowPos::Row(0)
                },
                column: 0,
            },
            RowPos::Row(n) => FinderCursor {
                row: RowPos::Row(n.saturating_sub(1)),
                column: 0,
            },
        }
    }

    fn next_column(&self) -> FinderCursor {
        match self.row {
            RowPos::Header => FinderCursor {
                row: RowPos::Header,
                column: self.column.saturating_add(1),
            },
            RowPos::Row(n) => FinderCursor {
                row: RowPos::Row(n),
                column: self.column.saturating_add(1),
            },
        }
    }

    fn prev_column(&self) -> FinderCursor {
        match self.row {
            RowPos::Header => FinderCursor {
                row: RowPos::Header,
                column: self.column.saturating_sub(1),
            },
            RowPos::Row(n) => FinderCursor {
                row: RowPos::Row(n),
                column: self.column.saturating_sub(1),
            },
        }
    }
}

pub struct Finder {
    internal: Arc<Mutex<FinderInternalState>>,
    pub cursor: Option<FinderCursor>,
    row_hint: RowPos,
    target: Regex,
    column_index: Option<usize>,
    sorter: Option<Arc<sort::Sorter>>,
    pub sort_order: SortOrder,
}

pub enum FoundEntry {
    Header(HeaderEntry),
    Row(RowEntry),
}

#[derive(Clone, Debug)]
pub struct RowEntry {
    row_index: usize,
    row_order: usize,
    column_index: usize,
}

impl RowEntry {
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
pub struct HeaderEntry {
    column_index: usize,
}

impl HeaderEntry {
    pub fn column_index(&self) -> usize {
        self.column_index
    }
}

#[derive(Clone, Debug)]
pub struct FoundHeader {
    column_indices: Vec<usize>,
}

impl FoundHeader {
    pub fn column_indices(&self) -> &Vec<usize> {
        &self.column_indices
    }

    pub fn get_entry(&self, entry_index: usize) -> Option<HeaderEntry> {
        self.column_indices
            .get(entry_index)
            .map(|column_index| HeaderEntry {
                column_index: *column_index,
            })
    }
}

#[derive(Clone, Debug)]
pub struct FoundRow {
    row_index: usize,
    row_order: usize,
    column_indices: Vec<usize>,
}

impl FoundRow {
    pub fn row_index(&self) -> usize {
        self.row_index
    }

    pub fn row_order(&self) -> usize {
        self.row_order
    }

    pub fn column_indices(&self) -> &Vec<usize> {
        &self.column_indices
    }

    pub fn get_entry(&self, entry_index: usize) -> Option<RowEntry> {
        self.column_indices
            .get(entry_index)
            .map(|column_index| RowEntry {
                row_index: self.row_index,
                row_order: self.row_order,
                column_index: *column_index,
            })
    }
}

impl Ord for FoundRow {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.row_order.cmp(&other.row_order)
    }
}

impl PartialOrd for FoundRow {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for FoundRow {
    fn eq(&self, other: &Self) -> bool {
        self.row_order == other.row_order
    }
}

impl Eq for FoundRow {}

impl Finder {
    pub fn new(
        config: Arc<csv::CsvConfig>,
        target: Regex,
        column_index: Option<usize>,
        sorter: Option<Arc<sort::Sorter>>,
        sort_order: SortOrder,
        columns_filter: Option<Arc<columns_filter::ColumnsFilter>>,
    ) -> CsvlensResult<Self> {
        let internal = FinderInternalState::init(
            config,
            target.clone(),
            column_index,
            sorter.clone(),
            sort_order,
            columns_filter,
        );
        let finder = Finder {
            internal,
            cursor: None,
            row_hint: RowPos::Header,
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

    pub fn found_any(&self) -> bool {
        let g = self.internal.lock().unwrap();
        g.count > 0 || g.found_header.is_some()
    }

    pub fn header_has_match(&self) -> bool {
        (self.internal.lock().unwrap()).found_header.is_some()
    }

    pub fn done(&self) -> bool {
        (self.internal.lock().unwrap()).done
    }

    pub fn cursor(&self) -> Option<FinderCursor> {
        self.cursor.as_ref().cloned()
    }

    pub fn cursor_row_order(&self) -> Option<usize> {
        let m_guard = self.internal.lock().unwrap();
        if let Some(FoundEntry::Row(entry)) = self.get_found_record_at_cursor(&m_guard) {
            Some(entry.row_order())
        } else {
            None
        }
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

    pub fn set_row_hint(&mut self, row_hint: RowPos) {
        self.row_hint = row_hint;
    }

    pub fn next(&mut self) -> Option<FoundEntry> {
        let m_guard = self.internal.lock().unwrap();
        let count = m_guard.count;
        let founds = &m_guard.founds;
        if let Some(cursor) = &self.cursor {
            let column_indices = match cursor.row {
                RowPos::Header => m_guard.found_header.as_ref().map(|x| x.column_indices()),
                RowPos::Row(n) => founds.get(n).map(|x| x.column_indices()),
            };
            if let Some(column_indices) = column_indices {
                if cursor.column + 1 < column_indices.len() {
                    // Try next column first if available
                    self.cursor = Some(cursor.next_column());
                } else {
                    // Next row if available
                    self.cursor = Some(cursor.next_row(count));
                }
            }
        } else if matches!(self.row_hint, RowPos::Header) && m_guard.found_header.is_some() {
            self.cursor = Some(FinderCursor {
                row: RowPos::Header,
                column: 0,
            });
        } else if count > 0 {
            let n = match self.row_hint {
                // If here, we know there is no matches in header even though row_hint is still
                // Header. Start from first found row.
                RowPos::Header => 0,
                RowPos::Row(n) => n,
            };
            self.cursor = Some(FinderCursor {
                row: RowPos::Row(m_guard.next_from(n)),
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
                self.cursor = Some(cursor.prev_column());
            } else {
                // Previous row if available
                self.cursor = Some(cursor.prev_row(m_guard.found_header.is_some()));
            }
        } else if matches!(self.row_hint, RowPos::Header) && m_guard.found_header.is_some() {
            self.cursor = Some(FinderCursor {
                row: RowPos::Header,
                column: 0,
            });
        } else if m_guard.count > 0
            && let RowPos::Row(n) = self.row_hint
        {
            self.cursor = Some(FinderCursor {
                row: RowPos::Row(m_guard.prev_from(n)),
                column: 0,
            });
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
            match cursor.row {
                RowPos::Header => m_guard
                    .found_header
                    .as_ref()
                    .and_then(|x| x.get_entry(cursor.column))
                    .map(FoundEntry::Header),
                RowPos::Row(n) => m_guard
                    .founds
                    .get(n)
                    .and_then(|x| x.get_entry(cursor.column))
                    .map(FoundEntry::Row),
            }
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
    found_header: Option<FoundHeader>,
    founds: SortedVec<FoundRow>,
    done: bool,
    should_terminate: bool,
    start: Instant,
    first_match_elapsed: Option<Duration>,
    elapsed: Option<Duration>,
}

impl FinderInternalState {
    pub fn init(
        config: Arc<csv::CsvConfig>,
        target: Regex,
        target_local_column_index: Option<usize>,
        sorter: Option<Arc<sort::Sorter>>,
        sort_order: SortOrder,
        columns_filter: Option<Arc<columns_filter::ColumnsFilter>>,
    ) -> Arc<Mutex<FinderInternalState>> {
        let internal = FinderInternalState {
            count: 0,
            found_header: None,
            founds: SortedVec::new(),
            done: false,
            should_terminate: false,
            start: Instant::now(),
            first_match_elapsed: None,
            elapsed: None,
        };

        let m_state = Arc::new(Mutex::new(internal));

        let _m = m_state.clone();
        let _filename = config.filename().to_owned();

        let _handle = thread::spawn(move || {
            let mut bg_reader = config.new_reader().unwrap();

            // search header
            let mut column_indices = vec![];
            if let Ok(header) = bg_reader.headers() {
                let mut local_column_index = 0;
                for (column_index, field) in header.iter().enumerate() {
                    if let Some(columns_filter) = &columns_filter
                        && !columns_filter.is_column_filtered(column_index)
                    {
                        continue;
                    }
                    if target.is_match(field) {
                        column_indices.push(local_column_index);
                    }
                    local_column_index += 1;
                }
            }
            if !column_indices.is_empty() {
                let found = FoundHeader { column_indices };
                let mut m = _m.lock().unwrap();
                m.found_header = Some(found);
            }

            // note that records() excludes header
            let records = bg_reader.records();

            for (row_index, r) in records.enumerate() {
                let mut column_indices = vec![];
                if let Ok(valid_record) = r {
                    let mut local_column_index = 0;
                    for (column_index, field) in valid_record.iter().enumerate() {
                        if let Some(columns_filter) = &columns_filter
                            && !columns_filter.is_column_filtered(column_index)
                        {
                            continue;
                        }
                        let should_check_regex =
                            if let Some(target_local_column_index) = target_local_column_index {
                                local_column_index == target_local_column_index
                            } else {
                                true
                            };
                        if should_check_regex && target.is_match(field) {
                            column_indices.push(local_column_index);
                        }
                        local_column_index += 1;
                    }
                }
                if !column_indices.is_empty() {
                    let row_order = match &sorter {
                        Some(s) => {
                            s.get_record_order(row_index as u64, sort_order).unwrap() as usize
                        }
                        _ => row_index,
                    };
                    let found = FoundRow {
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
            m.elapsed = Some(m.start.elapsed());
        });

        m_state
    }

    fn found_one(&mut self, found: FoundRow) {
        if self.first_match_elapsed.is_none() {
            self.first_match_elapsed = Some(self.start.elapsed());
        }
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
        if next > 0 { next - 1 } else { next }
    }

    fn terminate(&mut self) {
        self.should_terminate = true;
    }

    fn elapsed(&self) -> Option<Duration> {
        self.elapsed
    }
}
