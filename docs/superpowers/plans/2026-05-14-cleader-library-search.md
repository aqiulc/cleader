# cleader v0.4.5 Implementation Plan — Library Search

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Press `/` from the library view to open a live-filtering search box. Type to narrow the visible entries by case-insensitive substring match on title or author. Works in both grid and list modes. Esc clears and restores.

**Architecture:** A new pure-data module `src/search.rs` holds `SearchMode`, `SearchState`, and the `filter_indices` free function. `LibraryApp` gains a `SearchState` field plus a pre-lowercased haystack array; existing navigation handlers route through a new `display_indices()` accessor so filter narrowing transparently shrinks the navigation set. A new `Action::OpenSearch` (bound to `/`) opens the search box; `library_event_loop` in `main.rs` adds a top-of-loop branch that bypasses `translate()` while in Editing state so every printable key reaches `handle_search_input(KeyEvent)`. The renderer takes `display_indices` as input and surfaces a search box in the footer.

**Tech Stack:** Rust 2024, ratatui 0.28 (TUI), crossterm 0.28 (input), existing `LibraryApp` / `CoverCache` / `PrefsStore` infrastructure from v0.4.4.

---

## File Structure

**New files:**
- `src/search.rs` — `SearchMode`, `SearchState`, `filter_indices` + unit tests

**Modified files:**
- `src/lib.rs` — `pub mod search;`
- `src/input.rs` — add `Action::OpenSearch`, bind `/`
- `src/library_app.rs` — own `SearchState`, build `entries_lowercased` + `all_indices`, add accessors (`is_searching`, `has_filter`, `search_query`, `search_mode`, `display_indices`, `open_search`, `handle_search_input`), route existing nav arms through `display_indices()`
- `src/app.rs` — add no-op arm for `Action::OpenSearch` (reader-mode no-op, same pattern as other library-only actions)
- `src/reader.rs` — extend `LibraryRenderInput` with `display_indices` / `search_query` / `search_mode`, update `render_library_list` and `render_library_grid` to iterate via `display_indices`, render search box in footer when search mode != Idle, render "no matches" centered when filter result is empty, update existing tests' call sites
- `src/main.rs` — `library_event_loop` branches on `app.is_searching()`; cover-request math maps through `display_indices()`
- `tests/integration.rs` — end-to-end library search smoke test

---

## Task 1: Create `search` module

**Files:**
- Create: `src/search.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write `src/search.rs` with both implementation and tests**

```rust
//! Library search state and substring filter.
//!
//! Pure data + free function. The filter logic is decoupled from
//! LibraryApp so it can be tested in isolation. `SearchState` is the
//! container LibraryApp embeds; `filter_indices` is the work function
//! called on every keystroke.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// No search is active; the full entries list is shown.
    #[default]
    Idle,
    /// Search box is open and accepting keystrokes. The query updates
    /// live and the filter narrows immediately.
    Editing,
    /// Search box is closed but the filter is still in effect. Arrow
    /// keys navigate the filtered set; `/` re-opens the box for refine,
    /// Esc clears everything and returns to Idle.
    Applied,
}

#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub mode: SearchMode,
    pub query: String,
    /// Indices into the owning LibraryApp's `entries`. Only populated
    /// in Editing or Applied; LibraryApp uses `all_indices` instead
    /// when this is empty AND mode is Idle (mode disambiguates "empty
    /// because no filter" from "empty because zero matches").
    pub filtered: Vec<usize>,
}

/// Filter `haystacks` against `query`. `query` is expected to be
/// already-lowercased by the caller (saves repeated allocations).
/// Returns indices in source order. Empty query returns ALL indices
/// (so the renderer never has to special-case "filter present but
/// empty query — show what?").
pub fn filter_indices(haystacks: &[String], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..haystacks.len()).collect();
    }
    haystacks
        .iter()
        .enumerate()
        .filter_map(|(i, h)| if h.contains(query) { Some(i) } else { None })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Vec<String> {
        // Pre-lowercased title\nauthor strings, simulating the parallel
        // array that LibraryApp builds at construction time.
        vec![
            "firefly: generations\ntim lebbon".to_string(),
            "firefly: the magnificent nine\nj.m. straczynski".to_string(),
            "threshold\nclive cussler".to_string(),
            "tomorrow, and tomorrow, and tomorrow\ngabrielle zevin".to_string(),
        ]
    }

    #[test]
    fn empty_query_returns_all_indices() {
        let c = corpus();
        assert_eq!(filter_indices(&c, ""), vec![0, 1, 2, 3]);
    }

    #[test]
    fn substring_match_lowercase() {
        let c = corpus();
        assert_eq!(filter_indices(&c, "firefly"), vec![0, 1]);
    }

    #[test]
    fn no_match_returns_empty() {
        let c = corpus();
        assert!(filter_indices(&c, "zzzzzz").is_empty());
    }

    #[test]
    fn author_match_works() {
        let c = corpus();
        assert_eq!(filter_indices(&c, "lebbon"), vec![0]);
    }

    #[test]
    fn mid_word_substring_match() {
        let c = corpus();
        // "morrow" appears mid-word in "tomorrow"
        assert_eq!(filter_indices(&c, "morrow"), vec![3]);
    }

    #[test]
    fn multi_match_preserves_source_order() {
        let c = corpus();
        let r = filter_indices(&c, "fire");
        assert_eq!(r, vec![0, 1]);
    }

    #[test]
    fn title_match_works() {
        let c = corpus();
        assert_eq!(filter_indices(&c, "threshold"), vec![2]);
    }

    #[test]
    fn default_search_state_is_idle_empty() {
        let s = SearchState::default();
        assert_eq!(s.mode, SearchMode::Idle);
        assert!(s.query.is_empty());
        assert!(s.filtered.is_empty());
    }
}
```

- [ ] **Step 2: Wire the module in `src/lib.rs`**

Final `src/lib.rs`:

```rust
pub mod app;
pub mod ascii_art;
pub mod cover_cache;
pub mod epub;
pub mod error;
pub mod input;
pub mod library;
pub mod library_app;
pub mod persistence;
pub mod prefs;
pub mod reader;
pub mod search;
```

- [ ] **Step 3: Verify tests pass and clippy is clean**

Run:
```
cargo test --quiet search:: 2>&1 | tail -10
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: 8 search tests passing; clippy clean.

