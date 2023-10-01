use crate::csv::Row;
use crate::find;
use crate::input::InputMode;
use crate::view;
use crate::wrap;
use regex::Regex;
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::style::{Color, Modifier, Style};
use tui::symbols::line;
use tui::text::{Span, Spans};
use tui::widgets::Widget;
use tui::widgets::{Block, Borders, StatefulWidget};

use std::cmp::{max, min};

const NUM_SPACES_BETWEEN_COLUMNS: u16 = 4;
const MAX_COLUMN_WIDTH_FRACTION: f32 = 0.3;

#[derive(Debug)]
pub struct CsvTable<'a> {
    header: Vec<String>,
    rows: &'a [Row],
}

impl<'a> CsvTable<'a> {
    pub fn new(header: &[String], rows: &'a [Row]) -> Self {
        let _header = header.to_vec();
        Self {
            header: _header,
            rows,
        }
    }
}

impl<'a> CsvTable<'a> {
    fn get_column_widths(&self, area_width: u16) -> Vec<u16> {
        let mut column_widths = Vec::new();
        for s in &self.header {
            column_widths.push(s.len() as u16);
        }
        for row in self.rows.iter() {
            for (i, value) in row.fields.iter().enumerate() {
                if i >= column_widths.len() {
                    continue;
                }
                let v = column_widths.get_mut(i).unwrap();
                value.split('\n').for_each(|x| {
                    let value_len = x.len() as u16;
                    if *v < value_len {
                        *v = value_len;
                    }
                });
            }
        }
        for w in &mut column_widths {
            *w += NUM_SPACES_BETWEEN_COLUMNS;
            *w = min(*w, (area_width as f32 * MAX_COLUMN_WIDTH_FRACTION) as u16);
        }
        column_widths
    }

