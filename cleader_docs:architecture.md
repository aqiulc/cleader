# Architecture Overview

## Component Breakdown

### 1. The Core (State Machine)
The application will follow the **App-Model-Update** pattern common in Rust TUIs.
- **Model**: Holds the current library list, the active book content, and the current UI state (Library vs. Reader).
- **Update**: Handles keyboard events and updates the Model.
- **View**: Renders the Model using Ratatui widgets.

### 2. The EPUB Worker
A background worker (or simple module) that uses `epub-rs` to:
- Open the container.
- Parse the OEBPS structure.
- Provide a stream of chapter content to the Reader View.

### 3. ASCII Art Pipeline
1. **Extract**: Pull the `cover.jpg` (or equivalent) from the EPUB zip.
2. **Decode**: Use the `image` crate to load the raw pixels.
3. **Resize**: Shrink the image to fit the terminal pane (e.g., 40x40 characters).
4. **Convert**: Map pixel brightness to a character set (e.g., `.` `*` `#` `@`).

## Data Flow
1. **Startup**: Scan folder -> Build Metadata List -> Load Progress.
2. **User Interaction**: Select Book -> Trigger ASCII Conversion -> Load Chapter 1.
3. **Reading**: Update 'Last Read' offset on every page turn.
