use crate::csv::{CsvLensReader, Row};
use crate::find;
use crate::input::Control;
use crate::sort::Sorter;

use anyhow::Result;
use regex::Regex;
use std::cmp::min;
use std::sync::Arc;
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

#[derive(Debug)]
pub struct ColumnsFilter {
    pattern: Regex,
    indices: Vec<usize>,
    filtered_headers: Vec<String>,
    num_columns_before_filter: usize,
    disabled_because_no_match: bool,
}

impl ColumnsFilter {
    fn new(pattern: Regex, headers: &[String]) -> Self {
        let mut indices = vec![];
        let mut filtered_headers: Vec<String> = vec![];
        for (i, header) in headers.iter().enumerate() {
            if pattern.is_match(header) {
                indices.push(i);
                filtered_headers.push(header.clone());
            }
        }
        let disabled_because_no_match;
        if indices.is_empty() {
            indices = (0..headers.len()).collect();
            filtered_headers = headers.into();
            disabled_because_no_match = true;
        } else {
            disabled_because_no_match = false;
        }
        Self {
            pattern,
            indices,
            filtered_headers,
            num_columns_before_filter: headers.len(),
            disabled_because_no_match,
        }
    }

    fn filtered_headers(&self) -> &Vec<String> {
        &self.filtered_headers
    }

    fn indices(&self) -> &Vec<usize> {
        &self.indices
    }

    pub fn pattern(&self) -> Regex {
        self.pattern.to_owned()
    }

    pub fn num_filtered(&self) -> usize {
        self.indices.len()
    }

    pub fn num_original(&self) -> usize {
        self.num_columns_before_filter
    }

    pub fn disabled_because_no_match(&self) -> bool {
        self.disabled_because_no_match
    }
}

#[derive(Clone)]
pub struct SelectionDimension {
    index: Option<u64>,
    pub bound: u64,
    last_selected: Option<u64>,
}

impl SelectionDimension {
    /// Create a new SelectionDimension
    pub fn new(index: Option<u64>, bound: u64) -> Self {
        Self {
            index,
            bound,
            last_selected: None,
        }
    }

    /// The currently selected index
    ///
    /// This index is dumb as in it is always between 0 and bound - 1 and
    /// has nothing to do with the actual record number in the data.
    pub fn index(&self) -> Option<u64> {
        self.index
    }

    /// Set selected to the given index and adjust it to be within bounds
    pub fn set_index(&mut self, index: u64) {
        self.index = Some(min(index, self.bound.saturating_sub(1)));
        self.last_selected = Some(index);
    }

    /// Unset the selected index
    pub fn unset_index(&mut self) {
        self.index = None;
    }

    /// Set the maximum allowed value for for index
    pub fn set_bound(&mut self, bound: u64) {
        self.bound = bound;
        if let Some(i) = self.index {
            self.set_index(i);
        }
    }

    /// Increase selected index by 1. Does nothing if nothing is currently selected.
    pub fn select_next(&mut self) {
        if let Some(i) = self.index() {
            self.set_index(i.saturating_add(1));
        };
    }

    /// Decrease selected index by 1. Does nothing if nothing is currently selected.
    pub fn select_previous(&mut self) {
        if let Some(i) = self.index() {
            self.set_index(i.saturating_sub(1));
        };
    }

    /// Select the first index. Does nothing if nothing is currently selected.
    pub fn select_first(&mut self) {
        if self.index.is_some() {
            self.set_index(0);
        }
    }

    /// Select the last index. Does nothing if nothing is currently selected.
    pub fn select_last(&mut self) {
        if self.index.is_some() {
            self.set_index(self.bound.saturating_sub(1))
        }
    }

    /// Whether the given index is currently selected
    pub fn is_selected(&self, i: usize) -> bool {
        if let Some(selected) = self.index {
            return selected == i as u64;
        }
        false
    }

    /// The last selected index even if the current selection is None
    pub fn last_selected(&self) -> Option<u64> {
        self.last_selected
    }
}

pub enum SelectionType {
    Row,
    Column,
    Cell,
    None,
}

#[derive(Clone)]
pub struct Selection {
    pub row: SelectionDimension,
    pub column: SelectionDimension,
}

impl Selection {
    pub fn default(row_bound: u64) -> Self {
        Selection {
            row: SelectionDimension::new(Some(0), row_bound),
            column: SelectionDimension::new(None, 0),
        }
    }

    pub fn selection_type(&self) -> SelectionType {
        if self.row.index.is_some() && self.column.index.is_some() {
            SelectionType::Cell
        } else if self.row.index.is_some() {
            SelectionType::Row
        } else if self.column.index.is_some() {
            SelectionType::Column
        } else {
            SelectionType::None
        }
    }

    fn set_selection_type(&mut self, selection_type: SelectionType) {
        let target_row_index = self.row.last_selected().unwrap_or(0);
        let target_column_index = self.column.last_selected().unwrap_or(0);

        match selection_type {
            SelectionType::Row => {
                self.row.set_index(target_row_index);
                self.column.unset_index();
            }
            SelectionType::Column => {
                self.row.unset_index();
                self.column.set_index(target_column_index);
            }
            SelectionType::Cell => {
                self.row.set_index(target_row_index);
                self.column.set_index(target_column_index);
            }
            SelectionType::None => {
                self.row.unset_index();
                self.column.unset_index();
            }
        }
    }

