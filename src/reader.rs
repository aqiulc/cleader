use crate::epub::{Block, Span, SpanStyle};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span as TuiSpan};
use unicode_width::UnicodeWidthStr;

const HEADING_COLOR: Color = Color::Cyan;

/// Wrap a chapter's blocks into a flat list of styled lines for a given width.
/// Pure function: same input always produces same output.
///
/// Words longer than `width` are emitted on their own line and exceed `width`.
/// v1 deliberately does not break mid-word — graphemes, hyphenation, CJK, and
/// emoji ZWJ sequences make mid-word splitting a tar pit.
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
    // Drop the trailing blank that the last block added — keeps the
    // chapter from ending with a vestigial empty line below the last
    // paragraph.
    if let Some(last) = out.last() {
        if last.spans.is_empty() {
            out.pop();
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
        if is_word_break(ch) {
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

/// Whitespace characters that act as word boundaries for wrapping.
/// Explicitly excludes NBSP (U+00A0) and narrow NBSP (U+202F) — those
/// were preserved by the EPUB walker on purpose, so the wrapper must
/// treat them as part of the word, not as splittable space.
fn is_word_break(ch: char) -> bool {
    ch.is_whitespace() && ch != '\u{00A0}' && ch != '\u{202F}'
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

    fn line_styles(line: &Line) -> Vec<Style> {
        line.spans.iter().map(|s| s.style).collect()
    }

    #[test]
    fn short_paragraph_fits_one_line_no_trailing_blank() {
        let blocks = vec![pgraph("hello world")];
        let lines = wrap_chapter(&blocks, 80);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "hello world");
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
    fn blank_block_alone_is_trimmed() {
        // A chapter that's nothing but a Blank trims to nothing —
        // the trim treats Block::Blank's empty line the same as a
        // paragraph's trailing blank.
        let lines = wrap_chapter(&[Block::Blank], 80);
        assert!(lines.is_empty());
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

    #[test]
    fn heading_renders_with_bold_cyan() {
        let blocks = vec![Block::Heading {
            level: 1,
            spans: vec![Span::plain("Chapter One")],
        }];
        let lines = wrap_chapter(&blocks, 80);
        // Heading line followed by no trailing blank (we trim) — so 1 line.
        assert_eq!(lines.len(), 1);
        let style = lines[0].spans[0].style;
        assert_eq!(style.fg, Some(Color::Cyan));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn nbsp_does_not_break_words() {
        // "Mr.\u{00A0}Smith" must stay together even at narrow width.
        let blocks = vec![pgraph("Mr.\u{00A0}Smith and Mrs.\u{00A0}Jones")];
        // Width=12 forces wrap. Without NBSP handling the wrap could split
        // between "Mr." and "Smith"; with it, "Mr.\u{00A0}Smith" is one
        // word and goes on its own line if needed.
        let lines = wrap_chapter(&blocks, 12);
        for line in &lines {
            let text = line_text(line);
            // Each line either contains the FULL "Mr.\u{00A0}Smith"
            // unbroken or doesn't contain "Mr." at all (split landed
            // between words).
            if text.contains("Mr.") {
                assert!(
                    text.contains("Mr.\u{00A0}Smith"),
                    "NBSP must keep Mr. and Smith on the same line; got {text:?}"
                );
            }
            if text.contains("Mrs.") {
                assert!(
                    text.contains("Mrs.\u{00A0}Jones"),
                    "NBSP must keep Mrs. and Jones on the same line; got {text:?}"
                );
            }
        }
    }

    #[test]
    fn multi_block_chapter_has_no_trailing_blank() {
        let blocks = vec![pgraph("first"), pgraph("second")];
        let lines = wrap_chapter(&blocks, 80);
        assert!(
            !lines.last().unwrap().spans.is_empty(),
            "last line should be the last paragraph's text, not a blank"
        );
        assert_eq!(line_text(lines.last().unwrap()), "second");
    }

    #[test]
    fn mixed_style_spans_render_as_separate_tui_spans() {
        let blocks = vec![Block::Paragraph {
            spans: vec![
                Span::plain("plain "),
                Span::bold("bold "),
                Span::plain("plain"),
            ],
        }];
        let lines = wrap_chapter(&blocks, 80);
        // First (and only) content line should have multiple TuiSpans
        // because the bold one needs a different style.
        let styles = line_styles(&lines[0]);
        // Distinct styles present: at least one bold and at least one plain.
        let has_bold = styles.iter().any(|s| s.add_modifier.contains(Modifier::BOLD));
        let has_plain = styles.iter().any(|s| !s.add_modifier.contains(Modifier::BOLD));
        assert!(has_bold && has_plain, "expected mixed styles in same line");
    }

    #[test]
    fn cjk_wide_chars_count_as_two_columns() {
        let blocks = vec![pgraph("漢字 漢字 漢字")];
        let lines = wrap_chapter(&blocks, 6);
        // Each "漢字" is 4 columns; with a space they're 5; two pairs would
        // be 9 (4+1+4) — so each line should hold at most one "漢字" pair.
        for line in &lines {
            let w = UnicodeWidthStr::width(line_text(line).as_str());
            assert!(w <= 6, "line {:?} exceeds width 6 (got {})", line_text(line), w);
        }
    }
}
