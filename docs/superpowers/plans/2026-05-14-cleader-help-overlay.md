# cleader v0.4.8 Implementation Plan — Library Help Overlay (Shared)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** `?` opens a help overlay from BOTH library and reader modes, sharing one canonical content table that includes a short cleader description, library + reading + universal shortcuts, and creator info. Library footer hints add `· ? help` for discoverability.

**Architecture:** A new pure-data `src/help.rs` module owns the `HELP_LINES` table and `render_help_overlay` function. reader.rs and library renderer both call into it. LibraryApp gets a `show_help` field with the same modal-mutual-exclusion behavior as reader (TOC + help). One task, three commit phases (extract → wire library → polish).

**Tech Stack:** Rust 2024, existing ratatui infrastructure.

---

## File Structure

- **Create:** `src/help.rs` — `HELP_LINES` constant + `pub fn render_help_overlay(frame, area)`
- **Modify:** `src/lib.rs` — add `pub mod help;`
- **Modify:** `src/reader.rs` — delete local `HELP_LINES`/`HELP_LABEL_WIDTH`/`render_help_overlay`; call `crate::help::render_help_overlay` instead
- **Modify:** `src/library_app.rs` — new `show_help: bool`, accessor, ToggleHelp/Quit/OpenSearch/ToggleViewMode handler updates
- **Modify:** `src/render_library.rs` — new `LibraryRenderInput.show_help`, overlay rendering, footer hint updates
- **Modify:** `src/main.rs` — snapshot `show_help` for the renderer
- **Modify:** `tests/integration.rs` — add `show_help: false` to existing LibraryRenderInput literal

---

## Task 1: Create the shared `help` module

**Files:**
- Create: `src/help.rs`
- Modify: `src/lib.rs`
- Modify: `src/reader.rs` (delete the moved items + update call site)

- [ ] **Step 1: Write `src/help.rs`**

```rust
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
            // Vertical spacer.
            lines.push(Line::default());
            continue;
        }
        if keys.is_empty() {
            // Section header or prose line — render as styled text
            // without the key column.
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
            // Two-column label/keys row.
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

    // Modal width: longest line in columns + 4 for borders + breathing room.
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

    // Center within the area.
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
```

- [ ] **Step 2: Wire `src/lib.rs`**

Add `pub mod help;` alphabetically. Final:

```rust
pub mod app;
pub mod ascii_art;
pub mod cover_cache;
pub mod epub;
pub mod error;
pub mod help;
pub mod input;
pub mod library;
pub mod library_app;
pub mod persistence;
pub mod prefs;
pub mod reader;
pub mod render_library;
pub mod search;
```

- [ ] **Step 3: Delete the local help items from `src/reader.rs`**

Delete:
- `const HELP_LINES: &[(&str, &str)] = &[ ... ];` (the table — currently around lines 372–382)
- `const HELP_LABEL_WIDTH: usize = 22;` (around line 387)
- `fn render_help_overlay(frame: &mut Frame, area: Rect) { ... }` (around lines 447–502)

Update the single call site in reader.rs (around line 435) from:

```rust
        render_help_overlay(frame, area);
```

to:

```rust
        crate::help::render_help_overlay(frame, area);
```

The reader's existing help-overlay tests (e.g. `render_help_overlay_renders_on_narrow_terminal` if it exists in reader.rs tests) — if they call `render_help_overlay` directly, they'll fail to compile after the move. Either:
- Move those reader tests to `help.rs` (already have 3 tests covering the function there)
- Delete them as redundant with the new help.rs tests

Verify by reading the reader.rs test module. The likely candidates are `render_help_overlay_*` test names. Delete or migrate as needed; the new help.rs tests cover the same ground.

- [ ] **Step 4: Run tests + clippy**

```
cargo test --quiet 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo doc --no-deps 2>&1 | grep -i warning
```

Expected: test count drops by however many reader-side help-overlay tests were deleted, then rises by 3 from the new help.rs tests. Net likely +1 to +3. Clippy + doc clean.

- [ ] **Step 5: Commit**

```bash
git add src/help.rs src/lib.rs src/reader.rs
git commit -m "feat(help): extract shared help overlay module

New src/help.rs owns the HELP_LINES table and render_help_overlay so
both the reader and the library can use a single source of truth for
keyboard shortcuts. Content updated to include a short cleader
description, three sections (Library, Reading, Anywhere), and
creator info (Aqiul / aqiul.c@gmail.com).

reader.rs deletes its local copies and calls crate::help::
render_help_overlay instead. Library wiring follows in Task 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Wire help overlay into LibraryApp + renderer

**Files:**
- Modify: `src/library_app.rs`
- Modify: `src/render_library.rs`
- Modify: `src/main.rs`
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add `show_help` field to LibraryApp**

In `src/library_app.rs`, in the `LibraryApp` struct, add:

```rust
    /// True when the help overlay is showing. Set by `Action::ToggleHelp`,
    /// cleared by the same action or by `Action::Quit` (so Esc dismisses
    /// the overlay rather than quitting the library).
    show_help: bool,
