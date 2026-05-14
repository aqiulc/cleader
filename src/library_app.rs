//! Library-browsing app (separate event loop from the Reader).
//!
//! Active when the CLI path is a directory. Lists EPUBs found there,
//! lets the user pick one with arrow keys + Enter. On Enter, the app
//! sets `selected_path` and signals quit; main.rs then hands off to
//! the regular Reader with that book. On Esc/q/Ctrl+C, the app quits
//! with no selection.

use crate::cover_cache::CoverCache;
use crate::epub::BookId;
use crate::input::Action;
use crate::library::LibraryEntry;
use crate::prefs::{PrefsStore, ViewMode};
use crate::search::{filter_indices, SearchMode, SearchState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::time::Instant;

/// Time to hold at the start of a long title before scrolling begins,
/// so the user can read the beginning. 1000 ms.
const MARQUEE_HOLD_START_MS: u128 = 1000;

/// Time to hold at the end of a long title after the tail is visible,
/// so the user can read the end. 1000 ms.
const MARQUEE_HOLD_END_MS: u128 = 1000;

/// Time per character of scroll. 250 ms = 4 chars/sec — slow enough
/// to read mid-scroll.
const MARQUEE_PER_CHAR_MS: u128 = 250;

/// Pure scroll-offset calculator. Given the number of milliseconds
/// since the marquee started and the total characters the title
/// overflows the cell by, returns the current left-shift offset
/// (0..=overflow).
///
/// Cycle phases (in order):
/// 1. Hold at start (offset = 0) for MARQUEE_HOLD_START_MS
/// 2. Scroll left, one char per MARQUEE_PER_CHAR_MS, until offset == overflow
/// 3. Hold at end (offset = overflow) for MARQUEE_HOLD_END_MS
/// 4. Snap back to 0, repeat
///
/// If `overflow == 0` the function always returns 0 (no-op).
pub fn marquee_offset(elapsed_ms: u128, overflow: usize) -> usize {
    if overflow == 0 {
        return 0;
    }
    let scroll_ms = (overflow as u128) * MARQUEE_PER_CHAR_MS;
    let cycle_ms = MARQUEE_HOLD_START_MS + scroll_ms + MARQUEE_HOLD_END_MS;
    let t = elapsed_ms % cycle_ms;
    if t < MARQUEE_HOLD_START_MS {
        0
    } else if t < MARQUEE_HOLD_START_MS + scroll_ms {
        ((t - MARQUEE_HOLD_START_MS) / MARQUEE_PER_CHAR_MS) as usize
    } else {
        overflow
    }
}

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
    /// Set when the user presses Enter on a selection. The main.rs
    /// dispatcher reads this after the event loop exits to decide
    /// whether to launch the Reader.
    selected_path: Option<PathBuf>,
    view_mode: ViewMode,
    cover_cache: Option<CoverCache>,
    prefs: Option<PrefsStore>,
    save_error: Option<String>,
    search: SearchState,
    /// Selection captured when search began. Restored on Esc clear.
    pre_search_selection: usize,
    /// When the currently-selected cell started its marquee animation.
    /// Reset to `Some(Instant::now())` whenever `selection` changes (so
    /// every new selection begins at the start-of-cycle hold). `None`
    /// only at construction before the first selection change.
    marquee_start: Option<Instant>,
}

