//! Cover thumbnail cache for the library grid view.
//!
//! Holds an in-memory map of `BookId -> CoverState` plus a disk cache
//! at `<data_dir>/covers/<book_id>.txt`. All file I/O — including the
//! whole-file read needed to compute a `BookId` (SHA-256 of the EPUB
//! bytes) — runs on a background worker thread spawned by
//! `CoverCache::open`, so the UI thread never touches book bytes. Disk
//! reads/writes are best-effort: a failure leaves the memory cache
//! authoritative for the current session and silently retries next
//! launch.
//!
//! Public API:
//! - `CoverCache::open()` (or `open_at()` for tests) spawns the worker
//! - `enqueue(epub_path)` requests a cover. The worker reads the file,
//!   resolves its `BookId`, serves a disk-cache hit if present, and
//!   otherwise generates ASCII art. Callers must deduplicate paths they
//!   have already enqueued.
//! - `drain_finished()` pulls finished jobs off the channel each frame,
//!   storing the lines in memory and returning the resolved
//!   `(path, book_id)` pairs so the caller can associate ids with its
//!   entries (a `None` id means the file couldn't be read and the
//!   entry stays unresolved).
//! - `get(book_id)` returns `Some(&[String])` only when Ready
//! - `has_pending()` is true while any job is in flight
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
    Ready(Vec<String>),
}

struct CoverJob {
    epub_path: PathBuf,
}

/// What the worker sends back for one job, before the cache folds the
/// lines into memory.
struct WorkerResult {
    epub_path: PathBuf,
    book_id: Option<BookId>,
    lines: Vec<String>,
}

/// A resolved cover job surfaced to the caller by [`CoverCache::drain_finished`].
///
/// `book_id` is `None` only when the EPUB couldn't be read at all (deleted,
/// moved, unreadable) — the caller should leave its entry unresolved.
pub struct CoverResult {
    pub path: PathBuf,
    pub book_id: Option<BookId>,
}

