# cleader v0.4.7 Implementation Plan — Marquee Long Titles

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Scroll long titles in the selected grid cell so the user can read titles that exceed the 22-col cell width.

**Architecture:** A pure-function `marquee_offset(elapsed_ms, overflow) -> usize` computes the scroll offset over time using a 4-phase cycle (hold-start → scroll-left → hold-end → snap-back). LibraryApp owns an `Instant` (reset when selection changes) and exposes `marquee_offset_for_selection(title_width, max_cols) -> usize`. The renderer in `render_library.rs` uses that offset to slice the selected cell's title before truncation. The event loop sets `needs_redraw` every poll tick if the selected cell has a long title, so the marquee animates at ~20Hz.

**Tech Stack:** Rust, existing crossterm/ratatui/std::time::Instant infrastructure.

---

## File Structure

- Modify: `src/library_app.rs` — `marquee_start` field (`Option<Instant>`), reset on selection change, accessor `marquee_offset_for_selection(title_width, max_cols) -> usize`
- Modify: `src/render_library.rs` — `LibraryRenderInput.marquee_offset: usize`, apply to selected cell's title slicing
- Modify: `src/main.rs` — `library_event_loop` snapshots marquee_offset for the selected cell, force needs_redraw=true each poll when marquee is active
- Modify: `tests/integration.rs` — no change (existing search test passes through)

---

## Task 1: Add `marquee_offset` pure function + `MarqueeTiming` constants in library_app

**Files:**
- Modify: `src/library_app.rs`

- [ ] **Step 1: Add the pure-function helper + constants near the top of `impl LibraryApp` (above `new`)**

Add at the top of `src/library_app.rs` (after the imports), as module-level items:

```rust
/// Time to hold at the start of a long title before scrolling begins,
/// so the user can read the beginning. 1000 ms.
const MARQUEE_HOLD_START_MS: u128 = 1000;

/// Time to hold at the end of a long title after the tail is visible,
/// so the user can read the end. 1000 ms.
const MARQUEE_HOLD_END_MS: u128 = 1000;

/// Time per character of scroll. 250 ms = 4 chars/sec — slow enough
/// to read mid-scroll.
const MARQUEE_PER_CHAR_MS: u128 = 250;

/// Pure scroll-offset calculator. Given the number of milliseconds
/// since the marquee started and the total characters the title
/// overflows the cell by, returns the current left-shift offset
/// (0..=overflow).
///
/// Cycle phases (in order):
/// 1. Hold at start (offset = 0) for MARQUEE_HOLD_START_MS
/// 2. Scroll left, one char per MARQUEE_PER_CHAR_MS, until offset == overflow
/// 3. Hold at end (offset = overflow) for MARQUEE_HOLD_END_MS
/// 4. Snap back to 0, repeat
///
/// If `overflow == 0` the function always returns 0 (no-op).
pub fn marquee_offset(elapsed_ms: u128, overflow: usize) -> usize {
    if overflow == 0 {
        return 0;
    }
    let scroll_ms = (overflow as u128) * MARQUEE_PER_CHAR_MS;
    let cycle_ms = MARQUEE_HOLD_START_MS + scroll_ms + MARQUEE_HOLD_END_MS;
    let t = elapsed_ms % cycle_ms;
    if t < MARQUEE_HOLD_START_MS {
        0
    } else if t < MARQUEE_HOLD_START_MS + scroll_ms {
        ((t - MARQUEE_HOLD_START_MS) / MARQUEE_PER_CHAR_MS) as usize
    } else {
        overflow
    }
}
```

- [ ] **Step 2: Add tests for the pure function**

Append to the existing `#[cfg(test)] mod tests` block in `src/library_app.rs`:

