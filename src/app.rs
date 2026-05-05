use crate::epub::{Book, ChapterKind};
use crate::input::Action;
use crate::persistence::{Persistence, Position};
use crate::reader::{WrappedChapter, body_text_width, wrap_chapter};
use chrono::Utc;
use ratatui::text::Line;

pub struct App {
    book: Book,
    chapter_idx: usize,
    line_offset: usize,
    wrapped: WrappedChapter,
    viewport_size: (u16, u16),
    persistence: Persistence,
    should_quit: bool,
    /// Last persistence-flush error, displayed in the status bar until
    /// a subsequent flush succeeds. `None` = no warning to show. Replaces
    /// the previous `eprintln!`-into-alt-screen behavior, where the
    /// warning was eaten by the alternate screen and the user had no
    /// indication that their position wasn't being saved.
    save_error: Option<String>,
}

impl App {
    pub fn new(book: Book, mut persistence: Persistence, viewport: (u16, u16)) -> Self {
        // Invariant: Book::open returns EpubError::NoChapters for empty books,
        // so reaching here with no chapters is a programmer error elsewhere.
        debug_assert!(!book.chapters.is_empty(), "App requires at least one chapter");
        let key = book.registry_key();

        // Migration: if the new id-keyed entry doesn't exist, look for an
        // entry under the v0.1 path-based key. Copy it under the new id so
        // future flushes write to the right place. We don't remove the
        // legacy entry — orphan cleanup is a future concern.
        let mut migration_error: Option<String> = None;
        if persistence.get(&key).is_none() {
            let legacy_key = book.path.to_string_lossy().into_owned();
            if let Some(legacy_pos) = persistence.get(&legacy_key).cloned() {
                persistence.upsert(key.clone(), legacy_pos);
                // Make the migration durable now, independent of whether the
                // user later quits via 'q' (which would flush) or kills the
                // process abruptly.
                if let Err(e) = persistence.flush() {
                    migration_error = Some(format!("save failed: {e}"));
                }
            }
        }

        let (chapter_idx, line_offset) = match persistence.get(&key) {
            Some(p) if (p.chapter_idx as usize) < book.chapters.len() => {
                (p.chapter_idx as usize, p.line_offset as usize)
            }
            _ => (0, 0),
        };
        let wrapped = wrap_chapter(
            &book.chapters[chapter_idx].blocks,
            body_text_width(viewport.0),
        );
        let line_offset = line_offset.min(wrapped.len().saturating_sub(1));
        Self {
            book,
            chapter_idx,
            line_offset,
            wrapped,
            viewport_size: viewport,
            persistence,
            should_quit: false,
            save_error: migration_error,
        }
    }

    pub fn book(&self) -> &Book {
        &self.book
    }

    pub fn chapter_idx(&self) -> usize {
        self.chapter_idx
    }

    pub fn line_offset(&self) -> usize {
        self.line_offset
    }

    pub fn wrapped(&self) -> &[Line<'static>] {
        &self.wrapped.lines
    }

    /// Test-only accessor: source-char offset of the currently-top
    /// wrapped line. Used to verify resize preserves the user's
    /// position. Public via doc(hidden) so integration tests can
    /// reach it; not part of the documented public API.
    #[doc(hidden)]
    pub fn wrapped_source_offset_at_top(&self) -> usize {
        self.wrapped
            .source_offsets
            .get(self.line_offset)
            .copied()
            .unwrap_or(0)
    }

