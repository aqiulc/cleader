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
    let book = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH)
        .expect("should open a known-good EPUB");
    assert!(!book.chapters.is_empty(), "must extract at least one chapter");
    assert!(!book.title.trim().is_empty(), "title must not be empty");
    assert!(!book.id.as_str().is_empty(), "book id must not be empty");
}

#[test]
fn opening_same_epub_twice_yields_same_id() {
    let Some(path) = require_book(None) else { return; };
    let book1 = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH).unwrap();
    let book2 = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH).unwrap();
    assert_eq!(book1.id, book2.id);
}

#[test]
fn extracted_chapters_contain_paragraph_text() {
    let Some(path) = require_book(None) else { return; };
    let book = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH).unwrap();
    let any_paragraph = book
        .chapters
        .iter()
        .flat_map(|c| c.blocks.iter())
        .any(|b| matches!(b, Block::Paragraph { spans } if !spans.is_empty()));
    assert!(any_paragraph, "should find at least one non-empty paragraph");
}

#[test]
fn missing_path_returns_not_found_error() {
    let result = Book::open("/no/such/file.epub", cleader::reader::DEFAULT_MAX_BODY_WIDTH);
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
    let book = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH).unwrap();
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

/// Pinned shape of `books/Threshold (Will Wight).epub` — fails loudly if
/// the TOC parser regresses or the EPUB is replaced with a different one.
/// Numbers determined empirically against the v0.1 implementation; if
/// these change, investigate before bumping.
#[test]
fn threshold_known_structure() {
    let Some(path) = require_book(Some("books/Threshold (Will Wight).epub")) else {
        return;
    };
    let book = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH).unwrap();
    assert_eq!(book.title, "Threshold");
    assert_eq!(book.author, "Will Wight");
    assert_eq!(book.chapters.len(), 20, "total chapters (incl. cover)");

    let main = book
        .chapters
        .iter()
        .filter(|c| matches!(c.kind, cleader::epub::ChapterKind::Main))
        .count();
    let front = book
        .chapters
        .iter()
        .filter(|c| matches!(c.kind, cleader::epub::ChapterKind::FrontMatter))
        .count();
    assert_eq!(main, 19, "main chapters");
    assert_eq!(front, 1, "front matter (cover only)");

    // First chapter is the cover (FrontMatter, image-only).
    assert!(matches!(book.chapters[0].kind, cleader::epub::ChapterKind::FrontMatter));
    assert_eq!(book.chapters[0].title.as_deref(), Some("Threshold"));

    // Real first reading chapter is "The First Uncrowned King" at index 3.
    assert_eq!(
        book.chapters[3].title.as_deref(),
        Some("The First Uncrowned King")
    );
}

#[test]
fn threshold_cover_is_rendered_as_ascii_art() {
    use cleader::epub::{Block, ChapterKind};
    let Some(path) = require_book(Some("books/Threshold (Will Wight).epub")) else {
        return;
    };
    let book = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH).unwrap();
    let cover = book
        .chapters
        .iter()
        .find(|c| matches!(c.kind, ChapterKind::FrontMatter))
        .expect("Threshold should have a FrontMatter chapter (cover)");
    let has_image_block = cover
        .blocks
        .iter()
        .any(|b| matches!(b, Block::Image(_)));
    assert!(
        has_image_block,
        "Threshold cover should be rendered as Block::Image (ASCII art)"
    );
}

#[test]
fn books_with_inline_images_render_them_as_ascii_art() {
    use cleader::epub::{Block, ChapterKind};
    let Some(path) = require_book(None) else { return; };
    let book = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH).unwrap();

    // Count Block::Image inside Main chapters (not the cover).
    let main_image_count: usize = book
        .chapters
        .iter()
        .filter(|c| matches!(c.kind, ChapterKind::Main))
        .flat_map(|c| c.blocks.iter())
        .filter(|b| matches!(b, Block::Image(_)))
        .count();

    // Don't assert non-zero — books may or may not have inline images.
    // The point of this test is: if ANY are present, the pipeline didn't
    // crash and the variant is reachable end-to-end. A `cargo test --
    // --nocapture` run shows the count.
    eprintln!(
        "integration: {main_image_count} inline images in {:?}",
        path.file_name().unwrap()
    );
}