```rust
    #[test]
    fn marquee_offset_returns_zero_when_no_overflow() {
        assert_eq!(super::marquee_offset(0, 0), 0);
        assert_eq!(super::marquee_offset(99999, 0), 0);
    }

    #[test]
    fn marquee_offset_holds_at_start_for_one_second() {
        // At t < 1000ms with overflow=5, offset is 0 (hold-start phase).
        assert_eq!(super::marquee_offset(0, 5), 0);
        assert_eq!(super::marquee_offset(500, 5), 0);
        assert_eq!(super::marquee_offset(999, 5), 0);
    }

    #[test]
    fn marquee_offset_scrolls_one_char_per_step() {
        // From t=1000 to t=1000+5*250=2250 the offset ramps 0..5.
        // At t=1000 → offset 0 (first scroll tick)
        // At t=1250 → offset 1
        // At t=1500 → offset 2
        // At t=2249 → offset 4 (last scroll tick before end-hold)
        assert_eq!(super::marquee_offset(1000, 5), 0);
        assert_eq!(super::marquee_offset(1250, 5), 1);
        assert_eq!(super::marquee_offset(1500, 5), 2);
        assert_eq!(super::marquee_offset(2249, 5), 4);
    }

    #[test]
    fn marquee_offset_holds_at_end() {
        // From t=2250 (after scroll completes) for 1000ms, offset = overflow.
        assert_eq!(super::marquee_offset(2250, 5), 5);
        assert_eq!(super::marquee_offset(2500, 5), 5);
        assert_eq!(super::marquee_offset(3249, 5), 5);
    }

    #[test]
    fn marquee_offset_cycles() {
        // At t = cycle_ms (3250) it wraps to 0 again.
        let cycle_ms = 1000 + 5 * 250 + 1000;
        assert_eq!(super::marquee_offset(cycle_ms, 5), 0);
        assert_eq!(super::marquee_offset(cycle_ms + 500, 5), 0);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --quiet 2>&1 | grep "test result"` — expect 233 unit + 11 integration (was 228 + 11, +5 marquee tests).
Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -5` — clean.

- [ ] **Step 4: Commit**

```bash
git add src/library_app.rs
git commit -m "feat(library_app): add marquee_offset pure function

4-phase cycle (hold-start 1s → scroll-left 250ms/char → hold-end 1s
→ snap back) for animating long titles in grid cells. Pure function
so it can be unit-tested without time mocks. Wired into LibraryApp
state + renderer in follow-up commits.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Add marquee Instant tracking in LibraryApp + selection-change reset

**Files:**
- Modify: `src/library_app.rs`

- [ ] **Step 1: Add imports**

Add to top-of-file imports in `src/library_app.rs` (if not already present):

```rust
use std::time::Instant;
```

- [ ] **Step 2: Add fields and reset hook**

Add a field to `LibraryApp` struct (place it near `pre_search_selection`):

```rust
    /// When the currently-selected cell started its marquee animation.
    /// Reset to `Some(Instant::now())` whenever `selection` changes (so
    /// every new selection begins at the start-of-cycle hold). `None`
    /// only at construction before the first selection change.
    marquee_start: Option<Instant>,
```

In both `new_with` constructors (the public path delegates to `new_with`), initialize: `marquee_start: Some(Instant::now()),`

- [ ] **Step 3: Add a `set_selection` private helper that all selection changes route through**

Currently the `handle` arms mutate `self.selection` directly. Centralize:

```rust
    /// Set the selection and restart the marquee animation. All nav
    /// arms in `handle` should go through this helper rather than
    /// mutating `self.selection` directly.
    fn set_selection(&mut self, new_selection: usize) {
        if new_selection != self.selection {
            self.selection = new_selection;
            self.marquee_start = Some(Instant::now());
        }
    }
```

Update every `self.selection = ...;` in `handle` (LineUp/LineDown/PagePrev/PageNext arms) to use `self.set_selection(...)`. The patterns become:

