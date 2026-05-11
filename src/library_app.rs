//! Library-browsing app (separate event loop from the Reader).
//!
//! Active when the CLI path is a directory. Lists EPUBs found there,
//! lets the user pick one with arrow keys + Enter. On Enter, the app
//! sets `selected_path` and signals quit; main.rs then hands off to
//! the regular Reader with that book. On Esc/q/Ctrl+C, the app quits
//! with no selection.

use crate::input::Action;
use crate::library::LibraryEntry;
use std::path::PathBuf;

pub struct LibraryApp {
    entries: Vec<LibraryEntry>,
    selection: usize,
    viewport_size: (u16, u16),
    should_quit: bool,
    /// Set when the user presses Enter on a selection. The main.rs
    /// dispatcher reads this after the event loop exits to decide
    /// whether to launch the Reader.
    selected_path: Option<PathBuf>,
}

impl LibraryApp {
    pub fn new(entries: Vec<LibraryEntry>, viewport: (u16, u16)) -> Self {
        Self {
            entries,
            selection: 0,
            viewport_size: viewport,
            should_quit: false,
            selected_path: None,
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

    pub fn handle(&mut self, action: Action) {
        match action {
            Action::LineUp => {
                if self.selection > 0 {
                    self.selection -= 1;
                }
            }
            Action::LineDown => {
                if self.selection + 1 < self.entries.len() {
                    self.selection += 1;
                }
            }
            Action::PagePrev => {
                let step = self.lines_per_page().min(10);
                self.selection = self.selection.saturating_sub(step);
            }
            Action::PageNext => {
                let step = self.lines_per_page().min(10);
                let max = self.entries.len().saturating_sub(1);
                self.selection = (self.selection + step).min(max);
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

    fn entry(title: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(format!("/{title}.epub")),
            title: title.to_string(),
            author: "Anon".to_string(),
        }
    }

    #[test]
    fn line_down_moves_selection() {
        let mut app = LibraryApp::new(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
        );
        assert_eq!(app.selection(), 0);
        app.handle(Action::LineDown);
        assert_eq!(app.selection(), 1);
    }

    #[test]
    fn line_down_clamps_at_end() {
        let mut app = LibraryApp::new(vec![entry("A"), entry("B")], (80, 24));
        app.handle(Action::LineDown);
        app.handle(Action::LineDown);
        app.handle(Action::LineDown);
        assert_eq!(app.selection(), 1);
    }

    #[test]
    fn line_up_at_top_is_noop() {
        let mut app = LibraryApp::new(vec![entry("A"), entry("B")], (80, 24));
        app.handle(Action::LineUp);
        assert_eq!(app.selection(), 0);
    }

    #[test]
    fn confirm_sets_selected_path_and_should_quit() {
        let mut app = LibraryApp::new(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
        );
        app.handle(Action::LineDown);
        app.handle(Action::Confirm);
        assert!(app.should_quit());
        assert_eq!(app.selected_path(), Some(PathBuf::from("/B.epub").as_path()));
    }

    #[test]
    fn quit_sets_should_quit_without_selection() {
        let mut app = LibraryApp::new(vec![entry("A")], (80, 24));
        app.handle(Action::Quit);
        assert!(app.should_quit());
        assert!(app.selected_path().is_none());
    }

    #[test]
    fn confirm_on_empty_library_is_noop() {
        // Edge case: an empty library shouldn't be reachable (main.rs
        // exits with a clean error before launching), but be defensive.
        let mut app = LibraryApp::new(vec![], (80, 24));
        app.handle(Action::Confirm);
        assert!(!app.should_quit());
        assert!(app.selected_path().is_none());
    }

    #[test]
    fn page_next_advances_by_step() {
        let entries: Vec<LibraryEntry> = (0..50).map(|i| entry(&format!("E{i:02}"))).collect();
        let mut app = LibraryApp::new(entries, (80, 24));
        app.handle(Action::PageNext);
        assert_eq!(app.selection(), 10);
    }
}