    pub fn viewport_size(&self) -> (u16, u16) {
        self.viewport_size
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    fn lines_per_page(&self) -> usize {
        // viewport.1 = rows; minus 1 for status bar.
        (self.viewport_size.1 as usize).saturating_sub(1).max(1)
    }

    pub fn page(&self) -> usize {
        self.line_offset / self.lines_per_page() + 1
    }

    pub fn total_pages(&self) -> usize {
        self.wrapped.len().div_ceil(self.lines_per_page()).max(1)
    }

    /// `Some((1-based main-chapter index, total main chapters))` if the
    /// current chapter is Main; `None` if it's FrontMatter.
    pub fn main_chapter_position(&self) -> Option<(usize, usize)> {
        let current_kind = self.book.chapters[self.chapter_idx].kind;
        if !matches!(current_kind, ChapterKind::Main) {
            return None;
        }
        let total = self
            .book
            .chapters
            .iter()
            .filter(|c| matches!(c.kind, ChapterKind::Main))
            .count();
        let current_1based = self
            .book
            .chapters
            .iter()
            .take(self.chapter_idx + 1)
            .filter(|c| matches!(c.kind, ChapterKind::Main))
            .count();
        Some((current_1based, total))
    }

    fn load_chapter(&mut self, idx: usize, line_offset: usize) {
        self.chapter_idx = idx;
        self.wrapped = wrap_chapter(
            &self.book.chapters[idx].blocks,
            body_text_width(self.viewport_size.0),
        );
        self.line_offset = line_offset.min(self.wrapped.len().saturating_sub(1));
    }

    fn current_position(&self) -> Position {
        Position {
            title: self.book.title.clone(),
            author: self.book.author.clone(),
            chapter_idx: self.chapter_idx as u32,
            line_offset: self.line_offset as u32,
            last_read: Utc::now(),
        }
    }

    fn save(&mut self) {
        let key = self.book.registry_key();
        let pos = self.current_position();
        self.persistence.upsert(key, pos);
        match self.persistence.flush() {
            Ok(()) => {
                // Successful flush clears any prior warning so the user
                // knows recent saves are getting through.
                self.save_error = None;
            }
            Err(e) => {
                self.save_error = Some(format!("save failed: {e}"));
            }
        }
    }

    /// Most recent save failure, if any. Cleared by the next successful
    /// flush. The renderer surfaces this in the status bar; see
    /// `eprintln!` was the v0.1 behavior, eaten by the alternate screen.
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    pub fn handle(&mut self, action: Action) {
        match action {
            Action::LineDown => self.line_down(),
            Action::LineUp => self.line_up(),
            Action::PageNext => {
                self.page_next();
                self.save();
            }
            Action::PagePrev => {
                self.page_prev();
                self.save();
            }
            Action::ChapterNext => {
                self.chapter_next();
                self.save();
            }
            Action::ChapterPrev => {
                self.chapter_prev();
                self.save();
            }
            Action::Resize(w, h) => self.resize(w, h),
            Action::Quit => {
                self.save();
                self.should_quit = true;
            }
        }
    }

    fn line_down(&mut self) {
        if self.line_offset + 1 < self.wrapped.len() {
            self.line_offset += 1;
            return;
        }
        // At end of current chapter; advance if possible.
        if self.chapter_idx + 1 < self.book.chapters.len() {
            self.load_chapter(self.chapter_idx + 1, 0);
        }
        // Otherwise: no-op (last line of last chapter).
    }

    fn line_up(&mut self) {
        if self.line_offset > 0 {
            self.line_offset -= 1;
            return;
        }
        // At start of chapter; go back if possible.
        if self.chapter_idx > 0 {
            let prev = self.chapter_idx - 1;
            // We'll set offset to last line after re-wrap.
            self.load_chapter(prev, usize::MAX);
        }
    }

    fn page_next(&mut self) {
        let step = self.lines_per_page();
        let new_offset = self.line_offset + step;
        if new_offset < self.wrapped.len() {
            self.line_offset = new_offset;
        } else if self.chapter_idx + 1 < self.book.chapters.len() {
            self.load_chapter(self.chapter_idx + 1, 0);
        }
        // Else: stay put — last page of last chapter.
    }

    fn page_prev(&mut self) {
        let step = self.lines_per_page();
        if self.line_offset >= step {
            self.line_offset -= step;
        } else if self.line_offset > 0 {
            self.line_offset = 0;
        } else if self.chapter_idx > 0 {
            self.load_chapter(self.chapter_idx - 1, usize::MAX);
        }
    }

    fn chapter_next(&mut self) {
        if self.chapter_idx + 1 < self.book.chapters.len() {
            self.load_chapter(self.chapter_idx + 1, 0);
        }
    }

    fn chapter_prev(&mut self) {
        if self.chapter_idx > 0 {
            self.load_chapter(self.chapter_idx - 1, 0);
        }
    }

    fn resize(&mut self, w: u16, h: u16) {
        if (w, h) == self.viewport_size {
            return;
        }
        let width_changed = w != self.viewport_size.0;
        self.viewport_size = (w, h);
        if width_changed {
            // Save the source-char offset of the currently-visible top line
            // so we can land on the same content after rewrap.
            let target_source = self.wrapped_source_offset_at_top();
            self.wrapped = wrap_chapter(
                &self.book.chapters[self.chapter_idx].blocks,
                body_text_width(self.viewport_size.0),
            );
            self.line_offset = self
                .wrapped
                .find_line_for_source(target_source)
                .unwrap_or(0)
                .min(self.wrapped.len().saturating_sub(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub::{Block, BookId, Chapter, Span};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn book_with_chapters(chapters: Vec<Vec<Block>>) -> Book {
        let chs = chapters
            .into_iter()
            .map(|blocks| Chapter { title: None, blocks, kind: ChapterKind::Main })
            .collect();
        let path = PathBuf::from("/test/book.epub");
        Book {
            id: BookId::from_bytes(path.to_string_lossy().as_bytes()),
            title: "T".into(),
            author: "A".into(),
            path,
            chapters: chs,
        }
    }

    fn p(text: &str) -> Block {
        Block::Paragraph { spans: vec![Span::plain(text)] }
    }

    fn p_main(text: &str) -> Block {
        Block::Paragraph { spans: vec![Span::plain(text)] }
    }

    fn p_image(alt: &str) -> Block {
        Block::Paragraph {
            spans: vec![Span::plain(format!("[image: {alt}]"))],
        }
    }

    fn book_with_kinds(specs: Vec<(Vec<Block>, ChapterKind)>) -> Book {
        let chs = specs
            .into_iter()
            .map(|(blocks, kind)| Chapter { title: None, blocks, kind })
            .collect();
        let path = PathBuf::from("/test/book.epub");
        Book {
            id: BookId::from_bytes(path.to_string_lossy().as_bytes()),
            title: "T".into(),
            author: "A".into(),
            path,
            chapters: chs,
        }
    }

    fn fresh_persistence() -> (Persistence, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        (Persistence::open_at(path), dir)
    }

    #[test]
    fn new_app_starts_at_chapter_zero_when_no_saved_position() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p("hello")]]);
        let app = App::new(book, p_handle, (80, 24));
        assert_eq!(app.chapter_idx(), 0);
        assert_eq!(app.line_offset(), 0);
    }

    #[test]
    fn new_app_restores_saved_position() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        {
            let mut p_handle = Persistence::open_at(path.clone());
            p_handle.upsert(
                "/test/book.epub".into(),
                Position {
                    title: "T".into(),
                    author: "A".into(),
                    chapter_idx: 1,
                    line_offset: 0,
                    last_read: Utc::now(),
                },
            );
            p_handle.flush().unwrap();
        }
        let p_handle = Persistence::open_at(path);
        let book = book_with_chapters(vec![vec![p("a")], vec![p("b")]]);
        let app = App::new(book, p_handle, (80, 24));
        assert_eq!(app.chapter_idx(), 1);
    }