- [ ] **Step 4: Commit**

```bash
git add src/search.rs src/lib.rs
git commit -m "feat(search): add SearchState + filter_indices module

Pure data + free function. SearchMode tri-state (Idle/Editing/
Applied) plus a SearchState container with mode/query/filtered.
filter_indices is the work function called on every keystroke;
caller pre-lowercases both haystacks and query for speed. Empty
query returns all indices (so the renderer never special-cases).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Add `Action::OpenSearch` and bind `/`

**Files:**
- Modify: `src/input.rs`
- Modify: `src/library_app.rs` (add no-op arm for now; real handler in Task 3)
- Modify: `src/app.rs` (add no-op arms in both reader match blocks)

- [ ] **Step 1: Write the failing test**

Append to `#[cfg(test)] mod tests` in `src/input.rs`:

```rust
    #[test]
    fn slash_opens_search() {
        assert_eq!(
            translate(key(KeyCode::Char('/'))),
            Some(Action::OpenSearch)
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet input::tests::slash_opens_search 2>&1 | tail -10`
Expected: FAIL with `no variant or associated item named OpenSearch found for enum Action`.

- [ ] **Step 3: Add the variant**

In `src/input.rs`, in the `Action` enum, add `OpenSearch` between `ToggleViewMode` and `Confirm`:

```rust
    ToggleHelp,
    ToggleToc,
    ToggleViewMode,
    OpenSearch,
    Confirm,
```

- [ ] **Step 4: Bind the key**

In `src/input.rs`, in `translate_key`, immediately after the `(Char('g'), false, false)` arm, add:

```rust
        (Char('g'), false, false) => Some(Action::ToggleViewMode),
        (Char('/'), false, _) => Some(Action::OpenSearch),
        (Enter, _, _) => Some(Action::Confirm),
```

The `_` on the SHIFT modifier lets `/` come through whether or not the terminal reports Shift (US layouts produce `/` bare; some others may report it with SHIFT).

- [ ] **Step 5: Update LibraryApp's exhaustive match — temporary no-op arm**

In `src/library_app.rs`, find the existing reader-only no-op arm (the one covering `Action::ChapterNext | Action::ChapterPrev | Action::ToggleHelp | Action::ToggleToc`). Add a NEW arm above it for `OpenSearch` so the no-op intent is explicit (Task 3 replaces this with the real handler):

```rust
            Action::ToggleViewMode => {
                self.toggle_view_mode();
            }
            Action::OpenSearch => {
                // Temporary no-op; Task 3 of v0.4.5 replaces this with
                // the real handler that opens the search box.
            }
            // Reader-only actions are no-ops in library mode.
            Action::ChapterNext
            | Action::ChapterPrev
            | Action::ToggleHelp
            | Action::ToggleToc => {}
```

- [ ] **Step 6: Update reader-mode (app.rs) match arms**

In `src/app.rs`, there are two `match action` blocks that need to handle the new variant. Find them (one is inside the TOC-overlay-open branch around line 273; the other is in the main handle around line 319 where `ToggleViewMode` lands). Add `Action::OpenSearch` to the `ToggleViewMode` no-op arm in both blocks:

Block 1 (TOC-overlay branch):
```rust
                Action::ChapterNext | Action::ChapterPrev | Action::ToggleViewMode | Action::OpenSearch => {
                    // Chapter nav while TOC is up is ambiguous — the user
                    // probably wanted TOC selection nav. Treat as no-op.
                    // ToggleViewMode is always a no-op in reader mode (library-only action).
                    // OpenSearch is always a no-op in reader mode (library-only action).
                }
```

Block 2 (main handle, the `ToggleViewMode` arm):
```rust
                Action::ToggleViewMode | Action::OpenSearch => {
                    // No-op in reader mode; library_app handles these.
                }
```

(Both arms become OR-patterns covering both library-only actions.)

- [ ] **Step 7: Verify test passes + clippy clean**

Run:
```
cargo test --quiet 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: 204 unit + 10 integration (was 203 + 10, +1 input test). Clippy clean.

- [ ] **Step 8: Commit**

```bash
git add src/input.rs src/library_app.rs src/app.rs
git commit -m "feat(input): add Action::OpenSearch bound to /

Library mode will open a live-filter search box on /; reader mode
treats it as a no-op (library-only action). Real handler in v0.4.5
Task 3.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Wire SearchState into LibraryApp + display_indices + open_search

**Files:**
- Modify: `src/library_app.rs`

This task adds the search state container, the pre-lowercased haystack array, the pre-search-selection capture, the new accessors, the `open_search()` method, and updates existing navigation handlers to route through `display_indices()` so a filter transparently narrows the navigation set. Quite a bit happens here but it's all cohesive: every change is in support of "navigation sees a possibly-narrower index sequence."

- [ ] **Step 1: Update imports**

Replace the top-of-file imports in `src/library_app.rs`:

```rust
use crate::cover_cache::CoverCache;
use crate::epub::BookId;
use crate::input::Action;
use crate::library::LibraryEntry;
use crate::prefs::{PrefsStore, ViewMode};
use crate::search::{filter_indices, SearchMode, SearchState};
use std::path::PathBuf;
```

- [ ] **Step 2: Add new struct fields**

Replace the `LibraryApp` struct definition:

```rust
pub struct LibraryApp {
    entries: Vec<LibraryEntry>,
    /// Parallel to `entries`: `book_ids[i]` is the BookId for `entries[i]`,
    /// computed lazily on first call to `request_visible_covers(i)`.
    /// Length always equals `entries.len()`.
    book_ids: Vec<Option<BookId>>,
    /// Parallel to `entries`: pre-lowercased `"{title}\n{author}"` string
    /// for fast substring matching during search. Built once at
    /// construction; avoids re-lowercasing on every keystroke.
    entries_lowercased: Vec<String>,
    /// Precomputed `(0..entries.len()).collect()`. Returned by
    /// `display_indices()` when no search filter is active.
    all_indices: Vec<usize>,
    selection: usize,
    viewport_size: (u16, u16),
    should_quit: bool,
    selected_path: Option<PathBuf>,
    view_mode: ViewMode,
    cover_cache: Option<CoverCache>,
    prefs: Option<PrefsStore>,
    save_error: Option<String>,
    search: SearchState,
    /// Selection captured when search began. Restored on Esc clear.
    pre_search_selection: usize,
}
```

