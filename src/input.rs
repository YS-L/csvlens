use crate::util::events::{CsvlensEvent, CsvlensEvents};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::collections::hash_map::Entry::Vacant;
use std::collections::HashMap;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

pub enum Control {
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    ScrollTop,
    ScrollBottom,
    ScrollPageUp,
    ScrollPageDown,
    ScrollHalfPageUp,
    ScrollHalfPageDown,
    ScrollPageLeft,
    ScrollPageRight,
    ScrollLeftMost,
    ScrollRightMost,
    ScrollTo(usize),
    ScrollToNextFound,
    ScrollToPrevFound,
    IncreaseWidth,
    DecreaseWidth,
    Find(String),
    Filter(String),
    FilterColumns(String),
    Quit,
    BufferContent(Input),
    BufferReset,
    Select,
    ToggleSelectionType,
    ToggleLineWrap,
    Reset,
    Help,
    UnknownOption(String),
    Nothing,
}

impl Control {
    fn empty_buffer() -> Control {
        Control::BufferContent("".into())
    }
}

enum BufferState {
    Active(Input),
    Inactive,
}

#[derive(Clone, PartialEq, Eq, Hash, Copy)]
pub enum InputMode {
    Default,
    GotoLine,
    Find,
    Filter,
    FilterColumns,
    Option,
    Help,
}

struct BufferHistory {
    buffers: Vec<String>,
    cursor: usize,
}

impl BufferHistory {
    fn new_with(buf: &str) -> Self {
        BufferHistory {
            buffers: vec![buf.to_string()],
            cursor: 1,
        }
    }

    fn push(&mut self, buf: &str) {
        if buf.is_empty() {
            // Don't keep empty entries
            return;
        }
        if let Some(index) = self.buffers.iter().position(|x| x == buf) {
            // Don't keep duplicate entries
            self.buffers.remove(index);
        }
        self.buffers.push(buf.to_string());
        self.reset_cursor();
    }

    fn prev(&mut self) -> Option<String> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor = self.cursor.saturating_sub(1);
        Some(self.buffers[self.cursor].clone())
    }

    fn next(&mut self) -> Option<String> {
        if self.cursor >= self.buffers.len() - 1 {
            return None;
        }
        self.cursor = self.cursor.saturating_add(1);
        Some(self.buffers[self.cursor].clone())
    }

    fn reset_cursor(&mut self) {
        self.cursor = self.buffers.len();
    }
}

pub struct BufferHistoryContainer {
    inner: HashMap<InputMode, BufferHistory>,
}

impl BufferHistoryContainer {
    fn new() -> Self {
        BufferHistoryContainer {
            inner: HashMap::new(),
        }
    }

    fn set(&mut self, input_mode: InputMode, content: &str) {
        if let Vacant(e) = self.inner.entry(input_mode) {
            e.insert(BufferHistory::new_with(content));
        } else {
            // TODO: rewrite without unwrap?
            let history = self.inner.get_mut(&input_mode).unwrap();
            history.push(content);
        }
    }

    fn prev(&mut self, input_mode: InputMode) -> Option<String> {
        self.inner
            .get_mut(&input_mode)
            .and_then(|history| history.prev())
    }

    fn next(&mut self, input_mode: InputMode) -> Option<String> {
        self.inner
            .get_mut(&input_mode)
            .and_then(|history| history.next())
    }

    fn reset_cursors(&mut self) {
        for (_, history) in self.inner.iter_mut() {
            history.reset_cursor();
        }
    }
}

pub struct InputHandler {
    events: CsvlensEvents,
    mode: InputMode,
    buffer_state: BufferState,
    buffer_history_container: BufferHistoryContainer,
}

impl InputHandler {
    pub fn new() -> InputHandler {
        InputHandler {
            events: CsvlensEvents::new(),
            mode: InputMode::Default,
            buffer_state: BufferState::Inactive,
            buffer_history_container: BufferHistoryContainer::new(),
        }
    }

