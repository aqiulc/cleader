# Product Requirements & Specifications

## 1. Functional Requirements

### 1.1 Library Scanner
- The app shall accept a directory path as an argument or via a configuration file.
- It must recursively find all `.epub` files.
- It must extract the `Title` and `Creator` (Author) from the Dublin Core metadata in the `.opf` file.

### 1.2 The "Cover Gallery"
- Upon selecting a book in the library view, the app will attempt to locate the cover image identified in the manifest.
- The image will be downscaled and converted to ASCII characters using a grayscale-to-character mapping.
- The ASCII art will be displayed in a sidebar or a "Preview" pane.

### 1.3 Reader Engine
- **Content Extraction**: Extract HTML/XHTML chapters.
- **Reflowable Text**: Text must wrap according to the terminal width.
- **Pagination**: The app must calculate "pages" based on the terminal height to allow for clean scrolling.

### 1.4 Persistence
- The app will create a hidden directory `~/.termiread/`.
- A `registry.json` will store the mapping of book file paths to the last read position (index of the chapter and character offset).

## 2. User Experience (UX)

### Navigation Shortcuts
- `j / k` or `Up / Down`: Navigate lists or scroll text.
- `Enter`: Open selected book.
- `b`: Back to library.
- `/`: Search/Filter library.
- `q`: Quit application.
