use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, StatefulWidget, Widget, Wrap},
};

const HELP_CONTENT: &str = "
csvlens is an interactive CSV file viewer in the command line.

These are the key bindings. Press q to exit.

# Moving

hjkl (or ← ↓ ↑→ )       : Scroll one row or column in the given direction
Ctrl + f (or Page Down) : Scroll one window down
Ctrl + b (or Page Up)   : Scroll one window up
Ctrl + d (or d)         : Scroll half a window down
Ctrl + u (or u)         : Scroll half a window up
Ctrl + h                : Scroll one window left
Ctrl + l                : Scroll one window right
Ctrl + ←                : Scroll left to first column
Ctrl + →                : Scroll right to last column
G (or End)              : Go to bottom
g (or Home)             : Go to top
<n>G                    : Go to line n

# Search

/<regex>                : Find content matching regex and highlight matches
n (in Find mode)        : Jump to next result
N (in Find mode)        : Jump to previous result
&<regex>                : Filter rows using regex (show only matches)
*<regex>                : Filter columns using regex (show only matches)

# Selection modes

TAB                     : Toggle between row, column or cell selection modes
>                       : Increase selected column's width
<                       : Decrease selected column's width
Shift + ↓ (or J)        : Sort rows by the selected column
# (in Cell mode)        : Find and highlight rows like the selected cell
@ (in Cell mode)        : Filter rows like the selected cell
v                       : Open value picker for selected column (shows unique values)
y                       : Copy the selected row or cell to clipboard
Enter (in Cell mode)    : Print the selected cell to stdout and exit

# Value picker

↑ ↓                     : Navigate through unique values in the selected column
Enter                   : Filter rows to show only those with the selected value
Esc                     : Exit value picker without selecting

# Other options

-S                      : Toggle line wrapping
-W                      : Toggle line wrapping by words
f<n>                    : Freeze this number of columns from the left
r                       : Reset to default view (clear all filters and custom column widths)
H (or ?)                : Display this help
q                       : Exit";

pub struct HelpPage {}

pub struct HelpPageState {
    active: bool,
    offset: u16,
    render_complete: bool,
}

impl HelpPage {
    pub fn new() -> Self {
        HelpPage {}
    }
}

impl HelpPageState {
    pub fn new() -> Self {
        HelpPageState {
            active: false,
            offset: 0,
            render_complete: true,
        }
    }

    pub fn activate(&mut self) -> &Self {
        self.active = true;
        self.offset = 0;
        self
    }

    pub fn deactivate(&mut self) -> &Self {
        self.active = false;
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

impl StatefulWidget for HelpPage {
    type State = HelpPageState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        fn line_to_span(line: &str) -> Span<'_> {
            if line.starts_with("# ") && !line.contains(':') {
                let header_style = Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Rgb(200, 200, 200));
                let header_formatted = format!("[{}]", line.strip_prefix("# ").unwrap());
                Span::styled(header_formatted, header_style)
            } else {
                Span::raw(line)
            }
        }

        let text: Vec<Line> = HELP_CONTENT
            .split('\n')
            .map(|s| Line::from(line_to_span(s)))
            .collect();

        // Minus 2 to account for borders.
        let num_lines_to_be_rendered = (text.len() as u16).saturating_sub(state.offset);
        state.render_complete = area.height.saturating_sub(2) >= num_lines_to_be_rendered;

        let paragraph = Paragraph::new(text)
            .block(Block::default().title("Help").borders(Borders::ALL))
            .wrap(Wrap { trim: true })
            .scroll((state.offset, 0));

        paragraph.render(area, buf);
    }
}
