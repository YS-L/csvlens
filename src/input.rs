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
    Quit,
    BufferContent(String),
    BufferReset,
    Nothing,
}

enum BufferState {
    Active(String),
    Inactive,
}

pub struct InputHandler {
    events: Events,
    buffer_state: BufferState,
}

impl InputHandler {

    pub fn new() -> InputHandler {
        InputHandler {
            events: Events::new(),
            buffer_state: BufferState::Inactive,
        }
    }

    pub fn next(&mut self) -> Control {
        if let Event::Input(key) = self.events.next().unwrap() {
            if self.is_input_buffering() {
                return self.handler_buffering(key);
            }
            else {
                return self.handler_default(key);
            }
        }
        // tick event, no need to distinguish it for now
        return Control::Nothing;
    }

    fn handler_default(&mut self, key: Key) -> Control {
        match key {
            Key::Char('q') => {
                return Control::Quit;
            }
            // TODO: support arrow keys
            Key::Char('j') => {
                return Control::ScrollDown;
            }
            Key::Char('k') => {
                return Control::ScrollUp;
            }
            Key::Char('l') => {
                return Control::ScrollLeft;
            }
            Key::Char('h') => {
                return Control::ScrollRight;
            }
            Key::Char('G') => {
                return Control::ScrollBottom;
            }
            Key::Ctrl('f') | Key::PageDown => {
                return Control::ScrollPageDown;
            }
            Key::Ctrl('b') | Key::PageUp => {
                return Control::ScrollPageUp;
            }
            Key::Char(x) if "0123456789".contains(x.to_string().as_str()) => {
                let init_buffer = x.to_string();
                self.buffer_state = BufferState::Active(init_buffer.clone());
                return Control::BufferContent(init_buffer.clone());
            }
            _ => {
                return Control::Nothing;
            }
        }
    }

    fn handler_buffering(&mut self, key: Key) -> Control {
        match key {
            Key::Esc => {
                self.reset_buffer();
                return Control::BufferReset;
            }
            Key::Backspace => {
                let new_buffer = match &self.buffer_state {
                    BufferState::Active(buffer) => {
                        let mut chars = buffer.chars();
                        chars.next_back();
                        chars.as_str().to_owned()
                    }
                    _ => "".to_owned()
                };
                if new_buffer.len() > 0 {
                    self.buffer_state = BufferState::Active(new_buffer.clone());
                    return Control::BufferContent(new_buffer);
                }
                else {
                    self.reset_buffer();
                    return Control::BufferReset;
                }
            }
            Key::Char('G') => {
                let goto_line = match &self.buffer_state {
                    BufferState::Active(buf) => buf.parse::<usize>().ok(),
                    _ => None,
                };
                let res;
                if let Some(n) = goto_line {
                    res = Control::ScrollTo(n);
                }
                else {
                    res = Control::BufferReset;
                }
                self.reset_buffer();
                return res;
            }
            Key::Char(x) => {
                let new_buffer = match &self.buffer_state {
                    BufferState::Active(buffer) => buffer.to_owned() + x.to_string().as_str(),
                    _ => x.to_string(),
                };
                self.buffer_state = BufferState::Active(new_buffer.clone());
                return Control::BufferContent(new_buffer.clone());
            }
            _ => {
                return Control::Nothing;
            }
        }
    }

    fn is_input_buffering(&self) -> bool {
        match self.buffer_state {
            BufferState::Active(_) => {
                return true;
            }
            _ => {
                return false;
            }
        }
    }

    fn reset_buffer(&mut self) {
        self.buffer_state = BufferState::Inactive;
    }

}