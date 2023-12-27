extern crate csv_sniffer;

use crate::csv;
use crate::delimiter::{sniff_delimiter, Delimiter};
use crate::find;
use crate::help;
use crate::input::{Control, InputHandler};
use crate::ui::{CsvTable, CsvTableState, FilterColumnsState, FinderState};
use crate::view;

use anyhow::ensure;
use ratatui::backend::Backend;
use ratatui::{Frame, Terminal};

use anyhow::{Context, Result};
use regex::Regex;
use std::cmp::min;
use std::sync::Arc;
use std::usize;

fn get_offsets_to_make_visible(
    found_record: &find::FoundRecord,
    rows_view: &view::RowsView,
    csv_table_state: &CsvTableState,
) -> (Option<u64>, Option<u64>) {
    // TODO: row_index() should probably be u64
    let new_rows_offset = if rows_view.in_view(found_record.row_index() as u64) {
        None
    } else {
        Some(found_record.row_index() as u64)
    };

    let cols_offset = csv_table_state.cols_offset;
    let last_rendered_col = cols_offset.saturating_add(csv_table_state.num_cols_rendered);
    let column_index = found_record.first_column() as u64;
    let new_cols_offset = if column_index >= cols_offset && column_index < last_rendered_col {
        None
    } else {
        Some(column_index)
    };

    (new_rows_offset, new_cols_offset)
}

