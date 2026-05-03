# cleader v1 — Design

## Summary

A terminal-based EPUB reader written in Rust. v1 is a deliberately minimal slice: open one EPUB by path, render its text in a paginated reader, persist the user's position so they can quit and resume.

The library browser, ASCII cover art, search, and other features described in the original `cleader_docs:*` files are **out of scope for v1** and live on the backlog.

The project will be released publicly on GitHub for free download and use.

## Goals

- Open a single EPUB given on the command line (`cleader path/to/book.epub`).
- Render its text in the terminal with paragraph wrapping, headings, bold, and italic styling.
- Let the reader navigate by line and by page, jump between chapters, and quit cleanly.
- Save the reader's position automatically and restore it on next open.
- Work cross-platform (macOS, Linux, Windows) from day one. Primary development target is macOS.

## Non-goals (v1)

- No library / folder scan. One book per invocation.
- No ASCII cover art rendering.
- No search, filter, or TOC view.
- No font customization (terminal-controlled in v1; richer rendering is a backlog research topic).
- No image, table, or footnote popovers (handled minimally; see HTML→Block mapping).

## Target platforms

| OS | Status |
|---|---|
| macOS | Primary dev target. v1 must work end-to-end. |
| Linux | Supported via the same code path. Smoke-tested. |
| Windows | Supported via the same code path. Smoke-tested. |

Cross-platform paths come from the `directories` crate, terminal I/O from `crossterm`. No platform-specific code in v1.

## Architecture

### Pattern

App-Model-Update (Elm-style), single-threaded, blocking event loop. The terminal blocks on `crossterm::event::read()` between draws — CPU is 0% while the reader is reading.

```
loop {
    terminal.draw(|f| render(&app, f))?;
    let event = crossterm::event::read()?;
    if let Some(action) = input::translate(event) {
        app.handle(action);
    }
    if app.should_quit { break; }
}
```

### Module layout

```
cleader/
├── Cargo.toml
├── README.md
├── BACKLOG.md
├── books/                    (gitignored test EPUBs)
├── docs/superpowers/specs/   (this file lives here)
└── src/
    ├── main.rs               entry: parse CLI, setup terminal, run, restore
    ├── app.rs                App struct, event dispatch, state transitions
    ├── epub.rs               open EPUB, extract chapters as Vec<Block>
    ├── reader.rs             wrap blocks to terminal width, render reader screen
    ├── input.rs              KeyEvent → Option<Action>
    ├── persistence.rs        registry.json load/save, path resolution
    └── error.rs              typed error types
```

Target ~150–300 lines per file. Each file has one responsibility and is testable in isolation.

## Dependencies

| Crate | Purpose |
|---|---|
| `ratatui` | TUI framework |
| `crossterm` | Cross-platform terminal backend |
| `epub` | EPUB parser (handles ZIP + manifest) |
| `scraper` | XHTML parsing (DOM walk) |
| `textwrap` | Word-boundary line wrapping |
| `serde`, `serde_json` | Persistence |
| `directories` | OS-correct config/data paths |
| `clap` (derive) | CLI arg parsing |
| `anyhow` | Error handling at the binary boundary |
| `thiserror` | Typed errors in library modules |

## Data model

```rust
// Source-form: width-independent
pub struct Book {
    pub title: String,
    pub author: String,
    pub path: PathBuf,
    pub chapters: Vec<Chapter>,
}

pub struct Chapter {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
}

pub enum Block {
    Heading { level: u8, spans: Vec<Span> },
    Paragraph { spans: Vec<Span> },
    Blank,
}

pub struct Span {
    pub text: String,
    pub style: SpanStyle,    // Plain | Bold | Italic
}

// Wrapped-form: derived from Vec<Block> + viewport width
type WrappedLines = Vec<ratatui::text::Line<'static>>;

// App state
pub struct App {
    book: Book,
    chapter_idx: usize,
    line_offset: usize,             // top of viewport in wrapped lines
    wrapped: WrappedLines,          // current chapter, wrapped to current width
    viewport_size: (u16, u16),      // (cols, rows)
    persistence: Persistence,
    should_quit: bool,
}

pub enum Action {
    LineUp, LineDown,
    PageNext, PagePrev,
    ChapterNext, ChapterPrev,
    Quit,
    Resize(u16, u16),
}
```

The two layers (source `Vec<Block>` and wrapped `Vec<Line>`) are deliberately separated so a terminal resize re-wraps from blocks without re-parsing HTML.

## UI

### Layout

```
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│    [paragraph text, wrapped at ~80 columns, centered]        │ ← body
│    [paragraph text]                                          │
│                                                              │
│    [paragraph text]                                          │
│                                                              │
│ ── Title ─ Ch X/Y ─ Page A/B ─ q quit ───────────────────── │ ← status (1 row)
└──────────────────────────────────────────────────────────────┘
```