```rust
            Action::LineUp => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let cols = self.grid_cols();
                    let new = self.selection.saturating_sub(cols);
                    self.set_selection(new);
                } else if self.selection > 0 {
                    self.set_selection(self.selection - 1);
                }
            }
            Action::LineDown => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let cols = self.grid_cols();
                    let max = self.display_indices().len().saturating_sub(1);
                    let new = (self.selection + cols).min(max);
                    self.set_selection(new);
                } else if self.selection + 1 < self.display_indices().len() {
                    self.set_selection(self.selection + 1);
                }
            }
            Action::PagePrev => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    if self.selection > 0 {
                        self.set_selection(self.selection - 1);
                    }
                } else {
                    let step = self.lines_per_page().min(10);
                    let new = self.selection.saturating_sub(step);
                    self.set_selection(new);
                }
            }
            Action::PageNext => {
                if matches!(self.view_mode, ViewMode::Grid) {
                    let max = self.display_indices().len().saturating_sub(1);
                    if self.selection < max {
                        self.set_selection(self.selection + 1);
                    }
                } else {
                    let step = self.lines_per_page().min(10);
                    let max = self.display_indices().len().saturating_sub(1);
                    let new = (self.selection + step).min(max);
                    self.set_selection(new);
                }
            }
```

Also update `open_search`'s `self.selection = 0` to `self.set_selection(0)`, `clear_search`'s `self.selection = self.pre_search_selection` to `self.set_selection(self.pre_search_selection)`, and the `Backspace` and Char arms in `handle_search_input` that do `self.selection = 0` to `self.set_selection(0)`.

Also `Action::Resize` updates viewport but doesn't touch selection — leave alone.

- [ ] **Step 4: Add public accessor for renderer**

Add to `impl LibraryApp` near the other accessors:

```rust
    /// Elapsed milliseconds since the marquee started. Renderer uses
    /// this with the current title's overflow to compute the scroll
    /// offset via `marquee_offset`. Returns 0 if marquee is somehow
    /// unset (defensive — should always be Some after construction).
    pub fn marquee_elapsed_ms(&self) -> u128 {
        self.marquee_start
            .map(|start| start.elapsed().as_millis())
            .unwrap_or(0)
    }
```

- [ ] **Step 5: Add a test verifying selection-change resets marquee**

Append to `mod tests`:

```rust
    #[test]
    fn selection_change_resets_marquee_start() {
        let mut app = LibraryApp::new_with(
            vec![entry("A"), entry("B"), entry("C")],
            (80, 24),
            None,
            None,
        );
        // Toggle to list mode so LineDown moves by 1 deterministically.
        app.handle(Action::ToggleViewMode);
        let before = app.marquee_elapsed_ms();
        // Sleep briefly to ensure elapsed advances.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed_pre_nav = app.marquee_elapsed_ms();
        assert!(elapsed_pre_nav >= before, "elapsed should not decrease");
        // Navigate — should reset marquee_start.
        app.handle(Action::LineDown);
        let elapsed_post_nav = app.marquee_elapsed_ms();
        assert!(
            elapsed_post_nav < elapsed_pre_nav,
            "selection change should reset marquee_start; got pre={elapsed_pre_nav} post={elapsed_post_nav}"
        );
    }
```

- [ ] **Step 6: Run tests + clippy**

Run: `cargo test --quiet 2>&1 | grep "test result"` — expect 234 unit + 11 integration.
Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -5` — clean.

- [ ] **Step 7: Commit**

```bash
git add src/library_app.rs
git commit -m "feat(library_app): track marquee timing, reset on selection change

LibraryApp gains a marquee_start: Option<Instant>, refreshed whenever
selection moves (via a new private set_selection helper that all nav
arms route through). marquee_elapsed_ms is the renderer-facing
accessor.

Pairs with marquee_offset to compute the per-frame scroll offset for
the selected cell's title.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Wire marquee into render_library_grid + library_event_loop

**Files:**
- Modify: `src/render_library.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Extend `LibraryRenderInput`**

In `src/render_library.rs`, add a new field to `LibraryRenderInput`:

```rust
    /// Marquee scroll offset for the SELECTED cell's title only.
    /// 0 in Idle/non-marquee state. Renderer slices the selected
    /// cell's title from this index before truncating to the cell
    /// width. Caller (event loop) computes this each frame from
    /// LibraryApp::marquee_elapsed_ms and the selected cell's title
    /// overflow.
    pub marquee_offset: usize,