    pub fn toggle_selection_type(&mut self) {
        let selection_type = self.selection_type();
        match selection_type {
            SelectionType::Row => self.set_selection_type(SelectionType::Column),
            SelectionType::Column => self.set_selection_type(SelectionType::Cell),
            SelectionType::Cell => self.set_selection_type(SelectionType::Row), // for now don't allow toggling to None
            SelectionType::None => self.set_selection_type(SelectionType::Row),
        }
    }
}

#[derive(Debug)]
pub struct Header {
    pub name: String,
    pub origin_index: usize,
}

pub struct RowsView {
    reader: CsvLensReader,
    rows: Vec<Row>,
    headers: Vec<Header>,
    num_rows: u64,
    num_rows_rendered: u64,
    rows_from: u64,
    cols_offset: u64,
    filter: Option<RowsFilter>,
    columns_filter: Option<ColumnsFilter>,
    sorter: Option<Arc<Sorter>>,
    pub selection: Selection,
    elapsed: Option<u128>,
}

impl RowsView {
    pub fn new(mut reader: CsvLensReader, num_rows: u64) -> Result<RowsView> {
        let rows_from = 0;
        let rows = reader.get_rows(rows_from, num_rows)?;
        let headers = Self::get_default_headers_from_reader(&reader);
        let view = Self {
            reader,
            rows,
            headers,
            num_rows,
            num_rows_rendered: num_rows,
            rows_from,
            cols_offset: 0,
            filter: None,
            columns_filter: None,
            sorter: None,
            selection: Selection::default(num_rows),
            elapsed: None,
        };
        Ok(view)
    }

    pub fn headers(&self) -> &Vec<Header> {
        &self.headers
    }

    pub fn rows(&self) -> &Vec<Row> {
        &self.rows
    }

    pub fn get_cell_value(&self, column_name: &str) -> Option<String> {
        if let (Some(column_index), Some(row_index)) = (
            self.headers()
                .iter()
                .position(|header| header.name == column_name),
            self.selection.row.index(),
        ) {
            return self
                .rows()
                .get(row_index as usize)
                .and_then(|row| row.fields.get(column_index))
                .cloned();
        }
        None
    }

