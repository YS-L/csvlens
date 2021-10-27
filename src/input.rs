use crate::util::events::{Event, Events};
use termion::event::Key;

pub enum Control {
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    ScrollBottom,
    Quit,
    Nothing,
}

pub struct InputHandler {
    events: Events,
}

impl InputHandler {

    pub fn new() -> InputHandler {
        InputHandler {
            events: Events::new(),
        }
    }

    pub fn next(&self) -> Control {
        if let Event::Input(key) = self.events.next().unwrap() {
            match key {
                Key::Char('q') => {
                    return Control::Quit;
                }
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
                _ => {
                    return Control::Nothing;
                }
            }
        }
        // tick event, no need to distinguish it for now
        return Control::Nothing;
    }

}