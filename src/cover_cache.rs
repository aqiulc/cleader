//! Cover thumbnail cache for the library grid view.
//!
//! Holds an in-memory map of `BookId -> CoverState` plus a disk cache
//! at `<data_dir>/covers/<book_id>.txt`. ASCII generation runs on a
//! background worker thread spawned by `CoverCache::open`. Disk
//! reads/writes are best-effort: a failure leaves the memory cache
//! authoritative for the current session and silently retries next
//! launch.
//!
//! Public API:
//! - `CoverCache::open()` (or `open_at()` for tests) spawns the worker
//! - `enqueue(book_id, epub_path)` requests a cover (idempotent — disk
//!   hit short-circuits; otherwise queues the worker)
//! - `drain_finished()` pulls finished covers off the channel each frame
//! - `get(book_id)` returns `Some(&[String])` only when Ready
//! - Drop joins the worker cleanly
//!
//! Placeholder lines are 22 cols × 17 rows so cell layout never shifts
//! when a real cover arrives. Same dimensions as a fully generated
//! cover (matches COVER_THUMBNAIL_WIDTH / COVER_THUMBNAIL_HEIGHT).

use crate::epub::BookId;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

/// Cell-content width (cell = 24 cols total, 1 col of border on each side).
pub const COVER_THUMBNAIL_WIDTH: u16 = 22;

/// Rows of ASCII art per thumbnail. Pad with blanks if a generated cover
/// is shorter, truncate if longer.
pub const COVER_THUMBNAIL_HEIGHT: u16 = 17;

/// Static placeholder shown while a cover is Pending or unavailable.
/// 22 cols × 17 rows. Replace this constant with a real logo when
/// designed; cell layout is unaffected.
pub const PLACEHOLDER: [&str; 17] = [
    "+--------------------+",
    "|                    |",
    "|                    |",
    "|                    |",
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
    "|                    |",
    "|                    |",
    "+--------------------+",
];

/// Bump this when the on-disk cover format or rendering pipeline
/// changes in a way that should invalidate every previously-cached
/// cover. v1 was the initial release; v2 was triggered by the
/// letterbox fix + placeholder-not-cached fix in the v0.4.4 patch
/// cycle (covers generated before that were clipped, and any book
/// that ever failed to decode had the placeholder pinned to disk).
/// v4: COVER_THUMBNAIL_HEIGHT 12 → 17 (natural-aspect 2:3 cover at
/// width 22) and CELL_HEIGHT 16 → 21; output dimensions changed.
/// v5: ASCII_GRADIENT expanded from 5 chars to 7 (' .-+=​#@');
/// brightness-to-glyph mapping changed so old cached glyphs are wrong.
const COVER_CACHE_VERSION: &str = "v5";

/// Resolve `<data_dir>/covers/<version>/`. Returns `None` if the OS can't
/// tell us where the data dir is (rare; e.g. unset $HOME on a fresh CI
/// runner).
pub fn default_cache_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "cleader")?;
    Some(dirs.data_dir().join("covers").join(COVER_CACHE_VERSION))
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

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

/// State of a single cover in the in-memory map.
enum CoverState {
    Pending,
    Ready(Vec<String>),
}

struct CoverJob {
    book_id: BookId,
    epub_path: PathBuf,
}

struct CoverResult {
    book_id: BookId,
    lines: Vec<String>,
}

pub struct CoverCache {
    memory: HashMap<BookId, CoverState>,
    cache_dir: PathBuf,
    job_tx: Option<mpsc::Sender<CoverJob>>,
    result_rx: mpsc::Receiver<CoverResult>,
    worker: Option<thread::JoinHandle<()>>,
}

impl CoverCache {
    /// Construct a cache rooted at the OS-native data dir. Spawns the
    /// worker. Returns `None` if the OS doesn't expose a data dir
    /// (caller falls back to disabling the grid view).
    pub fn open() -> Option<Self> {
        let cache_dir = default_cache_dir()?;
        Some(Self::open_at(cache_dir))
    }