- Body is capped at ~80 columns and centered. If the terminal is wider, whitespace surrounds the column. ~3-column left padding inside the body column.
- Status bar is one row, dim color, full terminal width.
- Title in status bar truncates with `…` on the right if needed (e.g. `My Very Long Book Tit…`); the right side of the bar reserves space for `q quit`.

### Key bindings

| Action | Keys |
|---|---|
| LineUp | `↑`, `k` |
| LineDown | `↓`, `j` |
| PagePrev | `←`, `h`, `PgUp`, `b` |
| PageNext | `→`, `l`, `PgDn`, `Space` |
| ChapterNext | `n` |
| ChapterPrev | `N` (Shift+n) |
| Quit (saves position) | `q`, `Ctrl+C`, `Esc` |

Unknown keys are silently ignored.

### Boundary behavior

- `LineDown` at the last line of the last chapter — no-op.
- `LineDown` at the last line of any other chapter — advance to chapter+1, line 0, re-wrap.
- `PageNext` past chapter end — advance to chapter+1, snap to line 0 (no carry-over).
- Same logic mirrored backwards.
- `ChapterNext` at the last chapter — no-op.

## EPUB pipeline (epub.rs)

```
PathBuf
  → epub::doc::EpubDoc::new(path)            // crate handles ZIP + manifest
  → for each spine item: get_resource_str()   // raw XHTML
  → html_to_blocks(&xhtml) -> Vec<Block>      // our DOM walker
  → Chapter { title, blocks }
  → Book { title, author, path, chapters }
```

### HTML → Block mapping

| HTML | Rendered as |
|---|---|
| `<p>` | `Block::Paragraph` with styled spans |
| `<h1>`–`<h6>` | `Block::Heading { level }` (bold + cyan, blank line after) |
| `<b>`, `<strong>` | bold span |
| `<i>`, `<em>` | italic span |
| `<br>` | line break inside a paragraph |
| `<hr>` | a `Block::Paragraph` containing `─ ─ ─` centered |
| `<blockquote>` | `Block::Paragraph` with 4-space indent, italic |
| `<ul>` / `<ol>` | one `Block::Paragraph` per `<li>`, prefixed with `• ` or `1. ` |
| `<a>` | render text only, drop the href |
| `<img>` | `Block::Paragraph` containing `[image: <alt-text>]`; skipped if no alt |
| `<table>` | render cell text space-separated, one row per `Block::Paragraph` |
| Anything else | descend into children, ignore the wrapping tag |

### Footnotes (v1 strategy)

EPUBs use anchor links to a separate notes section. v1 leaves these as-is — the noteref renders as plain text (e.g., `1`), and the reader can flip to the endnotes chapter to read it. A proper popover (press `f` on a noteref) is on the backlog.

### Title/author extraction

`EpubDoc::mdata("title")` and `mdata("creator")`. If absent, fall back to filename (without `.epub`) and `"Unknown"`.

## Render pipeline (reader.rs)

`reader::wrap_chapter(blocks: &[Block], width: u16) -> Vec<Line>`:

```
for block in blocks:
    match block {
        Heading => textwrap to width-margin, mark each output line bold+cyan,
                   emit blank line after
        Paragraph => textwrap to width-margin, preserving span styles across wraps
        Blank => emit one empty Line
    }
```

`textwrap` operates on plain strings; styled spans need an adapter that wraps on the plain-text projection then walks back through spans rebuilding `Line`s with correct styles per output line. Pure function — fully testable.

### Pagination math

```rust
let lines_per_page = viewport_rows - 1;       // minus status bar
let page = line_offset / lines_per_page + 1;
let total_pages = wrapped.len().div_ceil(lines_per_page);
```

Page count is purely derived for display. Same model as Kindle: when the wrap width changes, total pages recomputes; the reader's spot in the actual text is preserved.

### Re-wrap triggers

- On chapter switch (always).
- On terminal resize when width changed (not on height-only changes).

## Persistence (persistence.rs)

### Path