    #[test]
    fn new_app_falls_back_to_zero_when_chapter_idx_out_of_range() {
        // Registry knows about a book that's since been re-encoded with
        // fewer chapters. Stale chapter_idx must not crash.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        {
            let mut p_handle = Persistence::open_at(path.clone());
            p_handle.upsert(
                "/test/book.epub".into(),
                Position {
                    title: "T".into(),
                    author: "A".into(),
                    chapter_idx: 99,
                    line_offset: 50,
                    last_read: Utc::now(),
                },
            );
            p_handle.flush().unwrap();
        }
        let p_handle = Persistence::open_at(path);
        let book = book_with_chapters(vec![vec![p("only one")]]);
        let app = App::new(book, p_handle, (80, 24));
        assert_eq!(app.chapter_idx(), 0);
        assert_eq!(app.line_offset(), 0);
    }

    #[test]
    fn new_app_clamps_line_offset_when_chapter_shrank() {
        // Registry has a line offset that's larger than the new wrapped
        // chapter. Clamp to the last available line rather than panic.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        {
            let mut p_handle = Persistence::open_at(path.clone());
            p_handle.upsert(
                "/test/book.epub".into(),
                Position {
                    title: "T".into(),
                    author: "A".into(),
                    chapter_idx: 0,
                    line_offset: 9999,
                    last_read: Utc::now(),
                },
            );
            p_handle.flush().unwrap();
        }
        let p_handle = Persistence::open_at(path);
        let book = book_with_chapters(vec![vec![p("short")]]);
        let app = App::new(book, p_handle, (80, 24));
        // Wrapped chapter has at most a few lines; offset must be clamped
        // to wrapped.len()-1.
        assert!(app.line_offset() < app.wrapped().len().max(1));
    }

    #[test]
    fn page_and_total_pages_default_to_one_for_empty_chapter() {
        // A chapter with nothing to wrap should render as "Page 1 / 1"
        // rather than "Page 1 / 0" (the .max(1) floor on total_pages).
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![Block::Blank]]);
        let app = App::new(book, p_handle, (80, 24));
        assert_eq!(app.page(), 1);
        assert_eq!(app.total_pages(), 1);
    }

    #[test]
    fn line_down_increments_within_chapter() {
        let (p_handle, _dir) = fresh_persistence();
        // Create a chapter with enough content to span 3+ wrapped lines.
        let blocks = vec![p("aaa bbb ccc"), p("ddd eee fff"), p("ggg hhh iii")];
        let book = book_with_chapters(vec![blocks]);
        let mut app = App::new(book, p_handle, (80, 24));
        let start = app.line_offset();
        app.handle(Action::LineDown);
        assert_eq!(app.line_offset(), start + 1);
    }

    #[test]
    fn line_down_at_chapter_end_advances_to_next_chapter() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p("ch1")], vec![p("ch2")]]);
        let mut app = App::new(book, p_handle, (80, 24));
        // Walk to the last line of chapter 0.
        while app.line_offset() + 1 < app.wrapped().len() {
            app.handle(Action::LineDown);
        }
        let chap_count_before = app.chapter_idx();
        assert_eq!(chap_count_before, 0);
        app.handle(Action::LineDown);
        assert_eq!(app.chapter_idx(), 1);
        assert_eq!(app.line_offset(), 0);
    }

    #[test]
    fn line_down_at_last_line_of_last_chapter_is_noop() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p("only")]]);
        let mut app = App::new(book, p_handle, (80, 24));
        while app.line_offset() + 1 < app.wrapped().len() {
            app.handle(Action::LineDown);
        }
        let before = (app.chapter_idx(), app.line_offset());
        app.handle(Action::LineDown);
        assert_eq!((app.chapter_idx(), app.line_offset()), before);
    }

    #[test]
    fn line_up_at_chapter_start_goes_to_previous_chapter_last_line() {
        // First chapter must wrap to multiple lines, otherwise the
        // "last line" and "first line" indices coincide and the test
        // would pass whether or not the usize::MAX clamping actually
        // works.
        let long = "alpha bravo charlie delta echo foxtrot golf hotel \
            india juliet kilo lima mike november oscar papa quebec \
            romeo sierra tango uniform victor whiskey xray yankee zulu";
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p(long)], vec![p("ch2")]]);
        let mut app = App::new(book, p_handle, (40, 24));
        // The long first chapter must produce multiple wrapped lines
        // at width 40 (after 3-col left pad → 37 cols of body); 26
        // greek-alphabet words at ~6 cols each won't fit in one line.
        let chapter_zero_last_line = {
            // Move to chapter 1 by repeated LineDown.
            while app.chapter_idx() == 0 {
                app.handle(Action::LineDown);
            }
            assert_eq!(app.chapter_idx(), 1);
            assert_eq!(app.line_offset(), 0);
            // Now go back to chapter 0; we should land on its last line.
            app.handle(Action::LineUp);
            assert_eq!(app.chapter_idx(), 0);
            app.line_offset()
        };
        assert_eq!(
            chapter_zero_last_line,
            app.wrapped().len() - 1,
            "LineUp at chapter start should land on chapter 0's last line"
        );
        assert!(app.wrapped().len() > 1, "test fixture must wrap to multiple lines");
    }

    #[test]
    fn quit_sets_should_quit_flag() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p("x")]]);
        let mut app = App::new(book, p_handle, (80, 24));
        assert!(!app.should_quit());
        app.handle(Action::Quit);
        assert!(app.should_quit());
    }

    #[test]
    fn page_next_advances_by_lines_per_page() {
        let (p_handle, _dir) = fresh_persistence();
        // Long content so we have multiple pages.
        let mut blocks = Vec::new();
        for _ in 0..50 {
            blocks.push(p("the quick brown fox jumps over the lazy dog"));
        }
        let book = book_with_chapters(vec![blocks]);
        let mut app = App::new(book, p_handle, (80, 24));
        let lines_per_page = 23;
        app.handle(Action::PageNext);
        assert_eq!(app.line_offset(), lines_per_page);
    }

    #[test]
    fn page_next_past_chapter_end_advances_chapter() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p("short")], vec![p("next")]]);
        let mut app = App::new(book, p_handle, (80, 24));
        app.handle(Action::PageNext);
        assert_eq!(app.chapter_idx(), 1);
        assert_eq!(app.line_offset(), 0);
    }

    #[test]
    fn page_prev_at_start_of_chapter_goes_to_previous_chapter() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p("ch1")], vec![p("ch2")]]);
        let mut app = App::new(book, p_handle, (80, 24));
        app.handle(Action::ChapterNext);
        assert_eq!(app.chapter_idx(), 1);
        app.handle(Action::PagePrev);
        assert_eq!(app.chapter_idx(), 0);
    }

    #[test]
    fn chapter_next_loads_next_chapter() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p("a")], vec![p("b")], vec![p("c")]]);
        let mut app = App::new(book, p_handle, (80, 24));
        app.handle(Action::ChapterNext);
        assert_eq!(app.chapter_idx(), 1);
        app.handle(Action::ChapterNext);
        assert_eq!(app.chapter_idx(), 2);
        app.handle(Action::ChapterNext);
        assert_eq!(app.chapter_idx(), 2); // no-op at end
    }

    #[test]
    fn resize_changes_viewport_and_rewraps_when_width_changes() {
        let (p_handle, _dir) = fresh_persistence();
        let blocks = vec![p("the quick brown fox jumps over the lazy dog")];
        let book = book_with_chapters(vec![blocks]);
        let mut app = App::new(book, p_handle, (80, 24));
        let lines_at_80 = app.wrapped().len();
        app.handle(Action::Resize(40, 24));
        let lines_at_40 = app.wrapped().len();
        assert!(lines_at_40 >= lines_at_80, "narrower terminal should wrap to more lines");
        assert_eq!(app.viewport_size(), (40, 24));
    }

    #[test]
    fn resize_preserves_viewport_source_position() {
        // User is mid-chapter at width 80. Resize to width 40 narrows
        // the wrap; their viewport's top line should still correspond
        // to the same source position (the same paragraph and roughly
        // the same point within it).
        let (p_handle, _dir) = fresh_persistence();
        // Make a chapter with enough content to span many wrapped lines.
        let mut blocks = Vec::new();
        for _ in 0..30 {
            blocks.push(p(
                "the quick brown fox jumps over the lazy dog repeatedly today and tomorrow"
            ));
        }
        let book = book_with_chapters(vec![blocks]);
        let mut app = App::new(book, p_handle, (80, 24));
        // Scroll some distance from the top so the viewport isn't at line 0.
        for _ in 0..15 {
            app.handle(Action::PageNext);
        }
        let pre_source = app.wrapped_source_offset_at_top();

        // Resize to a narrower terminal.
        app.handle(Action::Resize(40, 24));

        let post_source = app.wrapped_source_offset_at_top();
        // The new top-line should correspond to a source position
        // at-or-before the saved one (and as close to it as possible).
        assert!(
            post_source <= pre_source,
            "post-resize top should be at or before saved source ({post_source} > {pre_source})"
        );
        // It should be reasonably close — within one wrapped paragraph's
        // worth of source chars (a paragraph here is ~75 chars). If the
        // implementation snapped all the way to the start of the chapter
        // (e.g. always returning 0), this would fail.
        assert!(
            pre_source - post_source < 100,
            "post-resize top drifted too far ({} chars before)",
            pre_source - post_source
        );
    }

    #[test]
    fn resize_height_only_does_not_rewrap() {
        let (p_handle, _dir) = fresh_persistence();
        let blocks = vec![p("hello world")];
        let book = book_with_chapters(vec![blocks]);
        let mut app = App::new(book, p_handle, (80, 24));
        let lines_before = app.wrapped().len();
        app.handle(Action::Resize(80, 50));
        let lines_after = app.wrapped().len();
        assert_eq!(lines_before, lines_after);
        assert_eq!(app.viewport_size(), (80, 50));
    }

    #[test]
    fn main_chapter_position_returns_none_for_front_matter() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_kinds(vec![
            (vec![p_image("cover")], ChapterKind::FrontMatter),
            (vec![p_main("ch1")], ChapterKind::Main),
        ]);
        let app = App::new(book, p_handle, (80, 24));
        assert_eq!(app.chapter_idx(), 0);
        assert!(app.main_chapter_position().is_none());
    }

    #[test]
    fn main_chapter_position_counts_only_main_chapters() {
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_kinds(vec![
            (vec![p_image("cover")], ChapterKind::FrontMatter),
            (vec![p_main("ch1")], ChapterKind::Main),
            (vec![p_main("ch2")], ChapterKind::Main),
            (vec![p_main("ch3")], ChapterKind::Main),
        ]);
        let mut app = App::new(book, p_handle, (80, 24));
        // Walk to chapter 1 (the first Main chapter).
        app.handle(Action::ChapterNext);
        assert_eq!(app.chapter_idx(), 1);
        assert_eq!(app.main_chapter_position(), Some((1, 3)));
        // Walk to chapter 3 (the last).
        app.handle(Action::ChapterNext);
        app.handle(Action::ChapterNext);
        assert_eq!(app.chapter_idx(), 3);
        assert_eq!(app.main_chapter_position(), Some((3, 3)));
    }

    #[test]
    fn page_next_persists_position() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let p_handle = Persistence::open_at(path.clone());
        let mut blocks = Vec::new();
        for _ in 0..50 {
            blocks.push(p("the quick brown fox jumps"));
        }
        let book = book_with_chapters(vec![blocks]);
        let key = book.registry_key();
        let mut app = App::new(book, p_handle, (80, 24));
        app.handle(Action::PageNext);
        // Re-open the persistence handle and verify the offset was saved
        // under the id-derived registry key (not the legacy path key).
        let reopened = Persistence::open_at(path);
        let pos = reopened.get(&key).expect("position saved");
        assert!(pos.line_offset > 0);
    }

    #[test]
    fn new_app_migrates_from_legacy_path_key() {
        // Simulate a v0.1 registry with the position keyed by absolute
        // path. The new App must find and use it. The legacy entry
        // remains on disk (no remove() yet — future cleanup task).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let book_path = "/test/book.epub";
        {
            let mut p_handle = Persistence::open_at(path.clone());
            p_handle.upsert(
                book_path.into(),
                Position {
                    title: "T".into(),
                    author: "A".into(),
                    chapter_idx: 1,
                    line_offset: 0,
                    last_read: Utc::now(),
                },
            );
            p_handle.flush().unwrap();
        }
        let p_handle = Persistence::open_at(path.clone());
        let book = book_with_chapters(vec![vec![p("a")], vec![p("b")]]);
        let new_id_key = book.registry_key();
        let legacy_key = book.path.to_string_lossy().into_owned();
        let app = App::new(book, p_handle, (80, 24));
        assert_eq!(app.chapter_idx(), 1, "should restore from legacy path key");

        // Migration is durable: re-open the registry and confirm both
        // the new id-key (copied during migration) and the legacy
        // path-key (left in place) are present.
        let reopened = Persistence::open_at(path);
        assert!(
            reopened.get(&new_id_key).is_some(),
            "id-key entry should be on disk after migration"
        );
        assert!(
            reopened.get(&legacy_key).is_some(),
            "legacy path-key entry should remain (orphan cleanup is a future task)"
        );
    }

    #[test]
    fn new_app_prefers_id_key_when_both_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let book = book_with_chapters(vec![vec![p("a")], vec![p("b")], vec![p("c")]]);
        let id_key = book.registry_key();
        let legacy_key = book.path.to_string_lossy().into_owned();
        {
            let mut p_handle = Persistence::open_at(path.clone());
            // Old (stale) entry under legacy key — points to chapter 0.
            p_handle.upsert(
                legacy_key,
                Position {
                    title: "T".into(),
                    author: "A".into(),
                    chapter_idx: 0,
                    line_offset: 0,
                    last_read: Utc::now(),
                },
            );
            // New (current) entry under id key — points to chapter 2.
            p_handle.upsert(
                id_key,
                Position {
                    title: "T".into(),
                    author: "A".into(),
                    chapter_idx: 2,
                    line_offset: 0,
                    last_read: Utc::now(),
                },
            );
            p_handle.flush().unwrap();
        }
        let p_handle = Persistence::open_at(path);
        let app = App::new(book, p_handle, (80, 24));
        assert_eq!(app.chapter_idx(), 2, "should prefer the id-keyed entry");
    }

    #[test]
    fn new_app_with_no_saved_entries_starts_at_zero() {
        // Sanity: neither id-key nor legacy-key present → fresh start.
        let (p_handle, _dir) = fresh_persistence();
        let book = book_with_chapters(vec![vec![p("a")]]);
        let app = App::new(book, p_handle, (80, 24));
        assert_eq!(app.chapter_idx(), 0);
        assert_eq!(app.line_offset(), 0);
    }

    #[test]
    fn save_error_starts_none_and_stays_none_on_successful_flush() {
        // The negative path (forced flush failure) is hard to test
        // portably without mocking the filesystem. Pin the positive
        // contract: a fresh App reports no save_error, and a successful
        // PageNext (which flushes) keeps it None.
        let (p_handle, _dir) = fresh_persistence();
        let mut blocks = Vec::new();
        for _ in 0..30 {
            blocks.push(p("the quick brown fox"));
        }
        let book = book_with_chapters(vec![blocks]);
        let mut app = App::new(book, p_handle, (80, 24));
        assert!(app.save_error().is_none(), "fresh App has no save error");
        app.handle(Action::PageNext);
        assert!(
            app.save_error().is_none(),
            "successful flush should leave save_error as None"
        );
    }
}
