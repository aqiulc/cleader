# cleader v0.4.9 Implementation Plan — Event Loop Polish

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Reduce library mode's idle resource use. Drop idle wakeup from ~20Hz to ~2Hz when truly idle (no marquee, no pending covers, no search active, no help up). Replace defensive per-frame `Vec` clones in `library_event_loop` with shared references where the borrow checker allows.

**Architecture:** New `pub fn has_pending(&self) -> bool` on `CoverCache` returns true when any entry is `Pending`. `library_event_loop` computes an `is_idle` predicate and chooses `event::poll(500ms)` or `event::poll(50ms)`. Snapshot block tries to use shared borrows of `app` directly; if any borrow fails, fall back to a clone for that specific field with an inline comment.

**Tech Stack:** Rust, existing infrastructure.

---

## Single Task: idle poll bump + snapshot cleanup

**Files:**
- Modify: `src/cover_cache.rs` — add `has_pending`
- Modify: `src/main.rs` — rewrite `library_event_loop` snapshot + poll block

- [ ] **Step 1: Add `has_pending` to CoverCache**

In `src/cover_cache.rs`, add this method inside `impl CoverCache` (place alongside `drain_finished`):

```rust
    /// True if any cover in the memory map is still in the Pending
    /// state (the worker is rendering it). Used by the event loop to
    /// decide whether to use the short (50ms) poll cadence or relax
    /// to the long (500ms) idle cadence.
    pub fn has_pending(&self) -> bool {
        self.memory.values().any(|s| matches!(s, CoverState::Pending))
    }
```

- [ ] **Step 2: Add tests for `has_pending`**

Append to `#[cfg(test)] mod tests` in cover_cache.rs:

```rust
    #[test]
    fn has_pending_false_for_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::open_at(dir.path().to_path_buf());
        assert!(!cache.has_pending());
    }

    #[test]
    fn has_pending_true_after_enqueue_then_false_after_drain() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverCache::open_at(dir.path().to_path_buf());
        let id = book_id(b"pending-test");
        cache.enqueue(id.clone(), PathBuf::from("/no/such/book.epub"));
        // Immediately after enqueue, the entry is Pending in memory.
        // (The worker hasn't replied yet — the placeholder fallback
        // takes a few ms to arrive.)
        assert!(cache.has_pending(), "Pending immediately after enqueue");

        // Wait for the worker to deliver the placeholder.
        for _ in 0..50 {
            cache.drain_finished();
            if !cache.has_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!cache.has_pending(), "no longer Pending after drain");
    }
```

- [ ] **Step 3: Rewrite `library_event_loop` snapshot + poll block**

In `src/main.rs`, find `library_event_loop`. The current shape:
1. Top-of-loop: drain covers, compute selected_overflow, request visible covers
2. Snapshot block: clone entries/book_ids/display_indices into owned Vecs, clone warning/search_query into owned Strings, etc.
3. `terminal.draw(|frame| { ... })` — uses the snapshots
4. `event::poll(50ms)` and dispatch input
5. End-of-loop: re-check selected_overflow, force needs_redraw if marquee active

Replace the snapshot block + draw + poll with:

