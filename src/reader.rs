use crate::epub::{Block, Span, SpanStyle};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span as TuiSpan};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

const HEADING_COLOR: Color = Color::Cyan;

/// Output of `wrap_chapter`: parallel arrays of rendered lines and the
/// source-character offsets where each line begins. Offsets are
/// monotonic non-decreasing (wrap walks the source forward), so a
/// binary search recovers the new line for a given source offset
/// after a re-wrap. Used by `App::resize` to preserve the user's
/// viewport position when the terminal width changes.
#[derive(Debug, Default, Clone)]
pub struct WrappedChapter {
    pub lines: Vec<Line<'static>>,
    pub source_offsets: Vec<usize>,
}

impl WrappedChapter {
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Find the line at the given source offset, or the closest line
    /// that starts at-or-before it. If `target` is smaller than the
    /// first line's offset, clamps to the first line (returns
    /// `Some(0)`). Returns `None` only when there are no lines at all
    /// (empty chapter).
    pub fn find_line_for_source(&self, target: usize) -> Option<usize> {
        if self.source_offsets.is_empty() {
            return None;
        }
        // partition_point gives us the first index whose offset is > target.
        // We want the largest index whose offset is <= target. If no offset
        // is <= target (target precedes the first line), clamp to 0 — the
        // user lands on the start of the chapter, which is the right
        // recovery for an out-of-range query.
        let after = self.source_offsets.partition_point(|&off| off <= target);
        Some(after.saturating_sub(1))
    }
}

/// Wrap a chapter's blocks into a flat list of styled lines for a given width,
/// alongside the source-character offset where each line begins. Pure function:
/// same input always produces same output.
///
/// Words longer than `width` are emitted on their own line and exceed `width`.
/// v1 deliberately does not break mid-word — graphemes, hyphenation, CJK, and
/// emoji ZWJ sequences make mid-word splitting a tar pit.
pub fn wrap_chapter(blocks: &[Block], width: u16) -> WrappedChapter {
    let width_us = width as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut source_offsets: Vec<usize> = Vec::new();
    let mut chapter_offset: usize = 0;

    for block in blocks {
        match block {
            Block::Blank => {
                lines.push(Line::default());
                source_offsets.push(chapter_offset);
                // Blank consumes no source chars.
            }
            Block::Paragraph { spans } => {
                wrap_spans(
                    spans,
                    width_us,
                    false,
                    chapter_offset,
                    &mut lines,
                    &mut source_offsets,
                );
                let para_chars: usize = spans.iter().map(|s| s.text.chars().count()).sum();
                chapter_offset += para_chars;
                // Trailing blank — same offset as where paragraph ends.
                lines.push(Line::default());
                source_offsets.push(chapter_offset);
            }
            Block::Heading { spans, .. } => {
                wrap_spans(
                    spans,
                    width_us,
                    true,
                    chapter_offset,
                    &mut lines,
                    &mut source_offsets,
                );
                let head_chars: usize = spans.iter().map(|s| s.text.chars().count()).sum();
                chapter_offset += head_chars;
                lines.push(Line::default());
                source_offsets.push(chapter_offset);
            }
        }
    }

    // Drop the trailing blank that the last block added — keeps the
    // chapter from ending with a vestigial empty line below the last
    // paragraph.
    if let Some(last) = lines.last() {
        if last.spans.is_empty() {
            lines.pop();
            source_offsets.pop();
        }
    }

    WrappedChapter { lines, source_offsets }
}