```

- [ ] **Step 2: Apply the offset in `render_library_grid`**

Find the per-cell rendering loop. Where the title is currently:

```rust
            let title_truncated = truncate_to_width(&entry.title, crate::cover_cache::COVER_THUMBNAIL_WIDTH as usize);
```

Change to apply the marquee slice ONLY for the selected cell:

```rust
            let title_for_render: String = if is_selected && input.marquee_offset > 0 {
                // Apply marquee scroll: skip the first `marquee_offset`
                // characters before truncating to cell width.
                entry.title.chars().skip(input.marquee_offset).collect()
            } else {
                entry.title.clone()
            };
            let title_truncated = truncate_to_width(&title_for_render, crate::cover_cache::COVER_THUMBNAIL_WIDTH as usize);
```

The `is_selected && offset > 0` guard ensures non-selected cells render their title from the start, and the selected cell only does the skip-then-truncate dance when actually scrolling (offset=0 during hold-start phase falls through to the simple path).

- [ ] **Step 3: Update existing render_library tests' call sites with the new field**

Every test in `src/render_library.rs` that constructs `LibraryRenderInput { ... }` needs `marquee_offset: 0,` added. The existing 11 tests all have `LibraryRenderInput { ... }` literals. Add `marquee_offset: 0,` to each.

Also update the integration test in `tests/integration.rs` (the grid smoke test's `LibraryRenderInput { ... }` literal — add `marquee_offset: 0,`).

- [ ] **Step 4: Add a renderer test for marquee scroll**

Append to `mod tests` in `src/render_library.rs`:

```rust
    #[test]
    fn marquee_offset_shifts_selected_title() {
        let backend = TestBackend::new(80, 40);
        let mut term = Terminal::new(backend).unwrap();
        // Single entry with a very long title.
        let long_title = "AAAAAA_BBBBBB_CCCCCC_DDDDDD_EEEEEE";  // 34 chars, overflow=12
        let entries = vec![LibraryEntry {
            path: PathBuf::from("/long.epub"),
            title: long_title.to_string(),
            author: "Anon".to_string(),
        }];
        let book_ids = vec![None];
        let display_indices = vec![0];

        // First: render with marquee_offset = 0 (start of cycle).
        term.draw(|frame| {
            let area = frame.area();
            render_library(
                frame,
                area,
                LibraryRenderInput {
                    entries: &entries,
                    selection: 0,
                    view_mode: ViewMode::Grid,
                    cover_cache: None,
                    book_ids: &book_ids,
                    warning: None,
                    display_indices: &display_indices,
                    search_query: None,
                    search_mode: crate::search::SearchMode::Idle,
                    marquee_offset: 0,
                },
            );
        })
        .unwrap();
        let buf_zero: String = term.backend().buffer().content.iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf_zero.contains("AAAAAA"), "offset=0 should show title start");

        // Re-render with marquee_offset = 7 (mid-scroll: 'B's onward).
        term.draw(|frame| {
            let area = frame.area();
            render_library(
                frame,
                area,
                LibraryRenderInput {
                    entries: &entries,
                    selection: 0,
                    view_mode: ViewMode::Grid,
                    cover_cache: None,
                    book_ids: &book_ids,
                    warning: None,
                    display_indices: &display_indices,
                    search_query: None,
                    search_mode: crate::search::SearchMode::Idle,
                    marquee_offset: 7,
                },
            );
        })
        .unwrap();
        let buf_seven: String = term.backend().buffer().content.iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf_seven.contains("BBBBBB"), "offset=7 should reveal characters after the first 7");
    }
