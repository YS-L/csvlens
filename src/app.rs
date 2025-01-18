extern crate csv_sniffer;

use crate::columns_filter::ColumnsFilter;
use crate::csv;
use crate::delimiter::{sniff_delimiter, Delimiter};
use crate::errors::{CsvlensError, CsvlensResult};
use crate::find;
use crate::help;
use crate::input::{Control, InputHandler};
use crate::sort::{self, SortOrder, SorterStatus};
use crate::ui::{CsvTable, CsvTableState, FilterColumnsState, FinderState};
use crate::view;

#[cfg(feature = "clipboard")]
use arboard::Clipboard;
use ratatui::backend::Backend;
use ratatui::prelude::Position;
use ratatui::{Frame, Terminal};

use anyhow::Result;
use regex::Regex;
use std::cmp::min;
use std::sync::Arc;
use std::time::Instant;

fn get_offsets_to_make_visible(
    found_record: &find::FoundEntry,
    rows_view: &view::RowsView,
    csv_table_state: &CsvTableState,
) -> (Option<u64>, Option<u64>) {
    // TODO: row_index() should probably be u64
    let new_rows_offset = if let find::FoundEntry::Row(entry) = found_record {
        if rows_view.in_view(entry.row_order() as u64) {
            None
        } else {
            Some(entry.row_order() as u64)
        }
    } else {
        None
    };

    let cols_offset = csv_table_state.cols_offset;
    let last_rendered_col = cols_offset.saturating_add(csv_table_state.num_cols_rendered);
    let column_index = match found_record {
        find::FoundEntry::Header(entry) => entry.column_index(),
        find::FoundEntry::Row(entry) => entry.column_index(),
    } as u64;
    let new_cols_offset = if column_index >= cols_offset && column_index < last_rendered_col {
        None
    } else {
        Some(column_index)
    };

    (new_rows_offset, new_cols_offset)
}

fn scroll_to_found_entry(
    found_entry: find::FoundEntry,
    rows_view: &mut view::RowsView,
    csv_table_state: &mut CsvTableState,
) {
    let (new_rows_offset, new_cols_offset) =
        get_offsets_to_make_visible(&found_entry, rows_view, csv_table_state);

    // csv_table_state.debug = format!("{:?} {:?}", new_rows_offset, new_cols_offset);
    // csv_table_state.debug = format!("{:?}", found_record);
    if let Some(rows_offset) = new_rows_offset {
        rows_view.set_rows_from(rows_offset).unwrap();
        csv_table_state.set_rows_offset(rows_offset);
    }

    if let Some(cols_offset) = new_cols_offset {
        rows_view.set_cols_offset(cols_offset);
        csv_table_state.set_cols_offset(cols_offset);
    }
}

/// Returns the offset of the first column that can be shown in the current frame, while keeping the
/// column corresopnding to right_most_cols_offset in view.
fn get_cols_offset_to_fill_frame_width(
    frame_width: u16,
    right_most_cols_offset: u64,
    csv_table_state: &CsvTableState,
) -> Option<u64> {
    let view_layout = csv_table_state.view_layout.as_ref();
    let mut total: u16 = 0;
    let mut new_cols_offset = None;
    let unusable_width = csv_table_state.line_number_and_spaces_width();
    if let Some(layout) = view_layout {
        for c in (0..right_most_cols_offset.saturating_add(1) as usize).rev() {
            let maybe_width = layout.column_widths.get(c);
            if let Some(w) = maybe_width {
                if total + w > frame_width.saturating_sub(unusable_width) {
                    break;
                }
                new_cols_offset = Some(c as u64);
                total += w;
            } else {
                break;
            }
        }
        new_cols_offset
    } else {
        None
    }
}

#[derive(Default)]
pub struct LineWrapState {
    pub enable_line_wrap: bool,
    pub is_word_wrap: bool,
}

impl LineWrapState {
    pub fn toggle(&mut self, is_word_wrap: bool) {
        // Just switch between line wrap and word wrap if already enabled
        if self.enable_line_wrap && self.is_word_wrap != is_word_wrap {
            self.is_word_wrap = is_word_wrap;
            return;
        }

        // Toggle enabling or disabling wrapping
        self.enable_line_wrap = !self.enable_line_wrap;
        self.is_word_wrap = is_word_wrap
    }

    pub fn transient_message(&self) -> String {
        if self.enable_line_wrap {
            if self.is_word_wrap {
                "Word wrap enabled".to_string()
            } else {
                "Line wrap enabled".to_string()
            }
        } else {
            "Line wrap disabled".to_string()
        }
    }
}

