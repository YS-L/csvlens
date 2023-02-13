use crate::csv;
use crate::find;
use crate::input::{Control, InputHandler};
use crate::ui::{CsvTable, CsvTableState, FilterColumnsState, FinderState};
use crate::view;

use anyhow::ensure;
use tui::backend::Backend;
use tui::{Frame, Terminal};

use anyhow::{Context, Result};
use regex::Regex;
use std::cmp::min;
use std::sync::Arc;
use std::usize;

fn get_offsets_to_make_visible(
    found_record: find::FoundRecord,
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
        get_offsets_to_make_visible(found_record, rows_view, csv_table_state);

    if let Some(rows_offset) = new_rows_offset {
        rows_view.set_rows_from(rows_offset).unwrap();
        csv_table_state.set_rows_offset(rows_offset);
    }

    if let Some(cols_offset) = new_cols_offset {
        csv_table_state.set_cols_offset(cols_offset);
    }
}

fn get_page_left_cols_offset(frame_width: u16, csv_table_state: &CsvTableState) -> Option<u64> {
    let columns_widths = csv_table_state.column_widths.as_ref();
    let cols_offset = csv_table_state.cols_offset;
    let mut total: u16 = 0;
    let mut new_cols_offset = None;
    if let Some(columns_widths) = columns_widths {
        for c in (0..cols_offset as usize).rev() {
            let maybe_width = columns_widths.get(c);
            if let Some(w) = maybe_width {
                if total + w > frame_width {
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
    user_error: Option<String>,
    show_stats: bool,
    echo: Option<String>,
}

impl App {
    pub fn new(
        filename: &str,
        delimiter: Option<u8>,
        original_filename: Option<String>,
        show_stats: bool,
        echo: Option<String>,
    ) -> Result<Self> {
        let input_handler = InputHandler::new();

        // Some lines are reserved for plotting headers (3 lines for headers + 2 lines for status bar)
        let num_rows_not_visible: u16 = 5;

        // Number of rows that are visible in the current frame
        let num_rows = 50 - num_rows_not_visible;

        let mut config = csv::CsvConfig::new(filename);
        if let Some(d) = delimiter {
            config.delimiter = d;
        }
        let shared_config = Arc::new(config);

        let csvlens_reader = csv::CsvLensReader::new(shared_config.clone())
            .context(format!("Failed to open file: {filename}"))?;
        let rows_view = view::RowsView::new(csvlens_reader, num_rows as u64)?;

        if let Some(column_name) = &echo {
            ensure!(
                rows_view.headers().contains(column_name),
                format!("Column name not found: {column_name}"),
            );
        }

        let csv_table_state = CsvTableState::new(original_filename, rows_view.headers().len());

        let finder: Option<find::Finder> = None;
        let first_found_scrolled = false;
        let frame_width = None;

        let user_error: Option<String> = None;

        let app = App {
            input_handler,
            shared_config,
            num_rows_not_visible,
            rows_view,
            csv_table_state,
            finder,
            first_found_scrolled,
            frame_width,
            user_error,
            show_stats,
            echo,
        };

        Ok(app)
    }

    pub fn main_loop<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<Option<String>> {
        loop {
            let control = self.input_handler.next();
            if matches!(control, Control::Quit) {
                return Ok(None);
            }
            if matches!(control, Control::Select) {
                if let Some(column_name) = &self.echo {
                    if let Some(result) = self.rows_view.get_cell_value(column_name) {
                        return Ok(Some(result));
                    }
                }
            }
            self.step(control)?;
            self.draw(terminal)?;
        }
    }

    fn step(&mut self, control: Control) -> Result<()> {
        // clear error message without changing other states on any action
        if !matches!(control, Control::Nothing) {
            self.user_error = None;
        }

        self.rows_view.handle_control(&control)?;

        match &control {
            Control::ScrollTo(_) => {
                self.csv_table_state.reset_buffer();
            }
            Control::ScrollLeft => {
                let new_cols_offset = self.csv_table_state.cols_offset.saturating_sub(1);
                self.csv_table_state.set_cols_offset(new_cols_offset);
            }
            Control::ScrollRight => {
                if self.csv_table_state.has_more_cols_to_show() {
                    let new_cols_offset = self.csv_table_state.cols_offset.saturating_add(1);
                    self.csv_table_state.set_cols_offset(new_cols_offset);
                }
            }
            Control::ScrollPageLeft => {
                let new_cols_offset = match self.frame_width {
                    Some(frame_width) => {
                        get_page_left_cols_offset(frame_width, &self.csv_table_state)
                    }
                    _ => Some(0),
                };
                if let Some(new_cols_offset) = new_cols_offset {
                    self.csv_table_state.set_cols_offset(new_cols_offset);
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
                        self.csv_table_state.set_cols_offset(new_cols_offset);
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
                let re = Regex::new(s.as_str());
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
                    self.user_error = Some(format!("Invalid regex: {s}"));
                }
                self.csv_table_state.reset_buffer();
            }
            Control::FilterColumns(s) => {
                let re = Regex::new(s.as_str());
                if let Ok(target) = re {
                    self.rows_view.set_columns_filter(target).unwrap();
                } else {
                    self.rows_view.reset_columns_filter().unwrap();
                    self.user_error = Some(format!("Invalid regex: {s}"));
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
                if self.finder.is_some() {
                    self.finder = None;
                    self.csv_table_state.finder_state = FinderState::FinderInactive;
                    self.rows_view.reset_filter().unwrap();
                }
                self.rows_view.reset_columns_filter().unwrap();
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
        self.csv_table_state.selected = self.rows_view.selected();

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

        self.csv_table_state.user_error = self.user_error.clone();

        // self.csv_table_state.debug = format!("{:?}", self.rows_view.columns_filter());

        Ok(())
    }

    fn render_frame<B: Backend>(&mut self, f: &mut Frame<B>) {
        let size = f.size();

        // TODO: check type of num_rows too big?
        let frame_size_adjusted_num_rows =
            size.height.saturating_sub(self.num_rows_not_visible) as u64;
        self.rows_view
            .set_num_rows(frame_size_adjusted_num_rows)
            .unwrap();
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
    use tui::backend::TestBackend;
    use tui::buffer::Buffer;

    fn to_lines(buf: &Buffer) -> Vec<String> {
        let mut symbols: String = "".to_owned();
        let area = buf.area();
        for y in 0..area.bottom() {
            for x in 0..area.right() {
                let symbol = buf.get(x, y).symbol.clone();
                symbols.push_str(&symbol);
            }
            if y != area.bottom() - 1 {
                symbols.push('\n');
            }
        }

        symbols.split('\n').map(|s| s.to_string()).collect()
    }

    fn step_and_draw<B: Backend>(app: &mut App, terminal: &mut Terminal<B>, control: Control) {
        app.step(control).unwrap();

        // While it's possible to step multiple times before any draw when
        // testing, App::render_frame() can update App's state (e.g. based on
        // the current terminal frame size) with information that might be
        // required for stepping to work correctly. Also, immediately drawing
        // after each step is what App::main_loop() will be doing.
        terminal.draw(|f| app.render_frame(f)).unwrap();
    }

    #[test]
    fn test_simple() {
        let mut app = App::new("tests/data/simple.csv", None, None, false, None).unwrap();
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
        let mut app = App::new("tests/data/cities.csv", None, None, false, None).unwrap();
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
        let mut app = App::new("tests/data/cities.csv", None, None, false, None).unwrap();
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
    fn test_extra_fields_in_some_rows() {
        // Test getting column widths should not fail on data with bad formatting (some rows having
        // more fields than the header)
        let mut app = App::new("tests/data/bad_double_quote.csv", None, None, false, None).unwrap();
        thread::sleep(time::Duration::from_millis(100));

        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        step_and_draw(&mut app, &mut terminal, Control::Nothing);
        let expected = vec![
            "──────────────────────────────",
            "      Column1     \"column…    ",
            "───┬──────────────────────────",
            "1  │  1           \"quote\"     ",
            "2  │  5           \"Comma      ",
            "   │                          ",
            "   │                          ",
            "   │                          ",
            "───┴──────────────────────────",
            "stdin [Row 1/2, Col 1/2]      ",
        ];
        let actual_buffer = terminal.backend().buffer().clone();
        let lines = to_lines(&actual_buffer);
        assert_eq!(lines, expected);
    }
}
