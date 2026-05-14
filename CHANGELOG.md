# Changelog

## [1.0.0] — 2026-05-14

First stable release.

### Reader

- Open a single EPUB by path: `cleader path/to/book.epub`.
- Paginated rendering with word-boundary wrapping at a configurable width
  (`--width=N`, default 80).
- Bold, italic, and heading styles preserved from the source EPUB.
- Vim-style and arrow-key navigation; PageUp/PageDown and Space supported.
- Chapter jumping with `n` / `N`.
- Table of contents overlay (`t`) with arrow navigation and Enter-to-jump.
- Help overlay (`?`) listing all bindings, library and reader.
- Smart resize: when the terminal width changes, the same paragraph stays
  at the top of the viewport (tracks source-character offset, not just
  line offset).
- Reading position auto-saves on every page flip and chapter change;
  restored on next open.
- Position storage is atomic (tempfile + rename + fsync) so a crash mid-write
  can't corrupt the registry.
- Books are identified by SHA-256 of their content — moving an EPUB doesn't
  reset your progress.

### Library

- Open a directory of books: `cleader path/to/library/`.
- Two view modes:
  - **Grid view** — ASCII cover art thumbnails (24×21 cells, 22×17 covers).
    Background worker generates covers viewport-only; non-visible books
    never burn CPU. Disk cache makes re-opens instant.
  - **List view** — text-only with title and author per row.
- Toggle views with `g`; preference persists across sessions.
- Grid-aware navigation: arrow up/down move between rows, left/right move
  within a row.
- Live-filter search (`/`) over title and author, case-insensitive substring.
  3-state machine: Idle / Editing (live filter as you type) / Applied
  (filter committed, navigate filtered set). Esc clears and restores the
  pre-search selection.
- Marquee for long titles — the selected grid cell scrolls its title if it
  exceeds the cell width.
- Cover format includes first-image fallback: EPUBs whose cover lives as an
  inline `<img>` in the first chapter (rather than OPF metadata) still get
  rendered.

### Help / discoverability

- Press `?` from anywhere for a modal overlay listing all bindings,
  grouped by Library / Reading / Anywhere.
- Footer hints in every mode include the next-most-useful binding.
- Esc dismisses any overlay (TOC, help, search) before quitting.

### Engineering

- Cross-platform: macOS, Linux, Windows; one binary, one code path.
- Test suite: 246 unit + 11 integration tests, clippy clean with
  `-D warnings`, no doc warnings.
- Adaptive event loop: idle CPU drops to ~2Hz when nothing is animating
  or rendering in the background.
- Atomic disk operations throughout (persistence, prefs, cover cache).

### License

Dual-licensed under Apache-2.0 and MIT, contributor's choice.

---

See `BACKLOG.md` for what's not in 1.0.
