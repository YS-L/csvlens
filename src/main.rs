#[allow(dead_code)]
mod util;
mod csv;
mod input;
mod view;
mod find;
use crate::input::{InputHandler, Control, InputMode};

extern crate csv as sushi_csv;

use std::io;
use std::env;
use std::usize;
use tui::Terminal;
use tui::backend::TermionBackend;
use tui::widgets::Widget;
use tui::widgets::{StatefulWidget, Block, Borders};
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::text::{Span, Spans};
use tui::style::{Style, Modifier, Color};
use termion::{raw::IntoRawMode, screen::AlternateScreen};
use anyhow::{Context, Result};

#[derive(Debug)]
pub struct CsvTable<'a> {
    header: Vec<String>,
    rows: &'a [Vec<String>],
}

impl<'a> CsvTable<'a> {

    fn new(header: &[String], rows: &'a [Vec<String>]) -> Self {
        let _header = header.to_vec();
        Self {
            header: _header,
            rows,
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
            *w += 4;
        }
        column_widths
    }

    fn render_row_numbers(
        &self,
        buf: &mut Buffer,
        state: &CsvTableState,
        area: Rect,
        num_rows: usize,
    ) -> u16 {

        // TODO: better to derminte width from total number of records, so this is always fixed
        let max_row_num = state.rows_offset as usize + num_rows + 1;
        let mut section_width = format!("{}", max_row_num).len() as u16;

        // Render line numbers
        let y_first_record = area.y;
        let mut y = area.y;
        for i in 0..num_rows {
            let row_num = i + state.rows_offset as usize + 1;
            let row_num_formatted = format!("{}", row_num);
            let style = Style::default()
                .fg(Color::Rgb(64, 64, 64));
            let span = Span::styled(row_num_formatted, style);
            buf.set_span(0, y, &span, section_width);
            y += 1;
            if y >= area.bottom() {
                break;
            }
        }
        section_width = section_width + 2 + 1;  // one char reserved for line; add one for symmetry

        // Render vertical separator
        let line_number_block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(Color::Rgb(64, 64, 64)));
        let line_number_area = Rect::new(
            0,
            y_first_record,
            section_width,
            area.height,
        );
        line_number_block.render(line_number_area, buf);
        section_width += 2;