```

In `new_with`, add `show_help: false,` to the Self struct literal.

- [ ] **Step 2: Add accessor**

Add near the other accessors:

```rust
    pub fn show_help(&self) -> bool {
        self.show_help
    }
```

- [ ] **Step 3: Replace `Action::ToggleHelp` no-op with real handler + update other overlays**

In `handle`, find the existing no-op arm for ToggleHelp (currently lumped with ChapterNext/ChapterPrev/ToggleToc). Remove ToggleHelp from that arm and add a dedicated handler:

```rust
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            // Reader-only actions are no-ops in library mode.
            Action::ChapterNext
            | Action::ChapterPrev
            | Action::ToggleToc => {}
```

Also update `Action::OpenSearch` and `Action::ToggleViewMode` arms to dismiss help first (mutual exclusion — like the TOC pattern from v0.3):

```rust
            Action::ToggleViewMode => {
                self.show_help = false;
                self.toggle_view_mode();
            }
            Action::OpenSearch => {
                self.show_help = false;
                self.open_search();
            }
```

Also update `Action::Quit` arm — if help is showing, dismiss instead of quit (matches reader pattern). Find the existing Quit arm (it currently branches on `SearchMode::Applied`) and add the help check:

```rust
            Action::Quit => {
                if self.show_help {
                    self.show_help = false;
                } else if matches!(self.search.mode, SearchMode::Applied) {
                    self.clear_search();
                } else {
                    self.should_quit = true;
                }
            }
```

- [ ] **Step 4: Add tests**

Append to `#[cfg(test)] mod tests` in library_app.rs:

```rust
    #[test]
    fn toggle_help_flips_show_help() {
        let mut app = LibraryApp::new_with(
            vec![entry("A")],
            (80, 24),
            None,
            None,
        );
        assert!(!app.show_help());
        app.handle(Action::ToggleHelp);
        assert!(app.show_help());
        app.handle(Action::ToggleHelp);
        assert!(!app.show_help());
    }

    #[test]
    fn esc_dismisses_help_without_quitting() {
        let mut app = LibraryApp::new_with(
            vec![entry("A")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::ToggleHelp);
        assert!(app.show_help());
        app.handle(Action::Quit);
        assert!(!app.show_help(), "Esc should dismiss help");
        assert!(!app.should_quit(), "Esc with help up should not quit");
    }

    #[test]
    fn open_search_dismisses_help() {
        let mut app = LibraryApp::new_with(
            vec![entry("A")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::ToggleHelp);
        assert!(app.show_help());
        app.handle(Action::OpenSearch);
        assert!(!app.show_help(), "opening search should dismiss help");
        assert_eq!(app.search_mode(), SearchMode::Editing);
    }

    #[test]
    fn toggle_view_mode_dismisses_help() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(
            vec![entry("A")],
            (80, 24),
            Some(fresh_prefs(dir.path())),
            None,
        );
        app.handle(Action::ToggleHelp);
        assert!(app.show_help());
        app.handle(Action::ToggleViewMode);
        assert!(!app.show_help(), "toggling view mode should dismiss help");
    }
```

- [ ] **Step 5: Extend `LibraryRenderInput` + render overlay**

In `src/render_library.rs`, add a new field to `LibraryRenderInput`:

```rust
    /// True when the help overlay should be drawn on top of everything.
    pub show_help: bool,
```

At the end of `render_library` (the dispatcher), after the list/grid dispatch, add the overlay:

```rust
pub fn render_library(frame: &mut Frame, area: Rect, input: LibraryRenderInput<'_>) {
    use crate::prefs::ViewMode;
    let force_list = area.width < CELL_WIDTH || area.height < (CELL_HEIGHT + 2);
    let show_help = input.show_help;
    match (input.view_mode, force_list) {
        (ViewMode::Grid, false) => render_library_grid(frame, area, input),
        _ => render_library_list(frame, area, input),
    }
    if show_help {
        crate::help::render_help_overlay(frame, area);
    }
}
```

(Capture `show_help` before passing `input` to the inner renderer since `input` is moved.)