    fn get_row_heights(
        &self,
        rows: &[Row],
        column_widths: &[u16],
        enable_line_wrap: bool,
    ) -> Vec<u16> {
        if !enable_line_wrap {
            return rows.iter().map(|_| 1).collect();
        }
        let mut row_heights = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            for (j, content) in row.fields.iter().enumerate() {
                let mut num_lines = 0;
                for parts in content.split('\n') {
                    let parts_length = parts.chars().count();
                    num_lines += max(
                        1,
                        match column_widths.get(j) {
                            Some(w) => {
                                let usable_width = (*w).saturating_sub(NUM_SPACES_BETWEEN_COLUMNS);
                                (parts_length as f32 / usable_width as f32).ceil() as u16
                            }
                            None => 1,
                        },
                    );
                }
                if let Some(height) = row_heights.get_mut(i) {
                    if *height < num_lines {
                        *height = num_lines;
                    }
                } else {
                    row_heights.push(num_lines);
                }
            }
        }
        row_heights
    }

    fn render_row_numbers(
        &self,
        buf: &mut Buffer,
        state: &mut CsvTableState,
        area: Rect,
        rows: &[Row],
        view_layout: &ViewLayout,
    ) -> u16 {
        // TODO: better to determine width from total number of records, so this is always fixed
        let max_row_num = rows.iter().map(|x| x.record_num).max().unwrap_or(0);
        let mut section_width = format!("{max_row_num}").len() as u16;

        // Render line numbers
        let y_first_record = area.y;
        let mut y = area.y;
        for (i, row) in rows.iter().enumerate() {
            let row_num_formatted = row.record_num.to_string();
            let mut style = Style::default().fg(Color::Rgb(64, 64, 64));
            if let Some(selection) = &state.selection {
                if selection.row.is_selected(i) {
                    style = style
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::UNDERLINED);
                }
            }
            let span = Span::styled(row_num_formatted, style);
            buf.set_span(0, y, &span, section_width);
            y += view_layout.row_heights[i];
            if y >= area.bottom() {
                break;
            }
        }
        section_width = section_width + 2 + 1; // one char reserved for line; add one for symmetry

        state.borders_state = Some(BordersState {
            x_row_separator: section_width,
            y_first_record,
        });

        // Add more space before starting first column
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

    fn render_other_borders(&self, buf: &mut Buffer, area: Rect, state: &CsvTableState) {
        // TODO: maybe should be combined with render_header_borders() above
        // Render vertical separator
        if state.borders_state.is_none() {
            return;
        }

        let borders_state = state.borders_state.as_ref().unwrap();
        let y_first_record = borders_state.y_first_record;
        let section_width = borders_state.x_row_separator;

        let line_number_block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(Color::Rgb(64, 64, 64)));
        let line_number_area = Rect::new(0, y_first_record, section_width, area.height);
        line_number_block.render(line_number_area, buf);

        // Intersection with header separator
        buf.get_mut(section_width - 1, y_first_record - 1)
            .set_symbol(line::HORIZONTAL_DOWN);

        // Status separator at the bottom (rendered here first for the interesection)
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Rgb(64, 64, 64)));
        let status_separator_area = Rect::new(0, y_first_record + area.height, area.width, 1);
        block.render(status_separator_area, buf);

        // Intersection with bottom separator
        buf.get_mut(section_width - 1, y_first_record + area.height)
            .set_symbol(line::HORIZONTAL_UP);

        // Vertical line after last rendered column
        // TODO: refactor
        let col_ending_pos_x = state.col_ending_pos_x;
        if !state.has_more_cols_to_show() && col_ending_pos_x < area.right() {
            buf.get_mut(col_ending_pos_x, y_first_record.saturating_sub(1))
                .set_style(Style::default().fg(Color::Rgb(64, 64, 64)))
                .set_symbol(line::HORIZONTAL_DOWN);

            for y in y_first_record..y_first_record + area.height {
                buf.get_mut(col_ending_pos_x, y)
                    .set_style(Style::default().fg(Color::Rgb(64, 64, 64)))
                    .set_symbol(line::VERTICAL);
            }

            buf.get_mut(col_ending_pos_x, y_first_record + area.height)
                .set_style(Style::default().fg(Color::Rgb(64, 64, 64)))
                .set_symbol(line::HORIZONTAL_UP);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        buf: &mut Buffer,
        state: &mut CsvTableState,
        column_widths: &[u16],
        area: Rect,
        x: u16,
        y: u16,
        row_type: RowType,
        row: &'a [String],
        row_index: Option<usize>,
        view_layout: &ViewLayout,
        remaining_height: Option<u16>,
    ) -> u16 {
        let mut x_offset_header = x;
        let mut remaining_width = area.width.saturating_sub(x);
        let cols_offset = state.cols_offset as usize;
        // TODO: seems strange that these have to be set every row
        let mut has_more_cols_to_show = false;
        let mut col_ending_pos_x = 0;
        let mut num_cols_rendered: u64 = 0;
        let row_height = match row_type {
            RowType::Header => 1,
            RowType::Record(i) => match remaining_height {
                Some(h) => min(h, view_layout.row_heights[i]),
                None => view_layout.row_heights[i],
            },
        };
        for (col_index, (hname, &hlen)) in row.iter().zip(column_widths).enumerate() {
            if col_index < cols_offset {
                continue;
            }
            let effective_width = min(remaining_width, hlen);
            let mut content_style = Style::default();
            if let RowType::Header = row_type {
                content_style = content_style.add_modifier(Modifier::BOLD);
                if let Some(selection) = &state.selection {
                    if selection.column.is_selected(num_cols_rendered as usize) {
                        content_style = content_style.add_modifier(Modifier::UNDERLINED);
                    }
                }
            }
            let is_selected = if let Some(selection) = &state.selection {
                Self::is_position_selected(selection, &row_type, num_cols_rendered)
            } else {
                false
            };
            let mut filler_style = Style::default();
            if is_selected {
                let selected_style = Style::default()
                    .fg(Color::Rgb(192, 192, 192))
                    .bg(Color::Rgb(64, 64, 64))
                    .add_modifier(Modifier::BOLD);
                filler_style = filler_style.patch(selected_style);
                content_style = content_style.patch(selected_style);
            }
            let short_padding = match &state.selection {
                Some(selection) => !matches!(selection.selection_type(), view::SelectionType::Row),
                None => false,
            };
            let filler_style = FillerStyle {
                style: filler_style,
                short_padding,
            };
            match &state.finder_state {
                // TODO: seems like doing a bit too much of heavy lifting of
                // checking for matches (finder's work)
                FinderState::FinderActive(active)
                    if active.target.is_match(hname) && !matches!(row_type, RowType::Header) =>
                {
                    let mut highlight_style = filler_style.style.fg(Color::Rgb(200, 0, 0));
                    if let Some(hl) = &active.found_record {
                        if let Some(row_index) = row_index {
                            // TODO: vec::contains slow or does it even matter?
                            if row_index == hl.row_index()
                                && hl.column_indices().contains(&col_index)
                            {
                                highlight_style = highlight_style.bg(Color::LightYellow);
                            }
                        }
                    }
                    let spans = CsvTable::get_highlighted_spans(
                        active,
                        hname,
                        content_style,
                        highlight_style,
                    );
                    self.set_spans(
                        buf,
                        &spans,
                        x_offset_header,
                        y,
                        effective_width,
                        row_height,
                        filler_style,
                    );
                }
                _ => {
                    let span = Span::styled((*hname).as_str(), content_style);
                    self.set_spans(
                        buf,
                        &[span],
                        x_offset_header,
                        y,
                        effective_width,
                        row_height,
                        filler_style,
                    );
                }
            };
            x_offset_header += hlen;
            col_ending_pos_x = x_offset_header;
            num_cols_rendered += 1;
            if remaining_width < hlen {
                has_more_cols_to_show = true;
                break;
            }
            remaining_width = remaining_width.saturating_sub(hlen);
        }
        state.set_num_cols_rendered(num_cols_rendered);
        state.set_more_cols_to_show(has_more_cols_to_show);
        state.col_ending_pos_x = col_ending_pos_x;
        row_height
    }

    fn is_position_selected(
        selection: &view::Selection,
        row_type: &RowType,
        num_cols_rendered: u64,
    ) -> bool {
        match selection.selection_type() {
            view::SelectionType::Row => {
                if let RowType::Record(i) = *row_type {
                    selection.row.is_selected(i)
                } else {
                    false
                }
            }
            view::SelectionType::Column => {
                if let RowType::Record(_) = *row_type {
                    selection.column.is_selected(num_cols_rendered as usize)
                } else {
                    false
                }
            }
            view::SelectionType::Cell => {
                if let RowType::Record(i) = *row_type {
                    selection.row.is_selected(i)
                        && selection.column.is_selected(num_cols_rendered as usize)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn get_highlighted_spans(
        active: &FinderActiveState,
        hname: &'a str,
        style: Style,
        highlight_style: Style,
    ) -> Vec<Span<'a>> {
        // Each span can only have one style, hence split content into matches and non-matches and
        // set styles accordingly
        let mut matches = active.target.find_iter(hname);
        let non_matches = active.target.split(hname);
        let mut spans = vec![];
        for part in non_matches {
            let span = Span::styled(part, style);
            let cur_match = if let Some(m) = matches.next() {
                m.as_str()
            } else {
                ""
            };
            let p_span = Span::styled(cur_match, highlight_style);
            spans.push(span);
            spans.push(p_span);
        }
        spans.pop();
        spans
    }

    #[allow(clippy::too_many_arguments)]
    fn set_spans(
        &self,
        buf: &mut Buffer,
        spans: &[Span],
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        filler_style: FillerStyle,
    ) {
        const SUFFIX: &str = "…";
        const SUFFIX_LEN: u16 = 1;

        // Reserve some space before the next column (same number used in get_column_widths)
        let effective_width = width.saturating_sub(NUM_SPACES_BETWEEN_COLUMNS);

        let buffer_space = if filler_style.short_padding {
            NUM_SPACES_BETWEEN_COLUMNS / 2
        } else {
            NUM_SPACES_BETWEEN_COLUMNS
        } as usize;

        let mut spans_wrapper = wrap::SpansWrapper::new(spans, effective_width as usize);

        for offset in 0..height {
            if let Some(mut spans) = spans_wrapper.next() {
                // There is some content to render. Truncate with ... if there is no more vertical
                // space available.
                if offset == height - 1 && !spans_wrapper.finished() {
                    if let Some(last_span) = spans.0.pop() {
                        let truncate_length = last_span.width().saturating_sub(SUFFIX_LEN as usize);
                        let truncated_content: String =
                            last_span.content.chars().take(truncate_length).collect();
                        let truncated_span = Span::styled(truncated_content, last_span.style);
                        spans.0.push(truncated_span);
                        spans.0.push(Span::styled(SUFFIX, last_span.style));
                    }
                }
                let padding_width = min(
                    (effective_width as usize).saturating_sub(spans.width()) + buffer_space,
                    width as usize,
                );
                if padding_width > 0 {
                    spans
                        .0
                        .push(Span::styled(" ".repeat(padding_width), filler_style.style));
                }
                buf.set_spans(x, y + offset, &spans, width);
            } else {
                // There are extra vertical spaces that are just empty lines. Fill them with the
                // correct style.
                let mut content =
                    " ".repeat(min(effective_width as usize + buffer_space, width as usize));

                // It's possible that no spans are yielded due to insufficient remaining width.
                // Render ... in this case.
                if !spans_wrapper.finished() {
                    let truncated_content: String = content
                        .chars()
                        .take(content.len().saturating_sub(1))
                        .collect();
                    content = format!("{SUFFIX}{}", truncated_content.as_str());
                }
                let span = Span::styled(content, filler_style.style);
                buf.set_spans(x, y + offset, &Spans::from(vec![span]), width);
            }
        }
    }

    fn render_status(&self, area: Rect, buf: &mut Buffer, state: &mut CsvTableState) {
        // Content of status line (separator already plotted elsewhere)
        let style = Style::default().fg(Color::Rgb(128, 128, 128));
        let mut content: String;
        if let Some(msg) = &state.transient_message {
            content = msg.to_owned();
        } else if let BufferState::Enabled(buffer_mode, buf) = &state.buffer_content {
            content = buf.to_owned();
            let format_buffer = |prefix: &str| format!("{prefix}: {content}█");
            match buffer_mode {
                InputMode::GotoLine => {
                    content = format_buffer("Go to line");
                }
                InputMode::Find => {
                    content = format_buffer("Find");
                }
                InputMode::Filter => {
                    content = format_buffer("Filter");
                }
                InputMode::FilterColumns => {
                    content = format_buffer("Columns regex");
                }
                InputMode::Option => content = format_buffer("Option"),
                InputMode::Default => {}
            }
        } else {
            // Filename
            if let Some(f) = &state.filename {
                content = f.to_string();
            } else {
                content = "stdin".to_string();
            }

            // Row / Col
            let total_str = if state.total_line_number.is_some() {
                format!("{}", state.total_line_number.unwrap())
            } else {
                "?".to_owned()
            };
            let current_row;
            if let Some(selection) = &state.selection {
                current_row = if let Some(i) = selection.row.index() {
                    self.rows.get(i as usize)
                } else {
                    self.rows.first()
                }
            } else {
                current_row = self.rows.first()
            }

            let row_num = match current_row {
                Some(row) => row.record_num.to_string(),
                _ => "-".to_owned(),
            };
            content += format!(
                " [Row {}/{}, Col {}/{}]",
                row_num,
                total_str,
                state.cols_offset + 1,
                state.total_cols,
            )
            .as_str();

            // Finder
            if let FinderState::FinderActive(s) = &state.finder_state {
                content += format!(" {}", s.status_line()).as_str();
            }

            if let Some(stats_line) = &state.debug_stats.status_line() {
                content += format!(" {stats_line}").as_str();
            }

            // Filter columns
            if let FilterColumnsState::Enabled(info) = &state.filter_columns_state {
                content += format!(" {}", info.status_line()).as_str();
            }

            // Echo option
            if let Some(column_name) = &state.echo_column {
                content += format!(" [Echo {column_name} ↵]").as_str();
            }

            // Ignore case option
            if state.ignore_case {
                content += " [ignore-case]";
            }

            // Debug
            if !state.debug.is_empty() {
                content += format!(" (debug: {})", state.debug).as_str();
            }
        }
        let span = Span::styled(content, style);
        buf.set_span(area.x, area.bottom().saturating_sub(1), &span, area.width);
    }

    fn get_view_layout(&self, area: Rect, state: &CsvTableState) -> ViewLayout {
        let column_widths = self.get_column_widths(area.width);
        let row_heights = self.get_row_heights(self.rows, &column_widths, state.enable_line_wrap);
        ViewLayout {
            column_widths,
            row_heights,
        }
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

        let layout = self.get_view_layout(area, state);
        state.view_layout = Some(layout.clone());

        let (y_header, y_first_record) = self.render_header_borders(buf, area);

        // row area: including row numbers and row content
        let rows_area = Rect::new(
            area.x,
            y_first_record,
            area.width,
            area.height
                .saturating_sub(y_first_record)
                .saturating_sub(status_height),
        );

        let row_num_section_width =
            self.render_row_numbers(buf, state, rows_area, self.rows, &layout);

        self.render_row(
            buf,
            state,
            &layout.column_widths,
            rows_area,
            row_num_section_width,
            y_header,
            RowType::Header,
            &self.header,
            None,
            &layout,
            None,
        );

        let mut remaining_height = rows_area.height;
        let mut y_offset = y_first_record;
        for (i, row) in self.rows.iter().enumerate() {
            let rendered_height = self.render_row(
                buf,
                state,
                &layout.column_widths,
                rows_area,
                row_num_section_width,
                y_offset,
                RowType::Record(i),
                &row.fields,
                Some(row.record_num - 1),
                &layout,
                Some(remaining_height),
            );
            remaining_height = remaining_height.saturating_sub(rendered_height);
            y_offset += rendered_height;
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

        self.render_other_borders(buf, rows_area, state);
    }
}

pub enum RowType {
    /// Header row
    Header,
    /// Regular row. Contains the row index (not the record number) and the row itself.
    Record(usize),
}

/// Style to use for the fillers (spaces and elipses) between columns
#[derive(Clone, Copy)]
struct FillerStyle {
    style: Style,
    short_padding: bool,
}

#[derive(Clone)]
pub struct ViewLayout {
    pub column_widths: Vec<u16>,
    pub row_heights: Vec<u16>,
}

impl ViewLayout {
    pub fn num_rows_renderable(&self, frame_height: u16) -> usize {
        let mut out = 0;
        let mut remaining = frame_height;
        for h in &self.row_heights {
            if *h > remaining {
                if remaining > 0 {
                    // Include partially rendered row
                    out += 1;
                }
                break;
            }
            out += 1;
            remaining -= h;
        }
        out
    }
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
    pub fn from_finder(finder: &find::Finder, rows_view: &view::RowsView) -> FinderState {
        let active_state = FinderActiveState::new(finder, rows_view);
        FinderState::FinderActive(active_state)
    }
}

pub struct FinderActiveState {
    find_complete: bool,
    total_found: u64,
    cursor_index: Option<u64>,
    target: Regex,
    found_record: Option<find::FoundRecord>,
    selected_offset: Option<u64>,
    is_filter: bool,
}

impl FinderActiveState {
    pub fn new(finder: &find::Finder, rows_view: &view::RowsView) -> Self {
        FinderActiveState {
            find_complete: finder.done(),
            total_found: finder.count() as u64,
            cursor_index: finder.cursor().map(|x| x as u64),
            target: finder.target(),
            found_record: finder.current(),
            selected_offset: rows_view.selected_offset(),
            is_filter: rows_view.is_filter(),
        }
    }

    fn status_line(&self) -> String {
        let plus_marker;
        let line;
        if self.total_found == 0 {
            if self.find_complete {
                line = "Not found".to_owned();
            } else {
                line = "Finding...".to_owned();
            }
        } else {
            if self.find_complete {
                plus_marker = "";
            } else {
                plus_marker = "+";
            }
            let cursor_str;
            if self.is_filter {
                if let Some(i) = self.selected_offset {
                    cursor_str = i.saturating_add(1).to_string();
                } else {
                    cursor_str = "-".to_owned();
                }
            } else if let Some(i) = self.cursor_index {
                cursor_str = (i.saturating_add(1)).to_string();
            } else {
                cursor_str = "-".to_owned();
            }
            line = format!("{cursor_str}/{}{plus_marker}", self.total_found);
        }
        let action = if self.is_filter { "Filter" } else { "Find" };
        format!("[{action} \"{}\": {line}]", self.target)
    }
}

pub enum FilterColumnsState {
    Disabled,
    Enabled(FilterColumnsInfo),
}

impl FilterColumnsState {
    pub fn from_rows_view(rows_view: &view::RowsView) -> Self {
        if let Some(columns_filter) = rows_view.columns_filter() {
            Self::Enabled(FilterColumnsInfo {
                pattern: columns_filter.pattern(),
                shown: columns_filter.num_filtered(),
                total: columns_filter.num_original(),
                disabled_because_no_match: columns_filter.disabled_because_no_match(),
            })
        } else {
            Self::Disabled
        }
    }
}

pub struct FilterColumnsInfo {
    pattern: Regex,
    shown: usize,
    total: usize,
    disabled_because_no_match: bool,
}

impl FilterColumnsInfo {
    fn status_line(&self) -> String {
        let mut line;
        line = format!("[Filter \"{}\": ", self.pattern);
        if self.disabled_because_no_match {
            line += "no match, showing all columns]";
        } else {
            line += format!("{}/{} cols]", self.shown, self.total).as_str();
        }
        line
    }
}

struct BordersState {
    x_row_separator: u16,
    y_first_record: u16,
}

pub struct DebugStats {
    rows_view_elapsed: Option<f64>,
    finder_elapsed: Option<f64>,
}

impl DebugStats {
    pub fn new() -> Self {
        DebugStats {
            rows_view_elapsed: None,
            finder_elapsed: None,
        }
    }

    pub fn rows_view_elapsed(&mut self, elapsed: Option<u128>) {
        self.rows_view_elapsed = elapsed.map(|e| e as f64 / 1000.0);
    }

    pub fn finder_elapsed(&mut self, elapsed: Option<u128>) {
        self.finder_elapsed = elapsed.map(|e| e as f64 / 1000.0);
    }

    pub fn status_line(&self) -> Option<String> {
        let mut line = "[".to_string();
        if let Some(elapsed) = self.rows_view_elapsed {
            line += format!("rows:{elapsed}ms").as_str();
        }
        if let Some(elapsed) = self.finder_elapsed {
            line += format!(" finder:{elapsed}ms").as_str();
        }
        line += "]";
        if line == "[]" {
            None
        } else {
            Some(line)
        }
    }
}

pub struct CsvTableState {
    // TODO: types appropriate?
    pub rows_offset: u64,
    pub cols_offset: u64,
    pub num_cols_rendered: u64,
    pub more_cols_to_show: bool,
    filename: Option<String>,
    total_line_number: Option<usize>,
    total_cols: usize,
    pub debug_stats: DebugStats,
    buffer_content: BufferState,
    pub finder_state: FinderState,
    pub filter_columns_state: FilterColumnsState,
    borders_state: Option<BordersState>,
    // TODO: should probably be with BordersState
    col_ending_pos_x: u16,
    pub selection: Option<view::Selection>,
    pub transient_message: Option<String>,
    pub echo_column: Option<String>,
    pub ignore_case: bool,
    pub view_layout: Option<ViewLayout>,
    pub enable_line_wrap: bool,
    pub debug: String,
}

impl CsvTableState {
    pub fn new(
        filename: Option<String>,
        total_cols: usize,
        echo_column: &Option<String>,
        ignore_case: bool,
    ) -> Self {
        Self {
            rows_offset: 0,
            cols_offset: 0,
            num_cols_rendered: 0,
            more_cols_to_show: true,
            filename,
            total_line_number: None,
            total_cols,
            debug_stats: DebugStats::new(),
            buffer_content: BufferState::Disabled,
            finder_state: FinderState::FinderInactive,
            filter_columns_state: FilterColumnsState::Disabled,
            borders_state: None,
            col_ending_pos_x: 0,
            selection: None,
            transient_message: None,
            echo_column: echo_column.clone(),
            ignore_case,
            view_layout: None,
            enable_line_wrap: false,
            debug: "".into(),
        }
    }

    pub fn set_rows_offset(&mut self, offset: u64) {
        self.rows_offset = offset;
    }

    pub fn set_cols_offset(&mut self, offset: u64) {
        self.cols_offset = offset;
    }

    fn set_more_cols_to_show(&mut self, value: bool) {
        self.more_cols_to_show = value;
    }

    pub fn has_more_cols_to_show(&self) -> bool {
        self.more_cols_to_show
    }

    fn set_num_cols_rendered(&mut self, n: u64) {
        self.num_cols_rendered = n;
    }

    pub fn set_total_line_number(&mut self, n: usize) {
        self.total_line_number = Some(n);
    }

    pub fn set_total_cols(&mut self, n: usize) {
        self.total_cols = n;
    }

    pub fn set_buffer(&mut self, mode: InputMode, buf: &str) {
        self.buffer_content = BufferState::Enabled(mode, buf.to_string());
    }

    pub fn reset_buffer(&mut self) {
        self.buffer_content = BufferState::Disabled;
    }
}
