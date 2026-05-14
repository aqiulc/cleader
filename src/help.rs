//! Shared help overlay — content table + render function used by both
//! the reader (chapter view) and the library (grid/list view). Single
//! source of truth for keyboard shortcuts so they stay in sync across
//! modes. Same modal-rendering convention as the TOC overlay: Clear
//! the area first to suppress bleed-through, then render a centered
//! bordered block.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span as TuiSpan};
use ratatui::widgets::{Block as TuiBlock, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

/// Help content. Two kinds of entries:
/// - `(text, "")` → rendered as a styled section header or prose line
///   (no key column).
/// - `(label, keys)` → label/key two-column row inside a section.
///
/// Empty `("", "")` rows act as vertical spacers.
const HELP_LINES: &[(&str, &str)] = &[
    ("cleader — a distraction-free terminal EPUB reader.", ""),
    ("Browse a directory of books, search by title or author,", ""),
    ("and read with a clean view that remembers where you left off.", ""),
    ("", ""),
    ("Library", ""),
    ("  Navigate", "↑ ↓ ← → / h j k l"),
    ("  Toggle grid/list", "g"),
    ("  Search", "/"),
    ("  Open selected book", "Enter"),
    ("", ""),
    ("Reading", ""),
    ("  Scroll line", "↑ ↓ / k j"),
    ("  Flip page", "← → / h l / Space b / PgUp PgDn"),
    ("  Next chapter", "n"),
    ("  Previous chapter", "N (Shift+n)"),
    ("  Table of contents", "t"),
    ("", ""),
    ("Anywhere", ""),
    ("  Toggle this help", "?"),
    ("  Quit (saves position)", "q / Esc / Ctrl+C"),
    ("", ""),
    ("cleader · created by Aqiul · aqiul.c@gmail.com", ""),
];

/// Width of the label column in two-column rows (chars). Set to the
/// longest label ("  Quit (saves position)" = 24 chars) + 2.
const HELP_LABEL_WIDTH: usize = 26;

/// Render the help overlay centered over `area`. Caller is responsible
/// for only calling this when `show_help` is true.
pub fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::default());
    for (label, keys) in HELP_LINES {
        if label.is_empty() && keys.is_empty() {
            lines.push(Line::default());
            continue;
        }
        if keys.is_empty() {
            let is_section_header = !label.starts_with(' ')
                && !label.starts_with("cleader");
            let style = if is_section_header {
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED)
            } else {
                Style::default()
            };
            lines.push(Line::from(TuiSpan::styled(
                format!("  {label}"),
                style,
            )));
        } else {
            let padded_label = format!("{label:<HELP_LABEL_WIDTH$}");
            lines.push(Line::from(vec![
                TuiSpan::raw(format!("  {padded_label}")),
                TuiSpan::styled(*keys, Style::default().add_modifier(Modifier::BOLD)),
            ]));
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(TuiSpan::styled(
        "  Press ? Esc q Ctrl+C to close",
        Style::default().add_modifier(Modifier::DIM),
    )));

    let max_content_width = lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let modal_width = ((max_content_width as u16).saturating_add(4))
        .max(20)
        .min(area.width);
    let modal_height = ((lines.len() as u16).saturating_add(2))
        .max(5)
        .min(area.height);

    let x = area.x + area.width.saturating_sub(modal_width) / 2;
    let y = area.y + area.height.saturating_sub(modal_height) / 2;
    let modal_area = Rect {
        x,
        y,
        width: modal_width,
        height: modal_height,
    };

    let block = TuiBlock::default()
        .title(" Key bindings ")
        .borders(Borders::ALL);

    frame.render_widget(Clear, modal_area);
    frame.render_widget(Paragraph::new(lines).block(block), modal_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn render_help_overlay_does_not_panic_on_narrow_terminal() {
        let backend = TestBackend::new(10, 4);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|frame| {
            let area = frame.area();
            render_help_overlay(frame, area);
        })
        .unwrap();
    }

    #[test]
    fn render_help_overlay_includes_creator_info_on_large_terminal() {
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|frame| {
            let area = frame.area();
            render_help_overlay(frame, area);
        })
        .unwrap();
        let buf: String = term.backend().buffer().content.iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf.contains("Aqiul"), "creator name should appear");
        assert!(buf.contains("aqiul.c@gmail.com"), "creator email should appear");
        assert!(buf.contains("Library"), "Library section should appear");
        assert!(buf.contains("Reading"), "Reading section should appear");
    }

    #[test]
    fn render_help_overlay_lists_key_bindings() {
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|frame| {
            let area = frame.area();
            render_help_overlay(frame, area);
        })
        .unwrap();
        let buf: String = term.backend().buffer().content.iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf.contains("Toggle grid/list"));
        assert!(buf.contains("Search"));
        assert!(buf.contains("Table of contents"));
    }
}