- [ ] **Step 6: Update footer hints with `· ? help`**

In `render_library_list`, update the default hint:

```rust
            default_hint: " Enter open · ↑↓ navigate · / search · g grid · ? help · q quit ",
```

In `render_library_grid`, update the default hint:

```rust
            default_hint: " Enter open · ↑↓ navigate · / search · g list · ? help · q quit ",
```

If these footer strings are too long for a typical 80-col terminal (they will be — the existing strings are already close to 80 chars), the render_library_footer function already pads/clips gracefully via `saturating_sub`. Verify by running render tests.

- [ ] **Step 7: Update all existing test callsites + add new ones**

Every existing test in `src/render_library.rs` that constructs `LibraryRenderInput { ... }` needs `show_help: false,` added. There are now 12 existing literals (after Task 5 of v0.4.5 + Task 3 of v0.4.7 added two; the new field is the 11th).

Same for `tests/integration.rs`'s `library_grid_renders_without_panic_when_directory_has_epubs`: add `show_help: false,` to the LibraryRenderInput literal.

Add one new test in `src/render_library.rs`:

```rust
    #[test]
    fn help_overlay_renders_in_library_view() {
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        let entries = vec![lib_entry("Book")];
        let book_ids = vec![None];
        let display_indices = vec![0];
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
                    marquee_offset: 0,
                    show_help: true,
                },
            );
        })
        .unwrap();
        let buf: String = term.backend().buffer().content.iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf.contains("Key bindings"), "help overlay title should appear");
        assert!(buf.contains("Aqiul"), "creator info should appear");
    }
```

- [ ] **Step 8: Update `src/main.rs` library_event_loop**

Find the snapshot block before `terminal.draw`. Add:

```rust
            let show_help = app.show_help();
```

Pass `show_help` into the `LibraryRenderInput { ... }` call.

Also update the idle-redraw logic — currently `needs_redraw` is set by input events. Opening/closing help is an input event so this should already work; verify by reading the code.

- [ ] **Step 9: Run tests + clippy + doc + release**

```
cargo build 2>&1 | tail -5
cargo test --quiet 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo doc --no-deps 2>&1 | grep -i warning
cargo build --release 2>&1 | tail -5
```

Expected: 4 new library_app tests + 1 new renderer test + 3 from help.rs (Task 1) − any reader help test deleted = roughly 236 + 8 = ~244 unit + 11 integration. Clippy + doc clean.

- [ ] **Step 10: Commit**

```bash
git add src/library_app.rs src/render_library.rs src/main.rs tests/integration.rs
git commit -m "feat(library): help overlay + ? hint in footer

LibraryApp gains a show_help bool; Action::ToggleHelp now flips it,
Action::Quit dismisses help instead of quitting when it's up, and
Action::OpenSearch + Action::ToggleViewMode dismiss help first
(mutual exclusion, same pattern as TOC + help in the reader).

LibraryRenderInput gains show_help. render_library dispatches to
list/grid as before, then layers crate::help::render_help_overlay on
top when show_help is true.

Footer hints add '· ? help' in both list and grid modes so the
binding is discoverable.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review Checklist (after both tasks)

1. `cargo test --quiet 2>&1 | grep 'test result'` — confirm green
2. `cargo clippy --all-targets -- -D warnings` — clean
3. `cargo doc --no-deps 2>&1 | grep -i warning` — no doc warnings
4. `cargo build --release` — succeeds
5. Manual smoke test: `cargo run --release -- some/directory/`:
   - In library: press `?` → overlay appears with description, sections, creator info
   - Press `?` again → overlay dismisses
   - Press `?`, then `Esc` → overlay dismisses (doesn't quit)
   - Press `?`, then `/` → overlay dismisses + search opens
   - Press `?`, then `g` → overlay dismisses + view toggles
   - Enter a book, press `?` → reader's help overlay still works
   - The library footer mentions `· ? help`

## Spec Coverage Map

| Item | Covered by |
|---|---|
| `src/help.rs` shared module | Task 1 |
| Description + creator info in HELP_LINES | Task 1 |
| Library / Reading / Anywhere sections | Task 1 |
| reader.rs uses shared help | Task 1 |
| LibraryApp.show_help + accessor | Task 2 |
| Action::ToggleHelp in library | Task 2 |
| Esc dismisses help instead of quit | Task 2 |
| OpenSearch / ToggleViewMode dismiss help first | Task 2 |
| LibraryRenderInput.show_help + overlay rendering | Task 2 |
| Library footer adds `· ? help` | Task 2 |
| Integration test field update | Task 2 |
