use crate::epub::{Block, ChapterKind, Span, SpanStyle};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span as TuiSpan};
use ratatui::widgets::{Block as TuiBlock, Borders, Clear, Paragraph};
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
            Block::Image(ascii_lines) => {
                // Pre-rendered ASCII art: emit each line as-is.
                // No wrapping, no styling; the renderer handles clipping.
                //
                // Inline images contribute 0 to chapter_offset — the image
                // has no character footprint in the chapter's prose flow,
                // so subsequent paragraphs' source offsets stay correct
                // for smart-resize position tracking. All emitted lines
                // (including the trailing blank) share the same
                // source_offset.
                //
                // Mid-image resize landing: because the image lines and the
                // first line of the next paragraph all share the same
                // source_offset, `partition_point` lands the user on the
                // last line at that offset — which is the next paragraph's
                // first line, not the image. The user effectively "skips
                // past" the image on resize. Acceptable: the middle of an
                // image isn't a meaningful resume point, and landing on the
                // following prose is more useful than the image's first
                // row.
                //
                // Postcondition: chapter_offset is the same value as on entry.
                let image_offset = chapter_offset;
                for art_line in ascii_lines {
                    lines.push(Line::from(art_line.clone()));
                    source_offsets.push(image_offset);
                }
                // Trailing blank for visual breathing room.
                lines.push(Line::default());
                source_offsets.push(image_offset);
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

/// Truncate a string to fit within `max_cols` display columns.
/// Delegates to `render_library::truncate_to_width`; kept as a named
/// alias so existing callsites don't need a touch-up.
fn truncate_right(s: &str, max_cols: usize) -> String {
    crate::render_library::truncate_to_width(s, max_cols)
}

pub struct RenderInput<'a> {
    pub wrapped: &'a [Line<'static>],
    pub line_offset: usize,
    pub status: StatusInput<'a>,
    /// When true, draw the help-screen overlay (a centered modal
    /// listing every keybinding) on top of the body and status bar.
    pub show_help: bool,
    /// User-configured body width cap. The renderer uses this to size
    /// the centered body column; `body_text_width` uses it to set the
    /// wrap target. Both must agree or the wrap output gets clipped.
    pub max_body_width: u16,
    /// When `Some`, draw the table-of-contents overlay (a centered modal
    /// listing every chapter) on top of the body and status bar. The
    /// help overlay wins when both are somehow set; the App is expected
    /// to keep them mutually exclusive.
    pub toc: Option<TocOverlay>,
}

/// Data the renderer needs to draw the TOC overlay. `None` (in
/// `RenderInput::toc`) when the overlay is not visible.
pub struct TocOverlay {
    /// All chapter labels in order. Each entry is `(label, kind)`
    /// where label is what to display and kind distinguishes Main
    /// from FrontMatter for visual styling. Use `Chapter::title` with
    /// a fallback to `"Chapter N"` when None.
    pub entries: Vec<(String, ChapterKind)>,
    /// Currently-selected entry. Renderer draws this with a Reversed
    /// background so the user can see "where Enter would take me."
    pub selection: usize,
    /// Index of the chapter the user is actually reading. Highlighted
    /// with a `▶` prefix so the user can see "where I am" vs "where
    /// would Enter take me."
    pub current_chapter: usize,
}

/// The body's column cap when the user doesn't specify `--width`.
/// Reading at >80 columns is fatiguing for most fiction; this is the
/// industry-standard line-length sweet spot.
pub const DEFAULT_MAX_BODY_WIDTH: u16 = 80;
const BODY_LEFT_PAD: u16 = 3;


pub fn render(frame: &mut Frame, area: Rect, input: RenderInput<'_>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let body_area = chunks[0];
    let status_area = chunks[1];

    // Compute centered body column, capped at the user's configured width.
    let body_width = body_area.width.min(input.max_body_width);
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

    if input.show_help {
        crate::help::render_help_overlay(frame, area);
    } else if let Some(toc) = &input.toc {
        render_toc_overlay(frame, area, toc);
    }
}

