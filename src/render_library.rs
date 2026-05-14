//! Library view rendering — list and grid modes, cover cell layout,
//! footer with search box / warning / default hint, "no matches"
//! overlay. Reader-mode chapter rendering lives in `reader.rs`;
//! these two split was done in v0.4.6 hygiene to keep both modules
//! at a comfortable size.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span as TuiSpan};
use ratatui::widgets::{Block as TuiBlock, Borders, Paragraph};

pub struct LibraryRenderInput<'a> {
    pub entries: &'a [crate::library::LibraryEntry],
    pub selection: usize,
    pub view_mode: crate::prefs::ViewMode,
    /// Optional — `None` disables grid rendering even when view_mode == Grid
    /// (forces list fallback). Provided in production by `LibraryApp`.
    pub cover_cache: Option<&'a crate::cover_cache::CoverCache>,
    /// Lookup from entry index → BookId. Used to ask the cache for the
    /// cover. Index outside the slice maps to None.
    pub book_ids: &'a [Option<crate::epub::BookId>],
    /// Optional warning to surface in the footer (e.g. prefs save error).
    pub warning: Option<&'a str>,
    /// Indices into `entries` that should be shown. Either the full
    /// range (`0..entries.len()`) when no search filter is active, or
    /// the matching subset otherwise. Renderer iterates this; `selection`
    /// indexes into this sequence (not into `entries` directly).
    pub display_indices: &'a [usize],
    /// Current search query (for the footer search-box rendering).
    /// `None` when in Idle. `Some` even with an empty string when in
    /// Editing (to draw the cursor).
    pub search_query: Option<&'a str>,
    /// Current search mode. Drives footer rendering and the "no matches"
    /// content overlay.
    pub search_mode: crate::search::SearchMode,
}

pub fn render_library(frame: &mut Frame, area: Rect, input: LibraryRenderInput<'_>) {
    use crate::prefs::ViewMode;
    let force_list = area.width < CELL_WIDTH || area.height < (CELL_HEIGHT + 2);
    match (input.view_mode, force_list) {
        (ViewMode::Grid, false) => render_library_grid(frame, area, input),
        _ => render_library_list(frame, area, input),
    }
}

/// Cell width and height for the grid view. 24x16 = 22x14 inside the
/// 1-col border, split into a 22x12 cover region and a 22x2 title region.
pub const CELL_WIDTH: u16 = 24;
pub const CELL_HEIGHT: u16 = 21;

/// Compute the visible-cell index range for the library grid view.
///
/// `grid_w` / `grid_h` are the dimensions of the grid area (already
/// post-reservation — the renderer's outer layout strips 1 row for
/// title and 1 row for footer before calling this). Returns `None`
/// when the grid area is too small for even one cell.
///
/// Selection is page-snapped (top-of-screen is the start of the page
/// containing the selection).
///
/// Used by both `render_library_grid` (to know which cells to draw)
/// and `library_event_loop` (to know which covers to request). Keeping
/// the math in one place prevents drift between the two callsites.
pub fn visible_grid_range(
    grid_w: u16,
    grid_h: u16,
    total: usize,
    selection: usize,
) -> Option<std::ops::Range<usize>> {
    if grid_w < CELL_WIDTH || grid_h < CELL_HEIGHT {
        return None;
    }
    let cols = (grid_w / CELL_WIDTH).max(1) as usize;
    let rows = (grid_h / CELL_HEIGHT).max(1) as usize;
    let selection_row = selection / cols;
    let top_row = (selection_row / rows) * rows;
    let first = top_row * cols;
    let last = (first + cols * rows).min(total);
    Some(first..last)
}

