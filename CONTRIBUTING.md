# Contributing to cleader

Thanks for thinking about helping out. cleader is small enough that
contributions are easy to shepherd.

## Dev loop

```bash
cargo test                                  # unit + integration tests
cargo clippy --all-targets -- -D warnings   # lint, must be clean
cargo doc --no-deps                         # no warnings
cargo run -- path/to/book.epub              # manual smoke
```

## Design + plan workflow

Larger changes benefit from a brief design write-up before code lands —
architecture, state machine, non-goals, error handling — followed by an
implementation plan broken into bite-sized commits. A draft pasted into
the PR description is usually enough; the maintainer keeps long-form
versions in a local working directory rather than the repo so the
published crate stays focused on shipped code.

For small fixes or polish, a focused PR is fine.

## Manual smoke checklist (before opening a PR)

1. `cargo run --release -- path/to/book.epub` — the reader opens and
   renders text. Arrow keys, `j/k/h/l`, `Space`, `n/N` all work.
2. `cargo run --release -- path/to/directory/` — the library opens.
   `g` toggles grid/list, `/` opens search, Enter opens a book,
   `q`/Esc returns to library with cache intact.
3. `?` from anywhere shows the help overlay.
4. Open a book, scroll a few pages, quit. Re-open the same book —
   position is restored.
5. Resize the terminal mid-read. Page count updates; no crash.

## Testing

- Unit tests live inline at the bottom of each module as
  `#[cfg(test)] mod tests`.
- Integration tests live in `tests/integration.rs` and use the EPUBs in
  `books/` (gitignored — drop your own there to run them locally).
- Pure functions (`reader::wrap_chapter`, `input::translate`,
  `search::filter_indices`, etc.) deserve unit tests. Code that touches
  the terminal is best smoke-tested by hand.

## License

By contributing, you agree your work is dual-licensed under MIT and
Apache-2.0, matching the project as a whole.
