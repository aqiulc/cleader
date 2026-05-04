use crate::epub::{Book, ChapterKind};
use crate::input::Action;
use crate::persistence::{Persistence, Position};
use crate::reader::{body_text_width, wrap_chapter};
use chrono::Utc;
use ratatui::text::Line;

pub struct App {
    book: Book,
    chapter_idx: usize,
    line_offset: usize,
    wrapped: Vec<Line<'static>>,
    viewport_size: (u16, u16),
    persistence: Persistence,
    should_quit: bool,
}

impl App {
    pub fn new(book: Book, persistence: Persistence, viewport: (u16, u16)) -> Self {
        // Invariant: Book::open returns EpubError::NoChapters for empty books,
        // so reaching here with no chapters is a programmer error elsewhere.
        debug_assert!(!book.chapters.is_empty(), "App requires at least one chapter");
        let key = book.path.to_string_lossy().into_owned();
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
        &self.wrapped
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
        let key = self.book.path.to_string_lossy().into_owned();
        let pos = self.current_position();
        self.persistence.upsert(key, pos);
        if let Err(e) = self.persistence.flush() {
            eprintln!("cleader: warning: could not save position ({e})");
        }
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
            self.wrapped = wrap_chapter(
                &self.book.chapters[self.chapter_idx].blocks,
                body_text_width(self.viewport_size.0),
            );
            self.line_offset = self
                .line_offset
                .min(self.wrapped.len().saturating_sub(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub::{Block, Chapter, Span};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn book_with_chapters(chapters: Vec<Vec<Block>>) -> Book {
        let chs = chapters
            .into_iter()
            .map(|blocks| Chapter { title: None, blocks, kind: ChapterKind::Main })
            .collect();
        Book {
            title: "T".into(),
            author: "A".into(),
            path: PathBuf::from("/test/book.epub"),
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
        Book {
            title: "T".into(),
            author: "A".into(),
            path: PathBuf::from("/test/book.epub"),
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
        let mut app = App::new(book, p_handle, (80, 24));
        app.handle(Action::PageNext);
        // Re-open the persistence handle and verify the offset was saved.
        let reopened = Persistence::open_at(path);
        let pos = reopened.get("/test/book.epub").expect("position saved");
        assert!(pos.line_offset > 0);
    }
}