fn render_library_list(frame: &mut Frame, area: Rect, input: LibraryRenderInput<'_>) {
    // Layout: title bar (1 row), list (rest), footer (1 row).
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    // Title bar shows total library size, NOT the filtered match count
    // (which the footer surfaces as "N matches"). This deliberate split
    // gives the user both numbers at a glance.
    let title = TuiSpan::styled(
        format!(" cleader library — {} book(s) ", input.entries.len()),
        Style::default().add_modifier(Modifier::BOLD),
    );
    frame.render_widget(Paragraph::new(Line::from(title)), chunks[0]);

    // No-matches case: filter active and 0 results.
    if input.display_indices.is_empty()
        && !matches!(input.search_mode, crate::search::SearchMode::Idle)
    {
        render_no_matches(frame, chunks[1]);
    } else {
        // List body. `display_indices` is the sequence to render.
        let visible_rows = chunks[1].height as usize;
        let total = input.display_indices.len();
        let scroll = crate::reader::center_scroll(input.selection, total, visible_rows);
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(visible_rows);
        for offset in scroll..(scroll + visible_rows).min(total) {
            let entry_idx = input.display_indices[offset];
            let entry = &input.entries[entry_idx];
            let is_selected = offset == input.selection;
            let style = if is_selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            let label = format!(
                "  {:>3}. {}  —  {}",
                entry_idx + 1,
                entry.title,
                entry.author
            );
            lines.push(Line::from(TuiSpan::styled(label, style)));
        }
        frame.render_widget(
            Paragraph::new(lines).block(TuiBlock::default().borders(Borders::NONE)),
            chunks[1],
        );
    }

    // Footer: search box (if active), warning, or default hint.
    render_library_footer(
        frame,
        chunks[2],
        FooterInput {
            mode: input.search_mode,
            query: input.search_query,
            matches: input.display_indices.len(),
            warning: input.warning,
            default_hint: " Enter open · ↑↓ navigate · / search · g grid · q quit ",
        },
    );
}

