use crate::csv::{CsvLensReader, Row};
use crate::find;
use crate::input::Control;

use anyhow::Result;
use std::cmp::min;
use std::time::Instant;

struct RowsFilter {
    indices: Vec<u64>,
    total: usize,
}

impl RowsFilter {
    fn new(finder: &find::Finder, rows_from: u64, num_rows: u64) -> RowsFilter {
        let total = finder.count();
        let indices = finder.get_subset_found(rows_from as usize, num_rows as usize);
        RowsFilter { indices, total }
    }
}

pub struct RowsView {
    reader: CsvLensReader,
    headers: Vec<String>,
    rows: Vec<Row>,
    num_rows: u64,
    rows_from: u64,
    filter: Option<RowsFilter>,
    selected: Option<u64>,
    elapsed: Option<u128>,
}

impl RowsView {
    pub fn new(mut reader: CsvLensReader, num_rows: u64) -> Result<RowsView> {
        let rows_from = 0;
        let rows = reader.get_rows(rows_from, num_rows)?;
        let headers = reader.headers.clone();
        let view = Self {
            reader,
            headers,
            rows,
            num_rows,
            rows_from,
            filter: None,
            selected: Some(0),
            elapsed: None,
        };
        Ok(view)
    }

    pub fn headers(&self) -> &Vec<String> {
        &self.headers
    }

    pub fn rows(&self) -> &Vec<Row> {
        &self.rows
    }

    pub fn num_rows(&self) -> u64 {
        self.num_rows
    }

    pub fn set_num_rows(&mut self, num_rows: u64) -> Result<()> {
        if num_rows == self.num_rows {
            return Ok(());
        }
        self.num_rows = num_rows;
        self.do_get_rows()?;
        Ok(())
    }

    pub fn set_filter(&mut self, finder: &find::Finder) -> Result<()> {
        let filter = RowsFilter::new(finder, self.rows_from, self.num_rows);
        if let Some(cur_filter) = &self.filter {
            if cur_filter.indices == filter.indices {
                return Ok(());
            }
        }
        self.filter = Some(filter);
        self.do_get_rows()
    }

    pub fn is_filter(&self) -> bool {
        self.filter.is_some()
    }

    pub fn reset_filter(&mut self) -> Result<()> {
        if !self.is_filter() {
            return Ok(());
        }
        self.filter = None;
        self.do_get_rows()
    }

    pub fn rows_from(&self) -> u64 {
        self.rows_from
    }

    pub fn set_rows_from(&mut self, rows_from_: u64) -> Result<()> {
        let rows_from;
        if let Some(n) = self.bottom_rows_from() {
            rows_from = min(rows_from_, n);
        } else {
            rows_from = rows_from_;
        }
        if rows_from == self.rows_from {
            return Ok(());
        }
        self.rows_from = rows_from;
        self.do_get_rows()?;
        Ok(())
    }

    pub fn set_selected(&mut self, selected: u64) {
        let selected = min(selected, (self.rows.len() as u64).saturating_sub(1));
        self.selected = Some(selected);
    }

    #[allow(dead_code)]
    pub fn reset_selected(&mut self) {
        self.selected = None;
    }

    pub fn increase_selected(&mut self) {
        if let Some(i) = self.selected {
            self.set_selected(i.saturating_add(1));
        };
    }

    pub fn decrease_selected(&mut self) {
        if let Some(i) = self.selected {
            self.set_selected(i.saturating_sub(1));
        }
    }

    pub fn select_top(&mut self) {
        self.set_selected(0);
    }

    pub fn select_bottom(&mut self) {
        self.set_selected((self.rows.len() as u64).saturating_sub(1))
    }

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    pub fn selected_offset(&self) -> Option<u64> {
        self.selected.map(|x| x.saturating_add(self.rows_from))
    }

    pub fn elapsed(&self) -> Option<u128> {
        self.elapsed
    }

    pub fn get_total_line_numbers(&self) -> Option<usize> {
        self.reader.get_total_line_numbers()
    }

    pub fn get_total_line_numbers_approx(&self) -> Option<usize> {
        self.reader.get_total_line_numbers_approx()
    }

    pub fn in_view(&self, row_index: u64) -> bool {
        let last_row = self.rows_from().saturating_add(self.num_rows());
        if row_index >= self.rows_from() && row_index < last_row {
            return true;
        }
        false
    }

    pub fn handle_control(&mut self, control: &Control) -> Result<()> {
        match control {
            Control::ScrollDown => {
                if let Some(i) = self.selected {
                    if i == self.num_rows - 1 {
                        self.increase_rows_from(1)?;
                    } else {
                        self.increase_selected();
                    }
                } else {
                    self.increase_rows_from(1)?;
                }
            }
            Control::ScrollPageDown => {
                self.increase_rows_from(self.num_rows)?;
                if self.selected.is_some() {
                    self.select_top()
                }
            }
            Control::ScrollUp => {
                if let Some(i) = self.selected {
                    if i == 0 {
                        self.decrease_rows_from(1)?;
                    } else {
                        self.decrease_selected();
                    }
                } else {
                    self.decrease_rows_from(1)?;
                }
            }
            Control::ScrollPageUp => {
                self.decrease_rows_from(self.num_rows)?;
                if self.selected.is_some() {
                    self.select_top()
                }
            }
            Control::ScrollBottom => {
                if let Some(total) = self.get_total() {
                    let rows_from = total.saturating_sub(self.num_rows as usize) as u64;
                    self.set_rows_from(rows_from)?;
                }
                if self.selected.is_some() {
                    self.select_bottom()
                }
            }
            Control::ScrollTo(n) => {
                let mut rows_from = n.saturating_sub(1) as u64;
                if let Some(n) = self.bottom_rows_from() {
                    rows_from = min(rows_from, n);
                }
                self.set_rows_from(rows_from)?;
                if self.selected.is_some() {
                    self.select_top()
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn get_total(&self) -> Option<usize> {
        if let Some(filter) = &self.filter {
            return Some(filter.total);
        } else {
            if let Some(n) = self
                .reader
                .get_total_line_numbers()
                .or_else(|| self.reader.get_total_line_numbers_approx())
            {
                return Some(n);
            }
        }
        None
    }

    fn increase_rows_from(&mut self, delta: u64) -> Result<()> {
        let new_rows_from = self.rows_from.saturating_add(delta);
        self.set_rows_from(new_rows_from)?;
        Ok(())
    }

    fn decrease_rows_from(&mut self, delta: u64) -> Result<()> {
        let new_rows_from = self.rows_from.saturating_sub(delta);
        self.set_rows_from(new_rows_from)?;
        Ok(())
    }

    fn bottom_rows_from(&self) -> Option<u64> {
        // fix type conversion craziness
        if let Some(n) = self.get_total() {
            return Some(n.saturating_sub(self.num_rows as usize) as u64);
        }
        None
    }

    fn do_get_rows(&mut self) -> Result<()> {
        let start = Instant::now();
        let rows;
        if let Some(filter) = &self.filter {
            let indices = &filter.indices;
            rows = self.reader.get_rows_for_indices(indices)?;
        } else {
            rows = self.reader.get_rows(self.rows_from, self.num_rows)?;
        }
        let elapsed = start.elapsed().as_micros();
        self.rows = rows;
        self.elapsed = Some(elapsed);
        // current selected might be out of range, reset it
        if let Some(i) = self.selected {
            self.set_selected(i);
        }
        Ok(())
    }
}