/// End-to-end pipeline: load a real EPUB → wrap a real chapter → verify
/// the wrap output is non-empty and respects the width contract. The
/// existing reader unit tests use synthetic blocks; this proves the full
/// chain (EpubDoc → html_to_blocks → wrap_chapter) survives real input.
#[test]
fn wrap_pipeline_on_real_epub_respects_width() {
    use unicode_width::UnicodeWidthStr;
    let Some(path) = require_book(None) else { return; };
    let book = Book::open(&path, cleader::reader::DEFAULT_MAX_BODY_WIDTH).unwrap();
    // Pick the first Main chapter with substantial content (avoids a tiny
    // copyright page producing a misleading single-line wrap).
    let chapter = book
        .chapters
        .iter()
        .filter(|c| matches!(c.kind, cleader::epub::ChapterKind::Main))
        .find(|c| c.blocks.len() > 50)
        .expect("at least one substantive Main chapter");

    let width = 80u16;
    let wrapped = cleader::reader::wrap_chapter(&chapter.blocks, width);
    assert!(!wrapped.is_empty(), "wrap of substantive chapter should produce lines");

    // Every line must fit within the requested width (allowing an overflow
    // only for words that are themselves longer than width — wrap_chapter's
    // documented v1 long-word policy).
    for (idx, line) in wrapped.lines.iter().enumerate() {
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let cols = UnicodeWidthStr::width(text.as_str());
        if cols > width as usize {
            // Tolerated only if this line contains exactly one word longer
            // than width (the long-word overflow case).
            let words: Vec<&str> = text.split_whitespace().collect();
            assert_eq!(
                words.len(),
                1,
                "line {idx} exceeds width but has {} words: {:?}",
                words.len(),
                text
            );
            assert!(
                UnicodeWidthStr::width(words[0]) > width as usize,
                "line {idx} exceeds width without containing a single long word: {:?}",
                text
            );
        }
    }
}

#[test]
fn library_grid_renders_without_panic_when_directory_has_epubs() {
    use cleader::cover_cache::CoverCache;
    use cleader::library::scan_directory;
    use cleader::library_app::LibraryApp;
    use cleader::prefs::{PrefsStore, ViewMode};
    use cleader::reader::{render_library, LibraryRenderInput};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let Some(book_path) = require_book(None) else { return; };
    // Set up a tmp library dir with one real EPUB symlinked in.
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("book.epub");
    std::fs::copy(&book_path, &dest).unwrap();

    let entries = scan_directory(dir.path()).expect("scan");
    assert!(!entries.is_empty(), "scan should find at least one EPUB");

    let prefs_dir = tempfile::tempdir().unwrap();
    let prefs = PrefsStore::open_at(prefs_dir.path().join("prefs.json"));
    let cache_dir = tempfile::tempdir().unwrap();
    let cover_cache = CoverCache::open_at(cache_dir.path().to_path_buf());

    let mut app = LibraryApp::new_with(entries, (80, 40), Some(prefs), Some(cover_cache));
    app.request_visible_covers(0..1);
    // Give the worker a moment so the cover may land in the cache.
    for _ in 0..30 {
        if let Some(cache) = app.cover_cache_mut() {
            if cache.drain_finished() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let backend = TestBackend::new(80, 40);
    let mut term = Terminal::new(backend).unwrap();
    let entries_snapshot: Vec<_> = app.entries().to_vec();
    let book_ids_snapshot: Vec<Option<cleader::epub::BookId>> = (0..app.entries().len())
        .map(|i| app.book_id(i).cloned())
        .collect();
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
            },
        );
    })
    .unwrap();

    // We don't assert specific glyphs — the cover may or may not have
    // landed in 600ms depending on system load. We're testing that the
    // full path (scan → BookId → enqueue → render) doesn't panic.
}
