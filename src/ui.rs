use crate::common::InputMode;
use crate::csv::Row;
use crate::find;
use crate::sort;
use crate::sort::SortOrder;
use crate::sort::SortType;
use crate::theme::Theme;
use crate::view;
use crate::view::Header;
use crate::wrap;
use ansi_to_tui::IntoText as _;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::Position;
use ratatui::style::Styled;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::line;
use ratatui::text::Text;
use ratatui::text::{Line, Span};
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;
use ratatui::widgets::{Block, Borders, StatefulWidget};
use regex::Regex;
use tui_input::Input;

use std::cmp::{max, min};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const NUM_SPACES_AFTER_LINE_NUMBER: u16 = 2;
const NUM_SPACES_BETWEEN_COLUMNS: u16 = 4;
const MAX_COLUMN_WIDTH_FRACTION: f32 = 0.3;

pub fn set_line_safe(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    line: &Line<'_>,
    max_width: u16,
) -> Option<(u16, u16)> {
    if y < buf.area.bottom() {
        Some(buf.set_line(x, y, line, max_width))
    } else {
        None
    }
}

#[derive(Debug)]
pub struct ColumnWidthOverrides {
    overrides: HashMap<usize, u16>,
}

impl ColumnWidthOverrides {
    pub fn new() -> Self {
        Self {
            overrides: HashMap::new(),
        }
    }

    /// Sets the width override for the given origin column index
    pub fn set(&mut self, col_index: usize, width: u16) {
        self.overrides.insert(col_index, width);
    }

    /// Returns the width override for the given origin column index, if any
    pub fn get(&self, col_index: usize) -> Option<&u16> {
        self.overrides.get(&col_index)
    }

    /// Returns the list of origin column indices that have width overrides
    pub fn overriden_indices(&self) -> Vec<usize> {
        self.overrides.keys().copied().collect()
    }

    pub fn reset(&mut self) {
        self.overrides.clear();
    }
}

#[derive(Debug)]
pub struct CsvTable<'a> {
    header: &'a [Header],
    rows: &'a [Row],
}

impl<'a> CsvTable<'a> {
    pub fn new(header: &'a [Header], rows: &'a [Row]) -> Self {
        Self { header, rows }
    }
}

