mod csv;
mod find;
mod input;
mod ui;
#[allow(dead_code)]
mod util;
mod view;
use crate::input::{Control, InputHandler};
use crate::ui::{CsvTable, CsvTableState, FinderState};

extern crate csv as sushi_csv;

use anyhow::{bail, Context, Result};
use clap::Parser;
use regex::Regex;
use std::convert::TryInto;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::usize;
use tempfile::NamedTempFile;
use termion::{raw::IntoRawMode, screen::AlternateScreen};
use tui::backend::TermionBackend;
use tui::Terminal;

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

struct SeekableFile {
    filename: Option<String>,
    inner_file: Option<NamedTempFile>,
}

impl SeekableFile {
    fn new(maybe_filename: &Option<String>) -> Result<SeekableFile> {
        let mut inner_file = NamedTempFile::new()?;
        let inner_file_res;

        if let Some(filename) = maybe_filename {
            let err = format!("Failed to open file: {}", filename);
            let mut f = File::open(filename).context(err)?;
            // If not seekable, it most likely is due to process substitution using
            // pipe - write out to a temp file to make it seekable
            if f.seek(SeekFrom::Start(0)).is_err() {
                let mut buffer: Vec<u8> = vec![];
                // TODO: could have read by chunks, yolo for now
                f.read_to_end(&mut buffer)?;
                inner_file.write_all(&buffer)?;
                inner_file_res = Some(inner_file);
            } else {
                inner_file_res = None;
            }
        } else {
            // Handle input from stdin
            let mut stdin = std::io::stdin();
            let mut buffer: Vec<u8> = vec![];
            stdin.read_to_end(&mut buffer)?;
            inner_file.write_all(&buffer)?;
            inner_file_res = Some(inner_file);
        }

        Ok(SeekableFile {
            filename: maybe_filename.clone(),
            inner_file: inner_file_res,
        })
    }

    fn filename(&self) -> &str {
        if let Some(f) = &self.inner_file {
            f.path().to_str().unwrap()
        } else {
            // If data is from stdin, then inner_file must be there
            self.filename.as_ref().unwrap()
        }
    }
}

#[derive(Parser, Debug)]
struct Args {
    /// CSV filename
    filename: Option<String>,

    /// Delimiter character (comma by default)
    #[clap(short, long)]
    delimiter: Option<String>,

    /// Show stats for debugging
    #[clap(long)]
    debug: bool,
}

fn parse_delimiter(args: &Args) -> Result<Option<u8>> {
    if let Some(s) = &args.delimiter {
        let mut chars = s.chars();
        let c = chars.next().context("Delimiter should not be empty")?;
        if !c.is_ascii() {
            bail!(
                "Delimiter should be within the ASCII range: {} is too fancy",
                c
            );
        }
        if chars.next().is_some() {
            bail!("Delimiter should be exactly one character, got {}", s);
        }
        Ok(Some(c.try_into()?))
    } else {
        Ok(None)
    }
}

fn run_csvlens() -> Result<()> {
    let args = Args::parse();

    let show_stats = args.debug;
    let delimiter = parse_delimiter(&args)?;

    let file = SeekableFile::new(&args.filename)?;
    let filename = file.filename();

    // Some lines are reserved for plotting headers (3 lines for headers + 2 lines for status bar)
    let num_rows_not_visible = 5;

    // Number of rows that are visible in the current frame
    let num_rows = 50 - num_rows_not_visible;

    let mut config = csv::CsvConfig::new(filename);
    if let Some(d) = delimiter {
        config.delimiter = d;
    }
    let shared_config = Arc::new(config);

    let csvlens_reader = csv::CsvLensReader::new(shared_config.clone())
        .context(format!("Failed to open file: {}", filename))?;
    let mut rows_view = view::RowsView::new(csvlens_reader, num_rows)?;

    let headers = rows_view.headers().clone();

    let stdout = io::stdout().into_raw_mode().unwrap();
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut input_handler = InputHandler::new();
    let mut csv_table_state = CsvTableState::new(args.filename, headers.len());

    let mut finder: Option<find::Finder> = None;
    let mut first_found_scrolled = false;

    let mut user_error: Option<String> = None;

    loop {
        terminal
            .draw(|f| {
                let size = f.size();

                // TODO: check type of num_rows too big?
                let frame_size_adjusted_num_rows =
                    size.height.saturating_sub(num_rows_not_visible as u16) as u64;
                rows_view
                    .set_num_rows(frame_size_adjusted_num_rows)
                    .unwrap();

                let rows = rows_view.rows();
                let csv_table = CsvTable::new(&headers, rows);

                f.render_stateful_widget(csv_table, size, &mut csv_table_state);
            })
            .unwrap();

        let control = input_handler.next();

        // clear error message without changing other states on any action
        if !matches!(control, Control::Nothing) {
            user_error = None;
        }

        rows_view.handle_control(&control)?;

        match &control {
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
            Control::Find(s) | Control::Filter(s) => {
                let re = Regex::new(s.as_str());
                if let Ok(target) = re {
                    finder = Some(find::Finder::new(shared_config.clone(), target).unwrap());
                    match control {
                        Control::Find(_) => {
                            // will scroll to first result below once ready
                            first_found_scrolled = false;
                            rows_view.reset_filter().unwrap();
                        }
                        Control::Filter(_) => {
                            rows_view.set_rows_from(0).unwrap();
                            rows_view.set_filter(finder.as_ref().unwrap()).unwrap();
                        }
                        _ => {}
                    }
                } else {
                    finder = None;
                    // TODO: how to show multi-line error
                    user_error = Some(format!("Invalid regex: {}", s));
                }
                csv_table_state.reset_buffer();
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
            } else {
                rows_view.set_filter(fdr).unwrap();
            }
        }

        // update rows and elapsed time if there are new results
        if show_stats {
            csv_table_state.debug_stats.rows_view_elapsed(rows_view.elapsed());
            if let Some(fdr) = &finder {
                csv_table_state.debug_stats.finder_elapsed(fdr.elapsed());
            }
            else {
                csv_table_state.debug_stats.finder_elapsed(None);
            }
        }

        // TODO: is this update too late?
        csv_table_state.set_rows_offset(rows_view.rows_from());
        csv_table_state.selected = rows_view.selected();

        if let Some(n) = rows_view.get_total_line_numbers() {
            csv_table_state.set_total_line_number(n);
        } else if let Some(n) = rows_view.get_total_line_numbers_approx() {
            csv_table_state.set_total_line_number(n);
        }

        if let Some(f) = &finder {
            // TODO: need to create a new finder every time?
            csv_table_state.finder_state = FinderState::from_finder(f, &rows_view);
        }

        csv_table_state.user_error = user_error.clone();

        //csv_table_state.debug = format!("{:?}", rows_view.rows_from());
    }

    Ok(())
}

fn main() {
    if let Err(e) = run_csvlens() {
        println!("{}", e);
        std::process::exit(1);
    }
}