pub struct App {
    input_handler: InputHandler,
    num_rows_not_visible: u16,
    shared_config: Arc<csv::CsvConfig>,
    rows_view: view::RowsView,
    columns_filter: Option<Arc<ColumnsFilter>>,
    csv_table_state: CsvTableState,
    finder: Option<find::Finder>,
    first_found_scrolled: bool,
    frame_width: Option<u16>,
    transient_message: Option<String>,
    show_stats: bool,
    echo_column: Option<String>,
    ignore_case: bool,
    help_page_state: help::HelpPageState,
    sorter: Option<Arc<sort::Sorter>>,
    sort_order: SortOrder,
    line_wrap_state: LineWrapState,
    #[cfg(feature = "clipboard")]
    clipboard: Result<Clipboard>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        filename: &str,
        delimiter: Delimiter,
        original_filename: Option<String>,
        show_stats: bool,
        echo_column: Option<String>,
        ignore_case: bool,
        no_headers: bool,
        columns_regex: Option<String>,
        filter_regex: Option<String>,
        find_regex: Option<String>,
    ) -> CsvlensResult<Self> {
        let input_handler = InputHandler::new();

        // Some lines are reserved for plotting headers (3 lines for headers + 2 lines for status bar)
        let num_rows_not_visible: u16 = 5;

        // Number of rows that are visible in the current frame
        let num_rows = 50 - num_rows_not_visible;

        let delimiter = match delimiter {
            Delimiter::Default => b',',
            Delimiter::Tab => b'\t',
            Delimiter::Character(d) => d,
            Delimiter::Auto => sniff_delimiter(filename).unwrap_or(b','),
        };
        let config = csv::CsvConfig::new(filename, delimiter, no_headers);
        let shared_config = Arc::new(config);

        let csvlens_reader = csv::CsvLensReader::new(shared_config.clone())?;
        let rows_view = view::RowsView::new(csvlens_reader, num_rows as u64)?;

        if let Some(column_name) = &echo_column {
            if !rows_view.headers().iter().any(|h| h.name == *column_name) {
                return Err(CsvlensError::ColumnNameNotFound(column_name.clone()));
            }
        }

        let csv_table_state = CsvTableState::new(
            original_filename,
            rows_view.headers().len(),
            &echo_column,
            ignore_case,
        );

        let finder: Option<find::Finder> = None;
        let first_found_scrolled = false;
        let frame_width = None;

        let transient_message: Option<String> = None;
        let help_page_state = help::HelpPageState::new();

        #[cfg(feature = "clipboard")]
        let clipboard = match Clipboard::new() {
            Ok(clipboard) => Ok(clipboard),
            Err(e) => Err(anyhow::anyhow!(e)),
        };

        let mut app = App {
            input_handler,
            num_rows_not_visible,
            shared_config,
            rows_view,
            columns_filter: None,
            csv_table_state,
            finder,
            first_found_scrolled,
            frame_width,
            transient_message,
            show_stats,
            echo_column,
            ignore_case,
            help_page_state,
            sorter: None,
            sort_order: SortOrder::Ascending,
            line_wrap_state: LineWrapState::default(),
            #[cfg(feature = "clipboard")]
            clipboard,
        };

        if let Some(pat) = &columns_regex {
            app.set_columns_filter(pat);
        }

        if let Some(pat) = &filter_regex {
            app.handle_find_or_filter(pat, true, false);
        } else if let Some(pat) = &find_regex {
            app.handle_find_or_filter(pat, false, false);
        }

        app.rows_view.set_sort_order(app.sort_order)?;
        app.csv_table_state.debug_stats.show_stats(app.show_stats);

        Ok(app)
    }

    pub fn main_loop<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> CsvlensResult<Option<String>> {
        loop {
            let control = self.input_handler.next();
            if matches!(control, Control::Quit) {
                if self.help_page_state.is_active() {
                    self.help_page_state.deactivate();
                    self.input_handler.exit_help_mode();
                } else {
                    return Ok(None);
                }
            }
            if matches!(control, Control::Select) {
                if let Some(result) = self.get_selection() {
                    return Ok(Some(result));
                }
            }
            if matches!(control, Control::Help) {
                self.help_page_state.activate();
                self.input_handler.enter_help_mode();
            }
            self.step(&control)?;
            self.draw(terminal)?;
        }
    }

    fn step_help(&mut self, control: &Control) -> CsvlensResult<()> {
        match &control {
            Control::ScrollDown => {
                self.help_page_state.scroll_down();
            }
            Control::ScrollUp => {
                self.help_page_state.scroll_up();
            }
            _ => {}
        }
        Ok(())
    }

    fn step(&mut self, control: &Control) -> CsvlensResult<()> {
        if self.help_page_state.is_active() {
            return self.step_help(control);
        }

        // clear message without changing other states on any action
        if !matches!(control, Control::Nothing) {
            self.transient_message = None;
        }

        self.rows_view.handle_control(control)?;
        self.rows_view
            .selection
            .column
            .set_bound(self.csv_table_state.num_cols_rendered);

        match &control {
            Control::ScrollTo(_) => {
                self.csv_table_state.reset_buffer();
            }
            Control::ScrollLeft => {
                if let Some(i) = self.rows_view.selection.column.index() {
                    if i == 0 {
                        self.decrease_cols_offset();
                    } else {
                        self.rows_view.selection.column.select_previous();
                    }
                } else {
                    self.decrease_cols_offset();
                }
            }
            Control::ScrollRight => {
                if let Some(i) = self.rows_view.selection.column.index() {
                    if i == self.csv_table_state.num_cols_rendered - 1 {
                        self.increase_cols_offset();
                    } else {
                        self.rows_view.selection.column.select_next();
                    }
                } else {
                    self.increase_cols_offset();
                }
            }
            Control::ScrollPageLeft => {
                let new_cols_offset = match self.frame_width {
                    Some(frame_width) => get_cols_offset_to_fill_frame_width(
                        frame_width,
                        self.csv_table_state.cols_offset.saturating_sub(1),
                        &self.csv_table_state,
                    ),
                    _ => Some(0),
                };
                if let Some(new_cols_offset) = new_cols_offset {
                    self.rows_view.set_cols_offset(new_cols_offset);
                }
            }
            Control::ScrollPageRight => {
                if self.csv_table_state.has_more_cols_to_show() {
                    // num_cols_rendered includes the last truncated column
                    let mut new_cols_offset = self
                        .csv_table_state
                        .cols_offset
                        .saturating_add(self.csv_table_state.num_cols_rendered.saturating_sub(1));
                    new_cols_offset = min(
                        new_cols_offset,
                        self.rows_view.headers().len().saturating_sub(1) as u64,
                    );
                    if new_cols_offset != self.csv_table_state.cols_offset {
                        self.rows_view.set_cols_offset(new_cols_offset);
                    }
                }
            }
            Control::ScrollLeftMost => {
                self.rows_view.set_cols_offset(0);
            }
            Control::ScrollRightMost => {
                if self.csv_table_state.has_more_cols_to_show() {
                    let new_cols_offset = match self.frame_width {
                        Some(frame_width) => get_cols_offset_to_fill_frame_width(
                            frame_width,
                            self.rows_view.headers().len().saturating_sub(1) as u64,
                            &self.csv_table_state,
                        ),
                        _ => Some(0),
                    };
                    if let Some(new_cols_offset) = new_cols_offset {
                        self.rows_view.set_cols_offset(new_cols_offset);
                    }
                }
            }
            Control::ScrollToNextFound if !self.rows_view.is_filter() => {
                if let Some(fdr) = self.finder.as_mut() {
                    if let Some(found_entry) = fdr.next() {
                        scroll_to_found_entry(
                            found_entry,
                            &mut self.rows_view,
                            &mut self.csv_table_state,
                        );
                    }
                }
            }
            Control::ScrollToPrevFound if !self.rows_view.is_filter() => {
                if let Some(fdr) = self.finder.as_mut() {
                    if let Some(found_entry) = fdr.prev() {
                        scroll_to_found_entry(
                            found_entry,
                            &mut self.rows_view,
                            &mut self.csv_table_state,
                        );
                    }
                }
            }
            Control::Find(s) | Control::Filter(s) => {
                self.handle_find_or_filter(s, matches!(control, Control::Filter(_)), false);
            }
            Control::FindLikeCell | Control::FilterLikeCell => {
                if let Some(value) = self.rows_view.get_cell_value_from_selection() {
                    self.handle_find_or_filter(
                        value.as_str(),
                        matches!(control, Control::FilterLikeCell),
                        true,
                    );
                } else {
                    self.transient_message.replace(
                        "Select a cell first before finding (#) or filtering (@) rows like it"
                            .to_string(),
                    );
                }
            }
            Control::FilterColumns(pat) => {
                self.set_columns_filter(pat);
            }
            Control::BufferContent(input) => {
                self.csv_table_state
                    .set_buffer(self.input_handler.mode(), input.clone());
            }
            Control::BufferReset => {
                self.csv_table_state.reset_buffer();
                self.reset_filter();
                self.reset_columns_filter();
            }
            Control::ToggleSelectionType => {
                self.rows_view.selection.toggle_selection_type();
            }
            Control::ToggleLineWrap(word_wrap) => {
                self.csv_table_state.reset_buffer();
                self.line_wrap_state.toggle(*word_wrap);
                self.csv_table_state.enable_line_wrap = self.line_wrap_state.enable_line_wrap;
                self.csv_table_state.is_word_wrap = self.line_wrap_state.is_word_wrap;
                self.transient_message
                    .replace(self.line_wrap_state.transient_message());
            }
            Control::ToggleSort => {
                if let Some(selected_column_index) = self.get_global_selected_column_index() {
                    let mut should_create_new_sorter = false;
                    if let Some(column_index) = self.sorter.as_ref().map(|s| s.column_index) {
                        if selected_column_index as usize != column_index {
                            should_create_new_sorter = true;
                        } else {
                            match self.sort_order {
                                SortOrder::Ascending => {
                                    self.sort_order = SortOrder::Descending;
                                }
                                SortOrder::Descending => {
                                    self.sort_order = SortOrder::Ascending;
                                }
                            }
                            self.rows_view.set_sort_order(self.sort_order)?;
                        }
                    } else {
                        should_create_new_sorter = true;
                    }
                    if should_create_new_sorter {
                        let column_name = self
                            .rows_view
                            .get_column_name_from_global_index(selected_column_index as usize);
                        let _sorter = sort::Sorter::new(
                            self.shared_config.clone(),
                            selected_column_index as usize,
                            column_name,
                        );
                        self.sorter = Some(Arc::new(_sorter));
                    }
                } else {
                    self.transient_message
                        .replace("Press TAB and select a column before sorting".to_string());
                }
            }
            Control::IncreaseWidth => {
                self.adjust_column_width(4);
            }
            Control::DecreaseWidth => {
                self.adjust_column_width(-4);
            }
            #[cfg(feature = "clipboard")]
            Control::CopySelection => {
                if let Some(selected) = self.rows_view.get_cell_value_from_selection() {
                    match self.clipboard.as_mut().map(|c| c.set_text(&selected)) {
                        Ok(_) => self
                            .transient_message
                            .replace(format!("Copied {} to clipboard", selected.as_str())),
                        Err(e) => self
                            .transient_message
                            .replace(format!("Failed to copy to clipboard: {e}")),
                    };
                } else if let Some((index, row)) = self.rows_view.get_row_value() {
                    match self.clipboard.as_mut().map(|c| c.set_text(&row)) {
                        Ok(_) => self
                            .transient_message
                            .replace(format!("Copied row {} to clipboard", index)),
                        Err(e) => self
                            .transient_message
                            .replace(format!("Failed to copy to clipboard: {e}")),
                    };
                }
            }
            Control::Reset => {
                self.csv_table_state.column_width_overrides.reset();
                self.reset_filter();
                self.reset_columns_filter();
                self.reset_sorter();
            }
            Control::UnknownOption(s) => {
                self.csv_table_state.reset_buffer();
                self.transient_message
                    .replace(format!("Unknown option: {s}"));
            }
            _ => {}
        }

        if let Some(sorter) = &self.sorter {
            // Update rows_view sorter if outdated
            let mut should_set_rows_view_sorter = false;
            if sorter.status() == SorterStatus::Finished {
                if let Some(rows_view_sorter) = self.rows_view.sorter() {
                    // Sorter can be reused by rows view even if sort order is different.
                    if rows_view_sorter.column_index != sorter.column_index {
                        should_set_rows_view_sorter = true;
                    }
                } else {
                    should_set_rows_view_sorter = true;
                }
            }
            if should_set_rows_view_sorter {
                self.rows_view.set_sorter(sorter).unwrap();
            }

            // Update finder if sorter outdated
            let mut should_create_new_finder = false;
            if sorter.status() == SorterStatus::Finished {
                if let Some(finder) = &self.finder {
                    if let Some(finder_sorter) = finder.sorter() {
                        // Internal state of finder needs to be rebuilt if sorter is different,
                        // including sort order.
                        if finder_sorter.column_index != sorter.column_index
                            || finder.sort_order != self.sort_order
                        {
                            should_create_new_finder = true;
                        }
                    } else {
                        should_create_new_finder = true;
                    }
                }
            }
            if should_create_new_finder {
                let target = self.finder.as_ref().unwrap().target();
                let sorter = self.sorter.clone();
                if let Some(finder) = &self.finder {
                    // Inherit previous finder's column index if any, instead of using the current
                    // selected column intended for sorter
                    self.create_finder_with_column_index(
                        target,
                        self.rows_view.is_filter(),
                        finder.column_index(),
                        sorter,
                    );
                } else {
                    self.create_finder(target, self.rows_view.is_filter(), sorter);
                }
            }
        }

        if let Some(fdr) = self.finder.as_mut() {
            if !self.rows_view.is_filter() {
                // scroll to first result once ready
                if !self.first_found_scrolled && fdr.found_any() {
                    if let Some(found_entry) = fdr.next() {
                        scroll_to_found_entry(
                            found_entry,
                            &mut self.rows_view,
                            &mut self.csv_table_state,
                        );
                    }
                    self.first_found_scrolled = true;
                } else if self.first_found_scrolled {
                    // Conditioned on first_found_scrolled to retain the initial Header row hint,
                    // i.e. matches in the header row will be highlighted first

                    // reset cursor if out of view
                    if let Some(cursor_row_order) = fdr.cursor_row_order() {
                        if !self.rows_view.in_view(cursor_row_order as u64) {
                            fdr.reset_cursor();
                        }
                    }

                    fdr.set_row_hint(find::RowPos::Row(self.rows_view.rows_from() as usize));
                }
            } else {
                self.rows_view.set_filter(fdr).unwrap();
            }
        }

        // update rows and elapsed time if there are new results
        self.csv_table_state
            .debug_stats
            .rows_view_perf(self.rows_view.perf_stats());
        if let Some(fdr) = &self.finder {
            self.csv_table_state
                .debug_stats
                .finder_elapsed(fdr.elapsed());
        } else {
            self.csv_table_state.debug_stats.finder_elapsed(None);
        }

        // TODO: is this update too late?
        self.csv_table_state
            .set_rows_offset(self.rows_view.rows_from());
        self.csv_table_state
            .set_cols_offset(self.rows_view.cols_offset());
        self.csv_table_state.selection = Some(self.rows_view.selection.clone());

        if let Some(n) = self.rows_view.get_total_line_numbers() {
            self.csv_table_state.set_total_line_number(n, false);
        } else if let Some(n) = self.rows_view.get_total_line_numbers_approx() {
            self.csv_table_state.set_total_line_number(n, true);
        }
        self.csv_table_state
            .set_total_cols(self.rows_view.headers().len());

        if let Some(f) = &self.finder {
            // TODO: need to create a new finder every time?
            self.csv_table_state.finder_state = FinderState::from_finder(f, &self.rows_view);
        }
        self.csv_table_state.filter_columns_state =
            FilterColumnsState::from_rows_view(&self.rows_view);

        self.csv_table_state
            .update_sorter(&self.sorter, self.sort_order);

        self.csv_table_state
            .transient_message
            .clone_from(&self.transient_message);

        // if let Some(finder) = &self.finder {
        //     self.csv_table_state.debug = format!("cursor: {:?}", finder.cursor);
        // }

        Ok(())
    }

    fn get_selection(&self) -> Option<String> {
        if let Some(result) = self.rows_view.get_cell_value_from_selection() {
            return Some(result);
        } else if let Some(column_name) = &self.echo_column {
            if let Some(result) = self.rows_view.get_cell_value(column_name) {
                return Some(result);
            }
        };
        None
    }

    fn create_finder(&mut self, target: Regex, is_filter: bool, sorter: Option<Arc<sort::Sorter>>) {
        self.create_finder_with_column_index(
            target,
            is_filter,
            self.get_selected_column_index().map(|x| x as usize),
            sorter,
        );
    }

    fn create_finder_with_column_index(
        &mut self,
        target: Regex,
        is_filter: bool,
        column_index: Option<usize>,
        sorter: Option<Arc<sort::Sorter>>,
    ) {
        let _finder = find::Finder::new(
            self.shared_config.clone(),
            target,
            column_index,
            sorter,
            self.sort_order,
            self.columns_filter.clone(),
        )
        .unwrap();
        self.finder = Some(_finder);
        if is_filter {
            self.rows_view.set_rows_from(0).unwrap();
            self.rows_view
                .set_filter(self.finder.as_ref().unwrap())
                .unwrap();
        } else {
            // will scroll to first result below once ready
            self.first_found_scrolled = false;
            self.rows_view.reset_filter().unwrap();
        }
    }

    fn create_regex(&mut self, s: &str, escape: bool) -> std::result::Result<Regex, regex::Error> {
        let s = if escape {
            format!("^{}$", regex::escape(s))
        } else {
            s.to_string()
        };
        let lower_s = s.to_lowercase();
        if self.ignore_case && lower_s.starts_with(s.as_str()) {
            Regex::new(&format!("(?i){}", s))
        } else {
            Regex::new(s.as_str())
        }
    }

    fn set_columns_filter(&mut self, pat: &str) {
        let re = self.create_regex(pat, false);
        if let Ok(target) = re {
            let columns_filter = Arc::new(ColumnsFilter::new(target, self.rows_view.raw_headers()));
            self.columns_filter = Some(columns_filter.clone());
            self.rows_view.set_columns_filter(&columns_filter).unwrap();
        } else {
            self.reset_columns_filter();
            self.transient_message = Some(format!("Invalid regex: {pat}"));
        }
        self.csv_table_state.reset_buffer();
        self.csv_table_state.set_cols_offset(0);
    }

    fn reset_columns_filter(&mut self) {
        self.columns_filter = None;
        self.rows_view.reset_columns_filter().unwrap();
    }

    fn handle_find_or_filter(&mut self, pat: &str, is_filter: bool, escape: bool) {
        let re = self.create_regex(pat, escape);
        if let Ok(target) = re {
            let _sorter = if let Some(s) = &self.sorter {
                if s.status() == SorterStatus::Finished {
                    Some(s.clone())
                } else {
                    None
                }
            } else {
                None
            };
            self.create_finder(target, is_filter, _sorter);
        } else {
            self.finder = None;
            // TODO: how to show multi-line error
            self.transient_message = Some(format!("Invalid regex: {pat}"));
        }
        self.csv_table_state.reset_buffer();
    }

    fn increase_cols_offset(&mut self) {
        if self.csv_table_state.has_more_cols_to_show() {
            let new_cols_offset = self.rows_view.cols_offset().saturating_add(1);
            self.rows_view.set_cols_offset(new_cols_offset);
        }
    }

    fn decrease_cols_offset(&mut self) {
        let new_cols_offset = self.rows_view.cols_offset().saturating_sub(1);
        self.rows_view.set_cols_offset(new_cols_offset);
    }

    fn adjust_column_width(&mut self, delta: i16) {
        if let Some(column_index) = self.get_selected_column_index() {
            if let Some(view_layout) = &mut self.csv_table_state.view_layout {
                let current_width = view_layout.column_widths[column_index as usize];
                let new_width = (current_width as i16).saturating_add(delta);

                if new_width > 0 {
                    let origin_index = self
                        .rows_view
                        .get_column_origin_index(column_index as usize);
                    self.csv_table_state
                        .column_width_overrides
                        .set(origin_index, new_width as u16);
                }
            }
        }
    }

    fn get_selected_column_index(&self) -> Option<u64> {
        // local index as in local to the view port
        if let Some(local_column_index) = self.rows_view.selection.column.index() {
            return Some(local_column_index.saturating_add(self.csv_table_state.cols_offset));
        }
        None
    }

    fn get_global_selected_column_index(&self) -> Option<u64> {
        // TODO: maybe this and above should be methods provided by RowsView directly?
        self.get_selected_column_index()
            .map(|local_index| self.rows_view.get_column_origin_index(local_index as usize) as u64)
    }

    fn reset_filter(&mut self) {
        if self.finder.is_some() {
            self.finder = None;
            self.csv_table_state.finder_state = FinderState::FinderInactive;
            self.rows_view.reset_filter().unwrap();
        }
    }

    fn reset_sorter(&mut self) {
        // TODO: consolidate rows_view reset
        self.sorter = None;
        self.rows_view.reset_sorter().unwrap();
    }

    fn render_frame(&mut self, f: &mut Frame) {
        let size = f.area();

        // Render help; if so exit early.
        if self.help_page_state.is_active() {
            f.render_stateful_widget(help::HelpPage::new(), size, &mut self.help_page_state);
            return;
        }

        // Render table
        // TODO: check type of num_rows too big?
        let num_rows_adjusted = size.height.saturating_sub(self.num_rows_not_visible) as u64;
        if let Some(view_layout) = &self.csv_table_state.view_layout {
            self.rows_view.set_num_rows_rendered(
                view_layout.num_rows_renderable(num_rows_adjusted as u16) as u64,
            );
        }
        self.rows_view.set_num_rows(num_rows_adjusted).unwrap();
        self.frame_width = Some(size.width);

        let rows = self.rows_view.rows();
        let csv_table = CsvTable::new(self.rows_view.headers(), rows);
        f.render_stateful_widget(csv_table, size, &mut self.csv_table_state);
        if let Some((x, y)) = self.csv_table_state.cursor_xy {
            f.set_cursor_position(Position::new(x, y));
        }
    }

    fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> CsvlensResult<()> {
        let start = Instant::now();
        terminal.draw(|f| {
            self.render_frame(f);
        })?;
        self.csv_table_state
            .debug_stats
            .render_elapsed(Some(start.elapsed()));
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    struct AppBuilder {
        filename: String,
        delimiter: Delimiter,
        original_filename: Option<String>,
        show_stats: bool,
        echo_column: Option<String>,
        ignore_case: bool,
        no_headers: bool,
        columns_regex: Option<String>,
        filter_regex: Option<String>,
        find_regex: Option<String>,
    }

    impl AppBuilder {
        fn new(filename: &str) -> Self {
            AppBuilder {
                filename: filename.to_owned(),
                delimiter: Delimiter::Default,
                original_filename: None,
                show_stats: false,
                echo_column: None,
                ignore_case: false,
                no_headers: false,
                columns_regex: None,
                filter_regex: None,
                find_regex: None,
            }
        }

        fn build(self) -> CsvlensResult<App> {
            App::new(
                self.filename.as_str(),
                self.delimiter,
                self.original_filename,
                self.show_stats,
                self.echo_column,
                self.ignore_case,
                self.no_headers,
                self.columns_regex,
                self.filter_regex,
                self.find_regex,
            )
        }

        fn delimiter(mut self, delimiter: Delimiter) -> Self {
            self.delimiter = delimiter;
            self
        }

        fn ignore_case(mut self, ignore_case: bool) -> Self {
            self.ignore_case = ignore_case;
            self
        }

        fn no_headers(mut self, no_headers: bool) -> Self {
            self.no_headers = no_headers;
            self
        }

        fn columns_regex(mut self, columns: Option<String>) -> Self {
            self.columns_regex = columns;
            self
        }

        fn filter_regex(mut self, filter: Option<String>) -> Self {
            self.filter_regex = filter;
            self
        }

        fn echo_column(mut self, column: &str) -> Self {
            self.echo_column = Some(column.to_owned());
            self
        }
    }

    fn to_lines(buf: &Buffer) -> Vec<String> {
        let mut symbols: String = "".to_owned();
        let area = buf.area();
        for y in 0..area.bottom() {
            for x in 0..area.right() {
                let symbol = buf[Position::new(x, y)].symbol();
                symbols.push_str(symbol);
            }
            if y != area.bottom() - 1 {
                symbols.push('\n');
            }
        }

        symbols.split('\n').map(|s| s.to_string()).collect()
    }

    fn step_and_draw<B: Backend>(app: &mut App, terminal: &mut Terminal<B>, control: Control) {
        app.step(&control).unwrap();

        // While it's possible to step multiple times before any draw when
        // testing, App::render_frame() can update App's state (e.g. based on
        // the current terminal frame size) with information that might be
        // required for stepping to work correctly. Also, immediately drawing
        // after each step is what App::main_loop() will be doing.
        terminal.draw(|f| app.render_frame(f)).unwrap();
    }

    fn till_app_ready(app: &App) {
        app.rows_view.wait_internal();
        if let Some(sorter) = &app.sorter {
            sorter.wait_internal();
        }
        if let Some(finder) = &app.finder {
            finder.wait_internal();
        }
    }

    #[test]
    fn test_simple() {
        let mut app = AppBuilder::new("tests/data/simple.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        for _ in 0..7 {
            step_and_draw(&mut app, &mut terminal, Control::ScrollDown);
        }

        // TODO: need a less clunky way of checking expected output, and include
        // checking of styles. Probably using automatically generated fixtures
        // that are readable and can be easily updated
        let expected = vec![
            "──────────────────────────────",
            "      a     b                 ",
            "───┬──────────────┬───────────",
            "4  │  A4    B4    │           ",
            "5  │  A5    B5    │           ",
            "6  │  A6    B6    │           ",
            "7  │  A7    B7    │           ",
            "8  │  A8    B8    │           ",
            "───┴──────────────┴───────────",
            "stdin [Row 8/5000, Col 1/2]   ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_scroll_horizontal() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "──────────────────────────────",
            "      La…    La…    La…    …  ",
            "───┬──────────────────────────",
            "1  │  41     5      59     …  ",
            "2  │  42     52     48     …  ",
            "3  │  46     35     59     …  ",
            "4  │  42     16     12     …  ",
            "5  │  43     37     48     …  ",
            "───┴──────────────────────────",
            "stdin [Row 1/128, Col 1/10]   ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ScrollPageRight);
        let expected = vec![
            "──────────────────────────────",
            "      NS    Lo…    Lo…    …   ",
            "───┬──────────────────────────",
            "1  │  N     80     39     …   ",
            "2  │  N     97     23     …   ",
            "3  │  N     120    30     …   ",
            "4  │  N     71     48     …   ",
            "5  │  N     89     46     …   ",
            "───┴──────────────────────────",
            "stdin [Row 1/128, Col 4/10]   ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ScrollPageLeft);
        let expected = vec![
            "──────────────────────────────",
            "      La…    La…    La…    …  ",
            "───┬──────────────────────────",
            "1  │  41     5      59     …  ",
            "2  │  42     52     48     …  ",
            "3  │  46     35     59     …  ",
            "4  │  42     16     12     …  ",
            "5  │  43     37     48     …  ",
            "───┴──────────────────────────",
            "stdin [Row 1/128, Col 1/10]   ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_filter_columns() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("Lon|City".into()),
        );
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "      LonD    LonM    LonS    City                                              ",
            "───┬─────────────────────────────────────────────┬──────────────────────────────",
            "1  │  80      39      0       Youngstown         │                              ",
            "2  │  97      23      23      Yankton            │                              ",
            "3  │  120     30      36      Yakima             │                              ",
            "4  │  71      48      0       Worcester          │                              ",
            "5  │  89      46      11      Wisconsin Dells    │                              ",
            "───┴─────────────────────────────────────────────┴──────────────────────────────",
            "stdin [Row 1/128, Col 1/4] [Filter \"Lon|City\": 4/10 cols]                       ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_filter_columns_irregular() {
        let mut app = AppBuilder::new("tests/data/irregular.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("COL2".into()),
        );
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "       COL2                                                                     ",
            "───┬───────────┬────────────────────────────────────────────────────────────────",
            "1  │           │                                                                ",
            "2  │   v2      │                                                                ",
            "   │           │                                                                ",
            "   │           │                                                                ",
            "   │           │                                                                ",
            "───┴───────────┴────────────────────────────────────────────────────────────────",
            "stdin [Row 1/2, Col 1/1] [Filter \"COL2\": 1/2 cols]                              ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_filter_columns_case_sensitive() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("city|state|wa".into()),
        );
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "      LatD    LatM    LatS    NS    LonD    LonM    LonS    EW    City          ",
            "───┬────────────────────────────────────────────────────────────────────────────",
            "1  │  41      5       59      N     80      39      0       W     Youngstown    ",
            "2  │  42      52      48      N     97      23      23            Yankton       ",
            "3  │  46      35      59      N     120     30      36      W     Yakima        ",
            "4  │  42      16      12      N     71      48      0       W     Worcester     ",
            "5  │  43      37      48      N     89      46      11      W     Wisconsin…    ",
            "───┴────────────────────────────────────────────────────────────────────────────",
            "stdin [Row 1/128, Col 1/10] [Filter \"city|state|wa\": no match, showing all colum",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_filter_columns_ignore_case() {
        let mut app = AppBuilder::new("tests/data/cities.csv")
            .ignore_case(true)
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("city|state|wa".into()),
        );
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "      City               State                                                  ",
            "───┬──────────────────────────────┬─────────────────────────────────────────────",
            "1  │  Youngstown         OH       │                                             ",
            "2  │  Yankton            SD       │                                             ",
            "3  │  Yakima             WA       │                                             ",
            "4  │  Worcester          MA       │                                             ",
            "5  │  Wisconsin Dells    WI       │                                             ",
            "───┴──────────────────────────────┴─────────────────────────────────────────────",
            "stdin [Row 1/128, Col 1/2] [Filter \"(?i)city|state|wa\": 2/10 cols] [ignore-case]",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_extra_fields_in_some_rows() {
        // Test getting column widths should not fail on data with bad formatting (some rows having
        // more fields than the header)
        let mut app = AppBuilder::new("tests/data/bad_double_quote.csv")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(35, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "───────────────────────────────────",
            "      Column1     \"column2\"        ",
            "───┬───────────────────────────┬───",
            "1  │  1           \"quote\"      │   ",
            "2  │  5           \"Comma       │   ",
            "   │                           │   ",
            "   │                           │   ",
            "   │                           │   ",
            "───┴───────────────────────────┴───",
            "stdin [Row 1/2, Col 1/2]           ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_extra_fields_right_most_border() {
        let mut app = AppBuilder::new("tests/data/bad_73.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(35, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "───────────────────────────────────",
            "      COL1     COL2                ",
            "───┬───────────────────┬───────────",
            "1  │  c1               │           ",
            "2  │  c2       v2      │           ",
            "3  │  c2       4       │           ",
            "4  │  c3               │           ",
            "5  │  c4               │           ",
            "───┴───────────────────┴───────────",
            "stdin [Row 1/13, Col 1/2]          ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_sniff_delimiter() {
        let mut app = AppBuilder::new("tests/data/small.bsv")
            .delimiter(Delimiter::Auto)
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "──────────────────────────────",
            "      COL1    COL2            ",
            "───┬──────────────────┬───────",
            "1  │  c1      v1      │       ",
            "2  │  c2      v2      │       ",
            "   │                  │       ",
            "   │                  │       ",
            "   │                  │       ",
            "───┴──────────────────┴───────",
            "stdin [Row 1/2, Col 1/2]      ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_multi_lines() {
        let mut app = AppBuilder::new("tests/data/multi_lines.csv")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(50, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b                      c               ",
            "───┬─────────────────────────────────────────────┬",
            "1  │  1    this is a very lon…    12345          │",
            "2  │  2    thi…                   678910         │",
            "3  │  3    normal text now        123,456,789    │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "───┴─────────────────────────────────────────────┴",
            "stdin [Row 1/3, Col 1/3]                          ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ToggleLineWrap(false));
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b                      c               ",
            "───┬─────────────────────────────────────────────┬",
            "1  │  1    this is a very long    12345          │",
            "   │        text that surely w                   │",
            "   │       ill not fit in your                   │",
            "   │        small screen                         │",
            "2  │  2    this                   678910         │",
            "   │       is                                    │",
            "   │       an                                    │",
            "   │       even                                  │",
            "   │       longer                                │",
            "   │       text                                  │",
            "   │       that                                  │",
            "   │       surely                                │",
            "   │       will                                  │",
            "   │       not                                   │",
            "   │       fit                                   │",
            "   │       in                                    │",
            "   │       your                                  │",
            "   │       small                                 │",
            "   │       screen                                │",
            "3  │  3    normal text now        123,456,789    │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "───┴─────────────────────────────────────────────┴",
            "Line wrap enabled                                 ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ToggleLineWrap(true));
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b                      c               ",
            "───┬─────────────────────────────────────────────┬",
            "1  │  1    this is a very         12345          │",
            "   │       long text that                        │",
            "   │       surely will not                       │",
            "   │       fit in your small                     │",
            "   │       screen                                │",
            "2  │  2    this                   678910         │",
            "   │       is                                    │",
            "   │       an                                    │",
            "   │       even                                  │",
            "   │       longer                                │",
            "   │       text                                  │",
            "   │       that                                  │",
            "   │       surely                                │",
            "   │       will                                  │",
            "   │       not                                   │",
            "   │       fit                                   │",
            "   │       in                                    │",
            "   │       your                                  │",
            "   │       small                                 │",
            "   │       screen                                │",
            "3  │  3    normal text now        123,456,789    │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "───┴─────────────────────────────────────────────┴",
            "Word wrap enabled                                 ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_multi_lines_degenerate_width() {
        let mut app = AppBuilder::new("tests/data/multi_lines.csv")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(50, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        step_and_draw(&mut app, &mut terminal, Control::ScrollRight);
        step_and_draw(&mut app, &mut terminal, Control::ToggleLineWrap(false));
        step_and_draw(&mut app, &mut terminal, Control::DecreaseWidth);
        step_and_draw(&mut app, &mut terminal, Control::DecreaseWidth);
        step_and_draw(&mut app, &mut terminal, Control::DecreaseWidth);
        step_and_draw(&mut app, &mut terminal, Control::DecreaseWidth);
        step_and_draw(&mut app, &mut terminal, Control::DecreaseWidth);

        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    …   c                                  ",
            "───┬──────────────────────────┬───────────────────",
            "1  │  1    …   12345          │                   ",
            "2  │  2    …   678910         │                   ",
            "3  │  3    …   123,456,789    │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "   │                          │                   ",
            "───┴──────────────────────────┴───────────────────",
            "stdin [Row 1/3, Col 1/3]                          ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_multiple_newlines() {
        let mut app = AppBuilder::new("tests/data/multiple_newlines.csv")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(50, 45);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::ToggleLineWrap(true));
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b                      c               ",
            "───┬─────────────────────────────────────────────┬",
            "1  │  1    this is a very         12345          │",
            "   │       long text that                        │",
            "   │       surely will not                       │",
            "   │       fit in your small                     │",
            "   │       screen                                │",
            "2  │  2    this                   678910         │",
            "   │                                             │",
            "   │       is                                    │",
            "   │                                             │",
            "   │       an                                    │",
            "   │                                             │",
            "   │       even                                  │",
            "   │                                             │",
            "   │       longer                                │",
            "   │                                             │",
            "   │       text                                  │",
            "   │                                             │",
            "   │       that                                  │",
            "   │                                             │",
            "   │       surely                                │",
            "   │                                             │",
            "   │       will                                  │",
            "   │                                             │",
            "   │       not                                   │",
            "   │                                             │",
            "   │       fit                                   │",
            "   │                                             │",
            "   │       in                                    │",
            "   │                                             │",
            "   │       your                                  │",
            "   │                                             │",
            "   │       small                                 │",
            "   │                                             │",
            "   │       screen                                │",
            "3  │  3    normal text now        123,456,789    │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "───┴─────────────────────────────────────────────┴",
            "Word wrap enabled                                 ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_carriage_returns() {
        let mut app = AppBuilder::new("tests/data/multi_lines_carriage_return.csv")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(50, 45);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::ToggleLineWrap(true));
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b                      c               ",
            "───┬─────────────────────────────────────────────┬",
            "1  │  1    this is a very         12345          │",
            "   │       long text that                        │",
            "   │       surely will not                       │",
            "   │       fit in your small                     │",
            "   │       screen                                │",
            "2  │  2    this                   678910         │",
            "   │                                             │",
            "   │       is                                    │",
            "   │                                             │",
            "   │       an                                    │",
            "   │                                             │",
            "   │       even                                  │",
            "   │                                             │",
            "   │       longer                                │",
            "   │                                             │",
            "   │       text                                  │",
            "   │                                             │",
            "   │       that                                  │",
            "   │                                             │",
            "   │       surely                                │",
            "   │                                             │",
            "   │       will                                  │",
            "   │                                             │",
            "   │       not                                   │",
            "   │                                             │",
            "   │       fit                                   │",
            "   │                                             │",
            "   │       in                                    │",
            "   │                                             │",
            "   │       your                                  │",
            "   │                                             │",
            "   │       small                                 │",
            "   │                                             │",
            "   │       screen                                │",
            "3  │  3    normal text now        123,456,789    │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "───┴─────────────────────────────────────────────┴",
            "Word wrap enabled                                 ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_starts_with_newline() {
        let mut app = AppBuilder::new("tests/data/starts_with_newline.csv")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b                      c               ",
            "───┬─────────────────────────────────────────────┬",
            "1  │  1    this is a very lon…    12345          │",
            "2  │  2    …                      678910         │",
            "3  │  3    normal text now        123,456,789    │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "───┴─────────────────────────────────────────────┴",
            "stdin [Row 1/3, Col 1/3]                          ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ToggleLineWrap(true));
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b                      c               ",
            "───┬─────────────────────────────────────────────┬",
            "1  │  1    this is a very         12345          │",
            "   │       long text that                        │",
            "   │       surely will not                       │",
            "   │       fit in your small                     │",
            "   │       screen                                │",
            "2  │  2                           678910         │",
            "   │       starts with new                       │",
            "   │       line                                  │",
            "3  │  3    normal text now        123,456,789    │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "   │                                             │",
            "───┴─────────────────────────────────────────────┴",
            "Word wrap enabled                                 ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_column_widths_boundary_condition() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(120, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(
            &mut app,
            &mut terminal,
            Control::Filter("Salt Lake City".into()),
        );
        till_app_ready(&app);
        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("City".into()),
        );
        till_app_ready(&app);
        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
            "       City                                                                                                             ",
            "────┬────────────────────┬──────────────────────────────────────────────────────────────────────────────────────────────",
            "97  │  Salt Lake City    │                                                                                              ",
            "    │                    │                                                                                              ",
            "    │                    │                                                                                              ",
            "    │                    │                                                                                              ",
            "    │                    │                                                                                              ",
            "────┴────────────────────┴──────────────────────────────────────────────────────────────────────────────────────────────",
            "stdin [Row 97/128, Col 1/1] [Filter \"Salt Lake City\": 1/1] [Filter \"City\": 1/10 cols]                                   "];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_scroll_right_most() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // TODO: why is this first nothing step needed?
        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::ScrollRightMost);
        let expected = vec![
            "────────────────────────────────────────",
            "      LonS    EW    City      State     ",
            "───┬───────────────────────────────────┬",
            "1  │  0       W     Young…    OH       │",
            "2  │  23            Yankt…    SD       │",
            "3  │  36      W     Yakima    WA       │",
            "4  │  0       W     Worce…    MA       │",
            "5  │  11      W     Wisco…    WI       │",
            "───┴───────────────────────────────────┴",
            "stdin [Row 1/128, Col 7/10]             ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_scroll_left_most() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // TODO: why is this first nothing step needed?
        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::ScrollRightMost);
        step_and_draw(&mut app, &mut terminal, Control::ScrollLeftMost);
        let expected = vec![
            "────────────────────────────────────────",
            "      LatD    LatM    LatS    NS    …   ",
            "───┬────────────────────────────────────",
            "1  │  41      5       59      N     …   ",
            "2  │  42      52      48      N     …   ",
            "3  │  46      35      59      N     …   ",
            "4  │  42      16      12      N     …   ",
            "5  │  43      37      48      N     …   ",
            "───┴────────────────────────────────────",
            "stdin [Row 1/128, Col 1/10]             ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_scroll_half_page() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // TODO: since some states are updated late only during rendering, sometimes some extra
        // no-ops are required to warm up the states. I don't like it, but this is how it has to be
        // in tests for now.
        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::ScrollHalfPageDown);
        step_and_draw(&mut app, &mut terminal, Control::ScrollHalfPageDown);
        let expected = vec![
            "────────────────────────────────────────",
            "      LatD    LatM    LatS    NS    …   ",
            "───┬────────────────────────────────────",
            "5  │  43      37      48      N     …   ",
            "6  │  36      5       59      N     …   ",
            "7  │  49      52      48      N     …   ",
            "8  │  39      11      23      N     …   ",
            "9  │  34      14      24      N     …   ",
            "───┴────────────────────────────────────",
            "stdin [Row 5/128, Col 1/10]             ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::ScrollHalfPageUp);
        let expected = vec![
            "────────────────────────────────────────",
            "      LatD    LatM    LatS    NS    …   ",
            "───┬────────────────────────────────────",
            "3  │  46      35      59      N     …   ",
            "4  │  42      16      12      N     …   ",
            "5  │  43      37      48      N     …   ",
            "6  │  36      5       59      N     …   ",
            "7  │  49      52      48      N     …   ",
            "───┴────────────────────────────────────",
            "stdin [Row 3/128, Col 1/10]             ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_resize_column() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // Check column widths are adjusted correctly
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        step_and_draw(&mut app, &mut terminal, Control::ScrollRight);
        step_and_draw(&mut app, &mut terminal, Control::ScrollRight);
        step_and_draw(&mut app, &mut terminal, Control::IncreaseWidth);
        step_and_draw(&mut app, &mut terminal, Control::IncreaseWidth);
        step_and_draw(&mut app, &mut terminal, Control::ScrollLeft);
        step_and_draw(&mut app, &mut terminal, Control::DecreaseWidth);
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "      LatD    …   LatS            NS    LonD    LonM    LonS    EW    City      ",
            "───┬────────────────────────────────────────────────────────────────────────────",
            "1  │  41      …   59              N     80      39      0       W     Young…    ",
            "2  │  42      …   48              N     97      23      23            Yankt…    ",
            "3  │  46      …   59              N     120     30      36      W     Yakima    ",
            "4  │  42      …   12              N     71      48      0       W     Worce…    ",
            "5  │  43      …   48              N     89      46      11      W     Wisco…    ",
            "───┴────────────────────────────────────────────────────────────────────────────",
            "stdin [Row 1/128, Col 1/10]                                                     ",
        ];
        assert_eq!(lines, expected);

        // Check overridden column widths still have  when columns are filtered
        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("Lat".into()),
        );
        till_app_ready(&app);
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "      LatD    …   LatS                                                          ",
            "───┬──────────────────────────────┬─────────────────────────────────────────────",
            "1  │  41      …   59              │                                             ",
            "2  │  42      …   48              │                                             ",
            "3  │  46      …   59              │                                             ",
            "4  │  42      …   12              │                                             ",
            "5  │  43      …   48              │                                             ",
            "───┴──────────────────────────────┴─────────────────────────────────────────────",
            "stdin [Row 1/128, Col 1/3] [Filter \"Lat\": 3/10 cols]                            ",
        ];
        assert_eq!(lines, expected);

        // Check reset
        step_and_draw(&mut app, &mut terminal, Control::Reset);
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "      LatD    LatM    LatS    NS    LonD    LonM    LonS    EW    City          ",
            "───┬────────────────────────────────────────────────────────────────────────────",
            "1  │  41      5       59      N     80      39      0       W     Youngstown    ",
            "2  │  42      52      48      N     97      23      23            Yankton       ",
            "3  │  46      35      59      N     120     30      36      W     Yakima        ",
            "4  │  42      16      12      N     71      48      0       W     Worcester     ",
            "5  │  43      37      48      N     89      46      11      W     Wisconsin…    ",
            "───┴────────────────────────────────────────────────────────────────────────────",
            "stdin [Row 1/128, Col 1/10]                                                     ",
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_sorting() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(100, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        // Sort by City (no tie)
        for _ in 0..8 {
            step_and_draw(&mut app, &mut terminal, Control::ScrollRight);
        }
        step_and_draw(&mut app, &mut terminal, Control::ToggleSort);
        till_app_ready(&app);
        step_and_draw(&mut app, &mut terminal, Control::Nothing);

        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────────────────────────",
            "        LatD    LatM    LatS    NS    LonD    LonM    LonS    EW    City [▴]      State             ",
            "─────┬─────────────────────────────────────────────────────────────────────────────────────┬────────",
            "128  │  41      9       35      N     81      14      23      W     Ravenna       OH       │        ",
            "127  │  40      19      48      N     75      55      48      W     Reading       PA       │        ",
            "126  │  40      10      48      N     122     14      23      W     Red Bluff     CA       │        ",
            "125  │  50      25      11      N     104     39      0       W     Regina        SA       │        ",
            "124  │  39      31      12      N     119     48      35      W     Reno          NV       │        ",
            "─────┴─────────────────────────────────────────────────────────────────────────────────────┴────────",
            "stdin [Row 128/128, Col 1/10]                                                                       ",
        ];
        assert_eq!(lines, expected);

        // Check descending
        step_and_draw(&mut app, &mut terminal, Control::ToggleSort);
        till_app_ready(&app);
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────────────────────────",
            "      LatD    LatM    LatS    NS    LonD    LonM    LonS    EW    City [▾]           State          ",
            "───┬──────────────────────────────────────────────────────────────────────────────────────────┬─────",
            "1  │  41      5       59      N     80      39      0       W     Youngstown         OH       │     ",
            "2  │  42      52      48      N     97      23      23            Yankton            SD       │     ",
            "3  │  46      35      59      N     120     30      36      W     Yakima             WA       │     ",
            "4  │  42      16      12      N     71      48      0       W     Worcester          MA       │     ",
            "5  │  43      37      48      N     89      46      11      W     Wisconsin Dells    WI       │     ",
            "───┴──────────────────────────────────────────────────────────────────────────────────────────┴─────",
            "stdin [Row 1/128, Col 1/10]                                                                         ",
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_sorting_with_filter() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        // Sort by City (no tie)
        for _ in 0..8 {
            step_and_draw(&mut app, &mut terminal, Control::ScrollRight);
        }
        step_and_draw(&mut app, &mut terminal, Control::ToggleSort);
        till_app_ready(&app);

        // Toggle back to row selection mode before filtering
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);

        step_and_draw(&mut app, &mut terminal, Control::Nothing);

        step_and_draw(&mut app, &mut terminal, Control::Filter("San".into()));
        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("Lat|City".into()),
        );

        till_app_ready(&app);
        step_and_draw(&mut app, &mut terminal, Control::Nothing);

        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "       LatD    LatM    LatS    City [▴]                                         ",
            "────┬────────────────────────────────────────────┬──────────────────────────────",
            "96  │  31      27      35      San Angelo        │                              ",
            "95  │  29      25      12      San Antonio       │                              ",
            "94  │  34      6       36      San Bernardino    │                              ",
            "93  │  32      42      35      San Diego         │                              ",
            "91  │  37      46      47      San Francisco     │                              ",
            "────┴────────────────────────────────────────────┴──────────────────────────────",
            "stdin [Row 96/128, Col 1/4] [Filter \"San\": 1/11] [Filter \"Lat|City\": 4/10 cols] ",
        ];
        assert_eq!(lines, expected);

        // Check descending
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        step_and_draw(&mut app, &mut terminal, Control::ToggleSort);
        till_app_ready(&app);
        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "       LatD    LatM    LatS    City [▾]                                         ",
            "────┬───────────────────────────────────────────┬───────────────────────────────",
            "86  │  38      26      23      Santa Rosa       │                               ",
            "87  │  35      40      48      Santa Fe         │                               ",
            "88  │  34      25      11      Santa Barbara    │                               ",
            "89  │  33      45      35      Santa Ana        │                               ",
            "92  │  41      27      0       Sandusky         │                               ",
            "────┴───────────────────────────────────────────┴───────────────────────────────",
            "stdin [Row 86/128, Col 1/4] [Filter \"San\": -/11] [Filter \"Lat|City\": 4/10 cols] ",
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_no_headers() {
        let mut app = AppBuilder::new("tests/data/no_headers.csv")
            .no_headers(true)
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        for _ in 0..7 {
            step_and_draw(&mut app, &mut terminal, Control::ScrollDown);
        }

        let expected = vec![
            "──────────────────────────────",
            "      1     2                 ",
            "───┬──────────────┬───────────",
            "4  │  A4    B4    │           ",
            "5  │  A5    B5    │           ",
            "6  │  A6    B6    │           ",
            "7  │  A7    B7    │           ",
            "8  │  A8    B8    │           ",
            "───┴──────────────┴───────────",
            "stdin [Row 8/20, Col 1/2]     ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_cli_columns_option() {
        let mut app = AppBuilder::new("tests/data/cities.csv")
            .columns_regex(Some("Lat".to_string()))
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        step_and_draw(&mut app, &mut terminal, Control::Nothing);

        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "      LatD    LatM    LatS                                                      ",
            "───┬──────────────────────────┬─────────────────────────────────────────────────",
            "1  │  41      5       59      │                                                 ",
            "2  │  42      52      48      │                                                 ",
            "3  │  46      35      59      │                                                 ",
            "4  │  42      16      12      │                                                 ",
            "5  │  43      37      48      │                                                 ",
            "───┴──────────────────────────┴─────────────────────────────────────────────────",
            "stdin [Row 1/128, Col 1/3] [Filter \"Lat\": 3/10 cols]                            ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_cli_filter_option() {
        let mut app = AppBuilder::new("tests/data/cities.csv")
            .filter_regex(Some("San".to_string()))
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        step_and_draw(&mut app, &mut terminal, Control::Nothing);

        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "       LatD    LatM    LatS    NS    LonD    LonM    LonS    EW    City         ",
            "────┬───────────────────────────────────────────────────────────────────────────",
            "86  │  38      26      23      N     122     43      12      W     Santa Ro…    ",
            "87  │  35      40      48      N     105     56      59      W     Santa Fe     ",
            "88  │  34      25      11      N     119     41      59      W     Santa Ba…    ",
            "89  │  33      45      35      N     117     52      12      W     Santa Ana    ",
            "90  │  37      20      24      N     121     52      47      W     San Jose     ",
            "────┴───────────────────────────────────────────────────────────────────────────",
            "stdin [Row 86/128, Col 1/10] [Filter \"San\": 1/11]                               ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_filter_rows_for_specific_column() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        step_and_draw(&mut app, &mut terminal, Control::ScrollRight);
        step_and_draw(&mut app, &mut terminal, Control::Filter("^1".into()));

        till_app_ready(&app);
        step_and_draw(&mut app, &mut terminal, Control::Nothing);

        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "       LatD    LatM    LatS    NS    LonD    LonM    LonS    EW    City         ",
            "────┬───────────────────────────────────────────────────────────────────────────",
            "4   │  42      16      12      N     71      48      0       W     Worcester    ",
            "8   │  39      11      23      N     78      9       36      W     Winchest…    ",
            "9   │  34      14      24      N     77      55      11      W     Wilmingt…    ",
            "12  │  41      15      0       N     77      0       0       W     Williams…    ",
            "20  │  31      13      11      N     82      20      59      W     Waycross     ",
            "────┴───────────────────────────────────────────────────────────────────────────",
            "stdin [Row 4/128, Col 1/10] [Filter \"^1\" in LatM: -/19]                         ",
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_filter_like_cell() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // Enter cell selection mode
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);

        // Select the State column
        for _ in 0..10 {
            step_and_draw(&mut app, &mut terminal, Control::ScrollRight);
        }
        step_and_draw(&mut app, &mut terminal, Control::FilterLikeCell);

        till_app_ready(&app);
        step_and_draw(&mut app, &mut terminal, Control::Nothing);

        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "       LatS    NS    LonD    LonM    LonS    EW    City            State        ",
            "────┬───────────────────────────────────────────────────────────────────────┬───",
            "1   │  59      N     80      39      0       W     Youngstown      OH       │   ",
            "50  │  0       N     83      32      24      W     Toledo          OH       │   ",
            "62  │  36      N     80      37      12      W     Steubenville    OH       │   ",
            "65  │  11      N     83      48      35      W     Springfield     OH       │   ",
            "92  │  0       N     82      42      35      W     Sandusky        OH       │   ",
            "────┴───────────────────────────────────────────────────────────────────────┴───",
            "stdin [Row 1/128, Col 3/10] [Filter \"^OH$\" in State: 1/6]                       ",
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_filter_like_cell_escape() {
        let mut app = AppBuilder::new("tests/data/filter.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // Enter cell selection mode
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);

        // Filter like the selected cell
        step_and_draw(&mut app, &mut terminal, Control::FilterLikeCell);
        till_app_ready(&app);

        step_and_draw(&mut app, &mut terminal, Control::Nothing);

        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────",
            "      a             b                                                           ",
            "───┬─────────────────────┬──────────────────────────────────────────────────────",
            "1  │  $(#1#2#.3)    1    │                                                      ",
            "   │                     │                                                      ",
            "   │                     │                                                      ",
            "   │                     │                                                      ",
            "   │                     │                                                      ",
            "───┴─────────────────────┴──────────────────────────────────────────────────────",
            "stdin [Row 1/3, Col 1/2] [Filter \"^\\$\\(\\#1\\#2\\#\\.3\\)$\" in a: 1/1]               ",
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_input_cursor() {
        let mut app = AppBuilder::new("tests/data/cities.csv").build().unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(
            &mut app,
            &mut terminal,
            Control::BufferContent("abc".into()),
        );
        assert_eq!(app.csv_table_state.cursor_xy, Some((3, 9)));
        let expected = vec![
            "──────────────────────────────",
            "      La…    La…    La…    …  ",
            "───┬──────────────────────────",
            "1  │  41     5      59     …  ",
            "2  │  42     52     48     …  ",
            "3  │  46     35     59     …  ",
            "4  │  42     16     12     …  ",
            "5  │  43     37     48     …  ",
            "───┴──────────────────────────",
            "abc                           ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_one_wide_column_but_terminal_is_wider() {
        let mut app = AppBuilder::new("tests/data/one_wide_column.txt")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(180, 8);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
            "      id    text                                                                                                                                        label                       ",
            "───┬──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┬─────────────",
            "1  │  1     this is a very very very very very very very very very very very very very very very very very very very very very very very long thing     hotdog        │             ",
            "2  │  2     this is a very very very very very very very very very very very very very very very very very very very very very very very short thing    not_hotdog    │             ",
            "   │                                                                                                                                                                  │             ",
            "───┴──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┴─────────────",
            "stdin [Row 1/2, Col 1/3]                                                                                                                                                            ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_echo_column() {
        let mut app = AppBuilder::new("tests/data/cities.csv")
            .echo_column("City")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(180, 8);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::ScrollDown);
        step_and_draw(&mut app, &mut terminal, Control::ScrollDown);
        step_and_draw(&mut app, &mut terminal, Control::ScrollDown);

        let selection = app.get_selection();
        assert_eq!(selection, Some("Worcester".to_string()));
    }

    #[test]
    fn test_echo_column_not_found() {
        let app = AppBuilder::new("tests/data/cities.csv")
            .echo_column("Cityz")
            .build();
        if let Err(e) = app {
            assert_eq!(e.to_string(), "Column name not found: Cityz");
        } else {
            panic!("Expected error");
        }
    }

    #[test]
    fn test_irregular_columns_scrolling() {
        let mut app = AppBuilder::new("tests/data/irregular_last_row.csv")
            .no_headers(true)
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(50, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      1            2            3            4    ",
            "───┬──────────────────────────────────────────────",
            "1  │  AAAAAAAA…    BBBBBBBB…    AAAAAAAA…    …    ",
            "2  │  AAAAAAAA…    BBBBBBBB…    AAAAAAAA…    …    ",
            "3  │  A                                           ",
            "   │                                              ",
            "   │                                              ",
            "───┴──────────────────────────────────────────────",
            "stdin [Row 1/3, Col 1/10]                         ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ScrollRight);
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      2            3            4            5    ",
            "───┬──────────────────────────────────────────────",
            "1  │  BBBBBBBB…    AAAAAAAA…    BBBBBBBB…    …    ",
            "2  │  BBBBBBBB…    AAAAAAAA…    BBBBBBBB…    …    ",
            "3  │                                              ",
            "   │                                              ",
            "   │                                              ",
            "───┴──────────────────────────────────────────────",
            "stdin [Row 1/3, Col 2/10]                         ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_irregular_filter_columns_then_rows() {
        let mut app = AppBuilder::new("tests/data/irregular_more_fields.csv")
            .build()
            .unwrap();
        till_app_ready(&app);

        let backend = TestBackend::new(50, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("COL1".into()),
        );
        step_and_draw(&mut app, &mut terminal, Control::Filter("x1".into()));
        // Toggle to cell selection
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);
        step_and_draw(&mut app, &mut terminal, Control::ToggleSelectionType);

        let expected = vec![
            "──────────────────────────────────────────────────",
            "      COL1                                        ",
            "───┬──────────┬───────────────────────────────────",
            "1  │  x1      │                                   ",
            "   │          │                                   ",
            "   │          │                                   ",
            "   │          │                                   ",
            "   │          │                                   ",
            "───┴──────────┴───────────────────────────────────",
            "stdin [Row 1/2, Col 1/1] [Filter \"x1\": 1/1] [Filte",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        let selection = app.get_selection();
        assert_eq!(selection, Some("x1".to_string()));
    }
}
