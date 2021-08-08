mod util;
mod csv;

extern crate csv as sushi_csv;
use crate::util::events::{Event, Events};

use std::io;
use std::env;
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

pub struct CsvTableState {
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
}

impl<'a> StatefulWidget for CsvTable<'a> {
    type State = CsvTableState;

    fn render(mut self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {

        if area.area() == 0 {
            return;
        }

        let column_widths = self.get_column_widths();

        let mut x_offset_header = 0;
        for (hname, hlen) in self.header.iter().zip(&column_widths) {
            let style = Style::default()
                .add_modifier(Modifier::BOLD);
            let span = Span::styled((*hname).as_str(), style);
            buf.set_span(x_offset_header, 0, &span, *hlen);
            x_offset_header += hlen;
        }

        let mut y_offset = 1;
        for row in self.rows.iter() {
            let mut x_offset_header = 0;
            for (value, hlen) in row.iter().zip(&column_widths) {
                let span = Span::from((*value).as_str());
                buf.set_span(x_offset_header, y_offset, &span, *hlen);
                x_offset_header += hlen;
            }
            y_offset += 1;
        }

    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let filename = args.get(1).expect("Filename not provided");
    println!("filename: {}", filename);

    let inner_rdr = sushi_csv::Reader::from_path(filename).unwrap();
    let mut csvlens_reader = csv::CsvLensReader::new(inner_rdr);
    let rows = csvlens_reader.get_rows(0, 30).unwrap();
    let headers = &csvlens_reader.headers;

    let stdout = io::stdout().into_raw_mode().unwrap();
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let events = Events::new();

    loop {
        terminal.draw(|f| {

            let size = f.size();

            let csv_table = CsvTable::new(&headers, &rows);
            let mut csv_table_state = CsvTableState {};

            f.render_stateful_widget(csv_table, size, &mut csv_table_state);

        }).unwrap();

        if let Event::Input(key) = events.next().unwrap() {
            match key {
                Key::Char('q') => {
                    break;
                }
                _ => {}
            }
        };
    }
}