        section_width
    }

    fn render_header_borders(&self, buf: &mut Buffer, area: Rect) -> (u16, u16) {
        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::Rgb(64, 64, 64)));
        let height = 3;
        let area = Rect::new(0, 0, area.width, height);
        block.render(area, buf);
        // y pos of header text and next line
        (height.saturating_sub(2), height)
    }

    fn render_row(
        &self,
        buf: &mut Buffer,
        state: &mut CsvTableState,
        column_widths: &[u16],
        area: Rect,
        x: u16,
        y: u16,
        is_header: bool,
        row: &[String],
        row_index: Option<usize>,
    ) {
        let mut x_offset_header = x;
        let mut remaining_width = area.width.saturating_sub(x);
        let cols_offset = state.cols_offset as usize;
        let mut has_more_cols_to_show = false;
        let mut num_cols_rendered = 0;
        for (col_index, (hname, &hlen)) in row.iter().zip(column_widths).enumerate() {
            if col_index < cols_offset {
                continue;
            }
            if remaining_width < hlen {
                has_more_cols_to_show = true;
                break;
            }
            let mut style = Style::default();
            if is_header {
                style = style.add_modifier(Modifier::BOLD);

            }
            match &state.highlight_state {
                HighlightState::Enabled(p, _highlighted) if (*hname).contains(p) => {
                    let mut highlight_style = Style::default().fg(Color::Rgb(200, 0, 0));
                    if let Some(hl) = _highlighted {
                        if let Some(row_index) = row_index {
                            // TODO: vec::contains slow or does it even matter?
                            if row_index == hl.row_index() && hl.column_indices().contains(&col_index) {
                                highlight_style = highlight_style.bg(Color::LightYellow);
                            }
                        }
                    }
                    let p_span = Span::styled((*p).as_str(), highlight_style);
                    let splitted = (*hname).split((*p).as_str());
                    let mut spans = vec![];
                    for part in splitted {
                        let span = Span::styled(part, style);
                        spans.push(span);
                        spans.push(p_span.clone());
                    }
                    spans.pop();
                    let spans = Spans::from(spans);
                    buf.set_spans(x_offset_header, y, &spans, hlen);
                }
                _ => {
                    let span = Span::styled((*hname).as_str(), style);
                    buf.set_span(x_offset_header, y, &span, hlen);
                }
            };
            x_offset_header += hlen;
            remaining_width = remaining_width.saturating_sub(hlen);
            num_cols_rendered += 1;
        }
        state.set_num_cols_rendered(num_cols_rendered);
        state.set_more_cols_to_show(has_more_cols_to_show);
    }

    fn render_status(&self, area: Rect, buf: &mut Buffer, state: &mut CsvTableState) {

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Rgb(64, 64, 64)));
        block.render(area, buf);
        let style = Style::default().fg(Color::Rgb(128, 128, 128));

        let mut content: String;
        if let BufferState::Enabled(buffer_mode, buf) = &state.buffer_content {
            content = buf.to_owned();
            match buffer_mode {
                InputMode::GotoLine => {
                    content = format!("Go to line: {}", content);
                }
                InputMode::Find => {
                    content = format!("Find: {}", content);
                }
                _ => {}
            }
        }
        else {
            content = state.filename.to_string();

            let total_str = if state.total_line_number.is_some() {
                format!("{}", state.total_line_number.unwrap())
            }  else {
                "?".to_owned()
            };
            content += format!(
                " [Row {}/{}, Col {}/{}]",
                state.rows_offset + 1,
                total_str,
                state.cols_offset + 1,
                state.total_cols,
            ).as_str();

            if let FinderState::FinderActive(s) = &state.finder_state {
                content += format!(" {}", s.status_line()).as_str();
            }

            if let Some(elapsed) = state.elapsed {
                content += format!(" [{}ms]", elapsed).as_str();
            }

            if !state.debug.is_empty() {
                content += format!(" (debug: {})", state.debug).as_str();
            }
        }
        let span = Span::styled(content, style);
        buf.set_span(area.x, area.bottom().saturating_sub(1), &span, area.width);
    }
}

impl<'a> StatefulWidget for CsvTable<'a> {
    type State = CsvTableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {

        // TODO: draw relative to the provided area

        if area.area() == 0 {
            return;
        }

        let status_height = 2;
        let column_widths = self.get_column_widths();
        let (y_header, y_first_record) = self.render_header_borders(buf, area);

        // row area: including row numbers and row content
        let rows_area = Rect::new(
            area.x,
            y_first_record,
            area.width,
            area.height.saturating_sub(y_first_record).saturating_sub(status_height),
        );

        let row_num_section_width = self.render_row_numbers(
            buf,
            state,
            rows_area,
            self.rows.len(),
        );

        self.render_row(
            buf,
            state,
            &column_widths,
            rows_area,
            row_num_section_width,
            y_header,
            true,
            &self.header,
            None,
        );

        let mut y_offset = y_first_record;
        for (rel_row_index, row) in self.rows.iter().enumerate() {
            let row_index = rel_row_index.saturating_add(state.rows_offset as usize);
            self.render_row(
                buf,
                state,
                &column_widths,
                rows_area,
                row_num_section_width,
                y_offset,
                false,
                row,
                Some(row_index),
            );
            y_offset += 1;
            if y_offset >= rows_area.bottom() {
                break;
            }
        }

        let status_area = Rect::new(
            area.x,
            area.bottom().saturating_sub(status_height),
            area.width,
            status_height,
        );
        self.render_status(status_area, buf, state);
    }
}

#[derive(Debug)]
pub enum HighlightState {
    Disabled,
    Enabled(String, Option<find::FoundRecord>),
}

pub enum BufferState {
    Disabled,
    Enabled(InputMode, String),
}

pub enum FinderState {
    FinderInactive,
    FinderActive(FinderActiveState),
}

impl FinderState {

    fn from_finder(finder: &find::Finder) -> FinderState {
        let active_state = FinderActiveState::new(finder);
        FinderState::FinderActive(active_state)
    }

}

pub struct FinderActiveState {
    find_complete: bool,
    total_found: u64,
    cursor_index: Option<u64>,
    target: String,
}

impl FinderActiveState {