impl LibraryApp {
    /// Production constructor: opens PrefsStore and CoverCache from the
    /// OS data dir. Falls back to disabled cache (grid view degrades to
    /// list) if the OS data dir is unavailable. Prefs failure falls back
    /// to default (ViewMode::Grid).
    pub fn new(entries: Vec<LibraryEntry>, viewport: (u16, u16)) -> Self {
        Self::new_with(entries, viewport, PrefsStore::open().ok(), CoverCache::open())
    }

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
            marquee_start: Some(Instant::now()),
        }
    }

    pub fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    pub fn selection(&self) -> usize {
        self.selection
    }

    pub fn viewport_size(&self) -> (u16, u16) {
        self.viewport_size
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn selected_path(&self) -> Option<&std::path::Path> {
        self.selected_path.as_deref()
    }

    /// Reset the "user selected and confirmed" state so the app can be
    /// re-entered for another selection. Preserves `entries` and
    /// `selection` so the user lands back on whichever book they just
    /// finished reading; reading position state is owned by Persistence
    /// (not by LibraryApp).
    pub fn reset_for_reselection(&mut self) {
        self.should_quit = false;
        self.selected_path = None;
    }

    /// Update the viewport size, e.g. after returning from the reader
    /// session where the terminal might have been resized.
    pub fn set_viewport(&mut self, viewport: (u16, u16)) {
        self.viewport_size = viewport;
    }

    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    pub fn cover_cache(&self) -> Option<&CoverCache> {
        self.cover_cache.as_ref()
    }

    pub fn cover_cache_mut(&mut self) -> Option<&mut CoverCache> {
        self.cover_cache.as_mut()
    }

    pub fn book_ids(&self) -> &[Option<BookId>] {
        &self.book_ids
    }

    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// Elapsed milliseconds since the marquee started. Renderer uses
    /// this with the current title's overflow to compute the scroll
    /// offset via `marquee_offset`. Returns 0 if marquee is somehow
    /// unset (defensive — should always be Some after construction).
    pub fn marquee_elapsed_ms(&self) -> u128 {
        self.marquee_start
            .map(|start| start.elapsed().as_millis())
            .unwrap_or(0)
    }

    /// Request covers for the given entry indices. Resolves each index
    /// to a `BookId` (lazily — computed once and stored in
    /// `self.book_ids[idx]`), then calls `enqueue`. Indices out of range
    /// are silently skipped.
    ///
    /// Each uncached index performs a synchronous file read on the
    /// calling thread (needed because `BookId` is content-hashed from
    /// the EPUB bytes). Callers must restrict `indices` to the visible
    /// window — typically the cells currently on screen — to avoid
    /// perceptible stalls when scrolling through large libraries.
    pub fn request_visible_covers(&mut self, indices: impl IntoIterator<Item = usize>) {
        let Some(cache) = self.cover_cache.as_mut() else {
            return;
        };
        for idx in indices {
            let Some(entry) = self.entries.get(idx) else {
                continue;
            };
            // Lazy-compute the BookId from the file bytes the first
            // time we need it. Failure (e.g. file moved) just skips.
            if self.book_ids[idx].is_none() {
                if let Ok(bytes) = std::fs::read(&entry.path) {
                    self.book_ids[idx] = Some(BookId::from_bytes(&bytes));
                }
            }
            if let Some(id) = &self.book_ids[idx] {
                cache.enqueue(id.clone(), entry.path.clone());
            }
        }
    }

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
            self.set_selection(0);
        }
        self.search.mode = SearchMode::Editing;
    }

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
                self.set_selection(0);
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
                self.set_selection(0);
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
        self.set_selection(self.pre_search_selection);
    }

    /// Recompute `search.filtered` from `search.query`. Called after
    /// every query mutation.
    fn refilter(&mut self) {
        let query_lc = self.search.query.to_lowercase();
        self.search.filtered = filter_indices(&self.entries_lowercased, &query_lc);
    }

    /// Set the selection and restart the marquee animation. All nav
    /// arms in `handle` should go through this helper rather than
    /// mutating `self.selection` directly — that way every selection
    /// change resets the marquee cycle to the start-of-hold phase.
    fn set_selection(&mut self, new_selection: usize) {
        if new_selection != self.selection {
            self.selection = new_selection;
            self.marquee_start = Some(Instant::now());
        }
    }

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

    fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Grid => ViewMode::List,
            ViewMode::List => ViewMode::Grid,
        };
        if let Some(prefs) = self.prefs.as_mut() {
            if let Err(e) = prefs.set_view_mode(self.view_mode) {
                self.save_error = Some(format!("could not save prefs: {e}"));
            } else {
                self.save_error = None;
            }
        }
    }

    /// Compute columns currently used by the grid renderer. Returns 1
    /// in list mode or when the terminal is too narrow for a single
    /// grid cell (in which case grid mode falls back to list anyway).
    fn grid_cols(&self) -> usize {
        let cell_width = crate::render_library::CELL_WIDTH as usize;
        (self.viewport_size.0 as usize / cell_width).max(1)
    }

    pub fn handle(&mut self, action: Action) {
        match action {
            Action::LineUp => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let cols = self.grid_cols();
                    let new = self.selection.saturating_sub(cols);
                    self.set_selection(new);
                } else if self.selection > 0 {
                    self.set_selection(self.selection - 1);
                }
            }
            Action::LineDown => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let cols = self.grid_cols();
                    let max = self.display_indices().len().saturating_sub(1);
                    let new = (self.selection + cols).min(max);
                    self.set_selection(new);
                } else if self.selection + 1 < self.display_indices().len() {
                    self.set_selection(self.selection + 1);
                }
            }
            Action::PagePrev => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    if self.selection > 0 {
                        self.set_selection(self.selection - 1);
                    }
                } else {
                    let step = self.lines_per_page().min(10);
                    let new = self.selection.saturating_sub(step);
                    self.set_selection(new);
                }
            }
            Action::PageNext => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let max = self.display_indices().len().saturating_sub(1);
                    if self.selection < max {
                        self.set_selection(self.selection + 1);
                    }
                } else {
                    let step = self.lines_per_page().min(10);
                    let max = self.display_indices().len().saturating_sub(1);
                    let new = (self.selection + step).min(max);
                    self.set_selection(new);
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
            Action::Quit => {
                // In Applied state, Esc clears the filter rather than
                // quitting (two-Esc pattern: first Esc closes the filter,
                // a second Esc actually quits the library). This matches
                // the modal-overlay convention used elsewhere in cleader.
                if matches!(self.search.mode, SearchMode::Applied) {
                    self.clear_search();
                } else {
                    self.should_quit = true;
                }
            }
            Action::Resize(w, h) => {
                self.viewport_size = (w, h);
            }
            Action::ToggleViewMode => {
                self.toggle_view_mode();
            }
            Action::OpenSearch => {
                self.open_search();
            }
            // Reader-only actions are no-ops in library mode.
            Action::ChapterNext
            | Action::ChapterPrev
            | Action::ToggleHelp
            | Action::ToggleToc => {}
        }
    }

    fn lines_per_page(&self) -> usize {
        // -2 for the title bar + footer.
        (self.viewport_size.1 as usize).saturating_sub(2).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchMode;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::path::Path;

    fn entry(title: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(format!("/{title}.epub")),
            title: title.to_string(),
            author: "Anon".to_string(),
        }
    }

    fn search_entry(title: &str, author: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(format!("/{title}.epub")),
            title: title.to_string(),
            author: author.to_string(),
        }
    }

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

    #[test]
    fn line_down_moves_selection() {
        // In list mode, LineDown moves by exactly 1.
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            Some(fresh_prefs(dir.path())),
            None,
        );
        app.handle(Action::ToggleViewMode); // → List
        assert_eq!(app.selection(), 0);
        app.handle(Action::LineDown);
        assert_eq!(app.selection(), 1);
    }

    #[test]
    fn line_down_clamps_at_end() {
        // In list mode, LineDown clamps at the last entry.
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B")],
            (80, 24),
            Some(fresh_prefs(dir.path())),
            None,
        );
        app.handle(Action::ToggleViewMode); // → List
        app.handle(Action::LineDown);
        app.handle(Action::LineDown);
        app.handle(Action::LineDown);
        assert_eq!(app.selection(), 1);
    }

    #[test]
    fn line_up_at_top_is_noop() {
        let mut app = LibraryApp::new_with(vec![entry("A"), entry("B")], (80, 24), None, None);
        app.handle(Action::LineUp);
        assert_eq!(app.selection(), 0);
    }

    #[test]
    fn confirm_sets_selected_path_and_should_quit() {
        // In list mode, LineDown moves by 1 so we land on entry B.
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            Some(fresh_prefs(dir.path())),
            None,
        );
        app.handle(Action::ToggleViewMode); // → List
        app.handle(Action::LineDown);
        app.handle(Action::Confirm);
        assert!(app.should_quit());
        assert_eq!(app.selected_path(), Some(PathBuf::from("/B.epub").as_path()));
    }

    #[test]
    fn quit_sets_should_quit_without_selection() {
        let mut app = LibraryApp::new_with(vec![entry("A")], (80, 24), None, None);
        app.handle(Action::Quit);
        assert!(app.should_quit());
        assert!(app.selected_path().is_none());
    }

    #[test]
    fn confirm_on_empty_library_is_noop() {
        // Edge case: an empty library shouldn't be reachable (main.rs
        // exits with a clean error before launching), but be defensive.
        let mut app = LibraryApp::new_with(vec![], (80, 24), None, None);
        app.handle(Action::Confirm);
        assert!(!app.should_quit());
        assert!(app.selected_path().is_none());
    }

    #[test]
    fn page_next_advances_by_step() {
        // PageNext in list mode jumps by a page (min(lines_per_page, 10) = 10).
        let entries: Vec<LibraryEntry> = (0..50).map(|i| entry(&format!("E{i:02}"))).collect();
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(entries, (80, 24), Some(fresh_prefs(dir.path())), None);
        app.handle(Action::ToggleViewMode); // → List
        app.handle(Action::PageNext);
        assert_eq!(app.selection(), 10);
    }

    #[test]
    fn reset_for_reselection_clears_completion_state() {
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::LineDown);
        app.handle(Action::Confirm);
        assert!(app.should_quit());
        assert!(app.selected_path().is_some());
        let saved_selection = app.selection();

        app.reset_for_reselection();
        assert!(!app.should_quit(), "should_quit must be cleared");
        assert!(app.selected_path().is_none(), "selected_path must be cleared");
        assert_eq!(
            app.selection(),
            saved_selection,
            "selection must be preserved"
        );
    }

    #[test]
    fn set_viewport_updates_viewport_size() {
        let mut app = LibraryApp::new_with(vec![entry("A")], (80, 24), None, None);
        assert_eq!(app.viewport_size(), (80, 24));
        app.set_viewport((100, 30));
        assert_eq!(app.viewport_size(), (100, 30));
    }

    fn fresh_prefs(dir: &Path) -> PrefsStore {
        PrefsStore::open_at(dir.join("prefs.json"))
    }

    #[test]
    fn toggle_view_mode_flips_grid_to_list() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = fresh_prefs(dir.path());
        let mut app = LibraryApp::new_with(
            vec![entry("A")],
            (80, 24),
            Some(prefs),
            None,
        );
        assert_eq!(app.view_mode(), ViewMode::Grid);
        app.handle(Action::ToggleViewMode);
        assert_eq!(app.view_mode(), ViewMode::List);
        app.handle(Action::ToggleViewMode);
        assert_eq!(app.view_mode(), ViewMode::Grid);
    }

    #[test]
    fn toggle_view_mode_persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let prefs = PrefsStore::open_at(path.clone());
        let mut app = LibraryApp::new_with(
            vec![entry("A")],
            (80, 24),
            Some(prefs),
            None,
        );
        app.handle(Action::ToggleViewMode);
        // Re-open the store from disk and verify.
        let reloaded = PrefsStore::open_at(path);
        assert_eq!(reloaded.view_mode(), ViewMode::List);
    }

    #[test]
    fn request_visible_covers_is_noop_with_no_cache() {
        // Without a cover_cache (the test fallback), this must not panic.
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B")],
            (80, 24),
            None,
            None,
        );
        app.request_visible_covers(0..2);
        assert!(app.book_id(0).is_none(), "no cache → no book_id resolution");
    }

    #[test]
    fn view_mode_defaults_to_grid_without_prefs() {
        let app = LibraryApp::new_with(vec![entry("A")], (80, 24), None, None);
        assert_eq!(app.view_mode(), ViewMode::Grid);
    }

    #[test]
    fn save_error_is_none_after_successful_toggle() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(
            vec![entry("A")],
            (80, 24),
            Some(fresh_prefs(dir.path())),
            None,
        );
        app.handle(Action::ToggleViewMode);
        assert!(app.save_error().is_none());
    }

    #[test]
    fn toggle_view_mode_without_prefs_still_flips_and_does_not_panic() {
        let mut app = LibraryApp::new_with(
            vec![entry("A")],
            (80, 24),
            None,
            None,
        );
        assert_eq!(app.view_mode(), ViewMode::Grid);
        app.handle(Action::ToggleViewMode);
        assert_eq!(app.view_mode(), ViewMode::List);
        assert!(app.save_error().is_none(), "save_error must stay None when prefs are absent");
    }

    #[test]
    fn grid_mode_line_down_moves_by_cols() {
        // viewport 80 wide → 80/24 = 3 cols
        let entries: Vec<LibraryEntry> = (0..10)
            .map(|i| entry(&format!("E{i:02}")))
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(
            entries,
            (80, 24),
            Some(fresh_prefs(dir.path())),
            None,
        );
        assert_eq!(app.view_mode(), ViewMode::Grid);
        assert_eq!(app.selection(), 0);
        app.handle(Action::LineDown);
        assert_eq!(app.selection(), 3, "LineDown in grid should move by cols (3 on 80-wide)");
        app.handle(Action::LineUp);
        assert_eq!(app.selection(), 0);
    }

    #[test]
    fn grid_mode_page_next_moves_one_cell() {
        let entries: Vec<LibraryEntry> = (0..10)
            .map(|i| entry(&format!("E{i:02}")))
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(
            entries,
            (80, 24),
            Some(fresh_prefs(dir.path())),
            None,
        );
        assert_eq!(app.selection(), 0);
        app.handle(Action::PageNext);
        assert_eq!(app.selection(), 1, "PageNext in grid should move 1 cell right");
        app.handle(Action::PagePrev);
        assert_eq!(app.selection(), 0);
    }

    #[test]
    fn list_mode_navigation_still_works_as_before() {
        // After toggling to list mode, LineDown moves by 1, PageNext jumps a page.
        let entries: Vec<LibraryEntry> = (0..50)
            .map(|i| entry(&format!("E{i:02}")))
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let mut app = LibraryApp::new_with(
            entries,
            (80, 24),
            Some(fresh_prefs(dir.path())),
            None,
        );
        app.handle(Action::ToggleViewMode); // → List
        assert_eq!(app.view_mode(), ViewMode::List);
        app.handle(Action::LineDown);
        assert_eq!(app.selection(), 1);
        app.handle(Action::PageNext);
        assert!(app.selection() > 1, "PageNext in list should jump a page");
    }

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
        // pre_search_selection is captured (not directly observable;
        // verified by Esc restore in esc_from_editing_clears_filter_and_restores_selection).
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
        // After open_search() we're in Editing mode (has_filter == true,
        // is_searching == true) with an empty query. display_indices()
        // returns &search.filtered, which equals all indices because
        // filter_indices("") returns 0..N. This pins the Editing-path
        // behavior — distinct from display_indices_returns_all_when_idle
        // which exercises the Idle-path that reads from all_indices.
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            None,
            None,
        );
        app.handle(Action::OpenSearch);
        assert!(app.is_searching(), "open_search should put us in Editing");
        assert!(app.has_filter(), "Editing mode counts as has_filter");
        assert_eq!(app.display_indices(), &[0, 1, 2]);
    }

    #[test]
    fn open_search_in_editing_is_idempotent() {
        // Re-pressing OpenSearch while already in Editing should not
        // reset state — it leaves mode, query, and pre_search_selection
        // intact. The full Applied → Editing round-trip is verified in
        // enter_transitions_editing_to_applied and esc_from_applied_clears_filter.
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

    #[test]
    fn applied_filter_survives_reset_for_reselection() {
        // After picking a book from filtered results and returning from
        // the reader, the search state must survive: selection stays at
        // the filtered position, the filter is still applied. This pins
        // reset_for_reselection's deliberate non-touch of search state.
        let mut app = LibraryApp::new_with(
            vec![
                search_entry("Foo One", "A"),
                search_entry("Bar", "B"),
                search_entry("Foo Two", "C"),
            ],
            (80, 24),
            None,
            None,
        );
        // Toggle to list mode so selection movement is deterministic.
        app.handle(Action::ToggleViewMode);
        app.handle(Action::OpenSearch);
        app.handle_search_input(key_press(KeyCode::Char('f')));
        app.handle_search_input(key_press(KeyCode::Char('o')));
        app.handle_search_input(key_press(KeyCode::Char('o')));
        app.handle_search_input(key_press(KeyCode::Down)); // select Foo Two
        app.handle_search_input(key_press(KeyCode::Enter)); // → Applied
        assert_eq!(app.search_mode(), SearchMode::Applied);
        assert_eq!(app.selection(), 1);
        assert_eq!(app.display_indices(), &[0, 2]);

        // Simulate Confirm-then-reader-quit path.
        app.handle(Action::Confirm); // sets should_quit + selected_path
        app.reset_for_reselection();

        // Filter still applied, selection still at filtered position.
        assert_eq!(app.search_mode(), SearchMode::Applied);
        assert_eq!(app.selection(), 1);
        assert_eq!(app.display_indices(), &[0, 2]);
        assert!(!app.should_quit());
        assert!(app.selected_path().is_none());
    }

    #[test]
    fn marquee_offset_returns_zero_when_no_overflow() {
        assert_eq!(super::marquee_offset(0, 0), 0);
        assert_eq!(super::marquee_offset(99999, 0), 0);
    }

    #[test]
    fn marquee_offset_holds_at_start_for_one_second() {
        // At t < 1000ms with overflow=5, offset is 0 (hold-start phase).
        assert_eq!(super::marquee_offset(0, 5), 0);
        assert_eq!(super::marquee_offset(500, 5), 0);
        assert_eq!(super::marquee_offset(999, 5), 0);
    }

    #[test]
    fn marquee_offset_scrolls_one_char_per_step() {
        // From t=1000 to t=1000+5*250=2250 the offset ramps 0..5.
        assert_eq!(super::marquee_offset(1000, 5), 0);
        assert_eq!(super::marquee_offset(1250, 5), 1);
        assert_eq!(super::marquee_offset(1500, 5), 2);
        assert_eq!(super::marquee_offset(2249, 5), 4);
    }

    #[test]
    fn marquee_offset_holds_at_end() {
        // From t=2250 (after scroll completes) for 1000ms, offset = overflow.
        assert_eq!(super::marquee_offset(2250, 5), 5);
        assert_eq!(super::marquee_offset(2500, 5), 5);
        assert_eq!(super::marquee_offset(3249, 5), 5);
    }

    #[test]
    fn marquee_offset_cycles() {
        // At t = cycle_ms (3250) it wraps to 0 again.
        let cycle_ms = 1000 + 5 * 250 + 1000;
        assert_eq!(super::marquee_offset(cycle_ms, 5), 0);
        assert_eq!(super::marquee_offset(cycle_ms + 500, 5), 0);
    }

    #[test]
    fn selection_change_resets_marquee_start() {
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            None,
            None,
        );
        // Toggle to list mode so LineDown moves by 1 deterministically.
        app.handle(Action::ToggleViewMode);
        // Sleep briefly to ensure elapsed advances.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed_pre_nav = app.marquee_elapsed_ms();
        assert!(elapsed_pre_nav >= 10, "elapsed should reflect the sleep");
        // Navigate — should reset marquee_start.
        app.handle(Action::LineDown);
        let elapsed_post_nav = app.marquee_elapsed_ms();
        assert!(
            elapsed_post_nav < elapsed_pre_nav,
            "selection change should reset marquee_start; got pre={elapsed_pre_nav} post={elapsed_post_nav}"
        );
    }
}