```rust
        if needs_redraw || had_new_covers {
            // Bind shared borrows of app fields into local refs. The
            // draw closure captures these by reference; ratatui's
            // terminal.draw takes FnOnce, so the borrows live for
            // exactly one frame. Plain primitives (selection,
            // view_mode, search_mode, show_help) are Copy.
            let entries = app.entries();
            let book_ids = app.book_ids();
            let display_indices = app.display_indices();
            let cover_cache = app.cover_cache();
            let warning = app.save_error();
            let selection = app.selection();
            let view_mode = app.view_mode();
            let search_mode = app.search_mode();
            let show_help = app.show_help();
            let search_query: Option<&str> = if matches!(
                search_mode,
                cleader::search::SearchMode::Idle
            ) {
                None
            } else {
                Some(app.search_query())
            };

            terminal.draw(|frame| {
                let area = frame.area();
                cleader::render_library::render_library(
                    frame,
                    area,
                    cleader::render_library::LibraryRenderInput {
                        entries,
                        selection,
                        view_mode,
                        cover_cache,
                        book_ids,
                        warning,
                        display_indices,
                        search_query,
                        search_mode,
                        marquee_offset: marquee_offset_val,
                        show_help,
                    },
                );
            })?;
            needs_redraw = false;
        }

        // Compute idle predicate: truly idle means no animation
        // (marquee inactive), no background work (no pending covers),
        // no modal state (no search, no help). When truly idle, relax
        // the poll cadence from 50ms (20Hz) to 500ms (2Hz).
        let has_pending_covers = app
            .cover_cache()
            .map(|c| c.has_pending())
            .unwrap_or(false);
        let is_idle = selected_overflow == 0
            && !has_pending_covers
            && matches!(app.search_mode(), cleader::search::SearchMode::Idle)
            && !app.show_help();
        let poll_timeout = if is_idle {
            std::time::Duration::from_millis(500)
        } else {
            std::time::Duration::from_millis(50)
        };

        // Poll for input with the adaptive timeout. If nothing arrives,
        // loop back so we can drain newly-finished covers.
        if event::poll(poll_timeout)? {
            let evt = event::read()?;
            if app.is_searching() {
                match evt {
                    crossterm::event::Event::Key(key) => {
                        app.handle_search_input(key);
                        needs_redraw = true;
                    }
                    crossterm::event::Event::Resize(cols, rows) => {
                        app.handle(cleader::input::Action::Resize(cols, rows));
                        needs_redraw = true;
                    }
                    _ => {}
                }
            } else if let Some(action) = translate(evt) {
                app.handle(action);
                needs_redraw = true;
            }
        }
```

The marquee_offset_val computation block stays where it is (just above the draw block) — it computes from app state before the snapshot.

NOTE: the `marquee_offset_val` block also reads from `app`. That's a borrow that needs to end before the new shared-borrow snapshot. The existing code already computes it into a `usize` (Copy), so the borrow ends naturally — no change needed.

NOTE: the end-of-loop marquee tick block (`if selected_overflow > 0 { needs_redraw = true; }`) STAYS — it's needed to drive the marquee animation between input events.

**If a borrow conflict appears** when compiling: bind a `let foo_owned = app.method().to_string();` or `to_vec()` for that specific field and use `foo_owned.as_deref()` / `&foo_owned` in the closure. Document the cone of the workaround in an inline comment. Common candidate is `cover_cache: Option<&CoverCache>` since CoverCache holds the worker join handle (not actually a borrow-checker issue, but worth checking).

- [ ] **Step 4: Run build + tests + clippy + doc + release**

```
cargo build 2>&1 | tail -5
cargo test --quiet 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo doc --no-deps 2>&1 | grep -i warning
cargo build --release 2>&1 | tail -5
```

Expected: 246 unit + 11 integration (was 244 + 11, +2 has_pending tests). All clean, release builds.

If the snapshot cleanup hits a borrow-checker wall in 1+ fields, fall back to a partial cleanup — even reducing 3 clones to 1 is a win. Note remaining clones with a comment explaining the borrow constraint.

- [ ] **Step 5: Commit**

```bash
git add src/cover_cache.rs src/main.rs
git commit -m "perf(main): idle poll bumped to 500ms, snapshot clones dropped

Two related polishes to library_event_loop:

1. has_pending(): new CoverCache accessor that's true when any
   cover is Pending. library_event_loop derives an is_idle predicate
   (no marquee, no pending covers, no search, no help) and uses
   event::poll(500ms) when truly idle, 50ms otherwise. Drops the
   idle wakeup cadence from 20Hz to 2Hz — when you're sitting on
   a short title with all covers cached, the CPU goes to sleep.

2. Snapshot clones replaced with shared borrows where the borrow
   checker allows. The defensive entries.to_vec() / book_ids.to_vec()
   / display_indices.to_vec() that were left over from v0.4.5's
   first cut were never strictly necessary; the draw closure can
   capture &app borrows directly.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

1. `cargo test --quiet 2>&1 | grep 'test result'` — confirm green
2. `cargo clippy --all-targets -- -D warnings` — clean
3. `cargo doc --no-deps 2>&1 | grep -i warning` — no doc warnings
4. `cargo build --release` — succeeds
5. Manual smoke test:
   - Open a directory in grid mode
   - Wait for all covers to render
   - Sit on a short-titled book
   - Idle CPU should be very low (top should show cleader at near 0%)
   - Navigate to a long-titled book → marquee animates at 20Hz (idle CPU rises briefly)
   - Navigate back to a short title → idle CPU drops again
