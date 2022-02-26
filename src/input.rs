use crate::util::events::{Event, Events};
use termion::event::Key;

pub enum Control {
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    ScrollBottom,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTo(usize),
    ScrollToNextFound,
    ScrollToPrevFound,
    Find(String),
    Filter(String),
    Quit,
    BufferContent(String),
    BufferReset,
    Nothing,
}

enum BufferState {
    Active(String),
    Inactive,
}

#[derive(Clone, PartialEq)]
pub enum InputMode {
    Default,
    GotoLine,
    Find,
    Filter,
}

pub struct InputHandler {
    events: Events,
    mode: InputMode,
    buffer_state: BufferState,
}

impl InputHandler {
    pub fn new() -> InputHandler {
        InputHandler {
            events: Events::new(),
            mode: InputMode::Default,
            buffer_state: BufferState::Inactive,
        }
    }

    pub fn next(&mut self) -> Control {
        if let Event::Input(key) = self.events.next().unwrap() {
            if self.is_input_buffering() {
                return self.handler_buffering(key);
            } else {
                return self.handler_default(key);
            }
        }
        // tick event, no need to distinguish it for now
        Control::Nothing
    }

    fn handler_default(&mut self, key: Key) -> Control {
        match key {
            Key::Char('q') => Control::Quit,
            Key::Char('j') | Key::Down => Control::ScrollDown,
            Key::Char('k') | Key::Up => Control::ScrollUp,
            Key::Char('l') | Key::Right => Control::ScrollRight,
            Key::Char('h') | Key::Left => Control::ScrollLeft,
            Key::Char('G') => Control::ScrollBottom,
            Key::Char('n') => Control::ScrollToNextFound,
            Key::Char('N') => Control::ScrollToPrevFound,
            Key::Ctrl('f') | Key::PageDown => Control::ScrollPageDown,
            Key::Ctrl('b') | Key::PageUp => Control::ScrollPageUp,
            Key::Char(x) if "0123456789".contains(x.to_string().as_str()) => {
                let init_buffer = x.to_string();
                self.buffer_state = BufferState::Active(init_buffer.clone());
                self.mode = InputMode::GotoLine;
                Control::BufferContent(init_buffer)
            }
            Key::Char('/') => {
                self.buffer_state = BufferState::Active("".to_owned());
                self.mode = InputMode::Find;
                Control::BufferContent("".to_owned())
            }
            Key::Char('&') => {
                self.buffer_state = BufferState::Active("".to_owned());
                self.mode = InputMode::Filter;
                Control::BufferContent("".to_owned())
            }
            _ => Control::Nothing,
        }
    }

    fn handler_buffering(&mut self, key: Key) -> Control {
        let cur_buffer = match &self.buffer_state {
            BufferState::Active(buffer) => buffer.as_str(),
            _ => "",
        };
        match key {
            Key::Esc => {
                self.reset_buffer();
                Control::BufferReset
            }
            Key::Backspace => {
                let new_buffer = match &self.buffer_state {
                    BufferState::Active(buffer) => {
                        let mut chars = buffer.chars();
                        chars.next_back();
                        chars.as_str().to_owned()
                    }
                    _ => "".to_owned(),
                };
                if !new_buffer.is_empty() {
                    self.buffer_state = BufferState::Active(new_buffer.clone());
                    Control::BufferContent(new_buffer)
                } else {
                    self.reset_buffer();
                    Control::BufferReset
                }
            }
            Key::Char('G') | Key::Char('\n') if self.mode == InputMode::GotoLine => {
                let goto_line = match &self.buffer_state {
                    BufferState::Active(buf) => buf.parse::<usize>().ok(),
                    _ => None,
                };
                let res;
                if let Some(n) = goto_line {
                    res = Control::ScrollTo(n);
                } else {
                    res = Control::BufferReset;
                }
                self.reset_buffer();
                res
            }
            Key::Char('\n') => {
                let control;
                if cur_buffer == "" {
                    control = Control::BufferReset;
                } else if self.mode == InputMode::Find {
                    control = Control::Find(cur_buffer.to_string());
                } else if self.mode == InputMode::Filter {
                    control = Control::Filter(cur_buffer.to_string());
                } else {
                    control = Control::BufferReset;
                }
                self.reset_buffer();
                control
            }
            Key::Char('/') => {
                if cur_buffer == "" && self.mode == InputMode::Find {
                    self.mode = InputMode::Filter;
                }
                Control::BufferContent("".to_string())
            }
            Key::Char(x) => {
                let new_buffer = match &self.buffer_state {
                    BufferState::Active(buffer) => buffer.to_owned() + x.to_string().as_str(),
                    _ => x.to_string(),
                };
                self.buffer_state = BufferState::Active(new_buffer.clone());
                Control::BufferContent(new_buffer)
            }
            _ => Control::Nothing,
        }
    }

    fn is_input_buffering(&self) -> bool {
        match self.buffer_state {
            BufferState::Active(_) => true,
            _ => false,
        }
    }

    fn reset_buffer(&mut self) {
        self.buffer_state = BufferState::Inactive;
        self.mode = InputMode::Default;
    }

    pub fn mode(&self) -> InputMode {
        self.mode.clone()
    }
}
