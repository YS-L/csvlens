#[allow(dead_code)]
mod util;
mod csv;

extern crate csv as sushi_csv;
use crate::util::events::{Event, Events};

use std::io;
use std::env;
use std::usize;
use tui::Terminal;
use tui::backend::TermionBackend;
use tui::widgets::StatefulWidget;
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::text::Span;
use tui::style::{Style, Modifier, Color};
use termion::{raw::IntoRawMode, screen::AlternateScreen, event::Key};

#[derive(Debug)]
pub struct CsvTable<'a> {
    header: Vec<String>,
    rows: &'a Vec<Vec<String>>,
}

impl<'a> CsvTable<'a> {

    fn new(header: &[String], rows: &'a Vec<Vec<String>>) -> Self {
        let _header = header.to_vec();
        Self {
            header: _header,
            rows: rows,
        }
    }

}

impl<'a> CsvTable<'a> {

    fn get_column_widths(&self) -> Vec<u16> {
        let mut column_widths = Vec::new();
        for s in self.header.iter() {
            column_widths.push(s.len() as u16);
        }
        for row in self.rows.iter() {
            for (i, value) in row.iter().enumerate() {
                let v = column_widths.get_mut(i).unwrap();
                let value_len = value.len() as u16;
                if *v < value_len {
                    *v = value_len;
                }
            }
        }
        for w in column_widths.iter_mut() {
            *w = *w + 4;
        }
        column_widths
    }

    fn render_row_number(&self, buf: &mut Buffer, state: &mut CsvTableState, row_index: usize, y: u16, is_header: bool) -> u16 {
        let row_num_width = 8;
        if is_header {
            return row_num_width
        }
        let row_num = row_index + state.rows_offset as usize + 1;
        let row_num_formatted = format!("{}", row_num);
        let style = Style::default()
            .fg(Color::LightRed);
        let span = Span::styled(row_num_formatted, style);
        buf.set_span(0, y, &span, row_num_width);
        row_num_width
    }
}

impl<'a> StatefulWidget for CsvTable<'a> {
    type State = CsvTableState;

    fn render(mut self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {

        // TODO: draw relative to the provided area

        if area.area() == 0 {
            return;
        }

        let column_widths = self.get_column_widths();

        let mut x_offset_header = self.render_row_number(buf, state, 0, 0, true);
        for (hname, hlen) in self.header.iter().zip(&column_widths) {
            let style = Style::default()
                .add_modifier(Modifier::BOLD);
            let span = Span::styled((*hname).as_str(), style);
            buf.set_span(x_offset_header, 0, &span, *hlen);
            x_offset_header += hlen;
        }

        let mut y_offset = 1;
        for (row_index, row) in self.rows.iter().enumerate() {
            if y_offset >= area.height {
                break;
            }
            let mut x_offset_header = self.render_row_number(buf, state, row_index, y_offset, false);
            for (value, hlen) in row.iter().zip(&column_widths) {
                let span = Span::from((*value).as_str());
                buf.set_span(x_offset_header, y_offset, &span, *hlen);
                x_offset_header += hlen;
            }
            y_offset += 1;
        }

    }
}
pub struct CsvTableState {
    rows_offset: u64,
}

impl CsvTableState {

    fn new() -> Self {
        Self {
            rows_offset: 0,
        }
    }

    fn set_rows_offset(&mut self, offset: u64) {
        self.rows_offset = offset;
    }

}

fn main() {
    let args: Vec<String> = env::args().collect();
    let filename = args.get(1).expect("Filename not provided");
    println!("filename: {}", filename);

    let inner_rdr = sushi_csv::Reader::from_path(filename).unwrap();
    let mut num_rows = 50;
    let mut rows_from = 0;
    let mut csvlens_reader = csv::CsvLensReader::new(inner_rdr);
    let mut rows = csvlens_reader.get_rows(rows_from, num_rows).unwrap();
    let headers = csvlens_reader.headers.clone();

    let stdout = io::stdout().into_raw_mode().unwrap();
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let events = Events::new();
    let mut csv_table_state = CsvTableState::new();

    loop {
        terminal.draw(|f| {

            let size = f.size();

            // TODO: check type of num_rows too big?
            if num_rows < size.height as u64 {
                num_rows = size.height as u64;
                rows = csvlens_reader.get_rows(rows_from, num_rows).unwrap();
            }

            let csv_table = CsvTable::new(&headers, &rows);

            f.render_stateful_widget(csv_table, size, &mut csv_table_state);

        }).unwrap();

        if let Event::Input(key) = events.next().unwrap() {
            match key {
                Key::Char('q') => {
                    break;
                }
                Key::Char('j') => {
                    rows_from = rows_from + 1;
                    rows = csvlens_reader.get_rows(rows_from, num_rows).unwrap();
                }
                Key::Char('k') => {
                    if rows_from > 0 {
                        rows_from = rows_from - 1;
                        rows = csvlens_reader.get_rows(rows_from, num_rows).unwrap();
                    }
                }
                _ => {}
            }
            csv_table_state.set_rows_offset(rows_from);
        };
    }
}