    /// Open against an explicit cache directory. Intended for tests and
    /// internal tooling.
    #[doc(hidden)]
    pub fn open_at(cache_dir: PathBuf) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<CoverJob>();
        let (result_tx, result_rx) = mpsc::channel::<CoverResult>();
        let worker_cache_dir = cache_dir.clone();
        let worker = thread::Builder::new()
            .name("cleader-cover-worker".into())
            .spawn(move || worker_loop(job_rx, result_tx, worker_cache_dir))
            .expect("spawning cover worker thread should succeed");
        Self {
            memory: HashMap::new(),
            cache_dir,
            job_tx: Some(job_tx),
            result_rx,
            worker: Some(worker),
        }
    }

    /// Returns `Some` only when a Ready cover is in memory. Pending and
    /// Miss both return `None`; renderer falls back to the placeholder.
    pub fn get(&self, book_id: &BookId) -> Option<&[String]> {
        match self.memory.get(book_id)? {
            CoverState::Ready(lines) => Some(lines),
            CoverState::Pending => None,
        }
    }

    /// Request a cover. No-op if already Ready or Pending. On a memory
    /// miss, tries disk first (fast hot-path); on disk miss, queues the
    /// worker.
    pub fn enqueue(&mut self, book_id: BookId, epub_path: PathBuf) {
        if self.memory.contains_key(&book_id) {
            return;
        }
        // Try disk first. Reject malformed cache (wrong line count)
        // and fall through to regenerate — `write_cached`'s doc comment
        // flagged this degenerate case.
        if let Some(lines) = read_cached(&self.cache_dir, &book_id) {
            if lines.len() == COVER_THUMBNAIL_HEIGHT as usize {
                self.memory.insert(book_id, CoverState::Ready(lines));
                return;
            }
        }
        // Disk miss — queue the worker.
        self.memory.insert(book_id.clone(), CoverState::Pending);
        if let Some(tx) = &self.job_tx {
            let _ = tx.send(CoverJob { book_id, epub_path });
        }
    }

    /// True if any cover in the memory map is still in the Pending
    /// state (the worker is rendering it). Used by the event loop to
    /// decide whether to use the short (50ms) poll cadence or relax
    /// to the long (500ms) idle cadence.
    pub fn has_pending(&self) -> bool {
        self.memory.values().any(|s| matches!(s, CoverState::Pending))
    }

    /// Pull any finished covers from the worker into the memory map.
    /// Returns true if at least one cover arrived (caller redraws).
    pub fn drain_finished(&mut self) -> bool {
        let mut any = false;
        while let Ok(CoverResult { book_id, lines }) = self.result_rx.try_recv() {
            self.memory.insert(book_id, CoverState::Ready(lines));
            any = true;
        }
        any
    }
}

