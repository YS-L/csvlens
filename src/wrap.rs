use ratatui::text::{Line, Span};

pub struct LineWrapper<'a> {
    spans: &'a [Span<'a>],
    max_width: usize,
    index: usize,
    pending: Option<Span<'a>>,
}

impl<'a> LineWrapper<'a> {
    pub fn new(spans: &'a [Span<'a>], max_width: usize) -> Self {
        LineWrapper {
            spans,
            max_width,
            index: 0,
            pending: None,
        }
    }

    pub fn next(&mut self) -> Option<Line<'a>> {
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
                let chars_count = span.content.chars().count();
                let newline_pos = span.content.chars().position(|c| c == '\n');
                if let Some(pos) = newline_pos {
                    if pos <= remaining_width {
                        out_spans.push(Span::styled(
                            span.content.chars().take(pos).collect::<String>(),
                            span.style,
                        ));
                        self.pending = Some(Span::styled(
                            span.content.chars().skip(pos + 1).collect::<String>(),
                            span.style,
                        ));
                    } else {
                        let current: String = span.content.chars().take(remaining_width).collect();
                        let pending: String = span.content.chars().skip(remaining_width).collect();
                        out_spans.push(Span::styled(current, span.style));
                        self.pending = Some(Span::styled(pending, span.style));
                    }
                    // Technically in the first case this might not be zero, but
                    // this is to force the loop to break - we must wrap now.
                    remaining_width = 0;
                } else if chars_count <= remaining_width {
                    remaining_width = remaining_width.saturating_sub(chars_count);
                    out_spans.push(span);
                } else {
                    let current: String = span.content.chars().take(remaining_width).collect();
                    let pending: String = span.content.chars().skip(remaining_width).collect();
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
        // Filter out empty spans
        let out_spans = out_spans
            .into_iter()
            .filter(|s| !s.content.is_empty())
            .collect::<Vec<_>>();
        if out_spans.is_empty() {
            return None;
        }
        Some(Line::from(out_spans))
    }

    pub fn finished(&self) -> bool {
        self.pending.is_none() && self.index >= self.spans.len()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use ratatui::style::{Color, Style};

    #[test]
    fn test_no_wrapping() {
        let s = Span::raw("hello");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 10);
        assert_eq!(wrapper.next(), Some(Line::from(vec![s.clone()])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_with_wrapping() {
        let s = Span::raw("hello");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 2);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("he")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ll")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("o")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_new_lines_before_max_width() {
        let s = Span::raw("hello\nworld");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 10);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("hello")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("world")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_new_lines_after_max_width() {
        let s = Span::raw("hello\nworld");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 3);
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
        let mut wrapper = LineWrapper::new(&spans, 5);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("hello")])));
        assert_eq!(
            wrapper.next(),
            Some(Line::from(vec![
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
        let mut wrapper = LineWrapper::new(&spans, 5);
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
        let mut wrapper = LineWrapper::new(&spans, 2);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("hé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ll")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("o")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_unicode_with_newline_w1() {
        let s = Span::raw("éé\néééééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 1);
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
        let mut wrapper = LineWrapper::new(&spans, 2);
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
        let mut wrapper = LineWrapper::new(&spans, 3);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("ééé")])));
        assert_eq!(wrapper.next(), None);
    }

    #[test]
    fn test_unicode_with_newline_w4() {
        let s = Span::raw("éé\néééééé");
        let spans = vec![s.clone()];
        let mut wrapper = LineWrapper::new(&spans, 4);
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éééé")])));
        assert_eq!(wrapper.next(), Some(Line::from(vec![Span::raw("éé")])));
        assert_eq!(wrapper.next(), None);
    }
}