fn render_library_grid(frame: &mut Frame, area: Rect, input: LibraryRenderInput<'_>) {
    use crate::cover_cache::{COVER_THUMBNAIL_HEIGHT, COVER_THUMBNAIL_WIDTH, PLACEHOLDER};

    // Outer layout: title (1), grid body (Min CELL_HEIGHT), footer (1).
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(CELL_HEIGHT),
            Constraint::Length(1),
        ])
        .split(area);

    // Title bar shows total library size, NOT the filtered match count
    // (which the footer surfaces as "N matches"). This deliberate split
    // gives the user both numbers at a glance.
    let title = TuiSpan::styled(
        format!(" cleader library — {} book(s) ", input.entries.len()),
        Style::default().add_modifier(Modifier::BOLD),
    );
    frame.render_widget(Paragraph::new(Line::from(title)), outer[0]);

    let grid_area = outer[1];

    if input.display_indices.is_empty()
        && !matches!(input.search_mode, crate::search::SearchMode::Idle)
    {
        render_no_matches(frame, grid_area);
    } else {
        // Grid math, using display_indices length as total.
        let total = input.display_indices.len();
        let cols = (grid_area.width / CELL_WIDTH).max(1) as usize;
        let rows = (grid_area.height / CELL_HEIGHT).max(1) as usize;
        let cells_per_screen = cols * rows;

        let visible = visible_grid_range(grid_area.width, grid_area.height, total, input.selection)
            .unwrap_or(0..0);
        let first_idx = visible.start;
        let last_idx = visible.end;

        // Build a vertical stack of horizontal cell rows.
        let mut cell_rects: Vec<Rect> = Vec::with_capacity(cells_per_screen);
        let row_constraints: Vec<Constraint> = (0..rows)
            .map(|_| Constraint::Length(CELL_HEIGHT))
            .collect();
        let row_rects = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(grid_area);
        for row_rect in row_rects.iter() {
            let col_constraints: Vec<Constraint> = (0..cols)
                .map(|_| Constraint::Length(CELL_WIDTH))
                .collect();
            let col_rects = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(col_constraints)
                .split(*row_rect);
            for r in col_rects.iter() {
                cell_rects.push(*r);
            }
        }

        for (cell_offset, display_pos) in (first_idx..last_idx).enumerate() {
            let Some(cell_rect) = cell_rects.get(cell_offset) else { break; };
            let entry_idx = input.display_indices[display_pos];
            let entry = &input.entries[entry_idx];
            let is_selected = display_pos == input.selection;

            let border_style = if is_selected {
                Style::default().fg(ratatui::style::Color::Yellow)
            } else {
                Style::default()
            };
            let block = TuiBlock::default()
                .borders(Borders::ALL)
                .border_style(border_style);

            let inner = block.inner(*cell_rect);
            frame.render_widget(block, *cell_rect);

            let cell_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(COVER_THUMBNAIL_HEIGHT),
                    Constraint::Min(1),
                ])
                .split(inner);

            let cover_lines: Vec<Line<'static>> = {
                let cached = input
                    .book_ids
                    .get(entry_idx)
                    .and_then(|opt| opt.as_ref())
                    .and_then(|id| input.cover_cache.and_then(|c| c.get(id)));
                match cached {
                    Some(lines) => lines
                        .iter()
                        .take(COVER_THUMBNAIL_HEIGHT as usize)
                        .map(|l| Line::from(l.clone()))
                        .collect(),
                    None => PLACEHOLDER
                        .iter()
                        .map(|s| {
                            Line::from(TuiSpan::styled(
                                s.to_string(),
                                Style::default().add_modifier(Modifier::DIM),
                            ))
                        })
                        .collect(),
                }
            };
            frame.render_widget(Paragraph::new(cover_lines), cell_chunks[0]);

            let title_style = if is_selected {
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };
            let title_truncated = truncate_to_width(&entry.title, COVER_THUMBNAIL_WIDTH as usize);
            let author_truncated = truncate_to_width(&entry.author, COVER_THUMBNAIL_WIDTH as usize);
            let title_lines = vec![
                Line::from(TuiSpan::styled(title_truncated, title_style)),
                Line::from(TuiSpan::styled(
                    author_truncated,
                    Style::default().add_modifier(Modifier::DIM),
                )),
            ];
            frame.render_widget(Paragraph::new(title_lines), cell_chunks[1]);
        }
    }

    render_library_footer(
        frame,
        outer[2],
        FooterInput {
            mode: input.search_mode,
            query: input.search_query,
            matches: input.display_indices.len(),
            warning: input.warning,
            default_hint: " Enter open · ↑↓ navigate · / search · g list · q quit ",
        },
    );
}

/// Inputs for the library footer renderer. Used by both list and grid
/// modes; the only difference between them is the default-hint string.
struct FooterInput<'a> {
    mode: crate::search::SearchMode,
    query: Option<&'a str>,
    matches: usize,
    warning: Option<&'a str>,
    default_hint: &'a str,
}