impl<'a> CsvTable<'a> {
    fn get_column_widths(
        &self,
        area_width: u16,
        overrides: &ColumnWidthOverrides,
        sorter_state: &SorterState,
    ) -> Vec<u16> {
        let mut column_widths = Vec::new();

        for h in self.header {
            let column_name = self.get_effective_column_name(h.name.as_str(), sorter_state);
            if let Some(w) = overrides.get(h.origin_index) {
                column_widths.push(*w);
                continue;
            } else {
                column_widths.push(column_name.len() as u16);
            }
        }

        let overriden_indices = overrides.overriden_indices();

        for row in self.rows {
            for (i, value) in row.fields.iter().enumerate() {
                if i >= column_widths.len() {
                    continue;
                }
                if overriden_indices.contains(&self.header.get(i).unwrap().origin_index) {
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

        // Limit maximum width for a column to make way for other columns
        let max_single_column_width = (area_width as f32 * MAX_COLUMN_WIDTH_FRACTION) as u16;
        let mut clipped_columns: Vec<(usize, u16)> = vec![];
        for (i, w) in column_widths.iter_mut().enumerate() {
            if overriden_indices.contains(&self.header.get(i).unwrap().origin_index) {
                *w = max(*w, NUM_SPACES_BETWEEN_COLUMNS);
            } else {
                *w += NUM_SPACES_BETWEEN_COLUMNS;
                if *w > max_single_column_width {
                    clipped_columns.push((i, *w));
                    *w = max_single_column_width;
                }
            }
        }

        // If clipping was too aggressive, redistribute the remaining width
        CsvTable::redistribute_widths_after_clipping(
            &mut column_widths,
            area_width,
            clipped_columns,
        );

        column_widths
    }

    fn redistribute_widths_after_clipping(
        column_widths: &mut [u16],
        area_width: u16,
        mut clipped_columns: Vec<(usize, u16)>,
    ) {
        if clipped_columns.is_empty() {
            // Nothing to adjust
            return;
        }

        let total_width: u16 = column_widths.iter().sum();
        if total_width >= area_width {
            // No need to adjust if we're already using the full width
            return;
        }

        // Greedily adjust from the narrowest column by equally distributing the remaining width. If
        // a column doesn't use the allocated adjustment, subsequent columns will get to use it.
        clipped_columns.sort_by_key(|x| x.1);

        // Subtract 1 to leave space for the right border. If not, this will be too greedy and
        // consume all the space making that border disappear.
        let mut remaining_width = area_width.saturating_sub(total_width).saturating_sub(1);

        let mut num_columns_to_adjust = clipped_columns.len();
        for (i, width_before_clipping) in clipped_columns {
            let adjustment = remaining_width / num_columns_to_adjust as u16;
            let width_after_adjustment = min(width_before_clipping, column_widths[i] + adjustment);
            let added_width = width_after_adjustment - column_widths[i];
            column_widths[i] = width_after_adjustment;
            remaining_width -= added_width;
            num_columns_to_adjust -= 1;
        }
    }

    fn get_row_heights(
        &self,
        area_height: u16,
        rows: &[Row],
        column_widths: &[u16],
        enable_line_wrap: bool,
        is_word_wrap: bool,
    ) -> Vec<u16> {
        if !enable_line_wrap {
            return rows.iter().map(|_| 1).collect();
        }
        let mut total_height = 0;
        let mut row_heights = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            if total_height >= area_height {
                // Exit early if we've already filled the available height. Important since
                // LineWrapper at its current state is not particularly efficient...
                row_heights.push(1);
                continue;
            }
            for (j, content) in row.fields.iter().enumerate() {
                let num_lines = match column_widths.get(j) {
                    Some(w) => {
                        let usable_width = (*w).saturating_sub(NUM_SPACES_BETWEEN_COLUMNS);
                        if usable_width > 0 {
                            let spans = [Span::styled(content.as_str(), Style::default())];
                            let mut line_wrapper =
                                wrap::LineWrapper::new(&spans, usable_width as usize, is_word_wrap);
                            let mut num_lines = 0;
                            loop {
                                line_wrapper.next();
                                num_lines += 1;
                                if line_wrapper.finished() {
                                    break;
                                }
                            }
                            num_lines
                        } else {
                            1
                        }
                    }
                    None => 1,
                };
                if let Some(height) = row_heights.get_mut(i) {
                    if *height < num_lines {
                        *height = num_lines;
                    }
                } else {
                    row_heights.push(num_lines);
                }
            }
            total_height += row_heights[i];
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
    ) {
        // Render line numbers
        let y_first_record = area.y;
        let mut y = area.y;
        for (i, row) in rows.iter().enumerate() {
            let row_num_formatted = row.record_num.to_string();
            let mut style = Style::default().fg(state.theme.row_number);
            if let Some(selection) = &state.selection
                && selection.row.is_selected(i)
            {
                style = style
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED);
            }
            let span = Span::styled(row_num_formatted, style);
            buf.set_span(0, y, &span, view_layout.row_number_layout.max_length);
            y += view_layout.row_heights[i];
            if y >= area.bottom() {
                break;
            }
        }

        state.borders_state = Some(BordersState {
            x_row_separator: view_layout.row_number_layout.x_row_separator,
            y_first_record,
            x_freeze_separator: view_layout.x_freeze_separator,
        });
    }

    fn render_header_borders(&self, buf: &mut Buffer, area: Rect, theme: &Theme) -> (u16, u16) {
        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border));
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

        if area.width < section_width {
            return;
        }

        let border_style = Style::default().fg(state.theme.border);

        // Line number separator
        let line_number_block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(border_style);
        let line_number_area = Rect::new(0, y_first_record, section_width, area.height);
        line_number_block.render(line_number_area, buf);

        // Intersection with header separator
        if let Some(cell) = buf.cell_mut(Position::new(section_width - 1, y_first_record - 1)) {
            cell.set_symbol(line::HORIZONTAL_DOWN);
        }

        // Status separator at the bottom (rendered here first for the interesection)
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(border_style);
        let status_separator_area = Rect::new(0, y_first_record + area.height, area.width, 1);
        block.render(status_separator_area, buf);

        // Intersection with bottom separator
        if let Some(cell) = buf.cell_mut(Position::new(
            section_width - 1,
            y_first_record + area.height,
        )) {
            cell.set_symbol(line::HORIZONTAL_UP);
        }

        // Vertical line after last rendered column
        // TODO: refactor
        let col_ending_pos_x = state.col_ending_pos_x;
        if !state.has_more_cols_to_show() && col_ending_pos_x < area.right() {
            if let Some(cell) = buf.cell_mut(Position::new(
                col_ending_pos_x,
                y_first_record.saturating_sub(1),
            )) {
                cell.set_style(border_style)
                    .set_symbol(line::HORIZONTAL_DOWN);
            }

            for y in y_first_record..y_first_record + area.height {
                if let Some(cell) = buf.cell_mut(Position::new(col_ending_pos_x, y)) {
                    cell.set_style(border_style).set_symbol(line::VERTICAL);
                }
            }

            if let Some(cell) = buf.cell_mut(Position::new(
                col_ending_pos_x,
                y_first_record + area.height,
            )) {
                cell.set_style(border_style).set_symbol(line::HORIZONTAL_UP);
            }
        }

        // Freeze separator
        if let Some(x_freeze_separator) = borders_state.x_freeze_separator {
            // Clear highlight style made by render_row before rendering the separator
            if x_freeze_separator < area.right() {
                let x_freeze_separator_area =
                    Rect::new(x_freeze_separator, y_first_record, 1, area.height);
                Clear.render(x_freeze_separator_area, buf);

                if let Some(cell) = buf.cell_mut(Position::new(
                    x_freeze_separator,
                    y_first_record.saturating_sub(1),
                )) {
                    cell.set_style(border_style).set_symbol("╥");
                }

                for y in y_first_record..y_first_record + area.height {
                    if let Some(cell) = buf.cell_mut(Position::new(x_freeze_separator, y)) {
                        cell.set_style(border_style)
                            .set_symbol(line::DOUBLE_VERTICAL);
                    }
                }

                if let Some(cell) = buf.cell_mut(Position::new(
                    x_freeze_separator,
                    y_first_record + area.height,
                )) {
                    cell.set_style(border_style).set_symbol("╨");
                }
            }
        }
    }