impl Drop for CoverCache {
    fn drop(&mut self) {
        // Drop the sender so the worker sees `recv` return `Err` and exits.
        self.job_tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    job_rx: mpsc::Receiver<CoverJob>,
    result_tx: mpsc::Sender<CoverResult>,
    cache_dir: PathBuf,
) {
    while let Ok(job) = job_rx.recv() {
        let lines = match generate_cover(&job.epub_path) {
            Some(real_cover) => {
                // Best-effort disk write — failure is non-fatal.
                let _ = write_cached(&cache_dir, &job.book_id, &real_cover);
                real_cover
            }
            None => {
                // Decode failed (no cover, unsupported format, corrupt
                // EPUB, etc.). Send placeholder to render but DO NOT
                // write it to disk — a future image-crate update or
                // EPUB fix should be able to retry.
                PLACEHOLDER.iter().map(|s| s.to_string()).collect()
            }
        };
        if result_tx
            .send(CoverResult { book_id: job.book_id, lines })
            .is_err()
        {
            // Receiver dropped — main thread shutting down. Stop.
            return;
        }
    }
}

/// Open the EPUB, find a cover (via OPF metadata or the first inline
/// `<img>` in the first chapter), then ASCII-render it at natural
/// aspect within `COVER_THUMBNAIL_WIDTH` columns. Pads or truncates the
/// height to `COVER_THUMBNAIL_HEIGHT` so the grid cell shape stays
/// uniform across books with slightly different aspect ratios.
///
/// Returns `None` if the EPUB can't be opened, has no cover by either
/// path, or the cover can't be decoded.
fn generate_cover(epub_path: &Path) -> Option<Vec<String>> {
    let mut doc = epub::doc::EpubDoc::new(epub_path).ok()?;
    let bytes = find_cover_bytes(&mut doc)?;
    let mut lines = crate::ascii_art::image_to_ascii(&bytes, COVER_THUMBNAIL_WIDTH).ok()?;
    // Trim if natural rendering produced more rows than the cell allows.
    lines.truncate(COVER_THUMBNAIL_HEIGHT as usize);
    // Pad with blank rows if natural rendering produced fewer rows
    // (squarish or landscape covers). Center-pad so the cover sits
    // visually centered within the cell.
    if lines.len() < COVER_THUMBNAIL_HEIGHT as usize {
        let blank = " ".repeat(COVER_THUMBNAIL_WIDTH as usize);
        let missing = COVER_THUMBNAIL_HEIGHT as usize - lines.len();
        let top_pad = missing / 2;
        let bot_pad = missing - top_pad;
        let mut padded = Vec::with_capacity(COVER_THUMBNAIL_HEIGHT as usize);
        for _ in 0..top_pad { padded.push(blank.clone()); }
        padded.extend(lines);
        for _ in 0..bot_pad { padded.push(blank.clone()); }
        lines = padded;
    }
    Some(lines)
}

/// Resolve cover bytes from an opened EpubDoc. Tries:
/// 1. The OPF-declared cover (`doc.get_cover()`).
/// 2. The first `<img src>` in the first spine item's XHTML, resolved
///    against the EPUB's manifest. Many EPUBs (especially older or
///    hand-rolled ones) don't declare a cover in OPF but include one
///    inline at the top of the first chapter.
fn find_cover_bytes(
    doc: &mut epub::doc::EpubDoc<std::io::BufReader<std::fs::File>>,
) -> Option<Vec<u8>> {
    // Path 1: OPF metadata.
    if let Some((bytes, _mime)) = doc.get_cover() {
        return Some(bytes);
    }

    // Path 2: first image in the first spine item.
    // doc starts positioned at the first spine item after EpubDoc::new.
    let current_path = doc.get_current_path();
    let content = doc.get_current_str().map(|(s, _)| s)?;
    let srcs = crate::epub::collect_img_srcs(&content);
    let first_src = srcs.into_iter().next()?;
    let resolved = crate::epub::resolve_image_path(current_path.as_deref(), &first_src);
    doc.get_resource_by_path(&resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_id(seed: &[u8]) -> BookId {
        BookId::from_bytes(seed)
    }

    #[test]
    fn placeholder_dimensions_are_22x17() {
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

    #[test]
    fn fresh_cache_returns_none_for_unknown_id() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::open_at(dir.path().to_path_buf());
        let id = book_id(b"never enqueued");
        assert!(cache.get(&id).is_none());
    }

    #[test]
    fn enqueue_with_disk_cache_hit_populates_memory_synchronously() {
        let dir = tempfile::tempdir().unwrap();
        let id = book_id(b"disk-hit");
        let lines: Vec<String> = (0..17).map(|i| format!("L{i:02}{}", " ".repeat(20))).collect();
        write_cached(dir.path(), &id, &lines).unwrap();

        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        // Path doesn't have to exist on disk — the disk-cache hit shortcircuits.
        cache.enqueue(id.clone(), PathBuf::from("/does/not/matter.epub"));
        let got = cache.get(&id).expect("disk hit should populate memory");
        assert_eq!(got.len(), 17);
        assert!(got[0].starts_with("L00"));
    }

    #[test]
    fn enqueue_is_idempotent() {
        // Calling enqueue twice on the same id should not panic or
        // double-queue; the second call observes the Pending state and
        // returns immediately.
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        let id = book_id(b"idempotent");
        let bogus = PathBuf::from("/tmp/does-not-exist.epub");
        cache.enqueue(id.clone(), bogus.clone());
        cache.enqueue(id.clone(), bogus.clone());
        // get() still None (Pending) — the duplicate enqueue is a no-op.
        // We can't assert "exactly one job sent" from outside, but we
        // can assert no panic and memory remains in Pending state by
        // dropping the cache (worker exit is the only synchronization).
        drop(cache);
    }

    #[test]
    fn worker_generates_cover_for_missing_path_as_placeholder() {
        // EPUB path that doesn't exist → generate_cover returns None →
        // worker falls back to placeholder → drain_finished delivers it.
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        let id = book_id(b"missing-epub");
        cache.enqueue(id.clone(), PathBuf::from("/no/such/book.epub"));

        // Wait up to 500 ms for the worker to deliver. In practice this
        // is <10 ms; the loop is for CI flakiness tolerance.
        let mut got = None;
        for _ in 0..50 {
            cache.drain_finished();
            if let Some(lines) = cache.get(&id) {
                got = Some(lines.to_vec());
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let got = got.expect("worker should deliver placeholder within 500ms");
        assert_eq!(got.len(), 17);
        assert_eq!(got[0], PLACEHOLDER[0]);
    }

    #[test]
    fn drop_signals_worker_to_exit() {
        // Smoke test: dropping the cache joins the worker without
        // hanging. If the worker doesn't observe Err on `recv`, this
        // test deadlocks and fails by timeout.
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::open_at(dir.path().to_path_buf());
        drop(cache);
        // If we got here, the worker exited cleanly.
    }

    #[test]
    fn worker_does_not_cache_placeholder_to_disk() {
        // A book that fails to decode (missing path) should NOT have
        // its placeholder written to disk — otherwise a one-time
        // failure becomes a permanent stuck placeholder.
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        let id = book_id(b"will-fail-to-decode");
        cache.enqueue(id.clone(), PathBuf::from("/no/such/book.epub"));

        // Wait for the worker to deliver placeholder.
        for _ in 0..50 {
            cache.drain_finished();
            if cache.get(&id).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(cache.get(&id).is_some(), "worker should deliver placeholder");

        // Drop the cache so the worker exits.
        drop(cache);

        // The disk should NOT have a cached file for this book.
        let cache_file = cache_path(dir.path(), &id);
        assert!(
            !cache_file.exists(),
            "placeholder must not be written to disk; found {cache_file:?}"
        );
    }

    #[test]
    fn generate_cover_returns_uniform_dimensions() {
        // Smoke test: when a fixture EPUB is available and its cover
        // is generated, the result is exactly COVER_THUMBNAIL_WIDTH x
        // COVER_THUMBNAIL_HEIGHT lines. Skip cleanly without a fixture.
        let path = match std::fs::read_dir("books").ok() {
            Some(entries) => entries.flatten().find_map(|e| {
                let p = e.path();
                (p.extension().and_then(|x| x.to_str()) == Some("epub")).then_some(p)
            }),
            None => None,
        };
        let Some(epub_path) = path else {
            eprintln!("cover_cache test: skipping — no .epub fixture in books/");
            return;
        };
        let lines = super::generate_cover(&epub_path);
        if let Some(lines) = lines {
            assert_eq!(lines.len(), COVER_THUMBNAIL_HEIGHT as usize);
            for line in &lines {
                assert_eq!(line.chars().count(), COVER_THUMBNAIL_WIDTH as usize);
            }
        }
    }

    #[test]
    fn default_cache_dir_uses_versioned_subdir() {
        if let Some(path) = default_cache_dir() {
            let s = path.to_string_lossy();
            assert!(
                s.contains("covers") && s.ends_with("v5"),
                "default_cache_dir should be .../covers/v5, got {s}"
            );
        }
    }

    #[test]
    fn has_pending_false_for_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::open_at(dir.path().to_path_buf());
        assert!(!cache.has_pending());
    }

    #[test]
    fn has_pending_true_after_enqueue_then_false_after_drain() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        let id = book_id(b"pending-test");
        cache.enqueue(id.clone(), PathBuf::from("/no/such/book.epub"));
        // Immediately after enqueue, the entry is Pending in memory.
        // (The worker hasn't replied yet — the placeholder fallback
        // takes a few ms to arrive.)
        assert!(cache.has_pending(), "Pending immediately after enqueue");

        // Wait for the worker to deliver the placeholder.
        for _ in 0..50 {
            cache.drain_finished();
            if !cache.has_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!cache.has_pending(), "no longer Pending after drain");
    }
}
