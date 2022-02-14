#[allow(dead_code)]
mod util;
mod csv;
mod input;
mod view;
mod find;
mod ui;
use crate::input::{InputHandler, Control};
use crate::ui::{CsvTable, CsvTableState, FinderState};

extern crate csv as sushi_csv;

use std::io;
use std::env;
use std::usize;
use tui::Terminal;
use tui::backend::TermionBackend;
use termion::{raw::IntoRawMode, screen::AlternateScreen};
use anyhow::{Context, Result};


fn get_offsets_to_make_visible(
    found_record: find::FoundRecord,
    rows_view: &view::RowsView,
    csv_table_state: &CsvTableState,
) -> (Option<u64>, Option<u64>) {

    let new_rows_offset;
    // TODO: row_index() should probably be u64
    if rows_view.in_view(found_record.row_index() as u64) {
        new_rows_offset = None;
    }
    else {
        new_rows_offset = Some(found_record.row_index() as u64);
    }

    let new_cols_offset;
    let cols_offset = csv_table_state.cols_offset;
    let last_rendered_col = cols_offset.saturating_add(csv_table_state.num_cols_rendered);
    let column_index = found_record.first_column() as u64;
    if column_index >= cols_offset && column_index < last_rendered_col {
        new_cols_offset = None;
    }
    else {
        new_cols_offset = Some(column_index)
    }

    (new_rows_offset, new_cols_offset)
}

fn scroll_to_found_record(
    found_record: find::FoundRecord,
    rows_view: &mut view::RowsView,
    csv_table_state: &mut CsvTableState,
) {

    let (new_rows_offset, new_cols_offset) = get_offsets_to_make_visible(
        found_record.clone(), rows_view, csv_table_state
    );

    if let Some(rows_offset) = new_rows_offset {
        rows_view.set_rows_from(rows_offset).unwrap();
        csv_table_state.set_rows_offset(rows_offset);
    }

    if let Some(cols_offset) = new_cols_offset {
        csv_table_state.set_cols_offset(cols_offset);
    }

}

fn run_csvlens() -> Result<()> {

    let args: Vec<String> = env::args().collect();
    let filename = args.get(1).expect("Filename not provided");

    // Some lines are reserved for plotting headers (3 lines for headers + 2 lines for status bar)
    let num_rows_not_visible = 5;

    // Number of rows that are visible in the current frame
    let num_rows = 50 - num_rows_not_visible;
    let csvlens_reader = csv::CsvLensReader::new(filename)
        .context(format!("Failed to open file: {}", filename))?;
    let mut rows_view = view::RowsView::new(csvlens_reader, num_rows)?;

    let headers = rows_view.headers().clone();

    let stdout = io::stdout().into_raw_mode().unwrap();
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut input_handler = InputHandler::new();
    let mut csv_table_state = CsvTableState::new(
        filename.to_string(), headers.len()
    );

    let mut finder: Option<find::Finder> = None;
    let mut first_found_scrolled = false;

    loop {
        terminal.draw(|f| {

            let size = f.size();

            // TODO: check type of num_rows too big?
            let frame_size_adjusted_num_rows = size.height.saturating_sub(num_rows_not_visible as u16) as u64;
            rows_view.set_num_rows(frame_size_adjusted_num_rows).unwrap();

            let rows = rows_view.rows();
            let csv_table = CsvTable::new(&headers, rows);

            f.render_stateful_widget(csv_table, size, &mut csv_table_state);

        }).unwrap();

        let control = input_handler.next();

        rows_view.handle_control(&control)?;

        match control {
            Control::Quit => {
                break;
            }
            Control::ScrollTo(_) => {
                csv_table_state.reset_buffer();
            }
            Control::ScrollLeft => {
                let new_cols_offset = csv_table_state.cols_offset.saturating_sub(1);
                csv_table_state.set_cols_offset(new_cols_offset);
            }
            Control::ScrollRight => {
                if csv_table_state.has_more_cols_to_show() {
                    let new_cols_offset = csv_table_state.cols_offset.saturating_add(1);
                    csv_table_state.set_cols_offset(new_cols_offset);
                }
            }
            Control::ScrollToNextFound if !rows_view.is_filter() => {
                if let Some(fdr) = finder.as_mut() {
                    if let Some(found_record) = fdr.next() {
                        scroll_to_found_record(found_record, &mut rows_view, &mut csv_table_state);
                    }
                }
            }
            Control::ScrollToPrevFound if !rows_view.is_filter() => {
                if let Some(fdr) = finder.as_mut() {
                    if let Some(found_record) = fdr.prev() {
                        scroll_to_found_record(found_record, &mut rows_view, &mut csv_table_state);
                    }
                }
            }
            Control::Find(s) => {
                finder = Some(find::Finder::new(filename, s.as_str()).unwrap());
                first_found_scrolled = false;
                rows_view.reset_filter().unwrap();
                csv_table_state.reset_buffer();
            }
            Control::Filter(s) => {
                finder = Some(find::Finder::new(filename, s.as_str()).unwrap());
                csv_table_state.reset_buffer();
                rows_view.set_rows_from(0).unwrap();
                rows_view.init_filter().unwrap();
            }
            Control::BufferContent(buf) => {
                csv_table_state.set_buffer(input_handler.mode(), buf.as_str());
            }
            Control::BufferReset => {
                csv_table_state.reset_buffer();
                if finder.is_some() {
                    finder = None;
                    csv_table_state.finder_state = FinderState::FinderInactive;
                    rows_view.reset_filter().unwrap();
                }
            }
            _ => {}
        }

        if let Some(fdr) = finder.as_mut() {
            if !rows_view.is_filter() {
                // scroll to first result once ready
                if !first_found_scrolled && fdr.count() > 0 {
                    // set row_hint to 0 so that this always scrolls to first result
                    fdr.set_row_hint(0);
                    if let Some(found_record) = fdr.next() {
                        scroll_to_found_record(found_record, &mut rows_view, &mut csv_table_state);
                    }
                    first_found_scrolled = true;
                }

                // reset cursor if out of view
                if let Some(cursor_row_index) = fdr.cursor_row_index() {
                    if !rows_view.in_view(cursor_row_index as u64) {
                        fdr.reset_cursor();
                    }
                }

                fdr.set_row_hint(rows_view.rows_from() as usize);
            }
            else {
                // TODO: this is making too much copies all the time?
                let filter_indices: Vec<u64> = fdr.get_all_found().iter().map(|x| x.row_index as u64).collect();
                rows_view.set_filter(&filter_indices).unwrap();
            }
        }

        // update rows and elapsed time if there are new results
        if let Some(elapsed) = rows_view.elapsed() {
            csv_table_state.elapsed = Some(elapsed as f64 / 1000.0);
        }

        // TODO: is this update too late?
        csv_table_state.set_rows_offset(rows_view.rows_from());

        if let Some(n) = rows_view.get_total_line_numbers() {
            csv_table_state.set_total_line_number(n);
        }
        else if let Some(n) = rows_view.get_total_line_numbers_approx() {
            csv_table_state.set_total_line_number(n);
        }

        if let Some(f) = &finder {
            // TODO: need to create a new finder every time?
            csv_table_state.finder_state = FinderState::from_finder(f);
        }

        //csv_table_state.debug = format!("{:?}", csv_table_state.cols_offset);
    }

    Ok(())
}

fn main() {
    if let Err(e) = run_csvlens() {
        println!("{}", e.to_string());
        std::process::exit(1);
    }
}