fn wrap_spans(
    spans: &[Span],
    width: usize,
    heading: bool,
    block_start_offset: usize,
    out_lines: &mut Vec<Line<'static>>,
    out_offsets: &mut Vec<usize>,
) {
    let width = width.max(1);
    let mut current: Vec<TuiSpan<'static>> = Vec::new();
    let mut current_width: usize = 0;
    // Source-char position relative to the start of this block.
    let mut chars_consumed: usize = 0;
    // Snapshot of chars_consumed at the moment the current line started.
    let mut current_line_start_offset: usize = chars_consumed;

    for span in spans {
        let style = tui_style(span.style, heading);
        for token in tokens(&span.text) {
            match token {
                Token::Word(w) => {
                    let w_chars = w.chars().count();
                    let w_width = UnicodeWidthStr::width(w);
                    let need_space = !current.is_empty() && current_width > 0;
                    let extra = if need_space { 1 } else { 0 };
                    if current_width + extra + w_width > width && !current.is_empty() {
                        // Flush current line; record its offset.
                        out_lines.push(Line::from(std::mem::take(&mut current)));
                        out_offsets.push(block_start_offset + current_line_start_offset);
                        current_width = 0;
                        current_line_start_offset = chars_consumed;
                    }
                    if !current.is_empty() && current_width > 0 {
                        push_span(&mut current, " ".to_string(), style);
                        current_width += 1;
                        // The space we inserted is a synthetic separator, not in the
                        // source — don't bump chars_consumed for it.
                    }
                    push_span(&mut current, w.to_string(), style);
                    current_width += w_width;
                    chars_consumed += w_chars;
                }
                Token::Whitespace => {
                    // Inter-word whitespace in the source DOES contribute one
                    // char to the cursor so the next line's offset reflects
                    // "after the space".
                    chars_consumed += 1;
                }
            }
        }
    }
    if !current.is_empty() {
        out_lines.push(Line::from(current));
        out_offsets.push(block_start_offset + current_line_start_offset);
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

const STATUS_RIGHT: &str = "q quit";

pub struct StatusInput<'a> {
    pub title: &'a str,
    /// `Some((current_1based, total_main))` when on a main chapter;
    /// `None` when on front matter (e.g., the cover).
    pub chapter_display: Option<(usize, usize)>,
    pub page: usize,
    pub total_pages: usize,
    /// Most recent persistence-flush failure, if any. Replaces the
    /// `q quit` hint on the right side of the status bar so the user
    /// sees that their position isn't being saved. Cleared by the App
    /// on the next successful flush.
    pub warning: Option<&'a str>,
    pub width: u16,
}

pub fn build_status_bar(s: StatusInput<'_>) -> String {
    let width = s.width as usize;
    if width < 4 {
        return "".into();
    }

    let progress = match s.chapter_display {
        Some((cur, total)) => format!(
            " ── Ch {}/{} ─ Page {}/{} ─ ",
            cur, total, s.page, s.total_pages
        ),
        None => format!(" ── Page {}/{} ─ ", s.page, s.total_pages),
    };
    // Replace the `q quit` hint with the warning when present so the
    // failure is visible without crowding the bar with a new segment.
    let right_text = s.warning.unwrap_or(STATUS_RIGHT);
    let right = format!(" {right_text} ");

    // Reserve space (in unicode columns, NOT bytes) for: leading `── ` (3),
    // progress, right. Trailing dashes are filled by the pad loop. Using
    // .len() here would over-truncate the title because progress contains
    // multi-byte ─ glyphs (3 bytes / 1 column each).
    let title_budget = width.saturating_sub(
        UnicodeWidthStr::width(progress.as_str())
            + UnicodeWidthStr::width(right.as_str())
            + 3,
    );
    let title = truncate_right(s.title, title_budget);

    let mut out = String::with_capacity(width);
    out.push_str("── ");
    out.push_str(&title);
    out.push_str(&progress);
    out.push_str(&right);
    // Pad the rest with the same dash glyph used at the start.
    while UnicodeWidthStr::width(out.as_str()) < width {
        out.push('─');
    }
    // Hard truncate if our math overshot due to wide chars.
    while UnicodeWidthStr::width(out.as_str()) > width {
        out.pop();
    }
    out
}

fn truncate_right(s: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(s) <= budget {
        return s.to_string();
    }
    // Reserve 1 column for the ellipsis.
    let mut acc = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = UnicodeWidthStr::width(ch.to_string().as_str());
        if w + cw + 1 > budget {
            break;
        }
        acc.push(ch);
        w += cw;
    }
    acc.push('…');
    acc
}

