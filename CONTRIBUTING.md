# Contributing to cleader

Thanks for thinking about helping out. cleader is small enough that contributions are easy to shepherd.

## Dev loop

```bash
cargo test                                  # unit + integration tests
cargo clippy --all-targets -- -D warnings   # lint
cargo run -- books/<some>.epub              # manual smoke
```

## Manual smoke checklist (before opening a PR)

1. `cargo run --release -- books/<some>.epub` — the reader opens and renders text.
2. Arrow keys, `j/k/h/l`, `Space`, `n/N` all do what the README claims.
3. `q`, `Esc`, and `Ctrl+C` each exit cleanly with the terminal restored.
4. Open a book, scroll a few pages, quit. Re-open the same book — position is restored.
5. Resize the terminal mid-read. Page count updates; no crash.

## Testing

- Unit tests live inline at the bottom of each module as `#[cfg(test)] mod tests`.
- Integration tests live in `tests/integration.rs` and use the EPUBs in `books/` (gitignored — drop a few of your own there to run them locally).
- Pure functions (`reader::wrap_chapter`, `input::translate`, etc.) deserve unit tests. Code that touches the terminal is best smoke-tested by hand.

## License

By contributing, you agree your work is dual-licensed under MIT and Apache-2.0, matching the project as a whole.