fn scroll_to_found_record(
    found_record: find::FoundRecord,
    rows_view: &mut view::RowsView,
    csv_table_state: &mut CsvTableState,
) {
    let (new_rows_offset, new_cols_offset) =
        get_offsets_to_make_visible(&found_record, rows_view, csv_table_state);

    if let Some(rows_offset) = new_rows_offset {
        rows_view.set_rows_from(rows_offset).unwrap();
        csv_table_state.set_rows_offset(rows_offset);
    }

    if let Some(cols_offset) = new_cols_offset {
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

pub struct App {
    input_handler: InputHandler,
    num_rows_not_visible: u16,
    shared_config: Arc<csv::CsvConfig>,
    rows_view: view::RowsView,
    csv_table_state: CsvTableState,
    finder: Option<find::Finder>,
    first_found_scrolled: bool,
    frame_width: Option<u16>,
    transient_message: Option<String>,
    show_stats: bool,
    echo_column: Option<String>,
    ignore_case: bool,
    help_page_state: help::HelpPageState,
}

impl App {
    pub fn new(
        filename: &str,
        delimiter: Delimiter,
        original_filename: Option<String>,
        show_stats: bool,
        echo_column: Option<String>,
        ignore_case: bool,
    ) -> Result<Self> {
        let input_handler = InputHandler::new();

        // Some lines are reserved for plotting headers (3 lines for headers + 2 lines for status bar)
        let num_rows_not_visible: u16 = 5;

        // Number of rows that are visible in the current frame
        let num_rows = 50 - num_rows_not_visible;

        let delimiter = match delimiter {
            Delimiter::Default => b',',
            Delimiter::Character(d) => d,
            Delimiter::Auto => sniff_delimiter(filename).unwrap_or(b','),
        };
        let config = csv::CsvConfig::new(filename, delimiter);
        let shared_config = Arc::new(config);

        let csvlens_reader = csv::CsvLensReader::new(shared_config.clone())
            .context(format!("Failed to open file: {filename}"))?;
        let rows_view = view::RowsView::new(csvlens_reader, num_rows as u64)?;

        if let Some(column_name) = &echo_column {
            ensure!(
                rows_view.headers().iter().any(|h| h.name == *column_name),
                format!("Column name not found: {column_name}"),
            );
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

        let app = App {
            input_handler,
            num_rows_not_visible,
            shared_config,
            rows_view,
            csv_table_state,
            finder,
            first_found_scrolled,
            frame_width,
            transient_message,
            show_stats,
            echo_column,
            ignore_case,
            help_page_state,
        };

        Ok(app)
    }

    pub fn main_loop<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<Option<String>> {
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
                if let Some(result) = self.rows_view.get_cell_value_from_selection() {
                    return Ok(Some(result));
                } else if let Some(column_name) = &self.echo_column {
                    if let Some(result) = self.rows_view.get_cell_value(column_name) {
                        return Ok(Some(result));
                    }
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

    fn step_help(&mut self, control: &Control) -> Result<()> {
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

    fn step(&mut self, control: &Control) -> Result<()> {
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
                    if let Some(found_record) = fdr.next() {
                        scroll_to_found_record(
                            found_record,
                            &mut self.rows_view,
                            &mut self.csv_table_state,
                        );
                    }
                }
            }
            Control::ScrollToPrevFound if !self.rows_view.is_filter() => {
                if let Some(fdr) = self.finder.as_mut() {
                    if let Some(found_record) = fdr.prev() {
                        scroll_to_found_record(
                            found_record,
                            &mut self.rows_view,
                            &mut self.csv_table_state,
                        );
                    }
                }
            }
            Control::Find(s) | Control::Filter(s) => {
                let re = self.create_regex(s);
                if let Ok(target) = re {
                    // TODO: need to reset row views filter if any first?
                    self.finder =
                        Some(find::Finder::new(self.shared_config.clone(), target).unwrap());
                    match control {
                        Control::Find(_) => {
                            // will scroll to first result below once ready
                            self.first_found_scrolled = false;
                            self.rows_view.reset_filter().unwrap();
                        }
                        Control::Filter(_) => {
                            self.rows_view.set_rows_from(0).unwrap();
                            self.rows_view
                                .set_filter(self.finder.as_ref().unwrap())
                                .unwrap();
                        }
                        _ => {}
                    }
                } else {
                    self.finder = None;
                    // TODO: how to show multi-line error
                    self.transient_message = Some(format!("Invalid regex: {s}"));
                }
                self.csv_table_state.reset_buffer();
            }
            Control::FilterColumns(s) => {
                let re = self.create_regex(s);
                if let Ok(target) = re {
                    self.rows_view.set_columns_filter(target).unwrap();
                } else {
                    self.rows_view.reset_columns_filter().unwrap();
                    self.transient_message = Some(format!("Invalid regex: {s}"));
                }
                self.csv_table_state.reset_buffer();
                self.csv_table_state.set_cols_offset(0);
            }
            Control::BufferContent(buf) => {
                self.csv_table_state
                    .set_buffer(self.input_handler.mode(), buf.as_str());
            }
            Control::BufferReset => {
                self.csv_table_state.reset_buffer();
                self.reset_filter();
                self.rows_view.reset_columns_filter().unwrap();
            }
            Control::ToggleSelectionType => {
                self.rows_view.selection.toggle_selection_type();
            }
            Control::ToggleLineWrap => {
                self.csv_table_state.reset_buffer();
                if self.csv_table_state.enable_line_wrap {
                    self.csv_table_state.enable_line_wrap = false;
                    self.transient_message
                        .replace("Line wrap disabled".to_string());
                } else {
                    self.csv_table_state.enable_line_wrap = true;
                    self.transient_message
                        .replace("Line wrap enabled".to_string());
                }
            }
            Control::IncreaseWidth => {
                self.adjust_column_width(4);
            }
            Control::DecreaseWidth => {
                self.adjust_column_width(-4);
            }
            Control::Reset => {
                self.csv_table_state.column_width_overrides.reset();
                self.reset_filter();
                self.rows_view.reset_columns_filter().unwrap();
            }
            Control::UnknownOption(s) => {
                self.csv_table_state.reset_buffer();
                self.transient_message
                    .replace(format!("Unknown option: {s}"));
            }
            _ => {}
        }

        if let Some(fdr) = self.finder.as_mut() {
            if !self.rows_view.is_filter() {
                // scroll to first result once ready
                if !self.first_found_scrolled && fdr.count() > 0 {
                    // set row_hint to 0 so that this always scrolls to first result
                    fdr.set_row_hint(0);
                    if let Some(found_record) = fdr.next() {
                        scroll_to_found_record(
                            found_record,
                            &mut self.rows_view,
                            &mut self.csv_table_state,
                        );
                    }
                    self.first_found_scrolled = true;
                }

                // reset cursor if out of view
                if let Some(cursor_row_index) = fdr.cursor_row_index() {
                    if !self.rows_view.in_view(cursor_row_index as u64) {
                        fdr.reset_cursor();
                    }
                }

                fdr.set_row_hint(self.rows_view.rows_from() as usize);
            } else {
                self.rows_view.set_filter(fdr).unwrap();
            }
        }

        // update rows and elapsed time if there are new results
        if self.show_stats {
            self.csv_table_state
                .debug_stats
                .rows_view_elapsed(self.rows_view.elapsed());
            if let Some(fdr) = &self.finder {
                self.csv_table_state
                    .debug_stats
                    .finder_elapsed(fdr.elapsed());
            } else {
                self.csv_table_state.debug_stats.finder_elapsed(None);
            }
        }

        // TODO: is this update too late?
        self.csv_table_state
            .set_rows_offset(self.rows_view.rows_from());
        self.csv_table_state
            .set_cols_offset(self.rows_view.cols_offset());
        self.csv_table_state.selection = Some(self.rows_view.selection.clone());

        if let Some(n) = self.rows_view.get_total_line_numbers() {
            self.csv_table_state.set_total_line_number(n);
        } else if let Some(n) = self.rows_view.get_total_line_numbers_approx() {
            self.csv_table_state.set_total_line_number(n);
        }
        self.csv_table_state
            .set_total_cols(self.rows_view.headers().len());

        if let Some(f) = &self.finder {
            // TODO: need to create a new finder every time?
            self.csv_table_state.finder_state = FinderState::from_finder(f, &self.rows_view);
        }
        self.csv_table_state.filter_columns_state =
            FilterColumnsState::from_rows_view(&self.rows_view);

        self.csv_table_state.transient_message = self.transient_message.clone();

        // self.csv_table_state.debug = format!("{:?}", self.csv_table_state.column_width_overrides);

        Ok(())
    }

    fn create_regex(&mut self, s: &String) -> std::result::Result<Regex, regex::Error> {
        let lower_s = s.to_lowercase();
        let re = if self.ignore_case && lower_s.starts_with(s) {
            Regex::new(&format!("(?i){}", s.as_str()))
        } else {
            Regex::new(s.as_str())
        };
        re
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
        // local index as in local to the view port
        if let Some(local_column_index) = self.rows_view.selection.column.index() {
            let column_index = local_column_index.saturating_add(self.csv_table_state.cols_offset);

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

    fn reset_filter(&mut self) {
        if self.finder.is_some() {
            self.finder = None;
            self.csv_table_state.finder_state = FinderState::FinderInactive;
            self.rows_view.reset_filter().unwrap();
        }
    }

    fn render_frame(&mut self, f: &mut Frame) {
        let size = f.size();

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
    }

    fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        terminal
            .draw(|f| {
                self.render_frame(f);
            })
            .unwrap();

        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use core::time;
    use std::thread;

    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn to_lines(buf: &Buffer) -> Vec<String> {
        let mut symbols: String = "".to_owned();
        let area = buf.area();
        for y in 0..area.bottom() {
            for x in 0..area.right() {
                let symbol = buf.get(x, y).symbol();
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

    #[test]
    fn test_simple() {
        let mut app = App::new(
            "tests/data/simple.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

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
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "──────────────────────────────",
            "      LatD    LatM    LatS    ",
            "───┬──────────────────────────",
            "1  │  41      5       59      ",
            "2  │  42      52      48      ",
            "3  │  46      35      59      ",
            "4  │  42      16      12      ",
            "5  │  43      37      48      ",
            "───┴──────────────────────────",
            "stdin [Row 1/128, Col 1/10]   ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ScrollPageRight);
        let expected = vec![
            "──────────────────────────────",
            "      NS    LonD    LonM    … ",
            "───┬──────────────────────────",
            "1  │  N     80      39      … ",
            "2  │  N     97      23      … ",
            "3  │  N     120     30      … ",
            "4  │  N     71      48      … ",
            "5  │  N     89      46      … ",
            "───┴──────────────────────────",
            "stdin [Row 1/128, Col 4/10]   ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ScrollPageLeft);
        let expected = vec![
            "──────────────────────────────",
            "      LatD    LatM    LatS    ",
            "───┬──────────────────────────",
            "1  │  41      5       59      ",
            "2  │  42      52      48      ",
            "3  │  46      35      59      ",
            "4  │  42      16      12      ",
            "5  │  43      37      48      ",
            "───┴──────────────────────────",
            "stdin [Row 1/128, Col 1/10]   ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_filter_columns() {
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

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
    fn test_filter_columns_case_sensitive() {
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

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
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            true,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

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
        let mut app = App::new(
            "tests/data/bad_double_quote.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

        let backend = TestBackend::new(35, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "───────────────────────────────────",
            "      Colum…     \"col…             ",
            "───┬──────────────────────┬────────",
            "1  │  1          \"quo…    │        ",
            "2  │  5          \"Com…    │        ",
            "   │                      │        ",
            "   │                      │        ",
            "   │                      │        ",
            "───┴──────────────────────┴────────",
            "stdin [Row 1/2, Col 1/2]           ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_sniff_delimiter() {
        let mut app = App::new(
            "tests/data/small.bsv",
            Delimiter::Auto,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

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
        let mut app = App::new(
            "tests/data/multi_lines.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

        let backend = TestBackend::new(50, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b              c                       ",
            "───┬─────────────────────────────────────┬────────",
            "1  │  1    this is a …    12345          │        ",
            "2  │  2    thi…           678910         │        ",
            "3  │  3    normal tex…    123,456,789    │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "   │                                     │        ",
            "───┴─────────────────────────────────────┴────────",
            "stdin [Row 1/3, Col 1/3]                          ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);

        step_and_draw(&mut app, &mut terminal, Control::ToggleLineWrap);
        let expected = vec![
            "──────────────────────────────────────────────────",
            "      a    b              c                       ",
            "───┬─────────────────────────────────────┬────────",
            "1  │  1    this is a v    12345          │        ",
            "   │       ery long te                   │        ",
            "   │       xt that sur                   │        ",
            "   │       ely will no                   │        ",
            "   │       t fit in yo                   │        ",
            "   │       ur small sc                   │        ",
            "   │       reen                          │        ",
            "2  │  2    this           678910         │        ",
            "   │       is                            │        ",
            "   │       an                            │        ",
            "   │       even                          │        ",
            "   │       longer                        │        ",
            "   │       text                          │        ",
            "   │       that                          │        ",
            "   │       surely                        │        ",
            "   │       will                          │        ",
            "   │       not                           │        ",
            "   │       fit                           │        ",
            "   │       in                            │        ",
            "   │       your                          │        ",
            "   │       small                         │        ",
            "   │       screen                        │        ",
            "3  │  3    normal text    123,456,789    │        ",
            "   │        now                          │        ",
            "   │                                     │        ",
            "───┴─────────────────────────────────────┴────────",
            "Line wrap enabled                                 ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_column_widths_boundary_condition() {
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

        let backend = TestBackend::new(120, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(
            &mut app,
            &mut terminal,
            Control::Filter("Salt Lake City".into()),
        );
        step_and_draw(
            &mut app,
            &mut terminal,
            Control::FilterColumns("City".into()),
        );
        thread::sleep(time::Duration::from_millis(100));
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
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // TODO: why is this first nothing step needed?
        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        step_and_draw(&mut app, &mut terminal, Control::ScrollRightMost);
        let expected = vec![
            "────────────────────────────────────────",
            "      EW    City        State           ",
            "───┬─────────────────────────────┬──────",
            "1  │  W     Youngst…    OH       │      ",
            "2  │        Yankton     SD       │      ",
            "3  │  W     Yakima      WA       │      ",
            "4  │  W     Worcest…    MA       │      ",
            "5  │  W     Wiscons…    WI       │      ",
            "───┴─────────────────────────────┴──────",
            "stdin [Row 1/128, Col 8/10]             ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }

    #[test]
    fn test_scroll_left_most() {
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

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
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

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
        let mut app = App::new(
            "tests/data/cities.csv",
            Delimiter::Default,
            None,
            false,
            None,
            false,
        )
        .unwrap();
        thread::sleep(time::Duration::from_millis(100));

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
        thread::sleep(time::Duration::from_millis(100));
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
}