- [ ] **Step 3: Update constructors to populate new fields**

Replace `new_with` (the canonical constructor — `new` already delegates to it):

```rust
    /// Test/internal constructor: caller injects prefs and cache (or
    /// `None`s for a minimal smoke harness).
    #[doc(hidden)]
    pub fn new_with(
        entries: Vec<LibraryEntry>,
        viewport: (u16, u16),
        prefs: Option<PrefsStore>,
        cover_cache: Option<CoverCache>,
    ) -> Self {
        let view_mode = prefs
            .as_ref()
            .map(|p| p.view_mode())
            .unwrap_or_default();
        let book_ids = vec![None; entries.len()];
        let entries_lowercased: Vec<String> = entries
            .iter()
            .map(|e| format!("{}\n{}", e.title.to_lowercase(), e.author.to_lowercase()))
            .collect();
        let all_indices: Vec<usize> = (0..entries.len()).collect();
        Self {
            entries,
            book_ids,
            entries_lowercased,
            all_indices,
            selection: 0,
            viewport_size: viewport,
            should_quit: false,
            selected_path: None,
            view_mode,
            cover_cache,
            prefs,
            save_error: None,
            search: SearchState::default(),
            pre_search_selection: 0,
        }
    }
```

`new()` already delegates to `new_with()`, no change needed there.

- [ ] **Step 4: Add accessors and `open_search` method**

Add these methods inside `impl LibraryApp`, placed alongside the other accessors (after `book_id`, before `handle`):

```rust
    /// True when the search box is open (Editing state). Used by the
    /// event loop to route keystrokes into the search buffer instead
    /// of the normal translate-action path.
    pub fn is_searching(&self) -> bool {
        matches!(self.search.mode, SearchMode::Editing)
    }

    /// True when a filter is in effect (Editing OR Applied). Used by
    /// renderer to decide whether to show the search box in the footer.
    pub fn has_filter(&self) -> bool {
        !matches!(self.search.mode, SearchMode::Idle)
    }

    pub fn search_query(&self) -> &str {
        &self.search.query
    }

    pub fn search_mode(&self) -> SearchMode {
        self.search.mode
    }

    /// Returns the indices into `entries` that should currently be
    /// shown. Either `all_indices` (no filter) or `search.filtered`
    /// (search active). Renderer iterates this; navigation moves
    /// `selection` within its bounds.
    pub fn display_indices(&self) -> &[usize] {
        if self.has_filter() {
            &self.search.filtered
        } else {
            &self.all_indices
        }
    }

    /// Open the search box. Captures the current `selection` so Esc
    /// can restore it; transitions to Editing mode. If already in
    /// Applied (filter set but box closed), this re-opens the box
    /// over the existing query for refinement.
    pub fn open_search(&mut self) {
        if matches!(self.search.mode, SearchMode::Idle) {
            self.pre_search_selection = self.selection;
            self.search.query.clear();
            self.refilter();
            self.selection = 0;
        }
        self.search.mode = SearchMode::Editing;
    }

    /// Recompute `search.filtered` from `search.query`. Called after
    /// every query mutation.
    fn refilter(&mut self) {
        let query_lc = self.search.query.to_lowercase();
        self.search.filtered = filter_indices(&self.entries_lowercased, &query_lc);
    }
```

- [ ] **Step 5: Replace the temporary `Action::OpenSearch` no-op with the real call**

In `handle`, find the `Action::OpenSearch` arm added in Task 2 and replace its body:

```rust
            Action::OpenSearch => {
                self.open_search();
            }
```

- [ ] **Step 6: Update existing nav and Confirm arms to use `display_indices()`**

In `handle`, the existing nav arms compare against `self.entries.len()`. Update each to use `self.display_indices().len()`. Replace the entire `match action { ... }` body for these arms:

```rust
            Action::LineUp => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let cols = self.grid_cols();
                    self.selection = self.selection.saturating_sub(cols);
                } else if self.selection > 0 {
                    self.selection -= 1;
                }
            }
            Action::LineDown => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let cols = self.grid_cols();
                    let max = self.display_indices().len().saturating_sub(1);
                    self.selection = (self.selection + cols).min(max);
                } else if self.selection + 1 < self.display_indices().len() {
                    self.selection += 1;
                }
            }
            Action::PagePrev => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    if self.selection > 0 {
                        self.selection -= 1;
                    }
                } else {
                    let step = self.lines_per_page().min(10);
                    self.selection = self.selection.saturating_sub(step);
                }
            }
            Action::PageNext => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let max = self.display_indices().len().saturating_sub(1);
                    if self.selection < max {
                        self.selection += 1;
                    }
                } else {
                    let step = self.lines_per_page().min(10);
                    let max = self.display_indices().len().saturating_sub(1);
                    self.selection = (self.selection + step).min(max);
                }
            }
            Action::Confirm => {
                let display = self.display_indices();
                if let Some(&entry_idx) = display.get(self.selection) {
                    if let Some(entry) = self.entries.get(entry_idx) {
                        self.selected_path = Some(entry.path.clone());
                        self.should_quit = true;
                    }
                }
            }
```

- [ ] **Step 7: Update `book_id` to resolve through display_indices**

The renderer (Task 5) and main.rs (Task 6) will call `book_id(display_idx)` to get the BookId for a cell. Update the method to do the indirection:

```rust
    /// Look up an already-computed BookId for a display position.
    /// `display_idx` indexes the currently visible sequence (which may
    /// be the full entries list or a filtered subset). Returns None
    /// if the index is out of range or if `request_visible_covers`
    /// hasn't computed the BookId for the underlying entry yet.
    #[doc(hidden)]
    pub fn book_id(&self, display_idx: usize) -> Option<&BookId> {
        let entry_idx = self.display_indices().get(display_idx)?;
        self.book_ids.get(*entry_idx).and_then(|opt| opt.as_ref())
    }
```

