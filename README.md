# cleader

A distraction-free terminal EPUB reader written in Rust.

```
  ── Firefly: Generations ─ Ch 4/22 ─ Page 18/247 ─ q quit ──
```

## Features (v1)

- Open any EPUB by path: `cleader path/to/book.epub`
- Paginated reader with word-boundary wrapping at a comfortable column width.
- Bold, italic, and heading styles preserved from the source EPUB.
- Vim-style and arrow-key navigation, plus Page Up/Down and Space.
- Chapter jumping with `n` / `N`.
- Reading position is saved automatically and restored next time you open the same book.
- Cross-platform: macOS, Linux, Windows. One binary, one code path.

See [`BACKLOG.md`](BACKLOG.md) for what's coming next.

## Installation

While `cleader` is pre-release, install from source:

```bash
git clone https://github.com/aqiulc/cleader
cd cleader
cargo install --path .
```

This installs the `cleader` binary into `~/.cargo/bin/`. Make sure that's on your `$PATH`.

Distribution via Homebrew, Chocolatey/Scoop, `cargo install cleader`, and prebuilt GitHub Release binaries is on the roadmap — see backlog item 9.

## Usage

```
cleader <path-to-book.epub>
```

Example:

```bash
cleader ~/Books/Firefly_Generations.epub
```

### Options

- `-w`, `--width=N` — target body text width in columns (default 80).

### Key bindings

| Action | Keys |
|---|---|
| Scroll one line | `↑` / `↓` or `k` / `j` |
| Flip a page | `←` / `→` or `h` / `l` or `Space` / `b` or `PgUp` / `PgDn` |
| Next chapter | `n` |
| Previous chapter | `N` (Shift+n) |
| Open table of contents | `t` |
| Quit (saves position) | `q`, `Esc`, or `Ctrl+C` |

### Where your reading position is saved

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/cleader/registry.json` |
| Linux | `~/.local/share/cleader/registry.json` |
| Windows | `%APPDATA%\cleader\registry.json` |

The registry is a small JSON file you can inspect, back up, or delete (cleader will start fresh if it's gone).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