    fn get_effective_column_name(&self, column_name: &str, sorter_state: &SorterState) -> String {
        if let SorterState::Enabled(info) = sorter_state
            && info.status == sort::SorterStatus::Finished
            && info.column_name == column_name
        {
            let indicator = match info.order {
                SortOrder::Ascending => "▴",
                SortOrder::Descending => "▾",
            };

            let sort_type_indicator = match info.sort_type {
                SortType::Natural => "N",
                _ => "",
            };
            return format!("{} [{}{}]", column_name, indicator, sort_type_indicator);
        }
        column_name.to_string()
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
            if !state
                .cols_offset
                .should_filtered_column_index_be_rendered(col_index as u64)
            {
                continue;
            }
            let effective_width = min(remaining_width, hlen);
            let mut content_style = Style::default();
            if state.color_columns {
                content_style = content_style
                    .fg(state.theme.column_colors[col_index % state.theme.column_colors.len()]);
            }
            if let RowType::Header = row_type {
                content_style = content_style.add_modifier(Modifier::BOLD);
                if let Some(selection) = &state.selection
                    && selection.column.is_selected(num_cols_rendered as usize)
                {
                    content_style = content_style.add_modifier(Modifier::UNDERLINED);
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
                    .fg(state.theme.selected_foreground)
                    .bg(state.theme.selected_background)
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

            let should_highlight_cell = |active: &FinderActiveState, content: &str| {
                // Only highlight the selected column in column selection mode. But header search is
                // always across all columns regardless of the selection mode.
                if let Some((target_column_index, _)) = active.column_index
                    && target_column_index != col_index
                    && matches!(row_type, RowType::Record(_))
                {
                    return false;
                }
                if active.is_filter && matches!(row_type, RowType::Header) {
                    return false;
                }
                active.target.is_match(content)
            };
            match &state.finder_state {
                // TODO: seems like doing a bit too much of heavy lifting of
                // checking for matches (finder's work)
                FinderState::FinderActive(active) if should_highlight_cell(active, hname) => {
                    let mut highlight_style = filler_style.style.fg(state.theme.found);
                    if let Some(found_record) = &active.found_record {
                        match found_record {
                            find::FoundEntry::Row(entry) => {
                                if let Some(row_index) = row_index
                                    && row_index == entry.row_index()
                                    && entry.column_index() == col_index
                                {
                                    highlight_style =
                                        highlight_style.bg(state.theme.found_selected_background);
                                }
                            }
                            find::FoundEntry::Header(entry) => {
                                if matches!(row_type, RowType::Header)
                                    && entry.column_index() == col_index
                                {
                                    highlight_style =
                                        highlight_style.bg(state.theme.found_selected_background);
                                }
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
                        state.is_word_wrap,
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
                        state.is_word_wrap,
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
        state.num_cols_rendered = max(state.num_cols_rendered, num_cols_rendered);
        state.update_more_cols_to_show(has_more_cols_to_show);
        state.col_ending_pos_x = max(state.col_ending_pos_x, col_ending_pos_x);
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
            if !part.is_empty() {
                spans.push(Span::styled(part, style));
            }
            if let Some(m) = matches.next() {
                spans.push(Span::styled(m.as_str(), highlight_style));
            }
        }
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
        is_word_wrap: bool,
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

        let mut line_wrapper =
            wrap::LineWrapper::new(spans, effective_width as usize, is_word_wrap);

        for offset in 0..height {
            if let Some(mut line) = line_wrapper.next() {
                // There is some content to render. Truncate with ... if there is no more vertical
                // space available.
                if offset == height - 1
                    && !line_wrapper.finished()
                    && let Some(last_span) = line.spans.pop()
                {
                    let truncate_length = last_span.width().saturating_sub(SUFFIX_LEN as usize);
                    let truncated_content: String =
                        last_span.content.chars().take(truncate_length).collect();
                    let truncated_span = Span::styled(truncated_content, last_span.style);
                    line.spans.push(truncated_span);
                    line.spans.push(Span::styled(SUFFIX, last_span.style));
                }
                let padding_width = min(
                    (effective_width as usize + buffer_space).saturating_sub(line.width()),
                    width as usize,
                );
                if padding_width > 0 {
                    line.spans
                        .push(Span::styled(" ".repeat(padding_width), filler_style.style));
                }
                set_line_safe(buf, x, y + offset, &line, width);
            } else {
                // There are extra vertical spaces that are just empty lines. Fill them with the
                // correct style.
                let mut content =
                    " ".repeat(min(effective_width as usize + buffer_space, width as usize));

                // It's possible that no spans are yielded due to insufficient remaining width.
                // Render ... in this case.
                if !line_wrapper.finished() {
                    let truncated_content: String = content
                        .chars()
                        .take(content.len().saturating_sub(1))
                        .collect();
                    content = format!("{SUFFIX}{}", truncated_content.as_str());
                }
                let span = Span::styled(content, filler_style.style);
                set_line_safe(buf, x, y + offset, &Line::from(vec![span]), width);
            }
        }
    }

    fn render_status(&self, area: Rect, buf: &mut Buffer, state: &mut CsvTableState) {
        // Content of status line (separator already plotted elsewhere)
        let style = Style::default().fg(state.theme.status);
        let mut prompt_text: Text;
        let mut content: String;
        state.cursor_xy = None;
        if let Some(msg) = &state.transient_message {
            prompt_text = Text::default();
            content = msg.to_owned();
        } else if let BufferState::Enabled(buffer_mode, input) = &state.buffer_content {
            prompt_text = Text::default();
            let get_prefix = |&input_mode| {
                let prefix = match input_mode {
                    InputMode::GotoLine => "Go to line",
                    InputMode::Find => "Find",
                    InputMode::Filter => "Filter",
                    InputMode::FilterColumns => "Columns regex",
                    InputMode::Option => "Option",
                    InputMode::FreezeColumns => "Number of columns to freeze",
                    _ => "",
                };
                if prefix.is_empty() {
                    "".to_string()
                } else {
                    format!("{prefix}: ")
                }
            };
            let prefix = get_prefix(buffer_mode);
            content = format!("{prefix}{}", input.value());
            state.cursor_xy = Some((
                area.x
                    .saturating_add(prefix.len() as u16)
                    .saturating_add(input.cursor() as u16),
                area.bottom().saturating_sub(1),
            ));
        } else {
            // User provided prompt
            prompt_text = if let Some(prompt) = &state.prompt {
                prompt.into_text().unwrap_or(Text::default())
            } else {
                Text::default()
            };
            // Filename
            if state.prompt.is_some() {
                content = "".to_string();
            } else if let Some(f) = &state.filename {
                content = f.to_string();
            } else {
                content = "stdin".to_string();
            }

            // Row / Col
            let total_str = match state.total_line_number {
                Some((total, false)) => format!("{}", total),
                Some((total, true)) => format!("{}+", total),
                _ => "?".to_owned(),
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
                state.cols_offset.num_skip + 1,
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

            // Sorter
            if let SorterState::Enabled(info) = &state.sorter_state {
                let sorter_status_line = info.status_line();
                if !sorter_status_line.is_empty() {
                    content += format!(" {}", sorter_status_line).as_str();
                }
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
        prompt_text = prompt_text.set_style(style);
        prompt_text.push_span(Span::from(content));
        let prompt_area = Rect::new(area.x, area.y + 1, area.width, area.height);
        prompt_text.render(prompt_area, buf);
    }

    fn get_view_layout(&self, area: Rect, state: &mut CsvTableState, rows: &[Row]) -> ViewLayout {
        let max_row_num = rows.iter().map(|x| x.record_num).max().unwrap_or(0);
        let max_row_num_length = format!("{max_row_num}").len() as u16;
        let row_num_section_width_with_spaces =
            max_row_num_length + 2 * NUM_SPACES_AFTER_LINE_NUMBER + 1;
        let x_row_separator = max_row_num_length + NUM_SPACES_AFTER_LINE_NUMBER + 1;

        let column_widths = self.get_column_widths(
            area.width.saturating_sub(row_num_section_width_with_spaces),
            &state.column_width_overrides,
            &state.sorter_state,
        );
        let _tic = std::time::Instant::now();
        let row_heights = self.get_row_heights(
            area.height,
            self.rows,
            &column_widths,
            state.enable_line_wrap,
            state.is_word_wrap,
        );
        state.num_cols_rendered = 0;
        state.col_ending_pos_x = 0;

        let row_number_layout = RowNumberLayout {
            max_length: max_row_num_length,
            width_with_spaces: row_num_section_width_with_spaces,
            x_row_separator,
        };

        // x-position of the vertical separator if any columns are frozen
        let x_freeze_separator = if state.cols_offset.num_freeze > 0 {
            let mut x_sum = row_number_layout.x_row_separator;
            for (column_index, width) in column_widths.iter().enumerate() {
                if state.cols_offset.is_frozen(column_index as u64) {
                    x_sum += width;
                }
            }
            Some(x_sum)
        } else {
            None
        };

        ViewLayout {
            column_widths,
            row_heights,
            row_number_layout,
            x_freeze_separator,
        }
    }
}

impl StatefulWidget for CsvTable<'_> {
    type State = CsvTableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // TODO: draw relative to the provided area

        if area.area() == 0 {
            return;
        }

        let status_height = 2;

        let layout = self.get_view_layout(area, state, self.rows);
        state.view_layout = Some(layout.clone());

        let (y_header, y_first_record) = self.render_header_borders(buf, area, &state.theme);

        // row area: including row numbers and row content
        let rows_area = Rect::new(
            area.x,
            y_first_record,
            area.width,
            area.height
                .saturating_sub(y_first_record)
                .saturating_sub(status_height),
        );

        self.render_row_numbers(buf, state, rows_area, self.rows, &layout);
        let row_num_section_width = layout.row_number_layout.width_with_spaces;

        state.reset_more_cols_to_show();
        self.render_row(
            buf,
            state,
            &layout.column_widths,
            rows_area,
            row_num_section_width,
            y_header,
            RowType::Header,
            &self
                .header
                .iter()
                .map(|h| self.get_effective_column_name(h.name.as_str(), &state.sorter_state))
                .collect::<Vec<String>>(),
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

#[derive(Debug, Clone)]
pub struct RowNumberLayout {
    pub max_length: u16,
    pub width_with_spaces: u16,
    pub x_row_separator: u16,
}

#[derive(Debug, Clone)]
pub struct ViewLayout {
    pub column_widths: Vec<u16>,
    pub row_heights: Vec<u16>,
    pub row_number_layout: RowNumberLayout,
    pub x_freeze_separator: Option<u16>,
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
    Enabled(InputMode, Input),
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
    cursor: Option<find::FinderCursor>,
    target: Regex,
    column_index: Option<(usize, String)>,
    found_record: Option<find::FoundEntry>,
    selected_offset: Option<u64>,
    is_filter: bool,
    header_has_match: bool,
}

impl FinderActiveState {
    pub fn new(finder: &find::Finder, rows_view: &view::RowsView) -> Self {
        let header_has_match = finder.header_has_match();
        let total_count = finder.count() + if header_has_match { 1 } else { 0 };
        FinderActiveState {
            find_complete: finder.done(),
            total_found: total_count as u64,
            cursor: finder.cursor(),
            target: finder.target(),
            column_index: finder
                .column_index()
                .map(|i| (i, rows_view.get_column_name_from_local_index(i))),
            found_record: finder.current(),
            selected_offset: rows_view.selected_offset(),
            is_filter: rows_view.is_filter(),
            header_has_match,
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
            } else if let Some(cursor) = &self.cursor {
                cursor_str = match cursor.row {
                    find::RowPos::Row(i) => (i
                        .saturating_add(1)
                        .saturating_add(if self.header_has_match { 1 } else { 0 }))
                    .to_string(),
                    find::RowPos::Header => "1".to_string(),
                };
            } else {
                cursor_str = "-".to_owned();
            }
            line = format!("{cursor_str}/{}{plus_marker}", self.total_found);
        }
        let action = if self.is_filter { "Filter" } else { "Find" };
        let target_column = self
            .column_index
            .as_ref()
            .map(|(_, name)| format!(" in {}", name))
            .unwrap_or_default();
        format!("[{action} \"{}\"{target_column}: {line}]", self.target)
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

enum SorterState {
    Disabled,
    Enabled(SorterInfo),
}

impl SorterState {
    fn from_sorter(sorter: &sort::Sorter, sort_order: SortOrder) -> Self {
        Self::Enabled(SorterInfo {
            status: sorter.status(),
            column_name: sorter.column_name().to_string(),
            order: sort_order,
            sort_type: sorter.sort_type(),
        })
    }
}

struct SorterInfo {
    status: sort::SorterStatus,
    column_name: String,
    order: SortOrder,
    sort_type: sort::SortType,
}

impl SorterInfo {
    fn status_line(&self) -> String {
        let sort_type_str = match self.sort_type {
            sort::SortType::Natural => "natural",
            sort::SortType::Auto => "auto based on type",
        };
        let prefix = format!("[Sorting by {} ({})", self.column_name, sort_type_str);
        match &self.status {
            sort::SorterStatus::Running => format!("{prefix}...]").to_string(),
            sort::SorterStatus::Error(error_msg) => {
                format!("{} failed: {}]", prefix, error_msg).to_string()
            }
            _ => "".to_string(),
        }
    }
}

struct BordersState {
    x_row_separator: u16,
    y_first_record: u16,
    x_freeze_separator: Option<u16>,
}

pub struct DebugStats {
    show_stats: bool,
    rows_view_stats: Option<crate::view::PerfStats>,
    finder_elapsed: Option<Duration>,
    render_elapsed: Option<Duration>,
}

impl DebugStats {
    pub fn new() -> Self {
        DebugStats {
            show_stats: false,
            rows_view_stats: None,
            finder_elapsed: None,
            render_elapsed: None,
        }
    }

    pub fn show_stats(&mut self, show: bool) {
        self.show_stats = show;
    }

    pub fn rows_view_perf(&mut self, stats: Option<crate::view::PerfStats>) {
        self.rows_view_stats = stats;
    }

    pub fn finder_elapsed(&mut self, elapsed: Option<Duration>) {
        self.finder_elapsed = elapsed;
    }

    pub fn render_elapsed(&mut self, elapsed: Option<Duration>) {
        self.render_elapsed = elapsed;
    }

    pub fn status_line(&self) -> Option<String> {
        if !self.show_stats {
            return None;
        }
        let mut line = "[".to_string();
        if let Some(stats) = &self.rows_view_stats {
            line += format!(
                "rows:{:.3}ms pos:{}us npos:{} seek:{} parse:{}",
                stats.elapsed.as_micros() as f64 / 1000.0,
                stats
                    .reader_stats
                    .pos_table_elapsed
                    .as_ref()
                    .map_or(0, |e| e.as_micros()),
                stats.reader_stats.pos_table_entry,
                stats.reader_stats.num_seek,
                stats.reader_stats.num_parsed_record
            )
            .as_str();
        }
        if let Some(elapsed) = self.finder_elapsed {
            line += format!(" finder:{:.3}ms", elapsed.as_micros() as f64 / 1000.0).as_str();
        }
        if let Some(elapsed) = self.render_elapsed {
            line += format!(" render:{:.3}ms", elapsed.as_micros() as f64 / 1000.0).as_str();
        }
        line += "]";
        Some(line)
    }
}

pub struct CsvTableState {
    // TODO: types appropriate?
    pub rows_offset: u64,
    pub cols_offset: view::ColumnsOffset,
    pub num_cols_rendered: u64,
    pub more_cols_to_show: Option<bool>,
    filename: Option<String>,
    total_line_number: Option<(usize, bool)>,
    total_cols: usize,
    pub debug_stats: DebugStats,
    buffer_content: BufferState,
    pub finder_state: FinderState,
    pub filter_columns_state: FilterColumnsState,
    sorter_state: SorterState,
    borders_state: Option<BordersState>,
    // TODO: should probably be with BordersState
    col_ending_pos_x: u16,
    pub selection: Option<view::Selection>,
    pub transient_message: Option<String>,
    pub echo_column: Option<String>,
    pub ignore_case: bool,
    pub view_layout: Option<ViewLayout>,
    pub enable_line_wrap: bool,
    pub is_word_wrap: bool,
    pub column_width_overrides: ColumnWidthOverrides,
    pub cursor_xy: Option<(u16, u16)>,
    pub theme: Theme,
    pub color_columns: bool,
    pub prompt: Option<String>,
    pub debug: String,
}

impl CsvTableState {
    pub fn new(
        filename: Option<String>,
        total_cols: usize,
        echo_column: &Option<String>,
        ignore_case: bool,
        color_columns: bool,
        prompt: Option<String>,
    ) -> Self {
        Self {
            rows_offset: 0,
            cols_offset: view::ColumnsOffset::default(),
            num_cols_rendered: 0,
            more_cols_to_show: None,
            filename,
            total_line_number: None,
            total_cols,
            debug_stats: DebugStats::new(),
            buffer_content: BufferState::Disabled,
            finder_state: FinderState::FinderInactive,
            filter_columns_state: FilterColumnsState::Disabled,
            sorter_state: SorterState::Disabled,
            borders_state: None,
            col_ending_pos_x: 0,
            selection: None,
            transient_message: None,
            echo_column: echo_column.clone(),
            ignore_case,
            view_layout: None,
            enable_line_wrap: false,
            is_word_wrap: false,
            column_width_overrides: ColumnWidthOverrides::new(),
            cursor_xy: None,
            theme: Theme::default(),
            color_columns,
            prompt,
            debug: "".into(),
        }
    }

    pub fn set_rows_offset(&mut self, offset: u64) {
        self.rows_offset = offset;
    }

    pub fn set_cols_offset(&mut self, offset: view::ColumnsOffset) {
        self.cols_offset = offset;
    }

    fn reset_more_cols_to_show(&mut self) {
        self.more_cols_to_show = None;
    }

    fn update_more_cols_to_show(&mut self, value: bool) {
        // If any rows have more columns to show, keep it that way
        if let Some(current) = self.more_cols_to_show {
            self.more_cols_to_show = Some(current || value);
        } else {
            self.more_cols_to_show = Some(value);
        }
    }

    pub fn has_more_cols_to_show(&self) -> bool {
        self.more_cols_to_show.is_none_or(|v| v)
    }

    pub fn set_total_line_number(&mut self, n: usize, is_approx: bool) {
        self.total_line_number = Some((n, is_approx));
    }

    pub fn set_total_cols(&mut self, n: usize) {
        self.total_cols = n;
    }

    pub fn set_buffer(&mut self, mode: InputMode, input: Input) {
        self.buffer_content = BufferState::Enabled(mode, input);
    }

    pub fn reset_buffer(&mut self) {
        self.buffer_content = BufferState::Disabled;
    }

    pub fn line_number_and_spaces_width(&self) -> u16 {
        self.borders_state
            .as_ref()
            .map_or(0, |bs| bs.x_row_separator)
            + NUM_SPACES_AFTER_LINE_NUMBER
    }

    pub fn update_sorter(&mut self, sorter: &Option<Arc<sort::Sorter>>, sort_order: SortOrder) {
        if let Some(s) = sorter {
            self.sorter_state = SorterState::from_sorter(s.as_ref(), sort_order);
        } else {
            self.sorter_state = SorterState::Disabled;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sort::{SortType, SorterStatus};

    #[test]
    fn test_sorter_info_status_line() {
        let info = SorterInfo {
            status: SorterStatus::Running,
            column_name: "test_column".to_string(),
            order: SortOrder::Ascending,
            sort_type: SortType::Natural,
        };

        let status_line = info.status_line();
        assert!(status_line.contains("Sorting by test_column (natural)"));

        let info_lex = SorterInfo {
            status: SorterStatus::Running,
            column_name: "test_column".to_string(),
            order: SortOrder::Ascending,
            sort_type: SortType::Auto,
        };

        let status_line_lex = info_lex.status_line();
        assert!(status_line_lex.contains("Sorting by test_column (auto based on type)"));
    }
}