- [ ] **Step 8: Update `book_ids()` accessor — keep returning the parallel-to-entries array (unchanged)**

The `book_ids()` method returns `&[Option<BookId>]` indexed by ENTRY index, which is what the renderer needs (it gets display_indices separately). No code change needed here, just confirm it's unchanged in the file.

- [ ] **Step 9: Add tests for the new state**

Append to `#[cfg(test)] mod tests`:

```rust
    use crate::search::SearchMode;

    #[test]
    fn open_search_transitions_idle_to_editing() {
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B")],
            (80, 24),
            None,
            None,
        );
        assert_eq!(app.search_mode(), SearchMode::Idle);
        app.handle(Action::OpenSearch);
        assert_eq!(app.search_mode(), SearchMode::Editing);
        assert!(app.is_searching());
    }

    #[test]
    fn open_search_captures_pre_search_selection() {
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            None,
            None,
        );
        // Move to selection 2, then open search.
        app.handle(Action::LineDown);
        app.handle(Action::LineDown);
        assert_eq!(app.selection(), 2);
        app.handle(Action::OpenSearch);
        assert_eq!(app.selection(), 0, "selection resets to 0 on open_search");
        // pre_search_selection is captured (not directly observable; we
        // verify it by Esc-style restore in Task 4's tests).
    }

    #[test]
    fn display_indices_returns_all_when_idle() {
        let app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            None,
            None,
        );
        assert_eq!(app.display_indices(), &[0, 1, 2]);
    }

    #[test]
    fn empty_query_shows_all_after_open_search() {
        // After open_search() the query is empty; display_indices()
        // should return all entries (since empty-query filter is "all").
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::OpenSearch);
        assert_eq!(app.display_indices(), &[0, 1, 2]);
    }

    #[test]
    fn open_search_from_applied_re_enters_editing_preserving_query() {
        // Mimic Applied state by directly opening search and committing
        // (commit is Task 4). For Task 3, just verify Editing → Editing
        // is idempotent and doesn't clobber the captured pre_search_selection.
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::LineDown);
        app.handle(Action::OpenSearch);
        assert_eq!(app.search_mode(), SearchMode::Editing);
        // Second open_search while already in Editing should not clear
        // pre_search_selection or reset query (idempotent re-open).
        app.handle(Action::OpenSearch);
        assert_eq!(app.search_mode(), SearchMode::Editing);
    }
```

The `entries_lowercased` field is private; coverage comes from the search-behavior tests (typing 'hello' and seeing it match a 'Hello World' title proves the haystack got lowercased correctly). The five tests above cover the Task 3 public surface; Task 4 will add the typing/refilter tests.

- [ ] **Step 10: Run library_app tests + clippy**

Run:
```
cargo test --quiet library_app:: 2>&1 | tail -15
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: existing library_app tests still pass + 5 new tests pass. Clippy clean.

- [ ] **Step 11: Commit**

```bash
git add src/library_app.rs
git commit -m "feat(library_app): wire SearchState + display_indices routing

LibraryApp gains a SearchState, a pre-lowercased haystack array, and
a precomputed all_indices Vec. New accessors: is_searching,
has_filter, search_query, search_mode, display_indices, plus an
open_search() that captures pre_search_selection and transitions
Idle→Editing.

Existing nav arms (LineUp/Down, PagePrev/Next, Confirm) now route
through display_indices() so a filter transparently narrows the
navigation set. book_id() resolves display-index → entry-index → BookId.

Task 4 adds handle_search_input for in-Editing keystroke handling.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Add `handle_search_input(KeyEvent)` for Editing-state input

**Files:**
- Modify: `src/library_app.rs`

This task adds the method that consumes raw KeyEvents while in Editing state. The event loop (Task 6) calls this directly when `is_searching()` is true.

- [ ] **Step 1: Add the import for KeyEvent + KeyCode + KeyModifiers**

Add to the top-of-file imports in `src/library_app.rs`:

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
```

- [ ] **Step 2: Add `handle_search_input` to `impl LibraryApp`**

Place this method after `open_search` and before `refilter`:

```rust
    /// Consume a raw KeyEvent while in Editing state. Dispatches:
    /// - Enter → transition Editing → Applied (close box, keep filter)
    /// - Esc → clear filter, restore pre_search_selection, → Idle
    /// - Backspace → pop last char, refilter
    /// - Up/Down/Left/Right → navigate the filtered set (via handle)
    /// - Ctrl+C → quit the library entirely (matches global Quit)
    /// - Printable Char → append, refilter, reset selection to 0
    /// - Anything else → ignore
    ///
    /// Caller (library_event_loop) checks `is_searching()` first and
    /// routes raw KeyEvents here; this bypasses translate() so every
    /// printable key is available as query input.
    pub fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.search.mode = SearchMode::Applied;
            }
            KeyCode::Esc => {
                self.clear_search();
            }
            KeyCode::Backspace => {
                self.search.query.pop();
                self.refilter();
                self.selection = 0;
            }
            KeyCode::Up => {
                self.handle(Action::LineUp);
            }
            KeyCode::Down => {
                self.handle(Action::LineDown);
            }
            KeyCode::Left => {
                self.handle(Action::PagePrev);
            }
            KeyCode::Right => {
                self.handle(Action::PageNext);
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    // Ctrl+C quits the library (mirrors the global Quit).
                    // Other Ctrl combos ignored.
                    if c == 'c' {
                        self.should_quit = true;
                    }
                    return;
                }
                self.search.query.push(c);
                self.refilter();
                self.selection = 0;
            }
            _ => {}
        }
    }

    /// Clear the search state: empty query, drop filter, return to Idle,
    /// restore the selection that was active when search began. Called
    /// by Esc in either Editing or Applied state.
    fn clear_search(&mut self) {
        self.search.query.clear();
        self.search.filtered.clear();
        self.search.mode = SearchMode::Idle;
        self.selection = self.pre_search_selection;
    }