pub struct CoverCache {
    memory: HashMap<BookId, CoverState>,
    job_tx: Option<mpsc::Sender<CoverJob>>,
    result_rx: mpsc::Receiver<WorkerResult>,
    worker: Option<thread::JoinHandle<()>>,
    /// Number of jobs sent to the worker that haven't come back yet.
    /// Every job yields exactly one result (or the sender drops on
    /// shutdown), so this tracks `has_pending` without needing the
    /// BookId up front.
    in_flight: usize,
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
        let (result_tx, result_rx) = mpsc::channel::<WorkerResult>();
        let worker_cache_dir = cache_dir.clone();
        let worker = thread::Builder::new()
            .name("cleader-cover-worker".into())
            .spawn(move || worker_loop(job_rx, result_tx, worker_cache_dir))
            .expect("spawning cover worker thread should succeed");
        Self {
            memory: HashMap::new(),
            job_tx: Some(job_tx),
            result_rx,
            worker: Some(worker),
            in_flight: 0,
        }
    }

    /// Returns `Some` only when a Ready cover is in memory. Missing
    /// covers return `None`; renderer falls back to the placeholder.
    pub fn get(&self, book_id: &BookId) -> Option<&[String]> {
        match self.memory.get(book_id)? {
            CoverState::Ready(lines) => Some(lines),
        }
    }

    /// Request a cover for an EPUB file. The worker reads the file and
    /// computes its `BookId`, so the caller doesn't need to touch the
    /// bytes (and must deduplicate paths it has already enqueued).
    /// Disk-cache hits are served inside the worker, so this is a
    /// no-op only if the worker channel is gone.
    pub fn enqueue(&mut self, epub_path: PathBuf) {
        if let Some(tx) = &self.job_tx {
            if tx.send(CoverJob { epub_path }).is_ok() {
                self.in_flight += 1;
            }
        }
    }

    /// True if any cover job is still being processed by the worker.
    /// Used by the event loop to decide whether to use the short (50ms)
    /// poll cadence or relax to the long (500ms) idle cadence.
    pub fn has_pending(&self) -> bool {
        self.in_flight > 0
    }

    /// Pull any finished covers from the worker into the memory map and
    /// return the resolved `(path, book_id)` pairs so the caller can
    /// associate ids with its entries. Returns true via the non-empty
    /// vec when at least one cover arrived (caller redraws).
    pub fn drain_finished(&mut self) -> Vec<CoverResult> {
        let mut results = Vec::new();
        while let Ok(WorkerResult { epub_path, book_id, lines }) = self.result_rx.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            results.push(CoverResult {
                path: epub_path,
                book_id: book_id.clone(),
            });
            if let Some(id) = book_id {
                self.memory.insert(id, CoverState::Ready(lines));
            }
        }
        results
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
    result_tx: mpsc::Sender<WorkerResult>,
    cache_dir: PathBuf,
) {
    while let Ok(job) = job_rx.recv() {
        // Reading the whole file to derive the content-hash BookId is
        // the one piece of per-book I/O the UI thread used to do; it
        // lives here so the event loop never blocks on book bytes.
        let (book_id, lines) = match std::fs::read(&job.epub_path) {
            Ok(bytes) => {
                let id = BookId::from_bytes(&bytes);
                let lines = match read_cached(&cache_dir, &id) {
                    // Reject malformed cache (wrong line count) and fall
                    // through to regenerate — `write_cached`'s doc
                    // comment flagged this degenerate case.
                    Some(cached) if cached.len() == COVER_THUMBNAIL_HEIGHT as usize => cached,
                    _ => match generate_cover(&job.epub_path) {
                        Some(real_cover) => {
                            // Best-effort disk write — failure is non-fatal.
                            let _ = write_cached(&cache_dir, &id, &real_cover);
                            real_cover
                        }
                        None => {
                            // Decode failed (no cover, unsupported format,
                            // corrupt EPUB, etc.). Send placeholder to render
                            // but DO NOT write it to disk — a future
                            // image-crate update or EPUB fix should be able
                            // to retry.
                            PLACEHOLDER.iter().map(|s| s.to_string()).collect()
                        }
                    },
                };
                (Some(id), lines)
            }
            Err(_) => {
                // Can't read the file at all (deleted, moved, unreadable).
                // Report no id so the caller leaves the entry unresolved
                // instead of re-requesting; render with the placeholder.
                (None, PLACEHOLDER.iter().map(|s| s.to_string()).collect())
            }
        };
        if result_tx
            .send(WorkerResult {
                epub_path: job.epub_path,
                book_id,
                lines,
            })
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

    /// Poll `drain_finished` until the worker delivers one result.
    /// Times out after 500ms so a hung worker fails the test instead of
    /// deadlocking it.
    fn wait_for_result(cache: &mut CoverCache) -> CoverResult {
        for _ in 0..50 {
            let results = cache.drain_finished();
            if let Some(result) = results.into_iter().next() {
                return result;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("no cover result within 500ms");
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
    fn enqueue_serves_cached_cover_from_disk_without_regenerating() {
        // The worker computes the BookId from the file bytes, then checks
        // the disk cache before decoding. A pre-seeded cache file must be
        // served as-is.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("book.epub");
        std::fs::write(&file, b"real epub bytes").unwrap();
        let id = BookId::from_bytes(b"real epub bytes");
        let lines: Vec<String> = (0..17).map(|i| format!("L{i:02}{}", " ".repeat(20))).collect();
        write_cached(dir.path(), &id, &lines).unwrap();

        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        cache.enqueue(file);
        let result = wait_for_result(&mut cache);
        assert_eq!(result.book_id, Some(id.clone()), "worker should resolve the content hash");
        let got = cache.get(&id).expect("cached cover should be in memory");
        assert_eq!(got.len(), 17);
        assert_eq!(got, lines.as_slice());
    }

    #[test]
    fn invalid_epub_yields_placeholder_but_resolves_book_id() {
        // A readable file that isn't a valid EPUB: the worker can hash it
        // (so the caller learns its BookId) but cover generation fails, so
        // the placeholder is served from memory under that id.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("garbage.epub");
        std::fs::write(&file, b"not a zip archive").unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        cache.enqueue(file.clone());
        let result = wait_for_result(&mut cache);
        assert_eq!(result.path, file);
        let id = result.book_id.expect("readable file should resolve a book_id");
        let got = cache.get(&id).expect("placeholder should be in memory");
        assert_eq!(got.len(), 17);
        assert_eq!(got[0], PLACEHOLDER[0]);
    }

    #[test]
    fn missing_path_yields_result_with_no_book_id() {
        // An unreadable path: the worker can't hash it, so the result
        // carries no BookId and the caller should leave its entry
        // unresolved instead of re-requesting.
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        let missing = PathBuf::from("/no/such/book.epub");
        cache.enqueue(missing.clone());
        let result = wait_for_result(&mut cache);
        assert_eq!(result.path, missing);
        assert!(result.book_id.is_none());
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
        // A book that fails to decode (readable, but not a valid EPUB)
        // should NOT have its placeholder written to disk — otherwise a
        // one-time failure becomes a permanent stuck placeholder.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("will-fail-to-decode.epub");
        std::fs::write(&file, b"garbage bytes").unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        cache.enqueue(file);
        let result = wait_for_result(&mut cache);
        let id = result
            .book_id
            .expect("readable file should resolve a book_id");

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
        let file = dir.path().join("book.epub");
        std::fs::write(&file, b"x").unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        cache.enqueue(file);
        assert!(cache.has_pending(), "in-flight job counts as pending");

        // Wait for the worker to deliver.
        for _ in 0..50 {
            cache.drain_finished();
            if !cache.has_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!cache.has_pending(), "no longer pending after drain");
    }
}
