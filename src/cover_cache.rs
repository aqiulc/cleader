//! Cover thumbnail cache for the library grid view.
//!
//! Holds an in-memory map of `BookId -> CoverState` plus a disk cache
//! at `<data_dir>/covers/<book_id>.txt`. ASCII generation runs on a
//! background worker thread (wired in Task 4). Disk reads/writes are
//! best-effort: a failure leaves the memory cache authoritative for
//! the current session and silently retries next launch.
//!
//! Public API:
//! - `new()` spawns the worker
//! - `enqueue(book_id, epub_path)` requests a cover (idempotent)
//! - `drain_finished()` pulls finished covers off the channel each frame
//! - `get(book_id)` returns Some(&[String]) only when Ready
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

/// Read a cached cover from disk. `Ok(None)` when the file doesn't
/// exist (a normal cache miss). `Ok(Some(lines))` when present. `Err`
/// only on disk I/O errors that aren't NotFound (e.g. permission
/// denied) — caller treats those as misses too.
pub fn read_cached(cache_dir: &Path, book_id: &BookId) -> std::io::Result<Option<Vec<String>>> {
    let path = cache_path(cache_dir, book_id);
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(Some(content.lines().map(|l| l.to_string()).collect())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write a generated cover to disk. Failure is non-fatal; caller logs
/// or ignores. Atomic write via tempfile + rename (same pattern as
/// `persistence::save_to`).
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
        let result = read_cached(dir.path(), &id).unwrap();
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
        let loaded = read_cached(dir.path(), &id).unwrap().unwrap();
        assert_eq!(loaded, lines);
    }

    #[test]
    fn write_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("does/not/exist/yet");
        let id = book_id(b"x");
        let lines = vec!["only one line".to_string()];
        write_cached(&nested, &id, &lines).unwrap();
        assert!(read_cached(&nested, &id).unwrap().is_some());
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
