use tui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, StatefulWidget, Widget, Wrap},
};

const HELP_CONTENT: &str = "
csvlens is an interactive CSV file viewer in the command line.

You are viewing its help page. Press q to exit.

# Navigation

hjkl (or ← ↓ ↑→ )       : Scroll one row or column in the given direction
Ctrl + f (or Page Down) : Scroll one window down
Ctrl + b (or Page Up)   : Scroll one window up
Ctrl + h (or Ctrl + ←)  : Scroll one window left
Ctrl + l (or Ctrl + →)  : Scroll one window right
G (or End)              : Go to bottom
g (or Home)             : Go to top
<n>G                    : Go to line n

# Find and filter

/<regex>                : Find content matching regex and highlight matches
n (in Find mode)        : Jump to next result
N (in Find mode)        : Jump to previous result
&<regex>                : Filter rows using regex (show only matches)
*<regex>                : Filter columns using regex (show only matches)

# Selection modes

TAB                     : Toggle between row, column or cell selection modes
Enter (in Cell mode)    : Print the selected cell to stdout and exit

# Other options

-S                      : Toggle line wrapping
H                       : Display this help
q                       : Exit
";

pub struct HelpPage {}

pub struct HelpPageState {
    active: bool,
    offset: u16,
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
}

impl StatefulWidget for HelpPage {
    type State = HelpPageState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        fn line_to_span(line: &str) -> Span {
            if line.starts_with("# ") {
                let header_style = Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Rgb(200, 200, 200));
                let header_formatted = format!("[{}]", line.strip_prefix("# ").unwrap());
                Span::styled(header_formatted, header_style)
            } else {
                Span::raw(line)
            }
        }

        let text: Vec<Spans> = HELP_CONTENT
            .split('\n')
            .map(|s| Spans::from(line_to_span(s)))
            .collect();
        let paragraph = Paragraph::new(text)
            .block(Block::default().title("Help").borders(Borders::ALL))
            // .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}
