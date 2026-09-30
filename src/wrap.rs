use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub struct LineWrapper<'a> {
    spans: &'a [Span<'a>],
    max_width: usize,
    word_wrap: bool,
    index: usize,
    pending: Option<Span<'a>>,
}

impl<'a> LineWrapper<'a> {
    pub fn new(spans: &'a [Span<'a>], max_width: usize, word_wrap: bool) -> Self {
        LineWrapper {
            spans,
            max_width,
            word_wrap,
            index: 0,
            pending: None,
        }
    }

    pub fn next(&mut self) -> Option<Line<'a>> {
        if self.finished() {
            return None;
        }
        let mut out_spans = vec![];
        let mut remaining_width = self.max_width;
        loop {
            let mut span = None;
            if let Some(s) = self.pending.take() {
                span = Some(s);
            } else if self.index < self.spans.len() {
                span = Some(self.spans.get(self.index).cloned().unwrap());
                self.index += 1;
            }
            if let Some(span) = span {
                let content: &str = &span.content;
                let newline_pos = content
                    .find('\n')
                    .map(|pos| (pos, content[..pos].width() <= remaining_width));
                if let Some((pos, true)) = newline_pos {
                    out_spans.push(Span::styled(content[..pos].to_string(), span.style));
                    self.pending = Some(Span::styled(content[pos + 1..].to_string(), span.style));
                    // Technically this might not be zero, but this is to force the loop to break -
                    // we must wrap now.
                    remaining_width = 0;
                } else if content.width() <= remaining_width {
                    remaining_width = remaining_width.saturating_sub(content.width());
                    out_spans.push(span);
                } else {
                    // A character wider than max_width can never fit. Take it anyway at the start
                    // of a line, otherwise wrapping would never make progress.
                    let force_one = remaining_width == self.max_width && self.max_width > 0;
                    let (front, rest) = split_at_width(content, remaining_width, force_one);
                    let mut current = front.to_string();
                    let mut pending = rest;

                    if self.word_wrap
                        && let Some(wrapped) = LineWrapper::wrap_by_whitespace(front)
                    {
                        pending = &content[wrapped.len()..];
                        current = wrapped;
                    }
                    out_spans.push(Span::styled(current, span.style));
                    // pending is empty only when force_one took the last character
                    if !pending.is_empty() {
                        self.pending = Some(Span::styled(pending.to_string(), span.style));
                    }
                    remaining_width = 0;
                }
            } else {
                break;
            }
            if remaining_width == 0 {
                break;
            }
        }
        Some(Line::from(out_spans))
    }

    pub fn finished(&self) -> bool {
        self.pending.is_none() && self.index >= self.spans.len()
    }

    fn wrap_by_whitespace(s: &str) -> Option<String> {
        let mut s_split = s.split(' ');
        let last = s_split.next_back();
        if last.is_some() {
            let front = s_split.collect::<Vec<&str>>().join(" ");
            if front.chars().filter(|c| !c.is_whitespace()).count() > 0 {
                Some(front + " ")
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Split `s` so that the front part fits in `max_width` terminal columns.
/// With `force_one`, the front part contains at least one character.
fn split_at_width(s: &str, max_width: usize, force_one: bool) -> (&str, &str) {
    let mut width = 0;
    for (i, c) in s.char_indices() {
        width += c.width().unwrap_or(0);
        if width > max_width {
            if i == 0 && force_one {
                return s.split_at(c.len_utf8());
            }
            return s.split_at(i);
        }
    }
    (s, "")
}

#[cfg(test)]
mod tests {

    use super::*;
    use ratatui::style::{Color, Style};

    #[test]
    fn test_no_wrapping() {
        let s = Span::raw("hello");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 10, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![s.clone()])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_with_wrapping() {
        let s = Span::raw("hello");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 2, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("he")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ll")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("o")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_new_lines_before_max_width() {
        let s = Span::raw("hello\nworld");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 10, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("hello")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("world")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_new_lines_after_max_width() {
        let s = Span::raw("hello\nworld");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("hel")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("lo")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("wor")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ld")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_multiple_spans() {
        let style = Style::default().fg(Color::Red);
        let spans = vec![
            Span::raw("hello\n"),
            Span::styled("my", style),
            Span::raw("world"),
        ];
        let mut wrapper = LineWrapper::new(&spans, 5, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("hello")])));
        assert_eq!(
            wrapper.next(),
            Some(Line::from(vec![
                Span::raw(""),
                Span::styled("my", style),
                Span::raw("wor")
            ]))
        );
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ld")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_wrap_at_styled_span() {
        let style = Style::default().fg(Color::Red);
        let spans = vec![
            Span::raw("hello"),
            Span::styled("m\ny", style),
            Span::raw("world"),
        ];
        let mut wrapper = LineWrapper::new(&spans, 5, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("hello")])));
        assert_eq!(
            wrapper.next(),
            Some(Line::from(vec![Span::styled("m", style)]))
        );
        assert_eq!(
            wrapper.next(),
            Some(Line::from(vec![
                Span::styled("y", style),
                Span::raw("worl")
            ]))
        );
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("d")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_unicode() {
        let s = Span::raw("héllo");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 2, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("hé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ll")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("o")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_unicode_with_newline_w1() {
        let s = Span::raw("éé\néééééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 1, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_unicode_with_newline_w2() {
        let s = Span::raw("éé\néééééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 2, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_unicode_with_newline_w3() {
        let s = Span::raw("éé\néééééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_unicode_with_newline_w4() {
        let s = Span::raw("éé\néééééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 4, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éééé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_wrap_by_whitespace_1() {
        let s = Span::raw("é é");
        let out = LineWrapper::wrap_by_whitespace(&s.content);
        assert_eq!(out, Some("é ".to_string()));
    }

    #[test]
    fn test_wrap_by_whitespace_2() {
        let s = Span::raw(" éé");
        let out = LineWrapper::wrap_by_whitespace(&s.content);
        assert_eq!(out, None);
    }

    #[test]
    fn test_word_wrap_1() {
        let s = Span::raw("éé\né éé ééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3, true);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é ")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé ")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_word_wrap_2() {
        let s = Span::raw("ééé é ééé ééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3, true);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw(" é ")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw(" éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("é")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_multiple_newlines() {
        let s = Span::raw("ééé\n\nééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 4, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_zero_max_width() {
        let s = Span::raw("ééé\n\nééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 0, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("")])));
        assert_eq!(wrapper.finished(), false);
    }
    #[test]
    fn test_wide_chars() {
        let s = Span::raw("日本語です");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 4, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("日本")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("語で")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("す")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_wide_char_does_not_fit_in_remaining_width() {
        let s = Span::raw("ab日本");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ab")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("日")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("本")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_wide_chars_with_newline() {
        let s = Span::raw("日本\n語");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("日")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("本")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("語")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_wide_chars_word_wrap() {
        let s = Span::raw("日本 語です");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 6, true);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("日本 ")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("語です")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_wide_char_wider_than_max_width() {
        // A character that can never fit is still emitted, one per line, so wrapping terminates
        let s = Span::raw("日本");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 1, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("日")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("本")])));
        assert_eq!(wrapper.next(), None);
    }
}
