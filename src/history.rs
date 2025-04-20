use crate::common::InputMode;
use std::collections::HashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};

pub struct BufferHistory {
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
    pub fn new() -> Self {
        BufferHistoryContainer {
            inner: HashMap::new(),
        }
    }

    pub fn set(&mut self, input_mode: InputMode, content: &str) {
        match self.inner.entry(input_mode) {
            Occupied(mut e) => {
                e.get_mut().push(content);
            }
            Vacant(e) => {
                e.insert(BufferHistory::new_with(content));
            }
        }
    }

    pub fn prev(&mut self, input_mode: InputMode) -> Option<String> {
        self.inner
            .get_mut(&input_mode)
            .and_then(|history| history.prev())
    }

    pub fn next(&mut self, input_mode: InputMode) -> Option<String> {
        self.inner
            .get_mut(&input_mode)
            .and_then(|history| history.next())
    }

    pub fn reset_cursors(&mut self) {
        for (_, history) in self.inner.iter_mut() {
            history.reset_cursor();
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_prev_next() {
        let mut history = BufferHistory::new_with("foo");
        history.push("bar");
        history.push("baz");
        history.push("foo");
        assert_eq!(history.prev(), Some("foo".to_string()));
        assert_eq!(history.prev(), Some("baz".to_string()));
        assert_eq!(history.prev(), Some("bar".to_string()));
        assert_eq!(history.prev(), None);
        assert_eq!(history.prev(), None);
        assert_eq!(history.next(), Some("baz".to_string()));
        assert_eq!(history.next(), Some("foo".to_string()));
        assert_eq!(history.next(), None);
        assert_eq!(history.next(), None);
    }

    #[test]
    fn test_push_duplicate() {
        let mut history = BufferHistory::new_with("foo");
        history.push("bar");
        history.push("baz");
        history.push("foo");
        history.push("bar");
        assert_eq!(history.prev(), Some("bar".to_string()));
        assert_eq!(history.prev(), Some("foo".to_string()));
        assert_eq!(history.prev(), Some("baz".to_string()));
        assert_eq!(history.prev(), None);
    }

    #[test]
    fn test_container() {
        let mut history_container = BufferHistoryContainer::new();
        history_container.set(InputMode::Find, "foo");
        history_container.set(InputMode::Find, "bar");
        history_container.set(InputMode::GotoLine, "123");
        history_container.set(InputMode::GotoLine, "456");
        assert_eq!(history_container.prev(InputMode::Default), None);
        assert_eq!(
            history_container.prev(InputMode::Find),
            Some("bar".to_string())
        );
        assert_eq!(
            history_container.prev(InputMode::GotoLine),
            Some("456".to_string())
        );
    }
}