`<data_dir>/cleader/registry.json`, where `data_dir` comes from `directories::ProjectDirs::data_dir()`:

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/cleader/registry.json` |
| Linux | `~/.local/share/cleader/registry.json` |
| Windows | `%APPDATA%\cleader\registry.json` |

### Schema (v1)

```json
{
  "version": 1,
  "books": {
    "<absolute_path_to_epub>": {
      "title": "Firefly: Generations",
      "author": "Tim Lebbon",
      "chapter_idx": 4,
      "line_offset": 312,
      "last_read": "2026-05-03T11:53:00Z"
    }
  }
}
```

Keys are absolute file paths. Move-resilient hash-based keys are on the backlog.

### Write strategy

- Save triggers: `PageNext`, `PagePrev`, `ChapterNext`, `ChapterPrev`, and `Quit`. Single-line scrolls (`LineUp`/`LineDown`) do not trigger a save — the next page navigation will. Worst-case data loss from a hard crash is the lines scrolled within the current page.
- Atomic write: serialize to `registry.json.tmp`, fsync, rename. Survives a crash mid-write.
- Whole file is small (a few KB even with hundreds of books) — no incremental writes.

### Recovery

- Missing file: start fresh, no warning.
- Malformed JSON or unknown `version`: log a one-line warning to stderr and start fresh. Never crash the reader because of a bad state file.

## Error handling

Two layers:

- Library modules (`epub`, `reader`, `persistence`) define `thiserror`-derived enums for their failure modes.
- `main.rs` returns `anyhow::Result<()>`; failures print as `cleader: <message>` to stderr with exit code 1.

### Failure modes (v1)

| Situation | Behavior |
|---|---|
| EPUB path doesn't exist | `cleader: no such file: <path>`, exit 1 |
| Path isn't an EPUB / malformed | `cleader: not a valid EPUB: <reason>`, exit 1 |
| EPUB has no readable chapters | `cleader: this EPUB has no readable chapters`, exit 1 |
| Terminal can't enter raw mode | Friendly error, exit 1 |
| Persistence write fails mid-session | Log to stderr (post-exit); reading not interrupted |
| Persistence read fails at startup | Warn, start fresh |
| Panic anywhere | A panic hook restores the terminal first, then prints |

The panic hook is the one piece of unavoidable global state in v1 — without it, a panic mid-read would leave the user's terminal in raw mode.

## Testing

| Layer | What | How |
|---|---|---|
| `epub.rs` | HTML → Block conversion | Unit tests with hand-written XHTML strings, one per tag mapping |
| `epub.rs` | End-to-end load | Integration tests against the four EPUBs in `books/`. Asserts: chapter count > 0, first chapter has text, title non-empty |
| `reader.rs` | Wrap math | Pure function. Unit tests with synthetic blocks at various widths. Edge cases: empty paragraph, single-word longer than width, span boundary mid-wrap |
| `input.rs` | Key → Action | Pure function. One test per binding row, plus "unknown key returns None" |
| `persistence.rs` | Save/load round-trip | Tempfile + assert. Plus a corrupt-JSON test that confirms graceful recovery |
| `app.rs` | State transitions | Boundary tests: PageNext at last chapter, LineDown at chapter end, etc. No terminal needed |

No automated end-to-end terminal driving in v1. Manual smoke checklist (lives in `CONTRIBUTING.md`):

1. `cargo run -- books/Firefly_*.epub` — opens, renders text
2. Arrow keys + j/k/h/l + Space + n/N all do the right thing
3. `q` exits cleanly, terminal restored
4. Re-open same book, position restored
5. Resize terminal mid-read, page count updates, no crash

## Deliverables (v1)

- `src/` Rust source as described above
- `Cargo.toml` with the dependencies listed
- `README.md`: project description, screenshots/asciinema, install instructions (`cargo install --path .` to start; package managers later), usage, key bindings, license
- `BACKLOG.md`: the nine items below
- `LICENSE`: choose at implementation time (likely MIT or Apache-2.0; user choice)
- `CONTRIBUTING.md`: manual smoke checklist, dev setup
- `.gitignore`: `target/`, `books/`, `registry.json` if local

## Backlog (post-v1)

1. **Font selection and size customization in the reader.** Depends on terminal capabilities (sixel / kitty graphics protocol) or a separate GUI mode. Research topic.
2. **Help screen.** Overlay (`?` to toggle) showing all key bindings.
3. **Smarter resize.** Preserve viewport content position across terminal resize by tracking byte-offset-into-source alongside line offset.
4. **Footnote popover.** Press `f` on a noteref to show the note in a modal.
5. **Text selection / copy from book.**
6. **ASCII art for inline book images.** Re-use the cover-art conversion pipeline (also v2).
7. **Configurable text width.** User-set wrap width preference.
8. **Hash-based book keys.** Move-resilient registry; identify books by content hash, not path.
9. **Distribution via package managers.** Homebrew (mac), Chocolatey/Scoop (Windows), `cargo install`, GitHub Releases prebuilt binaries, AUR/.deb/.rpm. Research topic; install instructions land in README as channels become available.

Plus the v2 features that were always planned: library scanner, ASCII covers, search/filter, TOC view.
