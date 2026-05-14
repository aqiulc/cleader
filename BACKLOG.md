# cleader Backlog

Nice-to-haves and post-1.0 work. Open an issue or PR if any of these jump out.

## Open

1. **Distribution via package managers.** Homebrew tap (macOS), Chocolatey
   or Scoop (Windows), `cargo install cleader` (crates.io), prebuilt
   GitHub Release binaries, AUR / .deb / .rpm. README install section
   grows as channels light up.

2. **Inline image polish.** Inline image ASCII rendering is wired
   end-to-end (Book::open extracts every `<img>`, html_to_blocks emits
   `Block::Image`, wrap_chapter handles it mid-flow), but hasn't been
   smoke-tested on a real illustrated EPUB. May want: image cache so
   re-opens skip the decode, height cap so a 60-row image doesn't
   dominate a chapter, alignment hints.

3. **Footnote popover.** EPUB notes are typically anchor links to a
   separate endnotes chapter. Detect notereferences and let `f` show
   the note inline as a modal. Requires a cursor concept (which
   noteref to expand) or a list-of-notes popover. Deferred from 1.0.

4. **Text selection / copy.** Mouse drag or vim-style visual mode that
   copies the selected text to the system clipboard. Builds on the
   cursor concept needed for footnotes.

5. **Font selection and size.** Terminals control their own fonts;
   real font control depends on either a graphics protocol (sixel,
   kitty graphics) or a separate GUI mode. Research-blocked.

## Documented limitations

- **Nested HTML tables** are emitted twice (once flattened into the
  parent cell, once as standalone rows) because `emit_table` uses
  `descendants()` for `<tbody>` support. Real fiction doesn't nest
  tables; documented in `src/epub.rs`.

- **DIM modifier on the status bar** is best-effort; some terminals
  (PuTTY, certain TTYs) render it as normal intensity. Not blocking.

- **Cover quality** is roughly 70% — ASCII at thumbnail resolution
  (22×17 cells) cannot match the original artwork. Larger cells
  trade off cells-per-screen. Eventual fix is the graphics protocol
  work in item 5.

## Shipped in 1.0

For reference, items that previous BACKLOG entries flagged that are now
in the released codebase: help overlay, smart resize, hash-based book
keys, configurable width, library scanner, cover ASCII art, search/filter,
TOC overlay, chapter titles from TOC.