    /// Get the value of the cell at the current selection. Only returns a value
    /// if the selection type is Cell.
    pub fn get_cell_value_from_selection(&self) -> Option<String> {
        if let (Some(column_index), Some(row_index)) =
            (self.selection.column.index(), self.selection.row.index())
        {
            // Note: row_index and column_index are "local" index.
            return self
                .rows()
                .get(row_index as usize)
                .and_then(|row| {
                    row.fields
                        .get(column_index.saturating_add(self.cols_offset()) as usize)
                })
                .cloned();
        }
        None
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

    pub fn set_num_rows_rendered(&mut self, num_rows_rendered: u64) {
        self.num_rows_rendered = num_rows_rendered;
        // current selected might be out of range, reset it
        self.selection.row.set_bound(num_rows_rendered);
    }

    pub fn set_filter(&mut self, finder: &find::Finder) -> Result<()> {
        let filter = RowsFilter::new(finder, self.rows_from, self.num_rows);
        // only need to reload rows if the currently shown indices changed
        let mut needs_reload = true;
        if let Some(cur_filter) = &self.filter {
            if cur_filter.indices == filter.indices {
                needs_reload = false;
            }
        }
        // but always need to update filter because it holds other states such
        // as total count
        self.filter = Some(filter);
        if needs_reload {
            self.do_get_rows()
        } else {
            Ok(())
        }
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

    pub fn columns_filter(&self) -> Option<&ColumnsFilter> {
        self.columns_filter.as_ref()
    }

    pub fn set_columns_filter(&mut self, target: Regex) -> Result<()> {
        let _columns_filter = ColumnsFilter::new(target, &self.reader.headers);
        self.headers = _columns_filter
            .indices()
            .iter()
            .zip(_columns_filter.filtered_headers())
            .map(|(i, h)| Header {
                name: h.clone(),
                origin_index: *i,
            })
            .collect();
        self.columns_filter = Some(_columns_filter);
        self.do_get_rows()
    }

    pub fn reset_columns_filter(&mut self) -> Result<()> {
        self.columns_filter = None;
        self.headers = Self::get_default_headers_from_reader(&self.reader);
        self.do_get_rows()
    }

    pub fn get_column_origin_index(&self, column_index: usize) -> usize {
        self.headers[column_index].origin_index
    }

    pub fn get_column_name_global(&self, global_column_index: usize) -> String {
        self.reader
            .headers
            .get(global_column_index)
            .cloned()
            .unwrap_or_default()
    }

    fn get_default_headers_from_reader(reader: &CsvLensReader) -> Vec<Header> {
        reader
            .headers
            .iter()
            .enumerate()
            .map(|(i, h)| Header {
                name: h.clone(),
                origin_index: i,
            })
            .collect::<Vec<_>>()
    }

    pub fn sorter(&self) -> &Option<Arc<Sorter>> {
        &self.sorter
    }

    pub fn set_sorter(&mut self, sorter: &Arc<Sorter>) -> Result<()> {
        self.sorter = Some(sorter.clone());
        self.do_get_rows()
    }

    pub fn reset_sorter(&mut self) -> Result<()> {
        self.sorter = None;
        self.do_get_rows()
    }

    pub fn rows_from(&self) -> u64 {
        self.rows_from
    }

    pub fn set_rows_from(&mut self, rows_from_: u64) -> Result<()> {
        let rows_from = if let Some(n) = self.bottom_rows_from() {
            min(rows_from_, n)
        } else {
            rows_from_
        };
        if rows_from == self.rows_from {
            return Ok(());
        }
        self.rows_from = rows_from;
        self.do_get_rows()?;
        Ok(())
    }

    /// Offset of the first column to show. All columns are still read into Row
    /// (per ColumnsFilter if any).
    pub fn cols_offset(&self) -> u64 {
        self.cols_offset
    }

    pub fn set_cols_offset(&mut self, cols_offset: u64) {
        self.cols_offset = min(cols_offset, self.headers().len() as u64);
    }

    pub fn selected_offset(&self) -> Option<u64> {
        self.selection
            .row
            .index()
            .map(|x| x.saturating_add(self.rows_from))
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
                if let Some(i) = self.selection.row.index() {
                    if i >= self.num_rows_rendered.saturating_sub(1) {
                        self.increase_rows_from(1)?;
                    } else {
                        self.selection.row.select_next();
                    }
                } else {
                    self.increase_rows_from(1)?;
                }
            }
            Control::ScrollHalfPageDown => {
                self.increase_rows_from(self.num_rows_rendered / 2)?;
                self.selection.row.select_first()
            }
            Control::ScrollPageDown => {
                self.increase_rows_from(self.num_rows_rendered)?;
                self.selection.row.select_first()
            }
            Control::ScrollUp => {
                if let Some(i) = self.selection.row.index() {
                    if i == 0 {
                        self.decrease_rows_from(1)?;
                    } else {
                        self.selection.row.select_previous();
                    }
                } else {
                    self.decrease_rows_from(1)?;
                }
            }
            Control::ScrollHalfPageUp => {
                self.decrease_rows_from(self.num_rows_rendered / 2)?;
                self.selection.row.select_first()
            }
            Control::ScrollPageUp => {
                self.decrease_rows_from(self.num_rows_rendered)?;
                self.selection.row.select_first()
            }
            Control::ScrollTop => {
                self.set_rows_from(0)?;
                self.selection.row.select_first()
            }
            Control::ScrollBottom => {
                if let Some(total) = self.get_total() {
                    // Note: Using num_rows_rendered is not exactly correct, but it's simple and
                    // a bit better than num_rows. To be exact, this should use row heights to
                    // determine exactly how many rows to show from the bottom.
                    let rows_from = total.saturating_sub(self.num_rows_rendered as usize) as u64;
                    self.set_rows_from(rows_from)?;
                }
                self.selection.row.select_last()
            }
            Control::ScrollTo(n) => {
                let mut rows_from = n.saturating_sub(1) as u64;
                if let Some(n) = self.bottom_rows_from() {
                    rows_from = min(rows_from, n);
                }
                self.set_rows_from(rows_from)?;
                self.selection.row.select_first()
            }
            _ => {}
        }
        Ok(())
    }

    fn get_total(&self) -> Option<usize> {
        if let Some(filter) = &self.filter {
            return Some(filter.total);
        } else if let Some(n) = self
            .reader
            .get_total_line_numbers()
            .or_else(|| self.reader.get_total_line_numbers_approx())
        {
            return Some(n);
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
            return Some(n.saturating_sub(self.num_rows_rendered as usize) as u64);
        }
        None
    }

    fn subset_columns(rows: &Vec<Row>, indices: &[usize]) -> Vec<Row> {
        let mut out = vec![];
        for row in rows {
            out.push(row.subset(indices));
        }
        out
    }

    fn do_get_rows(&mut self) -> Result<()> {
        let start = Instant::now();
        let mut rows = if let Some(filter) = &self.filter {
            let indices = &filter.indices;
            self.reader.get_rows_for_indices(indices)?
        } else if let Some(sorter) = &self.sorter {
            if let Some(sorted_indices) = sorter.get_sorted_indices(self.rows_from, self.num_rows) {
                self.reader.get_rows_for_indices(&sorted_indices)?
            } else {
                self.reader.get_rows(self.rows_from, self.num_rows)?
            }
        } else {
            self.reader.get_rows(self.rows_from, self.num_rows)?
        };
        let elapsed = start.elapsed().as_micros();
        if let Some(columns_filter) = &self.columns_filter {
            rows = Self::subset_columns(&rows, columns_filter.indices());
        }
        self.rows = rows;
        self.elapsed = Some(elapsed);
        // current selected might be out of range, reset it
        // self.selection.row.set_bound(self.rows.len() as u64);
        Ok(())
    }
}