```

- [ ] **Step 5: Update `library_event_loop` in `src/main.rs`**

Find the snapshot block before `terminal.draw`. Add a marquee_offset computation:

```rust
            // Compute marquee offset for the currently-selected cell.
            // The selected cell is at display_indices[selection] →
            // entry_idx → entries[entry_idx].title. Overflow is title's
            // char count minus the cell content width (22).
            let marquee_offset_val: usize = if matches!(view_mode, cleader::prefs::ViewMode::Grid) {
                let display = app.display_indices();
                let title_overflow = display.get(selection).and_then(|&entry_idx| {
                    app.entries().get(entry_idx).map(|e| {
                        let cell_w = cleader::cover_cache::COVER_THUMBNAIL_WIDTH as usize;
                        e.title.chars().count().saturating_sub(cell_w)
                    })
                }).unwrap_or(0);
                cleader::library_app::marquee_offset(app.marquee_elapsed_ms(), title_overflow)
            } else {
                0
            };
```

Place this just before the `terminal.draw(|frame| { ... })` block, after the other snapshots like `search_query_owned`.

Pass `marquee_offset: marquee_offset_val` into the `LibraryRenderInput { ... }` call.

Then update the bottom of the loop to force a redraw when the selected cell has overflow (so the marquee animates):

After the `event::poll(...)` branch, add a marquee tick:

```rust
        // If the selected cell has a long title, force redraw next
        // iteration so the marquee can advance.
        let selected_overflow = if matches!(app.view_mode(), cleader::prefs::ViewMode::Grid) {
            let display = app.display_indices();
            display.get(app.selection()).and_then(|&entry_idx| {
                app.entries().get(entry_idx).map(|e| {
                    let cell_w = cleader::cover_cache::COVER_THUMBNAIL_WIDTH as usize;
                    e.title.chars().count().saturating_sub(cell_w)
                })
            }).unwrap_or(0)
        } else {
            0
        };
        if selected_overflow > 0 {
            needs_redraw = true;
        }
```

This goes at the END of the loop body (after the input branch).

- [ ] **Step 6: Build + clippy + tests**

Run:
```
cargo build 2>&1 | tail -5
cargo test --quiet 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo doc --no-deps 2>&1 | grep -i warning
cargo build --release 2>&1 | tail -5
```
Expected: 235 unit + 11 integration (+1 renderer test). Clean across the board.

- [ ] **Step 7: Commit**

```bash
git add src/render_library.rs src/main.rs tests/integration.rs
git commit -m "feat(render_library, main): wire marquee for selected long titles

LibraryRenderInput gains marquee_offset; render_library_grid applies
it to the selected cell's title before truncation (skip-then-truncate)
so the title scrolls visibly. Non-selected cells render their titles
from char 0 — only the focused cell animates.

library_event_loop computes the offset each frame from
LibraryApp::marquee_elapsed_ms and the selected title's overflow, and
forces needs_redraw=true when the selected title overflows so the
~20Hz poll tick paces the animation. Cells with titles that fit
behave exactly as before (no extra redraws, idle CPU stays low).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review Checklist (after all tasks)

1. `cargo test --quiet 2>&1 | grep "test result"` — confirm 235 unit + 11 integration.
2. `cargo clippy --all-targets -- -D warnings` — clean.
3. `cargo doc --no-deps 2>&1 | grep -i warning` — no doc warnings.
4. `cargo build --release` — succeeds.
5. Manual smoke test: `cargo run --release -- some/directory/`:
   - Navigate to a book with a long title — title should scroll after a 1s hold
   - Navigate to a different book — its title (if long) should restart at the beginning
   - Navigate to a book with a short title — no scrolling, no extra CPU
   - In list mode (`g` toggle) — no marquee (only grid mode applies it)

## Spec Coverage Map

| Spec section | Covered by |
|---|---|
| `marquee_offset` pure function | Task 1 |
| 4-phase cycle (hold-start → scroll → hold-end → snap) | Task 1 |
| `marquee_start` Instant tracking | Task 2 |
| Selection-change resets marquee | Task 2 (set_selection helper) |
| `LibraryRenderInput.marquee_offset` | Task 3 |
| Selected cell only animates | Task 3 (is_selected guard) |
| Event-loop redraw cadence for active marquee | Task 3 (needs_redraw = true) |
| Tests: pure function, selection reset, renderer | Tasks 1, 2, 3 |
