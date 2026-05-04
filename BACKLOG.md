# cleader Backlog

Nice-to-haves and post-v1 work. Open an issue or PR if any of these jump out.

## Open items

1. **Font selection and size customization in the reader.** Terminals control their own fonts, so this depends on either a richer terminal protocol (sixel, kitty graphics) or a separate GUI mode. Research topic — figure out the right rendering layer before scoping.

2. **Help screen / keybinding cheatsheet.** Press `?` to overlay all bindings. Quick reference for newcomers.

3. **Smarter resize.** Track byte-offset-into-source alongside line offset so the same paragraph stays at the top of the viewport when the terminal width changes. v1 keeps line offset constant, so the reader can drift by a few lines on resize.

4. **Footnote popover.** EPUB notes are anchor links to a separate endnotes chapter. Detect notereferences and let `f` show the note inline as a modal, then dismiss back to the page.

5. **Text selection / copy from book.** Mouse drag or vim-style visual mode that copies to the system clipboard.

6. **ASCII art for inline book images.** Re-use the cover-art conversion pipeline (also v2) on `<img>` elements found inside chapter content.

7. **Configurable text width.** Per-user wrap width preference (config file or CLI flag).

8. **Hash-based book keys.** Move-resilient registry: identify books by content hash so moving a file doesn't reset progress.

9. **Distribution via package managers.** Research and ship channels for Homebrew (mac), Chocolatey/Scoop (Windows), `cargo install cleader`, GitHub Releases prebuilt binaries, and Linux packages (AUR, .deb, .rpm). The README installation section grows as channels light up.

## v2 features (always planned)

- Library scanner that walks a folder of EPUBs.
- ASCII cover art rendered from each book's cover image.
- Search/filter inside the library.
- TOC overlay inside the reader.
- Populate `Chapter::title` from `<h1>` or NCX TOC (v1 always sets it to `None`).

## Nested-tables (table descendants caveat)

`emit_table` uses `el.descendants()` to handle `<tbody>` wrapping. A nested `<table>` inside a `<td>` would have its rows emitted twice (once flattened into the outer cell, once as standalone rows). v1 fiction never nests tables; if it ever matters, filter to direct-table descendants.
