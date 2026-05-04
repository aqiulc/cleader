use cleader::epub::{Block, Book};
use std::path::PathBuf;

/// Returns the alphabetically-first `.epub` in `books/`, or `None` if the
/// directory is absent or contains no EPUBs. Tests that need a fixture
/// should `let Some(path) = first_test_book() else { return; };` and call
/// out the skip via `eprintln!` so it's visible in test output.
///
/// Contract: every `.epub` dropped into `books/` must be loadable by
/// `Book::open` (no deliberately-broken fixtures), or the happy-path tests
/// below will appear to fail for unrelated reasons.
fn first_test_book() -> Option<PathBuf> {
    let entries = std::fs::read_dir("books").ok()?;
    let mut paths: Vec<_> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x == "epub")
                .unwrap_or(false)
        })
        .collect();
    paths.sort_by_key(|e| e.file_name());
    paths.first().map(|e| e.path())
}

/// Fixture-presence guard. Returns the path or skips the test with a
/// visible `eprintln!`. CI without local fixtures sees these tests
/// reported as passing (no false-negative failures) but with a clear
/// "skipped" line in the output. Local dev with fixtures runs them.
fn require_book(path_hint: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = path_hint {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
        eprintln!("integration: skipping — fixture {p:?} not present");
        return None;
    }
    match first_test_book() {
        Some(p) => Some(p),
        None => {
            eprintln!("integration: skipping — no .epub in books/");
            None
        }
    }
}

#[test]
fn opens_a_real_epub_and_extracts_chapters() {
    let Some(path) = require_book(None) else { return; };
    let book = Book::open(&path).expect("should open a known-good EPUB");
    assert!(!book.chapters.is_empty(), "must extract at least one chapter");
    assert!(!book.title.trim().is_empty(), "title must not be empty");
}

#[test]
fn extracted_chapters_contain_paragraph_text() {
    let Some(path) = require_book(None) else { return; };
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

#[test]
fn toc_filtering_classifies_front_matter() {
    // Threshold's spine includes a cover; the cover is preserved in
    // book.chapters but classified as FrontMatter so it doesn't count
    // toward chapter numbering. Verifies the classifier correctly
    // identifies image-only chapters as FrontMatter.
    let Some(path) = require_book(Some("books/Threshold (Will Wight).epub")) else {
        return;
    };
    let book = Book::open(&path).unwrap();
    let any_front = book
        .chapters
        .iter()
        .any(|c| matches!(c.kind, cleader::epub::ChapterKind::FrontMatter));
    assert!(any_front, "expected at least one FrontMatter chapter (cover)");
    let any_main = book
        .chapters
        .iter()
        .any(|c| matches!(c.kind, cleader::epub::ChapterKind::Main));
    assert!(any_main, "expected at least one Main chapter");
}
