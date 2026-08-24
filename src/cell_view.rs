use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Widget, Wrap},
};

/// A pop-up that shows the full content of the selected cell (or row), useful when a value is too
/// wide to fit in the table and gets collapsed.
pub struct CellView {}

pub struct CellViewState {
    active: bool,
    content: String,
    offset: u16,
    render_complete: bool,
}

impl CellView {
    pub fn new() -> Self {
        CellView {}
    }
}

impl CellViewState {
    pub fn new() -> Self {
        CellViewState {
            active: false,
            content: String::new(),
            offset: 0,
            render_complete: true,
        }
    }

    pub fn activate(&mut self, content: String) -> &Self {
        self.active = true;
        self.content = content;
        self.offset = 0;
        self
    }

    pub fn deactivate(&mut self) -> &Self {
        self.active = false;
        self.content.clear();
        self.offset = 0;
        self
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn scroll_up(&mut self) -> &Self {
        if self.offset > 0 {
            self.offset -= 1;
        }
        self
    }

    pub fn scroll_down(&mut self) -> &Self {
        if !self.render_complete {
            self.offset += 1;
        }
        self
    }
}

fn popup_area(area: Rect, content: &str) -> Rect {
    let num_lines = content.lines().count().max(1) as u16;
    let longest = content
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0) as u16;

    // Leave a margin around the pop-up and account for the borders.
    let max_width = (area.width as f32 * 0.8) as u16;
    let max_height = (area.height as f32 * 0.8) as u16;
    let width = longest.saturating_add(2).clamp(10, max_width.max(10));
    let height = num_lines.saturating_add(2).clamp(3, max_height.max(3));

    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [centered] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(horizontal);
    centered
}

impl StatefulWidget for CellView {
    type State = CellViewState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let area = popup_area(area, &state.content);

        let num_lines = state.content.lines().count().max(1) as u16;
        // Minus 2 to account for borders.
        let num_lines_to_be_rendered = num_lines.saturating_sub(state.offset);
        state.render_complete = area.height.saturating_sub(2) >= num_lines_to_be_rendered;

        Clear.render(area, buf);

        let paragraph = Paragraph::new(state.content.as_str())
            .block(Block::default().title("Cell").borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((state.offset, 0));

        paragraph.render(area, buf);
    }
}
