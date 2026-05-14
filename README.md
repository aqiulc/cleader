# cleader

A distraction-free terminal EPUB reader. Opens a single book or a whole
directory; remembers where you left off; never gets in your way.

```
  ── Firefly: Generations ─ Ch 4/22 ─ Page 18/247 ─ q quit ──
```

## Install

Until cleader hits the package managers, install from source:

```bash
cargo install --path .   # from a local clone
```

Or directly:

```bash
git clone https://github.com/aqiulc/cleader
cd cleader
cargo install --path .
```

The `cleader` binary lands in `~/.cargo/bin/`. Make sure that's on your `$PATH`.

## Usage

**Read a single book:**

```bash
cleader path/to/book.epub
```

**Browse a directory of books:**

```bash
cleader path/to/library/
```

The directory form opens a library view where you can pick a book.
Toggle between grid (with ASCII cover art) and list views with `g`.
Search by title or author with `/`.

### Options

- `-w`, `--width=N` — target body text width in columns (default 80).
- `--help` / `--version` — standard clap.

## Key bindings

Press `?` from anywhere for the full overlay. Quick reference:

**Library**

| Action | Keys |
|---|---|
| Navigate | `↑` `↓` `←` `→` or `h` `j` `k` `l` |
| Toggle grid/list | `g` |
| Search | `/` |
| Open selected book | `Enter` |

**Reading**

| Action | Keys |
|---|---|
| Scroll line | `↑` `↓` or `k` `j` |
| Flip page | `←` `→` or `h` `l` or `Space` `b` or `PgUp` `PgDn` |
| Next / previous chapter | `n` / `N` (Shift+n) |
| Table of contents | `t` |

**Anywhere**

| Action | Keys |
|---|---|
| Toggle help overlay | `?` |
| Quit (saves position) | `q`, `Esc`, or `Ctrl+C` |

## Where things are saved

Reading positions, view preferences, and cached cover art live in the
OS-native data directory:

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/cleader/` |
| Linux | `~/.local/share/cleader/` |
| Windows | `%APPDATA%\cleader\` |

Three files plus a directory:

- `registry.json` — reading positions (one entry per book, content-hashed)
- `prefs.json` — view-mode preference
- `covers/v5/` — cached cover ASCII art (one file per book)

All are safe to delete; cleader will start fresh. Books are identified by
SHA-256 of their content, so moving an EPUB doesn't reset your progress.

## Highlights

- **Smart resize** — wrap width changes preserve your position in the source
  text, not just the line offset.
- **Atomic saves** — reading position writes go through a tempfile+rename,
  so a crash mid-write never corrupts the registry.
- **Background cover rendering** — the library worker generates covers
  viewport-only and caches them to disk; second-open is instant.
- **Cross-platform** — one binary, one code path. macOS, Linux, Windows.

## License

Dual-licensed:

- [Apache 2.0](LICENSE-APACHE)
- [MIT](LICENSE-MIT)

at your option. Contributions are accepted under the same dual license.

## Author

Created by Aqiul — `aqiul.c@gmail.com`.