```

- [ ] **Step 3: Esc from Applied also clears**

The `handle_search_input` method only runs while in Editing. In Applied, the box is closed and the regular `handle(Action::Quit)` path runs (Esc → `Action::Quit`). We need Esc in Applied to clear the filter instead of quitting. Update the `Action::Quit` arm in `handle`:

```rust
            Action::Quit => {
                if matches!(self.search.mode, SearchMode::Applied) {
                    self.clear_search();
                } else {
                    self.should_quit = true;
                }
            }
```

Note: this changes one existing test's expectation — `quit_sets_should_quit_without_selection` passes because the app starts in Idle, not Applied. Verify the existing tests still pass.

- [ ] **Step 4: Add tests for handle_search_input**

Append to `#[cfg(test)] mod tests`:

```rust
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key_press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn ctrl_c_key() -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn search_entry(title: &str, author: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(format!("/{title}.epub")),
            title: title.to_string(),
            author: author.to_string(),
        }
    }

    #[test]
    fn typing_chars_updates_query_and_refilters() {
        let mut app = LibraryApp::new_with(
            vec![
                search_entry("Firefly", "A"),
                search_entry("Threshold", "B"),
                search_entry("Tomorrow", "C"),
            ],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::OpenSearch);
        app.handle_search_input(key_press(KeyCode::Char('f')));
        app.handle_search_input(key_press(KeyCode::Char('i')));
        assert_eq!(app.search_query(), "fi");
        assert_eq!(app.display_indices(), &[0], "only 'Firefly' matches 'fi'");
    }

    #[test]
    fn backspace_pops_query_and_refilters() {
        let mut app = LibraryApp::new_with(
            vec![
                search_entry("Firefly", "A"),
                search_entry("Threshold", "B"),
            ],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::OpenSearch);
        app.handle_search_input(key_press(KeyCode::Char('f')));
        app.handle_search_input(key_press(KeyCode::Char('i')));
        assert_eq!(app.display_indices(), &[0]);
        app.handle_search_input(key_press(KeyCode::Backspace));
        assert_eq!(app.search_query(), "f");
        // 'f' matches "Firefly" but not "Threshold"
        assert_eq!(app.display_indices(), &[0]);
        app.handle_search_input(key_press(KeyCode::Backspace));
        assert_eq!(app.search_query(), "");
        // Empty query matches all
        assert_eq!(app.display_indices(), &[0, 1]);
    }

    #[test]
    fn enter_transitions_editing_to_applied() {
        let mut app = LibraryApp::new_with(
            vec![search_entry("A", "X")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::OpenSearch);
        assert_eq!(app.search_mode(), SearchMode::Editing);
        app.handle_search_input(key_press(KeyCode::Enter));
        assert_eq!(app.search_mode(), SearchMode::Applied);
        assert!(!app.is_searching(), "is_searching is false in Applied");
        assert!(app.has_filter(), "has_filter is true in Applied");
    }

    #[test]
    fn esc_from_editing_clears_filter_and_restores_selection() {
        let mut app = LibraryApp::new_with(
            vec![
                search_entry("A", "X"),
                search_entry("B", "Y"),
                search_entry("C", "Z"),
            ],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::LineDown);
        app.handle(Action::LineDown);
        assert_eq!(app.selection(), 2);
        app.handle(Action::OpenSearch);
        app.handle_search_input(key_press(KeyCode::Char('a')));
        assert_eq!(app.display_indices(), &[0]);
        app.handle_search_input(key_press(KeyCode::Esc));
        assert_eq!(app.search_mode(), SearchMode::Idle);
        assert!(!app.has_filter());
        assert_eq!(app.selection(), 2, "selection restored to pre-search value");
    }

    #[test]
    fn esc_from_applied_clears_filter() {
        let mut app = LibraryApp::new_with(
            vec![search_entry("A", "X"), search_entry("B", "Y")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::OpenSearch);
        app.handle_search_input(key_press(KeyCode::Char('a')));
        app.handle_search_input(key_press(KeyCode::Enter)); // → Applied
        assert_eq!(app.search_mode(), SearchMode::Applied);
        // Esc translates to Action::Quit; in Applied, that should clear
        // the filter rather than quit.
        app.handle(Action::Quit);
        assert_eq!(app.search_mode(), SearchMode::Idle);
        assert!(!app.should_quit(), "Esc from Applied must not quit");
    }

    #[test]
    fn arrow_keys_in_editing_navigate_filtered_results() {
        let mut app = LibraryApp::new_with(
            vec![
                search_entry("Foo One", "A"),
                search_entry("Foo Two", "B"),
                search_entry("Bar", "C"),
                search_entry("Foo Three", "D"),
            ],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::OpenSearch);
        app.handle_search_input(key_press(KeyCode::Char('f')));
        app.handle_search_input(key_press(KeyCode::Char('o')));
        app.handle_search_input(key_press(KeyCode::Char('o')));
        // 3 matches: Foo One, Foo Two, Foo Three (indices 0, 1, 3)
        assert_eq!(app.display_indices(), &[0, 1, 3]);
        assert_eq!(app.selection(), 0);
        // Down arrow in Editing should advance selection within the filtered set.
        // In Grid mode (default for new_with with no prefs), LineDown moves by cols.
        // Toggle to list mode first to make the test deterministic.
        app.handle_search_input(key_press(KeyCode::Esc)); // exit search
        app.handle(Action::ToggleViewMode); // → List
        app.handle(Action::OpenSearch);
        app.handle_search_input(key_press(KeyCode::Char('f')));
        app.handle_search_input(key_press(KeyCode::Char('o')));
        app.handle_search_input(key_press(KeyCode::Char('o')));
        app.handle_search_input(key_press(KeyCode::Down));
        assert_eq!(app.selection(), 1);
        app.handle_search_input(key_press(KeyCode::Down));
        assert_eq!(app.selection(), 2);
        // Confirm opens the 3rd match (display index 2 → entry index 3 = "Foo Three").
        app.handle_search_input(key_press(KeyCode::Enter)); // → Applied
        app.handle(Action::Confirm);
        assert_eq!(
            app.selected_path().map(|p| p.to_string_lossy().into_owned()),
            Some("/Foo Three.epub".to_string())
        );
    }

    #[test]
    fn ctrl_c_in_editing_quits_library() {
        let mut app = LibraryApp::new_with(
            vec![search_entry("A", "X")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::OpenSearch);
        app.handle_search_input(ctrl_c_key());
        assert!(app.should_quit());
    }
```