/// Render a centered modal over `area` listing every chapter and
/// highlighting the user's selection.
///
/// Mirrors `render_help_overlay`: `Clear` first to keep body text from
/// bleeding through, modal sized to fit content but clamped to `area`
/// so it never tries to draw outside the frame.
fn render_toc_overlay(frame: &mut Frame, area: Rect, toc: &TocOverlay) {
    // Modal sized to fit a reasonable number of entries on screen.
    let max_entry_width = toc
        .entries
        .iter()
        .map(|(label, _)| UnicodeWidthStr::width(label.as_str()))
        .max()
        .unwrap_or(20);
    // Entry width: prefix (2) + main-idx repr (5) + label + small padding.
    // Clamp to area.width so we never overflow the frame (which would
    // panic in ratatui). On terminals narrower than the desired
    // minimum (30), use the full width — the overlay still draws but
    // labels may truncate.
    let desired_width = (max_entry_width as u16).saturating_add(12).max(30);
    let modal_width = desired_width.min(area.width);
    // Modal height: borders (2) + entries + spacer + footer — capped
    // at area. visible_entries is bounded to at-least-1 so the modal
    // still draws on extremely short terminals (the TestBackend smoke
    // tests exercise this path).
    let visible_entries = (area.height.saturating_sub(6) as usize)
        .clamp(1, toc.entries.len().max(1));
    let desired_height = (visible_entries as u16).saturating_add(4).max(7);
    let modal_height = desired_height.min(area.height);

    let x = area.x + area.width.saturating_sub(modal_width) / 2;
    let y = area.y + area.height.saturating_sub(modal_height) / 2;
    let modal_area = Rect {
        x,
        y,
        width: modal_width,
        height: modal_height,
    };

    // Compute scroll: keep selection in view.
    let scroll = center_scroll(toc.selection, toc.entries.len(), visible_entries);

    let end = (scroll + visible_entries).min(toc.entries.len());
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(visible_entries + 2);
    for abs_idx in scroll..end {
        let (label, kind) = &toc.entries[abs_idx];
        let is_selected = abs_idx == toc.selection;
        let is_current = abs_idx == toc.current_chapter;
        let is_main = matches!(kind, ChapterKind::Main);

        let prefix = if is_current { "▶ " } else { "  " };
        let main_idx_repr = if is_main {
            // Compute 1-based main-chapter number on the fly.
            let main_count_so_far = toc
                .entries
                .iter()
                .take(abs_idx + 1)
                .filter(|(_, k)| matches!(k, ChapterKind::Main))
                .count();
            format!("{:>3}. ", main_count_so_far)
        } else {
            // Front matter: no number, just spacing.
            "     ".to_string()
        };

        let mut style = Style::default();
        if is_selected {
            style = style.add_modifier(Modifier::REVERSED);
        }
        if !is_main {
            style = style.add_modifier(Modifier::DIM);
        }

        let line = Line::from(vec![
            TuiSpan::raw(prefix.to_string()),
            TuiSpan::styled(format!("{}{}", main_idx_repr, label), style),
        ]);
        lines.push(line);
    }

    let footer = Line::from(vec![TuiSpan::styled(
        "  Enter jump · ↑↓ navigate · Esc close",
        Style::default().add_modifier(Modifier::DIM),
    )]);
    lines.push(Line::default());
    lines.push(footer);

    let block = TuiBlock::default()
        .title(" Table of contents ")
        .borders(Borders::ALL);

    frame.render_widget(Clear, modal_area);
    frame.render_widget(Paragraph::new(lines).block(block), modal_area);
}

/// Adjust scroll so `selection` is within the visible window. Centers
/// the selection when possible, clamps to start/end at the boundaries.
/// Shared by the TOC overlay and the Library list view — same math,
/// same semantics.
pub(crate) fn center_scroll(selection: usize, total: usize, visible: usize) -> usize {
    if total <= visible {
        return 0;
    }
    if selection < visible / 2 {
        return 0;
    }
    let max_scroll = total.saturating_sub(visible);
    selection.saturating_sub(visible / 2).min(max_scroll)
}

