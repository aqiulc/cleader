use cleader::epub::{Block, Book};
use std::path::PathBuf;

/// Returns the alphabetically-first `.epub` in `books/`.
///
/// Contract: every `.epub` dropped into `books/` must be loadable by
/// `Book::open` (no deliberately-broken fixtures), or the happy-path tests
/// below will appear to fail for unrelated reasons.
fn first_test_book() -> PathBuf {
    let mut entries: Vec<_> = std::fs::read_dir("books")
        .expect("books/ folder should exist")
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x == "epub")
                .unwrap_or(false)
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());
    entries
        .first()
        .map(|e| e.path())
        .expect("at least one EPUB in books/")
}

#[test]
fn opens_a_real_epub_and_extracts_chapters() {
    let path = first_test_book();
    let book = Book::open(&path).expect("should open a known-good EPUB");
    assert!(!book.chapters.is_empty(), "must extract at least one chapter");
    assert!(!book.title.trim().is_empty(), "title must not be empty");
}

#[test]
fn extracted_chapters_contain_paragraph_text() {
    let path = first_test_book();
    let book = Book::open(&path).unwrap();
    let any_paragraph = book
        .chapters
        .iter()
        .flat_map(|c| c.blocks.iter())
        .any(|b| matches!(b, Block::Paragraph { spans } if !spans.is_empty()));
    assert!(any_paragraph, "should find at least one non-empty paragraph");
}

#[test]
fn missing_path_returns_not_found_error() {
    let result = Book::open("/no/such/file.epub");
    assert!(result.is_err());
}