- [ ] **Step 5: Run tests + clippy**

Run:
```
cargo test --quiet library_app:: 2>&1 | tail -20
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: existing library_app tests still pass + 7 new tests pass. Clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src/library_app.rs
git commit -m "feat(library_app): handle_search_input + Esc-from-Applied semantics

handle_search_input consumes raw KeyEvents while in Editing state:
printable chars append to query, Backspace pops, Enter commits to
Applied, Esc clears + restores pre-search selection, arrow keys
navigate the filtered set, Ctrl+C quits library.

Esc from Applied (which translates to Action::Quit) now clears the
filter instead of quitting, so the user can Esc out of search in two
steps: Editing → Esc → Idle, or Applied → Esc → Idle.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Renderer — extend `LibraryRenderInput`, search box footer, no-matches message

**Files:**
- Modify: `src/reader.rs`

- [ ] **Step 1: Extend `LibraryRenderInput`**

In `src/reader.rs`, find the existing `LibraryRenderInput` struct and replace it with:

```rust
pub struct LibraryRenderInput<'a> {
    pub entries: &'a [crate::library::LibraryEntry],
    pub selection: usize,
    pub view_mode: crate::prefs::ViewMode,
    pub cover_cache: Option<&'a crate::cover_cache::CoverCache>,
    pub book_ids: &'a [Option<crate::epub::BookId>],
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
```

- [ ] **Step 2: Update `render_library_list` to iterate `display_indices`**

Replace `render_library_list`'s body:

```rust
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

    // Title bar.
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
        let scroll = center_scroll(input.selection, total, visible_rows);
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
```

- [ ] **Step 3: Update `render_library_grid` to iterate `display_indices`**

Find the cell-rendering loop inside `render_library_grid`. The existing code uses `(first_idx..last_idx)` directly over entries; switch to display_indices. Replace the section that walks visible cells:

Find:
```rust
    let visible = visible_grid_range(grid_area.width, grid_area.height, total, input.selection)
        .unwrap_or(0..0);
    let first_idx = visible.start;
    let last_idx = visible.end;
```

The `total` variable is currently `input.entries.len()`. Change it to use display_indices length:

```rust
    let total = input.display_indices.len();
    let visible = visible_grid_range(grid_area.width, grid_area.height, total, input.selection)
        .unwrap_or(0..0);
    let first_idx = visible.start;
    let last_idx = visible.end;
```

Then inside the per-cell loop, the existing code does:
```rust
    for (offset, abs_idx) in (first_idx..last_idx).enumerate() {
        ...
        let entry = &input.entries[abs_idx];
        let is_selected = abs_idx == input.selection;
```

Update so `abs_idx` is the entry index (resolved through display_indices), and `is_selected` compares against the display offset:

```rust
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
                Constraint::Length(crate::cover_cache::COVER_THUMBNAIL_HEIGHT),
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
                    .take(crate::cover_cache::COVER_THUMBNAIL_HEIGHT as usize)
                    .map(|l| Line::from(l.clone()))
                    .collect(),
                None => crate::cover_cache::PLACEHOLDER
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
        let title_truncated = truncate_to_width(&entry.title, crate::cover_cache::COVER_THUMBNAIL_WIDTH as usize);
        let author_truncated = truncate_to_width(&entry.author, crate::cover_cache::COVER_THUMBNAIL_WIDTH as usize);
        let title_lines = vec![
            Line::from(TuiSpan::styled(title_truncated, title_style)),
            Line::from(TuiSpan::styled(
                author_truncated,
                Style::default().add_modifier(Modifier::DIM),
            )),
        ];
        frame.render_widget(Paragraph::new(title_lines), cell_chunks[1]);
    }
```

And at the top of `render_library_grid`, when display_indices is empty AND a filter is active, render the "no matches" message instead of the grid:

```rust
    if input.display_indices.is_empty()
        && !matches!(input.search_mode, crate::search::SearchMode::Idle)
    {
        render_no_matches(frame, grid_area);
    } else {
        // ... existing cell-rendering code (the loop above) ...
    }
```

(Wrap the grid math + cell-rendering loop in this else branch.)

Replace the existing footer rendering (the `let footer_text = match input.warning {...}` block) with a call to the shared footer helper:

```rust
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
```

- [ ] **Step 4: Add the shared footer + no-matches helpers**

Add these private helpers in `src/reader.rs`, near the other library-rendering code (after `render_library_grid`, before the test module):

```rust
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
    let msg = "no matches";
    let line = Line::from(TuiSpan::styled(
        msg,
        Style::default().add_modifier(Modifier::DIM),
    ));
    let para = Paragraph::new(line).alignment(ratatui::layout::Alignment::Center);
    let center_y = area.y + area.height / 2;
    let centered = Rect {
        x: area.x,
        y: center_y,
        width: area.width,
        height: 1,
    };
    frame.render_widget(para, centered);
}
```

- [ ] **Step 5: Update existing test call sites**

Three existing render_library tests need to add the new fields. Find each test in `mod tests` and update the `LibraryRenderInput { ... }` literal to include `display_indices`, `search_query`, `search_mode`. For the simplest case (no search active), the values are `&[0, 1, ...]` matching `0..entries.len()`, `None`, and `SearchMode::Idle`.

`library_render_does_not_panic_on_narrow_terminal`:

```rust
            render_library(frame, area, LibraryRenderInput {
                entries: &[
                    LibraryEntry {
                        path: std::path::PathBuf::from("/a.epub"),
                        title: "A".into(),
                        author: "X".into(),
                    },
                ],
                selection: 0,
                view_mode: crate::prefs::ViewMode::List,
                cover_cache: None,
                book_ids: &[None],
                warning: None,
                display_indices: &[0],
                search_query: None,
                search_mode: crate::search::SearchMode::Idle,
            });
