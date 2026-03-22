use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

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
                let content_ref: &str = span.content.as_ref();
                let display_width = UnicodeWidthStr::width(content_ref);

                let newline_fits = content_ref.find('\n').map(|byte_pos| {
                    let before = &content_ref[..byte_pos];
                    let before_width = UnicodeWidthStr::width(before);
                    (byte_pos, before_width <= remaining_width)
                });

                if let Some((byte_pos, true)) = newline_fits {
                    out_spans.push(Span::styled(
                        content_ref[..byte_pos].to_string(),
                        span.style,
                    ));
                    self.pending = Some(Span::styled(
                        content_ref[byte_pos + 1..].to_string(),
                        span.style,
                    ));
                    // Technically this might not be zero, but this is to force the loop to break -
                    // we must wrap now.
                    remaining_width = 0;
                } else if display_width <= remaining_width {
                    remaining_width = remaining_width.saturating_sub(display_width);
                    out_spans.push(span);
                } else {
                    let (front, rest) = split_by_width(content_ref, remaining_width);
                    let mut current: String = front.to_string();
                    let pending: String;

                    if self.word_wrap {
                        if let Some(wrapped) = LineWrapper::wrap_by_whitespace(current.as_str()) {
                            current = wrapped;
                            pending = content_ref[current.len()..].to_string();
                        } else {
                            pending = rest.to_string();
                        }
                    } else {
                        pending = rest.to_string();
                    }
                    out_spans.push(Span::styled(current, span.style));
                    self.pending = Some(Span::styled(pending, span.style));
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

/// Split a string at a display width boundary.
/// Returns (front, rest) where front has display width <= max_width.
fn split_by_width(s: &str, max_width: usize) -> (&str, &str) {
    let mut width = 0;
    for (i, c) in s.char_indices() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + char_width > max_width {
            return (&s[..i], &s[i..]);
        }
        width += char_width;
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
    fn test_cjk_no_wrapping() {
        let s = Span::raw("日本語");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 10, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![s.clone()])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_cjk_wrapping() {
        let s = Span::raw("日本語");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 4, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("日本")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("語")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_cjk_width_boundary() {
        let s = Span::raw("日本語");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("日")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("本")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("語")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_cjk_mixed_with_ascii() {
        let s = Span::raw("A日B");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("A日")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("B")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_cjk_with_newline() {
        let s = Span::raw("日本\n語");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 6, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("日本")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("語")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_cjk_exact_fit() {
        let s = Span::raw("日本");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 4, false);
        assert_eq!(wrapper.next(), Some(Line::from(vec![s.clone()])));
        assert_eq!(wrapper.next(), None);
    }
}
