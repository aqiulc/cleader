//! Cover thumbnail cache for the library grid view.
//!
//! Holds an in-memory map of `BookId -> CoverState` plus a disk cache
//! at `<data_dir>/covers/<book_id>.txt`. ASCII generation runs on a
//! background worker thread (wired in Task 4). Disk reads/writes are
//! best-effort: a failure leaves the memory cache authoritative for
//! the current session and silently retries next launch.
//!
//! This commit (Task 3) provides only the pure I/O surface:
//! - `default_cache_dir()` / `cache_path()` for resolving paths
//! - `read_cached()` / `write_cached()` for the disk layer
//! - `PLACEHOLDER` constant for unrendered cells
//!
//! Task 4 adds the `CoverCache` struct with `enqueue` / `drain_finished`
//! / `get` plus the background worker thread.
//!
//! Placeholder lines are 22 cols × 12 rows so cell layout never shifts
//! when a real cover arrives. Same dimensions as a fully generated
//! cover (matches COVER_THUMBNAIL_WIDTH / COVER_THUMBNAIL_HEIGHT).

use crate::epub::BookId;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

/// Cell-content width (cell = 24 cols total, 1 col of border on each side).
pub const COVER_THUMBNAIL_WIDTH: u16 = 22;

/// Rows of ASCII art per thumbnail. Pad with blanks if a generated cover
/// is shorter, truncate if longer.
pub const COVER_THUMBNAIL_HEIGHT: u16 = 12;

/// Static placeholder shown while a cover is Pending or unavailable.
/// 22 cols × 12 rows. Replace this constant with a real logo when
/// designed; cell layout is unaffected.
pub const PLACEHOLDER: [&str; 12] = [
    "+--------------------+",
    "|                    |",
    "|                    |",
    "|                    |",
    "|                    |",
    "|      cleader       |",
    "|                    |",
    "|                    |",
    "|                    |",
    "|                    |",
    "|                    |",
    "+--------------------+",
];

/// Resolve `<data_dir>/covers/`. Returns `None` if the OS can't tell us
/// where the data dir is (rare; e.g. unset $HOME on a fresh CI runner).
pub fn default_cache_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "cleader")?;
    Some(dirs.data_dir().join("covers"))
}

/// Cache file path for a given book id.
pub fn cache_path(cache_dir: &Path, book_id: &BookId) -> PathBuf {
    cache_dir.join(format!("{}.txt", book_id.as_str()))
}

/// Read a cached cover from disk. Returns `None` for any failure
/// (file missing, permission denied, I/O error) — the cache is
/// best-effort and all read failures are treated as misses. A
/// successful read of a zero-byte file returns `Some(vec![])`,
/// which callers should treat as a malformed cache and either
/// regenerate or ignore.
pub fn read_cached(cache_dir: &Path, book_id: &BookId) -> Option<Vec<String>> {
    let path = cache_path(cache_dir, book_id);
    let content = std::fs::read_to_string(&path).ok()?;
    Some(content.lines().map(|l| l.to_string()).collect())
}

/// Write a generated cover to disk. Failure is non-fatal; caller logs
/// or ignores. Atomic write via tempfile + rename (same pattern as
/// `persistence::save_to`).
///
/// Callers should pass a non-empty `lines` slice. An empty slice writes
/// a zero-byte file that `read_cached` would return as `Some(vec![])`
/// — a degenerate cache hit. The cover generator always produces
/// `COVER_THUMBNAIL_HEIGHT` rows, so this is a constraint at the
/// callsite, not a hard precondition.
pub fn write_cached(
    cache_dir: &Path,
    book_id: &BookId,
    lines: &[String],
) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(cache_dir)?;
    let final_path = cache_path(cache_dir, book_id);
    let tmp_path = final_path.with_extension("txt.tmp");
    {
        let mut tmp = std::fs::File::create(&tmp_path)?;
        for line in lines {
            tmp.write_all(line.as_bytes())?;
            tmp.write_all(b"\n")?;
        }
        tmp.sync_all()?;
    }
    std::fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_id(seed: &[u8]) -> BookId {
        BookId::from_bytes(seed)
    }

    #[test]
    fn placeholder_dimensions_are_22x12() {
        assert_eq!(PLACEHOLDER.len(), COVER_THUMBNAIL_HEIGHT as usize);
        for (i, row) in PLACEHOLDER.iter().enumerate() {
            assert_eq!(
                row.chars().count(),
                COVER_THUMBNAIL_WIDTH as usize,
                "row {i} width mismatch: {row:?}"
            );
        }
    }

    #[test]
    fn read_cached_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let id = book_id(b"missing");
        let result = read_cached(dir.path(), &id);
        assert!(result.is_none());
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let id = book_id(b"hello");
        let lines: Vec<String> = (0..12)
            .map(|i| format!("row {i:02} content padding here"))
            .collect();
        write_cached(dir.path(), &id, &lines).unwrap();
        let loaded = read_cached(dir.path(), &id).unwrap();
        assert_eq!(loaded, lines);
    }

    #[test]
    fn write_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("does/not/exist/yet");
        let id = book_id(b"x");
        let lines = vec!["only one line".to_string()];
        write_cached(&nested, &id, &lines).unwrap();
        assert!(read_cached(&nested, &id).is_some());
    }

    #[test]
    fn cache_path_uses_hex_id_and_txt_extension() {
        let dir = std::path::Path::new("/tmp/c");
        let id = book_id(b"abc");
        let p = cache_path(dir, &id);
        let s = p.to_string_lossy();
        assert!(s.starts_with("/tmp/c/"));
        assert!(s.ends_with(".txt"));
        // BookId is hex of SHA-256 — 64 hex chars.
        assert_eq!(p.file_stem().unwrap().to_string_lossy().len(), 64);
    }
}