```

`render_library_grid_does_not_panic_on_tiny_terminal`:

```rust
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
```

`render_library_grid_renders_on_80x40_without_panic`:

```rust
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
```

`render_library_grid_uses_cover_cache_when_available`:

```rust
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
```

`library_footer_shows_warning_in_both_modes`:

```rust
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
```

(Note: `library_footer_shows_warning_in_both_modes` lacks a `book_ids` variable today — it's defined inline. Check the current file for the exact existing shape and add `display_indices` field accordingly.)

- [ ] **Step 6: Add tests for the search-box footer and no-matches**

Add these tests to `mod tests`:

```rust
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
        assert!(buf.contains("/ book"), "footer should show '/ book'");
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
```

- [ ] **Step 7: Run tests + clippy + cargo doc**

Run:
```
cargo test --quiet 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo doc --no-deps 2>&1 | grep -i warning
```
Expected: all unit tests pass (Task 5 adds 3 new renderer tests). Clippy clean. No doc warnings.

⚠️ The integration test still compiles but its existing `render_library` call passes the old field shape. The integration test will be updated in Task 7. For now, ensure `cargo test --test integration` either still passes (because Rust's default values for missing fields aren't a thing — the build WILL break here). Expected: integration test FAILS TO COMPILE. That's fine — Task 6 doesn't touch it either; Task 7 updates it. To keep `cargo test` exit code clean during the inter-task period, you can `#[ignore]` the integration test in this commit and remove the ignore in Task 7. OR you can update the integration call site here in Task 5 (preserving the test, just adding the new fields). I recommend updating it here so the build stays green:

In `tests/integration.rs`, find `library_grid_renders_without_panic_when_directory_has_epubs` and update its `LibraryRenderInput { ... }` literal:

```rust
    let display_indices: Vec<usize> = (0..app.entries().len()).collect();
    term.draw(|frame| {
        let area = frame.area();
        render_library(
            frame,
            area,
            LibraryRenderInput {
                entries: &entries_snapshot,
                selection: 0,
                view_mode: ViewMode::Grid,
                cover_cache: app.cover_cache(),
                book_ids: &book_ids_snapshot,
                warning: None,
                display_indices: &display_indices,
                search_query: None,
                search_mode: cleader::search::SearchMode::Idle,
            },
        );
    })
```

Now `cargo test` should pass end-to-end at this stage.

- [ ] **Step 8: Commit**