/// Render the library footer. Priority: search box (if mode != Idle),
/// warning banner (if set), default hint.
fn render_library_footer(frame: &mut Frame, area: Rect, input: FooterInput<'_>) {
    use crate::search::SearchMode;
    if !matches!(input.mode, SearchMode::Idle) {
        let query = input.query.unwrap_or("");
        let cursor = if matches!(input.mode, SearchMode::Editing) { "_" } else { "" };
        let hint = match input.mode {
            SearchMode::Editing => "Enter apply · Esc cancel",
            SearchMode::Applied => "/ refine · Esc clear",
            SearchMode::Idle => "",
        };
        let left = format!(" / {query}{cursor}");
        let right = format!("{} matches · {hint} ", input.matches);
        let total = left.chars().count() + right.chars().count();
        let padding = (area.width as usize).saturating_sub(total);
        let middle = " ".repeat(padding);
        let line = Line::from(vec![
            TuiSpan::styled(left, Style::default().add_modifier(Modifier::BOLD)),
            TuiSpan::raw(middle),
            TuiSpan::styled(right, Style::default().add_modifier(Modifier::DIM)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let footer_text = match input.warning {
        Some(msg) => format!(" ! {msg} ! "),
        None => input.default_hint.to_string(),
    };
    let style = if input.warning.is_some() {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(Line::from(TuiSpan::styled(footer_text, style))),
        area,
    );
}

/// Render a centered "no matches" message in the given area. Used by
/// both list and grid renderers when a filter returns zero entries.
fn render_no_matches(frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        // Degenerate layout — no room to paint anything safely.
        return;
    }
    let msg = "no matches";
    let line = Line::from(TuiSpan::styled(
        msg,
        Style::default().add_modifier(Modifier::DIM),
    ));
    let para = Paragraph::new(line).alignment(Alignment::Center);
    let center_y = area.y + area.height / 2;
    let centered = Rect {
        x: area.x,
        y: center_y,
        width: area.width,
        height: 1,
    };
    frame.render_widget(para, centered);
}

/// Truncate a string to fit within `max_cols` display columns. Uses
/// unicode-width; appends "…" if anything was cut.
pub(crate) fn truncate_to_width(s: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    // Collect (char, width) pairs so we can back-fill the ellipsis.
    let chars_widths: Vec<(char, usize)> = s
        .chars()
        .map(|ch| (ch, UnicodeWidthChar::width(ch).unwrap_or(0)))
        .collect();

    let total_width: usize = chars_widths.iter().map(|(_, w)| w).sum();
    if total_width <= max_cols {
        // Fits as-is.
        return s.to_string();
    }

    // Needs truncation. Reserve 1 column for the '…'.
    let budget = max_cols.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0usize;
    for (ch, cw) in &chars_widths {
        if used + cw > budget {
            break;
        }
        out.push(*ch);
        used += cw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cover_cache::CoverCache;
    use crate::epub::BookId;
    use crate::library::LibraryEntry;
    use crate::prefs::ViewMode;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn lib_entry(title: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(format!("/tmp/{title}.epub")),
            title: title.to_string(),
            author: "Anon".to_string(),
        }
    }

    #[test]
    fn library_render_does_not_panic_on_narrow_terminal() {
        let backend = TestBackend::new(10, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            let area = frame.area();
            render_library(frame, area, LibraryRenderInput {
                entries: &[
                    LibraryEntry {
                        path: PathBuf::from("/a.epub"),
                        title: "A".into(),
                        author: "X".into(),
                    },
                ],
                selection: 0,
                view_mode: ViewMode::List,
                cover_cache: None,
                book_ids: &[None],
                warning: None,
                display_indices: &[0],
                search_query: None,
                search_mode: crate::search::SearchMode::Idle,
            });
        }).unwrap();
    }

    #[test]
    fn render_library_grid_does_not_panic_on_tiny_terminal() {
        // 4x4 is well below CELL_WIDTH x CELL_HEIGHT (24x16) — dispatcher
        // falls back to list rendering. Must not panic.
        let backend = TestBackend::new(4, 4);
        let mut term = Terminal::new(backend).unwrap();
        let entries = vec![lib_entry("A"), lib_entry("B")];
        let display_indices: Vec<usize> = (0..entries.len()).collect();
        term.draw(|frame| {
            let area = frame.area();
            render_library(
                frame,
                area,
                LibraryRenderInput {
                    entries: &entries,
                    selection: 0,
                    view_mode: ViewMode::Grid,
                    cover_cache: None,
                    book_ids: &[None, None],
                    warning: None,
                    display_indices: &display_indices,
                    search_query: None,
                    search_mode: crate::search::SearchMode::Idle,
                },
            );
        })
        .unwrap();
    }

    #[test]
    fn render_library_grid_renders_on_80x40_without_panic() {
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        let entries: Vec<_> = (0..6).map(|i| lib_entry(&format!("Book{i}"))).collect();
        let book_ids: Vec<Option<BookId>> = (0..entries.len()).map(|_| None).collect();
        let display_indices: Vec<usize> = (0..entries.len()).collect();
        term.draw(|frame| {
            let area = frame.area();
            render_library(
                frame,
                area,
                LibraryRenderInput {
                    entries: &entries,
                    selection: 0,
                    view_mode: ViewMode::Grid,
                    cover_cache: None,
                    book_ids: &book_ids,
                    warning: None,
                    display_indices: &display_indices,
                    search_query: None,
                    search_mode: crate::search::SearchMode::Idle,
                },
            );
        })
        .unwrap();
    }

    #[test]
    fn render_library_grid_uses_cover_cache_when_available() {
        // Pre-populate a cache with a known cover for one entry, render,
        // and verify the cover lines appear in the rendered buffer.
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        let id = BookId::from_bytes(b"book-a");
        // Write a deliberately-recognizable cover row to disk so the
        // cache picks it up via enqueue (synchronous disk-hit path).
        let lines: Vec<String> = (0..17)
            .map(|i| format!("ROW{i:02}{}", " ".repeat(17)))
            .collect();
        crate::cover_cache::write_cached(dir.path(), &id, &lines).unwrap();
        cache.enqueue(id.clone(), PathBuf::from("/tmp/book-a.epub"));

        let entries = vec![lib_entry("Book A")];
        let book_ids = vec![Some(id.clone())];
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        let display_indices: Vec<usize> = (0..entries.len()).collect();
        term.draw(|frame| {
            let area = frame.area();
            render_library(
                frame,
                area,
                LibraryRenderInput {
                    entries: &entries,
                    selection: 0,
                    view_mode: ViewMode::Grid,
                    cover_cache: Some(&cache),
                    book_ids: &book_ids,
                    warning: None,
                    display_indices: &display_indices,
                    search_query: None,
                    search_mode: crate::search::SearchMode::Idle,
                },
            );
        })
        .unwrap();

        // Buffer should contain "ROW00" somewhere — the first row of our
        // injected cover.
        let buffer_text: String = term.backend().buffer().content.iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            buffer_text.contains("ROW00"),
            "rendered buffer should contain injected cover row, got:\n{buffer_text}"
        );
    }

    #[test]
    fn library_footer_shows_warning_in_both_modes() {
        // Verifies the `warning: Option<&str>` field surfaces in the
        // footer for both render variants.
        let entries = vec![lib_entry("A")];
        let book_ids = vec![None];
        let display_indices: Vec<usize> = (0..entries.len()).collect();

        for view_mode in [ViewMode::List, ViewMode::Grid] {
            let backend = TestBackend::new(80, 40);
            let mut term = Terminal::new(backend).unwrap();
            term.draw(|frame| {
                let area = frame.area();
                render_library(
                    frame,
                    area,
                    LibraryRenderInput {
                        entries: &entries,
                        selection: 0,
                        view_mode,
                        cover_cache: None,
                        book_ids: &book_ids,
                        warning: Some("could not save prefs: oh no"),
                        display_indices: &display_indices,
                        search_query: None,
                        search_mode: crate::search::SearchMode::Idle,
                    },
                );
            })
            .unwrap();

            let buffer_text: String = term.backend().buffer().content.iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(
                buffer_text.contains("could not save prefs: oh no"),
                "{view_mode:?} footer should contain warning, got:\n{buffer_text}"
            );
        }
    }

    #[test]
    fn truncate_to_width_appends_ellipsis_when_cut() {
        assert_eq!(truncate_to_width("hello world", 5), "hell…");
        assert_eq!(truncate_to_width("hi", 5), "hi");
        assert_eq!(truncate_to_width("", 5), "");
    }

    #[test]
    fn search_footer_shows_query_and_cursor_in_editing() {
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        let entries: Vec<_> = (0..3).map(|i| lib_entry(&format!("Book{i}"))).collect();
        let book_ids: Vec<Option<BookId>> = (0..entries.len()).map(|_| None).collect();
        let display_indices: Vec<usize> = vec![0, 1];  // pretend filter matches 2 of 3
        term.draw(|frame| {
            let area = frame.area();
            render_library(
                frame,
                area,
                LibraryRenderInput {
                    entries: &entries,
                    selection: 0,
                    view_mode: ViewMode::Grid,
                    cover_cache: None,
                    book_ids: &book_ids,
                    warning: None,
                    display_indices: &display_indices,
                    search_query: Some("book"),
                    search_mode: crate::search::SearchMode::Editing,
                },
            );
        })
        .unwrap();
        let buf: String = term.backend().buffer().content.iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf.contains("/ book_"), "footer should show query with cursor underscore");
        assert!(buf.contains("2 matches"), "footer should show match count");
        assert!(buf.contains("Enter apply"), "footer should show Editing hint");
    }

    #[test]
    fn search_footer_shows_applied_hint() {
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        let entries: Vec<_> = (0..3).map(|i| lib_entry(&format!("Book{i}"))).collect();
        let book_ids: Vec<Option<BookId>> = (0..entries.len()).map(|_| None).collect();
        let display_indices: Vec<usize> = vec![0, 1];
        term.draw(|frame| {
            let area = frame.area();
            render_library(
                frame,
                area,
                LibraryRenderInput {
                    entries: &entries,
                    selection: 0,
                    view_mode: ViewMode::List,
                    cover_cache: None,
                    book_ids: &book_ids,
                    warning: None,
                    display_indices: &display_indices,
                    search_query: Some("book"),
                    search_mode: crate::search::SearchMode::Applied,
                },
            );
        })
        .unwrap();
        let buf: String = term.backend().buffer().content.iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf.contains("/ refine"), "Applied footer should show '/ refine'");
        assert!(buf.contains("Esc clear"), "Applied footer should show 'Esc clear'");
        assert!(buf.contains("2 matches"), "Applied footer should show match count");
    }

    #[test]
    fn no_matches_message_renders_when_filter_empty() {
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        let entries: Vec<_> = (0..3).map(|i| lib_entry(&format!("Book{i}"))).collect();
        let book_ids: Vec<Option<BookId>> = (0..entries.len()).map(|_| None).collect();
        let display_indices: Vec<usize> = vec![];  // 0 matches
        term.draw(|frame| {
            let area = frame.area();
            render_library(
                frame,
                area,
                LibraryRenderInput {
                    entries: &entries,
                    selection: 0,
                    view_mode: ViewMode::Grid,
                    cover_cache: None,
                    book_ids: &book_ids,
                    warning: None,
                    display_indices: &display_indices,
                    search_query: Some("xyz"),
                    search_mode: crate::search::SearchMode::Editing,
                },
            );
        })
        .unwrap();
        let buf: String = term.backend().buffer().content.iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf.contains("no matches"), "should show 'no matches' message");
        assert!(buf.contains("0 matches"), "footer should show '0 matches'");
    }

    #[test]
    fn visible_grid_range_returns_none_when_too_small() {
        assert!(visible_grid_range(10, 10, 100, 0).is_none()); // width too small
        assert!(visible_grid_range(80, 10, 100, 0).is_none()); // height too small
    }

    #[test]
    fn visible_grid_range_page_snaps_selection() {
        // 80x42 grid area: cols=3, rows=2 → 6 cells/page
        // selection=0 → page 0 → 0..6
        let r = visible_grid_range(80, 42, 100, 0).unwrap();
        assert_eq!(r, 0..6);
        // selection=5 → still page 0 → 0..6
        let r = visible_grid_range(80, 42, 100, 5).unwrap();
        assert_eq!(r, 0..6);
        // selection=6 → page 1 → 6..12
        let r = visible_grid_range(80, 42, 100, 6).unwrap();
        assert_eq!(r, 6..12);
        // selection=99 → page 16 → 96..100 (clamped to total)
        let r = visible_grid_range(80, 42, 100, 99).unwrap();
        assert_eq!(r, 96..100);
    }
}
