use crate::epub::{Block, Span, SpanStyle};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span as TuiSpan};
use unicode_width::UnicodeWidthStr;

const HEADING_COLOR: Color = Color::Cyan;

/// Wrap a chapter's blocks into a flat list of styled lines for a given width.
/// Pure function: same input always produces same output.
pub fn wrap_chapter(blocks: &[Block], width: u16) -> Vec<Line<'static>> {
    let width = width as usize;
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Blank => out.push(Line::default()),
            Block::Paragraph { spans } => {
                wrap_spans(spans, width, false, &mut out);
                out.push(Line::default());
            }
            Block::Heading { spans, .. } => {
                wrap_spans(spans, width, true, &mut out);
                out.push(Line::default());
            }
        }
    }
    out
}

fn wrap_spans(
    spans: &[Span],
    width: usize,
    heading: bool,
    out: &mut Vec<Line<'static>>,
) {
    let width = width.max(1);
    let mut current: Vec<TuiSpan<'static>> = Vec::new();
    let mut current_width: usize = 0;

    for span in spans {
        let style = tui_style(span.style, heading);
        for token in tokens(&span.text) {
            match token {
                Token::Word(w) => {
                    let w_width = UnicodeWidthStr::width(w);
                    let need_space = !current.is_empty() && current_width > 0;
                    let extra = if need_space { 1 } else { 0 };
                    if current_width + extra + w_width > width && !current.is_empty() {
                        out.push(Line::from(std::mem::take(&mut current)));
                        current_width = 0;
                    }
                    if !current.is_empty() && current_width > 0 {
                        push_span(&mut current, " ".to_string(), style);
                        current_width += 1;
                    }
                    push_span(&mut current, w.to_string(), style);
                    current_width += w_width;
                }
                Token::Whitespace => {
                    // Word-boundary marker; the next word handles spacing.
                }
            }
        }
    }
    if !current.is_empty() {
        out.push(Line::from(current));
    }
}

fn push_span(line: &mut Vec<TuiSpan<'static>>, text: String, style: Style) {
    if let Some(last) = line.last_mut() {
        if last.style == style {
            last.content = format!("{}{}", last.content, text).into();
            return;
        }
    }
    line.push(TuiSpan::styled(text, style));
}

fn tui_style(style: SpanStyle, heading: bool) -> Style {
    let mut s = Style::default();
    if heading {
        s = s.fg(HEADING_COLOR).add_modifier(Modifier::BOLD);
    }
    match style {
        SpanStyle::Plain => s,
        SpanStyle::Bold => s.add_modifier(Modifier::BOLD),
        SpanStyle::Italic => s.add_modifier(Modifier::ITALIC),
    }
}

#[derive(Debug)]
enum Token<'a> {
    Word(&'a str),
    Whitespace,
}

fn tokens(s: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, ch) in s.char_indices() {
        if ch.is_whitespace() {
            if let Some(s_start) = start.take() {
                out.push(Token::Word(&s[s_start..i]));
            }
            out.push(Token::Whitespace);
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s_start) = start {
        out.push(Token::Word(&s[s_start..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub::Span;

    fn pgraph(text: &str) -> Block {
        Block::Paragraph { spans: vec![Span::plain(text)] }
    }

    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn short_paragraph_fits_one_line_plus_blank() {
        let blocks = vec![pgraph("hello world")];
        let lines = wrap_chapter(&blocks, 80);
        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "hello world");
        assert_eq!(line_text(&lines[1]), "");
    }

    #[test]
    fn long_paragraph_wraps_at_word_boundary() {
        let blocks = vec![pgraph("the quick brown fox jumps over the lazy dog")];
        let lines = wrap_chapter(&blocks, 20);
        // Lines must each be <= 20 columns.
        for line in &lines {
            let w = UnicodeWidthStr::width(line_text(line).as_str());
            assert!(w <= 20, "line {:?} is {} columns wide", line_text(line), w);
        }
        // Recombined text matches the original (modulo whitespace).
        let recombined: String = lines
            .iter()
            .map(|l| line_text(l))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(recombined, "the quick brown fox jumps over the lazy dog");
    }

    #[test]
    fn blank_block_emits_empty_line() {
        let lines = wrap_chapter(&[Block::Blank], 80);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "");
    }

    #[test]
    fn empty_chapter_emits_no_lines() {
        let lines = wrap_chapter(&[], 80);
        assert!(lines.is_empty());
    }

    #[test]
    fn very_narrow_width_does_not_panic() {
        let blocks = vec![pgraph("supercalifragilisticexpialidocious")];
        let lines = wrap_chapter(&blocks, 1);
        // The single long word becomes its own line (overflow accepted).
        assert!(!lines.is_empty());
    }
}
