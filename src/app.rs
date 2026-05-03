use crate::epub::Book;
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
    #[allow(dead_code)] // used by handle() in Task 19/20
    persistence: Persistence,
    should_quit: bool,
}

impl App {
    pub fn new(book: Book, persistence: Persistence, viewport: (u16, u16)) -> Self {
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

    #[allow(dead_code)] // used by handle() in Task 19/20
    fn load_chapter(&mut self, idx: usize, line_offset: usize) {
        self.chapter_idx = idx;
        self.wrapped = wrap_chapter(
            &self.book.chapters[idx].blocks,
            body_text_width(self.viewport_size.0),
        );
        self.line_offset = line_offset.min(self.wrapped.len().saturating_sub(1));
    }

    #[allow(dead_code)] // used by handle() in Task 19/20
    fn current_position(&self) -> Position {
        Position {
            title: self.book.title.clone(),
            author: self.book.author.clone(),
            chapter_idx: self.chapter_idx as u32,
            line_offset: self.line_offset as u32,
            last_read: Utc::now(),
        }
    }

    #[allow(dead_code)] // used by handle() in Task 19/20
    fn save(&mut self) {
        let key = self.book.path.to_string_lossy().into_owned();
        let pos = self.current_position();
        self.persistence.upsert(key, pos);
        if let Err(e) = self.persistence.flush() {
            eprintln!("cleader: warning: could not save position ({e})");
        }
    }

    pub fn handle(&mut self, action: Action) {
        // Implemented in following tasks.
        let _ = action;
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
            .map(|blocks| Chapter { title: None, blocks })
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
}