    fn new(finder: &find::Finder) -> Self {
        FinderActiveState {
            find_complete: finder.done(),
            total_found: finder.count() as u64,
            cursor_index: finder.cursor().map(|x| x as u64),
            target: finder.target(),
        }
    }

    fn status_line(&self) -> String {
        let plus_marker;
        let line;
        if self.total_found == 0 {
            if self.find_complete {
                line = "Not found".to_owned();
            }
            else {
                line = "Finding...".to_owned();
            }
        }
        else {
            if self.find_complete {
                plus_marker = "";
            }
            else {
                plus_marker = "+";
            }
            let cursor_str = if self.cursor_index.is_none() {
                "-".to_owned()
            } else {
                (self.cursor_index.unwrap() + 1).to_string()
            };
            line = format!(
                "{}/{}{}",
                cursor_str,
                self.total_found,
                plus_marker,
            );
        }
        format!("[\"{}\": {}]", self.target, line)
    }
}

pub struct CsvTableState {
    // TODO: types appropriate?
    rows_offset: u64,
    cols_offset: u64,
    num_cols_rendered: u64,
    more_cols_to_show: bool,
    filename: String,
    total_line_number: Option<usize>,
    total_cols: usize,
    elapsed: Option<f64>,
    buffer_content: BufferState,
    // TODO: highlight_state and finder_state should be combined?
    highlight_state: HighlightState,
    finder_state: FinderState,
    debug: String,
}

impl CsvTableState {

    fn new(filename: String, total_cols: usize) -> Self {
        Self {
            rows_offset: 0,
            cols_offset: 0,
            num_cols_rendered: 0,
            more_cols_to_show: true,
            filename,
            total_line_number: None,
            total_cols,
            elapsed: None,
            buffer_content: BufferState::Disabled,
            highlight_state: HighlightState::Disabled,
            finder_state: FinderState::FinderInactive,
            debug: "".into(),
        }
    }

    fn set_rows_offset(&mut self, offset: u64) {
        self.rows_offset = offset;
    }

    fn set_cols_offset(&mut self, offset: u64) {
        self.cols_offset = offset;
    }

    fn set_more_cols_to_show(&mut self, value: bool) {
        self.more_cols_to_show = value;
    }

    fn has_more_cols_to_show(&mut self) -> bool {
        self.more_cols_to_show
    }

    fn set_num_cols_rendered(&mut self, n: u64) {
        self.num_cols_rendered = n;
    }

    fn set_total_line_number(&mut self, n: usize) {
        self.total_line_number = Some(n);
    }

    fn set_buffer(&mut self, mode: InputMode, buf: &str) {
        self.buffer_content = BufferState::Enabled(mode, buf.to_string());
    }

    fn reset_buffer(&mut self) {
        self.buffer_content = BufferState::Disabled;
    }

    fn set_hightlight_record(&mut self, found_record: find::FoundRecord) {
        if let HighlightState::Enabled(p, _) = &self.highlight_state {
            self.highlight_state = HighlightState::Enabled(p.to_string(), Some(found_record));
        }
    }

}

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

    csv_table_state.set_hightlight_record(found_record);
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
            Control::ScrollToNextFound => {
                if let Some(fdr) = finder.as_mut() {
                    if let Some(found_record) = fdr.next() {
                        scroll_to_found_record(found_record, &mut rows_view, &mut csv_table_state);
                    }
                }
            }
            Control::ScrollToPrevFound => {
                if let Some(fdr) = finder.as_mut() {
                    if let Some(found_record) = fdr.prev() {
                        scroll_to_found_record(found_record, &mut rows_view, &mut csv_table_state);
                    }
                }
            }
            Control::Find(s) => {
                finder = Some(find::Finder::new(filename, s.as_str()).unwrap());
                first_found_scrolled = false;
                csv_table_state.reset_buffer();
                csv_table_state.highlight_state = HighlightState::Enabled(s, None);
            }
            Control::BufferContent(buf) => {
                csv_table_state.set_buffer(input_handler.mode(), buf.as_str());
            }
            Control::BufferReset => {
                csv_table_state.reset_buffer();
                if finder.is_some() {
                    finder = None;
                    csv_table_state.finder_state = FinderState::FinderInactive;
                    csv_table_state.highlight_state = HighlightState::Disabled;
                }
            }
            _ => {}
        }

        if let Some(fdr) = finder.as_mut() {

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