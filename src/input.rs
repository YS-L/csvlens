use crate::util::events::{CsvlensEvent, CsvlensEvents};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

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
    FilterColumns(String),
    Quit,
    BufferContent(String),
    BufferReset,
    Select,
    Nothing,
}

impl Control {
    fn empty_buffer() -> Control {
        Control::BufferContent("".into())
    }
}

enum BufferState {
    Active(String),
    Inactive,
}

#[derive(Clone, PartialEq, Eq, Hash, Copy)]
pub enum InputMode {
    Default,
    GotoLine,
    Find,
    Filter,
    FilterColumns,
}

pub struct BufferHistory {
    inner: HashMap<InputMode, String>,
}

impl BufferHistory {
    fn new() -> Self {
        BufferHistory {
            inner: HashMap::new(),
        }
    }

    fn set(&mut self, input_mode: InputMode, content: &str) {
        self.inner.insert(input_mode, content.to_string());
    }

    fn get(&mut self, input_mode: InputMode) -> Option<String> {
        self.inner.get(&input_mode).cloned()
    }
}

pub struct InputHandler {
    events: CsvlensEvents,
    mode: InputMode,
    buffer_state: BufferState,
    buffer_history: BufferHistory,
}

impl InputHandler {
    pub fn new() -> InputHandler {
        InputHandler {
            events: CsvlensEvents::new(),
            mode: InputMode::Default,
            buffer_state: BufferState::Inactive,
            buffer_history: BufferHistory::new(),
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
                    self.buffer_state = BufferState::Active(x.to_string());
                    self.mode = InputMode::GotoLine;
                    Control::BufferContent(x.to_string())
                }
                KeyCode::Char('/') => {
                    self.init_buffer(InputMode::Find);
                    Control::empty_buffer()
                }
                KeyCode::Char('&') => {
                    self.init_buffer(InputMode::Filter);
                    Control::empty_buffer()
                }
                KeyCode::Char('*') => {
                    self.init_buffer(InputMode::FilterColumns);
                    Control::empty_buffer()
                }
                KeyCode::Enter => Control::Select,
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
            BufferState::Inactive => "",
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
                    BufferState::Inactive => "".to_owned(),
                };
                self.buffer_state = BufferState::Active(new_buffer.clone());
                Control::BufferContent(new_buffer)
            }
            KeyCode::Char('g' | 'G') | KeyCode::Enter
                if self.mode == InputMode::GotoLine =>
            {
                let goto_line = match &self.buffer_state {
                    BufferState::Active(buf) => buf.parse::<usize>().ok(),
                    BufferState::Inactive => None,
                };
                let res = if let Some(n) = goto_line {
                    Control::ScrollTo(n)
                } else {
                    Control::BufferReset
                };
                self.reset_buffer();
                res
            }
            KeyCode::Up => {
                let mode = match self.mode {
                    InputMode::Filter => InputMode::Find,
                    _ => self.mode,
                };
                if let Some(buf) = self.buffer_history.get(mode) {
                    self.buffer_state = BufferState::Active(buf.clone());
                    Control::BufferContent(buf)
                } else {
                    Control::Nothing
                }
            }
            KeyCode::Enter => {
                let control;
                if cur_buffer.is_empty() {
                    control = Control::BufferReset;
                } else if self.mode == InputMode::Find {
                    control = Control::Find(cur_buffer.to_string());
                } else if self.mode == InputMode::Filter {
                    control = Control::Filter(cur_buffer.to_string());
                } else if self.mode == InputMode::FilterColumns {
                    control = Control::FilterColumns(cur_buffer.to_string());
                } else {
                    control = Control::BufferReset;
                }
                if self.mode == InputMode::Filter {
                    // Share buffer history between Find and Filter, see also KeyCode::Up
                    self.buffer_history.set(InputMode::Find, cur_buffer);
                } else {
                    self.buffer_history.set(self.mode, cur_buffer);
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
                    BufferState::Inactive => x.to_string(),
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

    fn init_buffer(&mut self, mode: InputMode) {
        self.buffer_state = BufferState::Active("".into());
        self.mode = mode;
    }

    fn reset_buffer(&mut self) {
        self.buffer_state = BufferState::Inactive;
        self.mode = InputMode::Default;
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }
}
