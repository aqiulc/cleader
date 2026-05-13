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
use std::path::PathBuf;

pub struct LibraryApp {
    entries: Vec<LibraryEntry>,
    /// Parallel to `entries`: `book_ids[i]` is the BookId for `entries[i]`,
    /// computed lazily on first call to `request_visible_covers(i)`.
    /// Length always equals `entries.len()`.
    book_ids: Vec<Option<BookId>>,
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
        Self {
            entries,
            book_ids,
            selection: 0,
            viewport_size: viewport,
            should_quit: false,
            selected_path: None,
            view_mode,
            cover_cache,
            prefs,
            save_error: None,
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

    /// Look up an already-computed BookId for an entry. Returns None if
    /// `request_visible_covers` hasn't been called for this index yet or
    /// if the file couldn't be read.
    #[doc(hidden)]
    pub fn book_id(&self, idx: usize) -> Option<&BookId> {
        self.book_ids.get(idx).and_then(|opt| opt.as_ref())
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
        let cell_width = crate::reader::CELL_WIDTH as usize;
        (self.viewport_size.0 as usize / cell_width).max(1)
    }

    pub fn handle(&mut self, action: Action) {
        match action {
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
                    let max = self.entries.len().saturating_sub(1);
                    self.selection = (self.selection + cols).min(max);
                } else if self.selection + 1 < self.entries.len() {
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
                    let max = self.entries.len().saturating_sub(1);
                    if self.selection < max {
                        self.selection += 1;
                    }
                } else {
                    let step = self.lines_per_page().min(10);
                    let max = self.entries.len().saturating_sub(1);
                    self.selection = (self.selection + step).min(max);
                }
            }
            Action::Confirm => {
                if let Some(entry) = self.entries.get(self.selection) {
                    self.selected_path = Some(entry.path.clone());
                    self.should_quit = true;
                }
            }
            Action::Quit => {
                self.should_quit = true;
            }
            Action::Resize(w, h) => {
                self.viewport_size = (w, h);
            }
            Action::ToggleViewMode => {
                self.toggle_view_mode();
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
    use std::path::Path;

    fn entry(title: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(format!("/{title}.epub")),
            title: title.to_string(),
            author: "Anon".to_string(),
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
}
