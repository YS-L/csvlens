use crate::util::events::{CsvlensEvent, CsvlensEvents};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Control {
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    ScrollTop,
    ScrollBottom,
    ScrollPageUp,
    ScrollPageDown,
    ScrollPageLeft,
    ScrollPageRight,
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

#[derive(Clone, PartialEq, Eq)]
pub enum InputMode {
    Default,
    GotoLine,
    Find,
    Filter,
}

pub struct InputHandler {
    events: CsvlensEvents,
    mode: InputMode,
    buffer_state: BufferState,
}

impl InputHandler {
    pub fn new() -> InputHandler {
        InputHandler {
            events: CsvlensEvents::new(),
            mode: InputMode::Default,
            buffer_state: BufferState::Inactive,
        }
    }

    pub fn next(&mut self) -> Control {
        if let CsvlensEvent::Input(key) = self.events.next().unwrap() {
            if self.is_input_buffering() {
                return self.handler_buffering(key);
            } else {
                return self.handler_default(key);
            }
        }
        // tick event, no need to distinguish it for now
        Control::Nothing
    }

    fn handler_default(&mut self, key_event: KeyEvent) -> Control {
        match key_event.modifiers {
            // SHIFT needed to capture capitalised characters
            KeyModifiers::NONE | KeyModifiers::SHIFT => match key_event.code {
                KeyCode::Char('q') => Control::Quit,
                KeyCode::Char('j') | KeyCode::Down => Control::ScrollDown,
                KeyCode::Char('k') | KeyCode::Up => Control::ScrollUp,
                KeyCode::Char('l') | KeyCode::Right => Control::ScrollRight,
                KeyCode::Char('h') | KeyCode::Left => Control::ScrollLeft,
                KeyCode::Char('g') => Control::ScrollTop,
                KeyCode::Char('G') => Control::ScrollBottom,
                KeyCode::Char('n') => Control::ScrollToNextFound,
                KeyCode::Char('N') => Control::ScrollToPrevFound,
                KeyCode::PageDown => Control::ScrollPageDown,
                KeyCode::PageUp => Control::ScrollPageUp,
                KeyCode::Char(x) if "0123456789".contains(x.to_string().as_str()) => {
                    let init_buffer = x.to_string();
                    self.buffer_state = BufferState::Active(init_buffer.clone());
                    self.mode = InputMode::GotoLine;
                    Control::BufferContent(init_buffer)
                }
                KeyCode::Char('/') => {
                    self.buffer_state = BufferState::Active("".to_owned());
                    self.mode = InputMode::Find;
                    Control::BufferContent("".to_owned())
                }
                KeyCode::Char('&') => {
                    self.buffer_state = BufferState::Active("".to_owned());
                    self.mode = InputMode::Filter;
                    Control::BufferContent("".to_owned())
                }
                _ => Control::Nothing,
            },
            KeyModifiers::CONTROL => match key_event.code {
                KeyCode::Char('f') => Control::ScrollPageDown,
                KeyCode::Char('b') => Control::ScrollPageUp,
                KeyCode::Char('h') | KeyCode::Left => Control::ScrollPageLeft,
                KeyCode::Char('l') | KeyCode::Right => Control::ScrollPageRight,
                _ => Control::Nothing,
            },
            _ => Control::Nothing,
        }
    }

    fn handler_buffering(&mut self, key_event: KeyEvent) -> Control {
        let cur_buffer = match &self.buffer_state {
            BufferState::Active(buffer) => buffer.as_str(),
            _ => "",
        };
        // SHIFT needed to capture capitalised characters
        if key_event.modifiers != KeyModifiers::NONE && key_event.modifiers != KeyModifiers::SHIFT {
            return Control::Nothing;
        }
        match key_event.code {
            KeyCode::Esc => {
                self.reset_buffer();
                Control::BufferReset
            }
            KeyCode::Backspace => {
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
            KeyCode::Char('g') | KeyCode::Char('G') | KeyCode::Enter
                if self.mode == InputMode::GotoLine =>
            {
                let goto_line = match &self.buffer_state {
                    BufferState::Active(buf) => buf.parse::<usize>().ok(),
                    _ => None,
                };
                let res = if let Some(n) = goto_line {
                    Control::ScrollTo(n)
                } else {
                    Control::BufferReset
                };
                self.reset_buffer();
                res
            }
            KeyCode::Enter => {
                let control;
                if cur_buffer.is_empty() {
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
            KeyCode::Char('/') => {
                if cur_buffer.is_empty() && self.mode == InputMode::Find {
                    self.mode = InputMode::Filter;
                }
                Control::BufferContent("".to_string())
            }
            KeyCode::Char(x) => {
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
        matches!(self.buffer_state, BufferState::Active(_))
    }

    fn reset_buffer(&mut self) {
        self.buffer_state = BufferState::Inactive;
        self.mode = InputMode::Default;
    }

    pub fn mode(&self) -> InputMode {
        self.mode.clone()
    }
}