```bash
git add src/reader.rs tests/integration.rs
git commit -m "feat(reader): wire search box footer + display_indices iteration

LibraryRenderInput gains display_indices, search_query, search_mode.
Both list and grid renderers iterate display_indices instead of the
full entries slice, so a filter transparently narrows the visible
sequence. Selection compares against the display position, not the
entry index.

Footer rendering is now 3-way: search box (Editing or Applied), warning
banner, or default hint. The search box uses a 3-span line: bold left
('/ <query><cursor>'), gray middle (padding), dim right ('<N> matches
· <hint>').

A centered DIM 'no matches' overlay renders when the filter returns
zero results.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: `library_event_loop` — branch on `is_searching()`, route through `display_indices()`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update imports**

Make sure `crossterm::event::Event` is imported. The file currently uses `crossterm::event;` — add a `use crossterm::event::Event;` alias if not already present (just confirm; the existing event loop already reads `event::read()` and the result is `crossterm::event::Event`).

- [ ] **Step 2: Rewrite `library_event_loop`**

Replace `library_event_loop`'s body with:

```rust
fn library_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut cleader::library_app::LibraryApp,
) -> anyhow::Result<()> {
    let mut needs_redraw = true; // first frame always renders
    while !app.should_quit() {
        // Drain any covers the worker finished since the last frame.
        let had_new_covers = app
            .cover_cache_mut()
            .map(|c| c.drain_finished())
            .unwrap_or(false);

        // Ask the cache to start generating covers for visible cells,
        // mapped through display_indices so a search filter narrows
        // the request set.
        if matches!(app.view_mode(), cleader::prefs::ViewMode::Grid) {
            let (term_w, term_h) = terminal
                .size()
                .map(|s| (s.width, s.height))
                .unwrap_or((80, 24));
            let grid_h = term_h.saturating_sub(2);
            let display_len = app.display_indices().len();
            if let Some(range) = cleader::reader::visible_grid_range(
                term_w,
                grid_h,
                display_len,
                app.selection(),
            ) {
                let display = app.display_indices().to_vec(); // snapshot
                let entry_indices: Vec<usize> = range.map(|i| display[i]).collect();
                app.request_visible_covers(entry_indices);
            }
        }

        if needs_redraw || had_new_covers {
            let entries_snapshot: Vec<_> = app.entries().to_vec();
            let book_ids_snapshot = app.book_ids().to_vec();
            let display_indices_snapshot: Vec<usize> = app.display_indices().to_vec();
            let selection = app.selection();
            let view_mode = app.view_mode();
            let warning_owned = app.save_error().map(|s| s.to_string());
            let cover_cache = app.cover_cache();
            let search_mode = app.search_mode();
            let search_query_owned: Option<String> = if matches!(
                search_mode,
                cleader::search::SearchMode::Idle
            ) {
                None
            } else {
                Some(app.search_query().to_string())
            };

            terminal.draw(|frame| {
                let area = frame.area();
                cleader::reader::render_library(
                    frame,
                    area,
                    cleader::reader::LibraryRenderInput {
                        entries: &entries_snapshot,
                        selection,
                        view_mode,
                        cover_cache,
                        book_ids: &book_ids_snapshot,
                        warning: warning_owned.as_deref(),
                        display_indices: &display_indices_snapshot,
                        search_query: search_query_owned.as_deref(),
                        search_mode,
                    },
                );
            })?;
            needs_redraw = false;
        }

        // Poll for input with a 50ms timeout. If nothing arrives, loop
        // back so we can drain newly-finished covers.
        if event::poll(std::time::Duration::from_millis(50))? {
            let evt = event::read()?;
            if app.is_searching() {
                // In Editing state, route raw KeyEvents directly to the
                // search handler — bypass translate() so every printable
                // key is available as query input.
                if let crossterm::event::Event::Key(key) = evt {
                    app.handle_search_input(key);
                    needs_redraw = true;
                }
            } else if let Some(action) = translate(evt) {
                app.handle(action);
                needs_redraw = true;
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Run cargo build + clippy + tests**

Run:
```
cargo build 2>&1 | tail -5
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo test --quiet 2>&1 | grep "test result"
```
Expected: clean build, clippy clean, all unit + integration tests pass (no count change — main.rs has no tests).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): route library event loop through display_indices

library_event_loop now snapshots and passes display_indices,
search_query, and search_mode to the renderer. Cover-request math
maps the visible-window range through display_indices, so a search
filter narrows the request set (filter to 3 matches → at most 3 cover
requests).

When the app reports is_searching() (Editing state), raw KeyEvents
go straight to handle_search_input — bypassing translate() so every
printable key is available as query input.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Integration smoke test for library search

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add the integration test**

Append to `tests/integration.rs`:

```rust
#[test]
fn library_search_filters_entries_end_to_end() {
    use cleader::cover_cache::CoverCache;
    use cleader::input::Action;
    use cleader::library::scan_directory;
    use cleader::library_app::LibraryApp;
    use cleader::prefs::PrefsStore;
    use cleader::search::SearchMode;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent {
            code: c,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    let Some(book_path) = require_book(None) else { return; };
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("book.epub");
    std::fs::copy(&book_path, &dest).unwrap();

    let entries = scan_directory(dir.path()).expect("scan");
    assert!(!entries.is_empty(), "scan should find at least one EPUB");
    let original_count = entries.len();
    let original_title_first_char = entries[0]
        .title
        .chars()
        .next()
        .unwrap_or('a')
        .to_ascii_lowercase();

    let prefs_dir = tempfile::tempdir().unwrap();
    let prefs = PrefsStore::open_at(prefs_dir.path().join("prefs.json"));
    let cache_dir = tempfile::tempdir().unwrap();
    let cover_cache = CoverCache::open_at(cache_dir.path().to_path_buf());

    let mut app = LibraryApp::new_with(entries, (80, 40), Some(prefs), Some(cover_cache));

    // Open search.
    app.handle(Action::OpenSearch);
    assert_eq!(app.search_mode(), SearchMode::Editing);
    assert!(app.is_searching());
    assert_eq!(app.display_indices().len(), original_count, "empty query → all entries");

    // Type a char that's in the first entry's title to ensure ≥1 match.
    app.handle_search_input(key(KeyCode::Char(original_title_first_char)));
    assert!(
        !app.display_indices().is_empty(),
        "first-char filter should keep at least the first entry"
    );

    // Type something that can't possibly match.
    app.handle_search_input(key(KeyCode::Char('§')));
    app.handle_search_input(key(KeyCode::Char('§')));
    assert!(app.display_indices().is_empty(), "no entries match '§§'");

    // Backspace twice to widen the filter back to '<first_char>'.
    app.handle_search_input(key(KeyCode::Backspace));
    app.handle_search_input(key(KeyCode::Backspace));
    assert!(!app.display_indices().is_empty(), "backspace restored to 1-char filter");

    // Enter to commit → Applied.
    app.handle_search_input(key(KeyCode::Enter));
    assert_eq!(app.search_mode(), SearchMode::Applied);
    assert!(!app.is_searching());
    assert!(app.has_filter());

    // Esc from Applied → clear filter.
    app.handle(Action::Quit);
    assert_eq!(app.search_mode(), SearchMode::Idle);
    assert!(!app.has_filter());
    assert!(!app.should_quit(), "Esc from Applied must not quit the library");
    assert_eq!(app.display_indices().len(), original_count);
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --quiet --test integration 2>&1 | tail -10`
Expected: all integration tests pass (or skip cleanly if no fixture EPUB is in `books/`).

- [ ] **Step 3: Full suite + clippy + release build**

Run:
```
cargo test --quiet 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo build --release 2>&1 | tail -5
cargo doc --no-deps 2>&1 | grep -i warning
```
Expected:
- Tests: ~225 unit + 11 integration (was 203 + 10 at v0.4.4; v0.4.5 adds ~22 unit + 1 integration)
- Clippy clean, doc clean, release build green

- [ ] **Step 4: Commit**

```bash
git add tests/integration.rs
git commit -m "test(integration): library search filters entries end-to-end

Exercises the full search pipeline on a real EPUB: open search,
type a matching char (≥1 match), type non-matching chars (0 matches),
backspace to widen, Enter to commit (→ Applied), Esc to clear (→ Idle).
Asserts state transitions and display_indices at each step.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review Checklist (run after all tasks complete)

1. `cargo test --quiet 2>&1 | grep 'test result'` — confirm ~225 unit + 11 integration.
2. `cargo clippy --all-targets -- -D warnings` — clean.
3. `cargo doc --no-deps 2>&1 | grep -i warning` — no doc warnings.
4. `cargo build --release` — succeeds.
5. Manual smoke test: `cargo run --release -- some/directory/`:
   - `/` opens search box at footer
   - Typing narrows the visible entries live
   - Esc clears + returns cursor to pre-search book
   - Enter commits, arrow keys navigate filtered set
   - `/` from Applied re-opens box pre-populated
   - Switching `g` toggle works only in Idle or Applied (in Editing it would be a query char)
   - Selecting a filtered book opens it in the reader; q/Esc returns to library with filter still in place (Applied)

## Spec Coverage Map

| Spec section | Covered by |
|---|---|
| `search.rs` module | Task 1 |
| `Action::OpenSearch` + `/` binding | Task 2 |
| `SearchState` + `entries_lowercased` + `pre_search_selection` in `LibraryApp` | Task 3 |
| `display_indices()` accessor | Task 3 |
| `open_search()` method | Task 3 |
| Nav handlers route through display_indices | Task 3 |
| `handle_search_input(KeyEvent)` | Task 4 |
| State machine transitions (Editing↔Applied, Esc→Idle) | Task 4 |
| Pre-search selection restore | Task 4 |
| Ctrl+C in Editing quits | Task 4 |
| `LibraryRenderInput` extensions (display_indices, search_query, search_mode) | Task 5 |
| Footer search box rendering (Editing + Applied) | Task 5 |
| Centered "no matches" message | Task 5 |
| Renderer iterates display_indices | Task 5 |
| `library_event_loop` branches on `is_searching()` | Task 6 |
| Cover requests routed through display_indices | Task 6 |
| End-to-end integration test | Task 7 |
