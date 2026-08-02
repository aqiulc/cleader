//! Lightweight library scanner: walks a directory for `.epub` files and
//! extracts just enough metadata (title + author) to render a selectable
//! list. Avoids the full `Book::open` cost (HTML walking, image
//! decoding) — opening 100 books up-front would be visibly slow.
//!
//! Only used when the user passes a directory to the CLI. Single-book
//! invocations (`cleader path/to/book.epub`) skip this module entirely.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LibraryEntry {
    pub path: PathBuf,
    pub title: String,
    pub author: String,
}

/// Scan a directory for `.epub` files. Returns entries sorted by title
/// (case-sensitive, locale-independent). Non-EPUBs are silently skipped.
/// Files that fail metadata extraction get filename-based fallbacks
/// (title from file stem, author "Unknown").
pub fn scan_directory(dir: &Path) -> std::io::Result<Vec<LibraryEntry>> {
    scan_directory_at_depth(dir, 0)
}

/// Scan a directory for `.epub` files, descending up to `depth` levels
/// of subdirectories. `depth == 0` scans the given directory only (no
/// recursion); `depth == 1` also includes its immediate subdirectories;
/// `usize::MAX` scans the whole tree recursively. Deeper books keep
/// their full path, so opening a result works unchanged regardless of
/// depth. Sort and metadata-fallback behavior match `scan_directory`.
pub fn scan_directory_at_depth(dir: &Path, depth: usize) -> std::io::Result<Vec<LibraryEntry>> {
    let mut entries: Vec<LibraryEntry> = Vec::new();
    // Explicit stack keeps the walk iterative — a very deep tree must
    // not overflow the call stack. Entries are `(dir, remaining_depth)`.
    let mut stack: Vec<(PathBuf, usize)> = vec![(dir.to_path_buf(), depth)];
    while let Some((current, remaining)) = stack.pop() {
        let read = std::fs::read_dir(&current)?;
        for dir_entry in read.flatten() {
            let path = dir_entry.path();
            if path.is_file() {
                if path.extension().and_then(|s| s.to_str()) == Some("epub") {
                    let (title, author) = extract_metadata(&path).unwrap_or_else(|| {
                        (
                            path.file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Untitled".into()),
                            "Unknown".into(),
                        )
                    });
                    entries.push(LibraryEntry { path, title, author });
                }
            } else if path.is_dir() && remaining > 0 {
                stack.push((path, remaining - 1));
            }
        }
    }
    entries.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(entries)
}

/// Open the EPUB just long enough to read Dublin Core title/creator.
/// Returns `None` if the file isn't a valid EPUB or metadata is absent
/// (in which case the caller falls back to the file stem).
fn extract_metadata(path: &Path) -> Option<(String, String)> {
    use epub::doc::EpubDoc;
    let doc = EpubDoc::new(path).ok()?;
    let title = doc.mdata("title").map(|m| m.value.clone())?;
    let author = doc
        .mdata("creator")
        .map(|m| m.value.clone())
        .unwrap_or_else(|| "Unknown".into());
    Some((title, author))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_skips_non_epub_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.txt"), "hi").unwrap();
        fs::write(dir.path().join("README.md"), "hi").unwrap();
        let entries = scan_directory(dir.path()).unwrap();
        assert!(entries.is_empty(), "should ignore non-EPUB files");
    }

    #[test]
    fn scan_returns_empty_for_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let entries = scan_directory(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_uses_filename_fallback_when_metadata_extraction_fails() {
        // A file with .epub extension but invalid contents (not a real EPUB)
        // should still appear in the listing, with the file stem as title.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("fake_book.epub"), "not really an epub").unwrap();
        let entries = scan_directory(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "fake_book");
        assert_eq!(entries[0].author, "Unknown");
    }

    #[test]
    fn scan_returns_entries_sorted_by_title() {
        // Three fake-EPUBs whose filenames sort differently than their
        // would-be titles. Since metadata extraction will fail (they're
        // not real EPUBs), the file stem becomes the title — so we're
        // testing sort-by-title on the stems.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("zeta.epub"), "x").unwrap();
        fs::write(dir.path().join("alpha.epub"), "x").unwrap();
        fs::write(dir.path().join("mu.epub"), "x").unwrap();
        let entries = scan_directory(dir.path()).unwrap();
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn scan_depth_zero_does_not_recurse() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("top.epub"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/nested.epub"), "x").unwrap();
        let entries = scan_directory(dir.path()).unwrap();
        let stems: Vec<String> = entries
            .iter()
            .map(|e| e.path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(stems, vec!["top"], "depth 0 must not find nested books");
    }

    #[test]
    fn scan_depth_one_includes_immediate_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("top.epub"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/child.epub"), "x").unwrap();
        fs::create_dir(dir.path().join("sub/deep")).unwrap();
        fs::write(dir.path().join("sub/deep/grandchild.epub"), "x").unwrap();
        let entries = scan_directory_at_depth(dir.path(), 1).unwrap();
        let stems: Vec<String> = entries
            .iter()
            .map(|e| e.path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            stems,
            vec!["child", "top"],
            "depth 1 must find direct children but not grandchildren"
        );
    }

    #[test]
    fn scan_depth_two_includes_grandchildren() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("top.epub"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/child.epub"), "x").unwrap();
        fs::create_dir(dir.path().join("sub/deep")).unwrap();
        fs::write(dir.path().join("sub/deep/grandchild.epub"), "x").unwrap();
        let entries = scan_directory_at_depth(dir.path(), 2).unwrap();
        let stems: Vec<String> = entries
            .iter()
            .map(|e| e.path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            stems,
            vec!["child", "grandchild", "top"],
            "depth 2 must reach grandchildren"
        );
    }

    #[test]
    fn scan_recursive_finds_all_nested_books() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("top.epub"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/child.epub"), "x").unwrap();
        fs::create_dir(dir.path().join("sub/deep")).unwrap();
        fs::write(dir.path().join("sub/deep/grandchild.epub"), "x").unwrap();
        let entries = scan_directory_at_depth(dir.path(), usize::MAX).unwrap();
        let stems: Vec<String> = entries
            .iter()
            .map(|e| e.path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(stems, vec!["child", "grandchild", "top"]);
    }

    #[test]
    fn scan_depth_keeps_full_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/book.epub"), "x").unwrap();
        let entries = scan_directory_at_depth(dir.path(), usize::MAX).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0].path.ends_with("sub/book.epub"),
            "nested entry must keep its full path, got {:?}",
            entries[0].path
        );
    }
}