pub struct RenderInput<'a> {
    pub wrapped: &'a [Line<'static>],
    pub line_offset: usize,
    pub status: StatusInput<'a>,
}

const MAX_BODY_WIDTH: u16 = 80;
const BODY_LEFT_PAD: u16 = 3;

pub fn render(frame: &mut Frame, area: Rect, input: RenderInput<'_>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let body_area = chunks[0];
    let status_area = chunks[1];

    // Compute centered body column, capped at MAX_BODY_WIDTH.
    let body_width = body_area.width.min(MAX_BODY_WIDTH);
    let h_offset = (body_area.width - body_width) / 2;
    let body_rect = Rect {
        x: body_area.x + h_offset,
        y: body_area.y,
        width: body_width,
        height: body_area.height,
    };

    // Slice the visible window of wrapped lines.
    let visible_rows = body_rect.height as usize;
    let end = (input.line_offset + visible_rows).min(input.wrapped.len());
    let visible = &input.wrapped[input.line_offset.min(input.wrapped.len())..end];
    let owned: Vec<Line<'static>> = visible.to_vec();

    // No .wrap() — we already wrap the chapter via wrap_chapter, so the
    // lines are at the right width by construction. Letting ratatui's
    // wrap re-flow on overflow would split styled spans mid-style
    // (ratatui doesn't know about our semantic boundaries). Over-wide
    // lines clip at the right edge, matching wrap_chapter's documented
    // long-word policy.
    let body = Paragraph::new(owned);
    // Add left padding by inset.
    let padded = Rect {
        x: body_rect.x + BODY_LEFT_PAD,
        y: body_rect.y,
        width: body_rect.width.saturating_sub(BODY_LEFT_PAD),
        height: body_rect.height,
    };
    frame.render_widget(body, padded);

    let status_str = build_status_bar(input.status);
    let status = Paragraph::new(status_str)
        .style(Style::default().add_modifier(Modifier::DIM));
    frame.render_widget(status, status_area);
}

