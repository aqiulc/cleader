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
    let read = std::fs::read_dir(dir)?;
    let mut entries: Vec<LibraryEntry> = Vec::new();
    for dir_entry in read.flatten() {
        let path = dir_entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("epub") {
            continue;
        }
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
}
