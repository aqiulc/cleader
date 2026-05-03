# Terminal Ebook Reader (Rust)

A high-performance, distraction-free CLI ebook reader built with Rust. This application allows users to manage and read EPUB files directly from the terminal with a sleek TUI (Text User Interface).

## Features
- **Library Management**: Scan a local folder for EPUB files and list them with metadata (Title, Author).
- **ASCII Cover Art**: Automatic extraction of book covers from EPUB metadata, rendered as ASCII art in the terminal.
- **Optimized Reading View**: Paginated text rendering with support for basic formatting (Bold, Italics, Headers).
- **Session Persistence**: Saves your reading progress (last chapter and scroll position) automatically.
- **Vim-inspired Navigation**: Navigate your library and books using familiar keyboard shortcuts.

## Tech Stack
- **Language**: Rust
- **TUI Framework**: [Ratatui](https://github.com/ratatui-org/ratatui)
- **EPUB Parser**: `epub-rs`
- **ASCII Rendering**: `artem` or `image` (custom character mapping)
- **State Management**: `serde` / `serde_json` for progress tracking