/// Width in columns the wrap step should target, given the terminal width.
///
/// Floored at 20 because narrower wrap targets produce one-word-per-line
/// output that's worse than letting the renderer clip a wider wrap. On
/// terminals narrower than ~24 cols the rendered output will be clipped;
/// that's expected — cleader is not designed for sub-terminal widths.
pub fn body_text_width(terminal_width: u16) -> u16 {
    terminal_width
        .min(MAX_BODY_WIDTH)
        .saturating_sub(BODY_LEFT_PAD)
        .max(20)
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
        let lines = wrap_chapter(&blocks, 80).lines;
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "hello world");
    }

    #[test]
    fn long_paragraph_wraps_at_word_boundary() {
        let blocks = vec![pgraph("the quick brown fox jumps over the lazy dog")];
        let lines = wrap_chapter(&blocks, 20).lines;
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
        let wrapped = wrap_chapter(&[Block::Blank], 80);
        assert!(wrapped.is_empty());
    }

    #[test]
    fn empty_chapter_emits_no_lines() {
        let wrapped = wrap_chapter(&[], 80);
        assert!(wrapped.is_empty());
    }

    #[test]
    fn very_narrow_width_does_not_panic() {
        let blocks = vec![pgraph("supercalifragilisticexpialidocious")];
        let wrapped = wrap_chapter(&blocks, 1);
        // The single long word becomes its own line (overflow accepted).
        assert!(!wrapped.is_empty());
    }

    #[test]
    fn heading_renders_with_bold_cyan() {
        let blocks = vec![Block::Heading {
            level: 1,
            spans: vec![Span::plain("Chapter One")],
        }];
        let lines = wrap_chapter(&blocks, 80).lines;
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
        let lines = wrap_chapter(&blocks, 12).lines;
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
        let lines = wrap_chapter(&blocks, 80).lines;
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
        let lines = wrap_chapter(&blocks, 80).lines;
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
        let lines = wrap_chapter(&blocks, 6).lines;
        // Each "漢字" is 4 columns; with a space they're 5; two pairs would
        // be 9 (4+1+4) — so each line should hold at most one "漢字" pair.
        for line in &lines {
            let w = UnicodeWidthStr::width(line_text(line).as_str());
            assert!(w <= 6, "line {:?} exceeds width 6 (got {})", line_text(line), w);
        }
    }

    #[test]
    fn italic_span_keeps_italic_modifier() {
        let blocks = vec![Block::Paragraph {
            spans: vec![Span::italic("hi")],
        }];
        let lines = wrap_chapter(&blocks, 80).lines;
        assert!(
            lines[0].spans[0].style.add_modifier.contains(Modifier::ITALIC),
            "italic span must keep ITALIC modifier"
        );
    }

    #[test]
    fn bold_span_split_across_two_lines_keeps_bold_on_both() {
        // Force a wrap by using a narrow width.
        let blocks = vec![Block::Paragraph {
            spans: vec![
                Span::plain("aaa "),
                Span::bold("bbb ccc ddd eee fff"),
            ],
        }];
        let lines = wrap_chapter(&blocks, 12).lines;
        // Find bold segments across all output lines; expect at least 2
        // distinct line positions where a bold span appears.
        let mut bold_lines = 0;
        for line in &lines {
            let any_bold = line.spans.iter().any(|s| {
                s.style.add_modifier.contains(Modifier::BOLD)
                    && !s.content.trim().is_empty()
            });
            if any_bold {
                bold_lines += 1;
            }
        }
        assert!(
            bold_lines >= 2,
            "bold modifier must survive across wrap; bold appeared on {bold_lines} line(s)"
        );
    }

    #[test]
    fn wrap_chapter_emits_offsets_parallel_to_lines() {
        let blocks = vec![pgraph("alpha bravo charlie delta echo")];
        let wrapped = wrap_chapter(&blocks, 12);
        assert_eq!(wrapped.lines.len(), wrapped.source_offsets.len());
    }

    #[test]
    fn wrap_chapter_offsets_are_monotonic_non_decreasing() {
        let blocks = vec![
            Block::Heading { level: 1, spans: vec![Span::plain("Chapter 1")] },
            Block::Blank,
            pgraph("First paragraph here."),
            pgraph("Second paragraph follows."),
        ];
        let wrapped = wrap_chapter(&blocks, 20);
        assert_eq!(
            wrapped.source_offsets.first(),
            Some(&0),
            "first line should always start at source offset 0"
        );
        for window in wrapped.source_offsets.windows(2) {
            assert!(
                window[0] <= window[1],
                "source offsets must be monotonic non-decreasing: {window:?}"
            );
        }
    }

    #[test]
    fn find_line_for_source_clamps_when_target_precedes_first_offset() {
        // Synthesize a WrappedChapter whose first source offset is non-zero
        // (e.g., a hypothetical caller injecting offsets directly). The
        // clamp behavior is the documented recovery for an out-of-range
        // target.
        let wc = WrappedChapter {
            lines: vec![Line::default(), Line::default()],
            source_offsets: vec![10, 20],
        };
        assert_eq!(wc.find_line_for_source(0), Some(0));
        assert_eq!(wc.find_line_for_source(5), Some(0));
        assert_eq!(wc.find_line_for_source(10), Some(0));
        assert_eq!(wc.find_line_for_source(15), Some(0));
        assert_eq!(wc.find_line_for_source(20), Some(1));
        assert_eq!(wc.find_line_for_source(99), Some(1));
    }

    #[test]
    fn find_line_for_source_returns_at_or_before_match() {
        let blocks = vec![pgraph("alpha bravo charlie delta echo foxtrot golf")];
        let wrapped = wrap_chapter(&blocks, 12);
        // Querying offset 0 always returns line 0.
        assert_eq!(wrapped.find_line_for_source(0), Some(0));
        // Querying a huge offset clamps to last line.
        assert_eq!(
            wrapped.find_line_for_source(99999),
            Some(wrapped.lines.len() - 1)
        );
    }

    #[test]
    fn find_line_for_source_on_empty_chapter_returns_none() {
        let wrapped = wrap_chapter(&[], 80);
        assert_eq!(wrapped.find_line_for_source(0), None);
    }

    #[test]
    fn status_bar_fits_exact_terminal_width() {
        let bar = build_status_bar(StatusInput {
            title: "Firefly",
            chapter_display: Some((4, 22)),
            page: 18,
            total_pages: 247,
            warning: None,
            width: 80,
        });
        assert_eq!(UnicodeWidthStr::width(bar.as_str()), 80);
        assert!(bar.contains("Firefly"));
        assert!(bar.contains("Ch 4/22"));
        assert!(bar.contains("Page 18/247"));
        assert!(bar.contains("q quit"));
    }

    #[test]
    fn status_bar_truncates_long_title_with_ellipsis_on_right() {
        let bar = build_status_bar(StatusInput {
            title: "An Extremely Long Book Title That Will Not Fit",
            chapter_display: Some((1, 1)),
            page: 1,
            total_pages: 1,
            warning: None,
            width: 50,
        });
        assert!(bar.contains("…"));
        assert!(
            bar.starts_with("── An Extremely"),
            "title budget should now respect column counts, not byte lengths; got {bar:?}"
        );
        assert_eq!(UnicodeWidthStr::width(bar.as_str()), 50);
    }

    #[test]
    fn status_bar_with_tiny_width_does_not_panic() {
        let bar = build_status_bar(StatusInput {
            title: "X",
            chapter_display: Some((1, 1)),
            page: 1,
            total_pages: 1,
            warning: None,
            width: 3,
        });
        // Just must not panic.
        let _ = bar;
    }

    #[test]
    fn status_bar_omits_chapter_segment_when_chapter_display_is_none() {
        let bar = build_status_bar(StatusInput {
            title: "Cover",
            chapter_display: None,
            page: 1,
            total_pages: 1,
            warning: None,
            width: 60,
        });
        assert!(!bar.contains("Ch "), "front matter should not show chapter number; got {bar:?}");
        assert!(bar.contains("Page 1/1"));
        assert_eq!(UnicodeWidthStr::width(bar.as_str()), 60);
    }

    #[test]
    fn status_bar_replaces_q_quit_with_warning_when_present() {
        let bar = build_status_bar(StatusInput {
            title: "Book",
            chapter_display: Some((1, 1)),
            page: 1,
            total_pages: 1,
            warning: Some("save failed: read-only filesystem"),
            width: 80,
        });
        assert!(
            bar.contains("save failed"),
            "warning should appear in the status bar; got {bar:?}"
        );
        assert!(
            !bar.contains("q quit"),
            "warning replaces q quit; got {bar:?}"
        );
        assert_eq!(UnicodeWidthStr::width(bar.as_str()), 80);
    }

    #[test]
    fn body_text_width_caps_at_max() {
        assert_eq!(body_text_width(200), MAX_BODY_WIDTH - BODY_LEFT_PAD);
        assert_eq!(body_text_width(80), 80 - BODY_LEFT_PAD);
        assert_eq!(body_text_width(40), 40 - BODY_LEFT_PAD);
    }

    #[test]
    fn body_text_width_floors_at_20() {
        // Tiny terminal.
        assert_eq!(body_text_width(10), 20);
    }
}