    pub fn next(&mut self) -> Control {
        if let CsvlensEvent::Input(key) = self.events.next().unwrap() {
            if self.is_help_mode() {
                return self.handler_help(key);
            } else if self.is_input_buffering() {
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
                KeyCode::Char('g') | KeyCode::Home => Control::ScrollTop,
                KeyCode::Char('G') | KeyCode::End => Control::ScrollBottom,
                KeyCode::Char('n') => Control::ScrollToNextFound,
                KeyCode::Char('N') => Control::ScrollToPrevFound,
                KeyCode::Char('H') => Control::Help,
                KeyCode::PageDown => Control::ScrollPageDown,
                KeyCode::PageUp => Control::ScrollPageUp,
                KeyCode::Char('d') => Control::ScrollHalfPageDown,
                KeyCode::Char('u') => Control::ScrollHalfPageUp,
                KeyCode::Char(x) if "0123456789".contains(x.to_string().as_str()) => {
                    self.buffer_state = BufferState::Active(Input::new(x.to_string()));
                    self.mode = InputMode::GotoLine;
                    Control::BufferContent(Input::new(x.to_string()))
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
                KeyCode::Char('-') => {
                    self.init_buffer(InputMode::Option);
                    Control::empty_buffer()
                }
                KeyCode::Enter => Control::Select,
                KeyCode::Tab => Control::ToggleSelectionType,
                KeyCode::Char('>') => Control::IncreaseWidth,
                KeyCode::Char('<') => Control::DecreaseWidth,
                KeyCode::Char('r') => Control::Reset,
                _ => Control::Nothing,
            },
            KeyModifiers::CONTROL => match key_event.code {
                KeyCode::Char('f') => Control::ScrollPageDown,
                KeyCode::Char('b') => Control::ScrollPageUp,
                KeyCode::Char('d') => Control::ScrollHalfPageDown,
                KeyCode::Char('u') => Control::ScrollHalfPageUp,
                KeyCode::Char('h') => Control::ScrollPageLeft,
                KeyCode::Char('l') => Control::ScrollPageRight,
                KeyCode::Left => Control::ScrollLeftMost,
                KeyCode::Right => Control::ScrollRightMost,
                _ => Control::Nothing,
            },
            _ => Control::Nothing,
        }
    }

    fn handler_buffering(&mut self, key_event: KeyEvent) -> Control {
        let input = match &mut self.buffer_state {
            BufferState::Active(input) => input,
            BufferState::Inactive => return Control::Nothing,
        };
        if self.mode == InputMode::Option {
            return self.handler_buffering_option_mode(key_event);
        }
        match key_event.code {
            KeyCode::Esc => {
                self.reset_buffer();
                Control::BufferReset
            }
            KeyCode::Char('g' | 'G') | KeyCode::Enter if self.mode == InputMode::GotoLine => {
                let goto_line = match &self.buffer_state {
                    BufferState::Active(input) => input.value().parse::<usize>().ok(),
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
                if let Some(buf) = self.buffer_history_container.prev(mode) {
                    self.buffer_state = BufferState::Active(Input::new(buf.clone()));
                    Control::BufferContent(Input::new(buf))
                } else {
                    Control::Nothing
                }
            }
            KeyCode::Down => {
                let mode = match self.mode {
                    InputMode::Filter => InputMode::Find,
                    _ => self.mode,
                };
                if let Some(buf) = self.buffer_history_container.next(mode) {
                    self.buffer_state = BufferState::Active(Input::new(buf.clone()));
                    Control::BufferContent(Input::new(buf))
                } else {
                    self.buffer_state = BufferState::Active(Input::default());
                    Control::BufferContent(Input::default())
                }
            }
            KeyCode::Enter => {
                let control;
                if input.value().is_empty() {
                    control = Control::BufferReset;
                } else if self.mode == InputMode::Find {
                    control = Control::Find(input.value().to_string());
                } else if self.mode == InputMode::Filter {
                    control = Control::Filter(input.value().to_string());
                } else if self.mode == InputMode::FilterColumns {
                    control = Control::FilterColumns(input.value().to_string());
                } else {
                    control = Control::BufferReset;
                }
                if self.mode == InputMode::Filter {
                    // Share buffer history between Find and Filter, see also KeyCode::Up
                    self.buffer_history_container
                        .set(InputMode::Find, input.value());
                } else {
                    self.buffer_history_container.set(self.mode, input.value());
                }
                self.reset_buffer();
                control
            }
            _ => {
                if input.handle_event(&Event::Key(key_event)).is_some() {
                    return Control::BufferContent(input.clone());
                }
                Control::Nothing
            }
        }
    }

    fn handler_buffering_option_mode(&mut self, key_event: KeyEvent) -> Control {
        match key_event.code {
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Enter => {
                self.reset_buffer();
                Control::BufferReset
            }
            KeyCode::Char('S') => {
                self.reset_buffer();
                Control::ToggleLineWrap
            }
            KeyCode::Char(x) => {
                self.reset_buffer();
                Control::UnknownOption(x.to_string())
            }
            _ => Control::Nothing,
        }
    }

    fn handler_help(&mut self, key_event: KeyEvent) -> Control {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Esc => Control::Quit,
            KeyCode::Char('j') | KeyCode::Down => Control::ScrollDown,
            KeyCode::Char('k') | KeyCode::Up => Control::ScrollUp,
            _ => Control::Nothing,
        }
    }

    fn is_input_buffering(&self) -> bool {
        matches!(self.buffer_state, BufferState::Active(_))
    }

    fn init_buffer(&mut self, mode: InputMode) {
        self.buffer_state = BufferState::Active(Input::default());
        self.mode = mode;
    }

    fn reset_buffer(&mut self) {
        self.buffer_state = BufferState::Inactive;
        self.buffer_history_container.reset_cursors();
        self.mode = InputMode::Default;
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }

    pub fn enter_help_mode(&mut self) {
        self.mode = InputMode::Help;
    }

    pub fn exit_help_mode(&mut self) {
        self.mode = InputMode::Default;
    }

    fn is_help_mode(&mut self) -> bool {
        self.mode == InputMode::Help
    }
}