/// Width in columns the wrap step should target, given the terminal
/// width and the user-configured body cap.
///
/// Floored at 20 because narrower wrap targets produce one-word-per-line
/// output that's worse than letting the renderer clip a wider wrap. A
/// user-passed `--width` value below ~24 will be silently bumped to the
/// floor.
pub fn body_text_width(terminal_width: u16, max_body_width: u16) -> u16 {
    terminal_width
        .min(max_body_width)
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
    fn wrap_chapter_emits_image_lines_unwrapped() {
        let blocks = vec![Block::Image(vec![
            "  .:-=  ".into(),
            "  =+*#  ".into(),
            "  *#%@  ".into(),
        ])];
        // Width 4 — the image lines are 8 chars wide, but we don't wrap them.
        // The renderer will clip; wrap_chapter just emits.
        let wrapped = wrap_chapter(&blocks, 4);
        // Three art lines + trailing blank, then the trailing-blank trim
        // removes the last blank → 3 lines.
        assert_eq!(wrapped.lines.len(), 3);
        // Each line preserved verbatim.
        let line_text = |line: &Line| -> String {
            line.spans.iter().map(|s| s.content.as_ref()).collect()
        };
        assert_eq!(line_text(&wrapped.lines[0]), "  .:-=  ");
        assert_eq!(line_text(&wrapped.lines[1]), "  =+*#  ");
        assert_eq!(line_text(&wrapped.lines[2]), "  *#%@  ");
    }

    #[test]
    fn wrap_chapter_image_does_not_advance_source_offset() {
        // Paragraph + Image + Paragraph. The Image should not advance
        // chapter_offset, so the second paragraph starts at the same
        // offset it would have without the image (just the first
        // paragraph's char count).
        let p1 = Block::Paragraph {
            spans: vec![Span::plain("alpha bravo")],
        };
        let img = Block::Image(vec!["####".into(), "####".into()]);
        let p2 = Block::Paragraph {
            spans: vec![Span::plain("charlie")],
        };
        let wrapped = wrap_chapter(&[p1, img, p2], 80);

        // After p1 there are 11 source chars ("alpha bravo"). The image
        // should NOT add to that, so p2's first line offset is 11.
        // (The pre-image trailing-blank line is at 11; the image lines
        // are all at 11; the post-image trailing-blank is at 11; p2's
        // first line is at 11 too — same source position because the
        // image contributed nothing.)
        let p2_first = wrapped
            .lines
            .iter()
            .enumerate()
            .find(|(_, l)| {
                let txt: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                txt == "charlie"
            })
            .map(|(i, _)| i)
            .expect("p2 should appear in wrapped");
        let p2_offset = wrapped.source_offsets[p2_first];
        assert_eq!(
            p2_offset, 11,
            "paragraph after image should start at the same offset the image started at"
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
            title: "Frankenstein",
            chapter_display: Some((4, 22)),
            page: 18,
            total_pages: 247,
            warning: None,
            width: 80,
        });
        assert_eq!(UnicodeWidthStr::width(bar.as_str()), 80);
        assert!(bar.contains("Frankenstein"));
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
    fn render_input_has_show_help_field() {
        // Compile-time verification that the field exists and has the
        // expected type. A heavier render-side test would pin the cell
        // buffer and lock us out of visual tweaks (see Task 17 review).
        let _ = RenderInput {
            wrapped: &[],
            line_offset: 0,
            status: StatusInput {
                title: "x",
                chapter_display: None,
                page: 1,
                total_pages: 1,
                warning: None,
                width: 80,
            },
            show_help: false,
            max_body_width: DEFAULT_MAX_BODY_WIDTH,
            toc: None,
        };
    }

    #[test]
    fn body_text_width_caps_at_max() {
        assert_eq!(
            body_text_width(200, DEFAULT_MAX_BODY_WIDTH),
            DEFAULT_MAX_BODY_WIDTH - BODY_LEFT_PAD
        );
        assert_eq!(body_text_width(80, DEFAULT_MAX_BODY_WIDTH), 80 - BODY_LEFT_PAD);
        assert_eq!(body_text_width(40, DEFAULT_MAX_BODY_WIDTH), 40 - BODY_LEFT_PAD);
    }

    #[test]
    fn body_text_width_floors_at_20() {
        // Tiny terminal.
        assert_eq!(body_text_width(10, DEFAULT_MAX_BODY_WIDTH), 20);
    }

    #[test]
    fn body_text_width_respects_custom_cap() {
        // User passes --width=120 on a 200-col terminal.
        assert_eq!(body_text_width(200, 120), 120 - BODY_LEFT_PAD);
        // User passes --width=120 on a 100-col terminal: terminal still wins.
        assert_eq!(body_text_width(100, 120), 100 - BODY_LEFT_PAD);
        // User passes --width=40 on a wide terminal: cap wins.
        assert_eq!(body_text_width(200, 40), 40 - BODY_LEFT_PAD);
    }

    #[test]
    fn help_overlay_does_not_panic_on_narrow_terminal() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // 4×4 — pathologically tiny.
        let backend = TestBackend::new(4, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let input = RenderInput {
                    wrapped: &[],
                    line_offset: 0,
                    status: StatusInput {
                        title: "x",
                        chapter_display: None,
                        page: 1,
                        total_pages: 1,
                        warning: None,
                        width: area.width,
                    },
                    show_help: true,
                    max_body_width: DEFAULT_MAX_BODY_WIDTH,
                    toc: None,
                };
                render(frame, area, input);
            })
            .unwrap();
    }

    #[test]
    fn help_overlay_does_not_panic_on_short_terminal() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // 10×3 — typical of a small tmux pane.
        let backend = TestBackend::new(10, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let input = RenderInput {
                    wrapped: &[],
                    line_offset: 0,
                    status: StatusInput {
                        title: "x",
                        chapter_display: None,
                        page: 1,
                        total_pages: 1,
                        warning: None,
                        width: area.width,
                    },
                    show_help: true,
                    max_body_width: DEFAULT_MAX_BODY_WIDTH,
                    toc: None,
                };
                render(frame, area, input);
            })
            .unwrap();
    }

    #[test]
    fn render_with_custom_width_above_terminal_does_not_panic() {
        // The user passed --width=120 on a 60-col terminal. The wrap
        // produced lines targeting min(60, 120)-3 = 57 cols. The
        // renderer's body rect should be sized to min(60, 120) = 60.
        // Both agree; no clipping, no panic.
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(60, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(
                    frame,
                    area,
                    RenderInput {
                        wrapped: &[],
                        line_offset: 0,
                        status: StatusInput {
                            title: "x",
                            chapter_display: None,
                            page: 1,
                            total_pages: 1,
                            warning: None,
                            width: area.width,
                        },
                        show_help: false,
                        max_body_width: 120,
                        toc: None,
                    },
                );
            })
            .unwrap();
    }

    #[test]
    fn toc_overlay_does_not_panic_on_narrow_terminal() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(
                    frame,
                    area,
                    RenderInput {
                        wrapped: &[],
                        line_offset: 0,
                        status: StatusInput {
                            title: "x",
                            chapter_display: None,
                            page: 1,
                            total_pages: 1,
                            warning: None,
                            width: area.width,
                        },
                        show_help: false,
                        max_body_width: DEFAULT_MAX_BODY_WIDTH,
                        toc: Some(TocOverlay {
                            entries: vec![
                                ("Cover".into(), ChapterKind::FrontMatter),
                                (
                                    "Chapter 1: A Long Title That Will Wrap".into(),
                                    ChapterKind::Main,
                                ),
                                ("Chapter 2".into(), ChapterKind::Main),
                            ],
                            selection: 1,
                            current_chapter: 0,
                        }),
                    },
                );
            })
            .unwrap();
    }

    #[test]
    fn toc_overlay_does_not_panic_on_pathologically_narrow_terminal() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use crate::epub::ChapterKind;

        let backend = TestBackend::new(4, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            let area = frame.area();
            render(frame, area, RenderInput {
                wrapped: &[],
                line_offset: 0,
                status: StatusInput {
                    title: "x",
                    chapter_display: None,
                    page: 1,
                    total_pages: 1,
                    warning: None,
                    width: area.width,
                },
                show_help: false,
                max_body_width: DEFAULT_MAX_BODY_WIDTH,
                toc: Some(TocOverlay {
                    entries: vec![
                        ("Cover".into(), ChapterKind::FrontMatter),
                        ("Chapter 1: A Long Title".into(), ChapterKind::Main),
                    ],
                    selection: 1,
                    current_chapter: 0,
                }),
            });
        }).unwrap();
    }

    #[test]
    fn toc_overlay_does_not_panic_on_short_terminal() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use crate::epub::ChapterKind;

        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            let area = frame.area();
            render(frame, area, RenderInput {
                wrapped: &[],
                line_offset: 0,
                status: StatusInput {
                    title: "x",
                    chapter_display: None,
                    page: 1,
                    total_pages: 1,
                    warning: None,
                    width: area.width,
                },
                show_help: false,
                max_body_width: DEFAULT_MAX_BODY_WIDTH,
                toc: Some(TocOverlay {
                    entries: (0..50)
                        .map(|i| (format!("Chapter {i}"), ChapterKind::Main))
                        .collect(),
                    selection: 25,
                    current_chapter: 10,
                }),
            });
        }).unwrap();
    }

    #[test]
    fn center_scroll_keeps_selection_in_view() {
        // Total 100 entries, window of 10.
        // Selection at start: scroll 0.
        assert_eq!(center_scroll(0, 100, 10), 0);
        assert_eq!(center_scroll(4, 100, 10), 0);
        // Selection in middle: centered (selection - visible/2).
        assert_eq!(center_scroll(50, 100, 10), 45);
        // Selection near end: clamps to max_scroll.
        assert_eq!(center_scroll(95, 100, 10), 90);
        assert_eq!(center_scroll(99, 100, 10), 90);
        // Total fits in window: no scroll.
        assert_eq!(center_scroll(5, 8, 10), 0);
    }

}
