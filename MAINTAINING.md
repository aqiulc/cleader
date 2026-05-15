# Maintaining cleader

Notes for the maintainer (or anyone forking this project and wanting to
ship updates the same way). For day-to-day contribution flow, see
[CONTRIBUTING.md](CONTRIBUTING.md).

## Where everything lives

| Thing | Location |
|---|---|
| Source repo | https://github.com/aqiulc/cleader |
| Crate on crates.io | https://crates.io/crates/cleader |
| Generated API docs | https://docs.rs/cleader |
| GitHub Releases (binaries) | https://github.com/aqiulc/cleader/releases |
| Homebrew tap | https://github.com/aqiulc/homebrew-cleader |
| Release workflow | `.github/workflows/release.yml` |

## What ships, in order

A new version of cleader appears on three channels:

1. **Cargo / crates.io** — `cargo install cleader` and as a library
   dependency. Source-only; users compile on their machine.
2. **GitHub Releases** — prebuilt binaries for Linux x86_64, Linux ARM64,
   macOS Apple Silicon, Windows x86_64. No Rust required.
3. **Homebrew** — `brew install aqiulc/cleader/cleader`. Uses the
   prebuilt binaries on supported targets, falls back to a `cargo`
   source build on Intel macOS.

The release workflow handles steps 1 and 2 automatically once a `v*`
tag is pushed. The Homebrew formula update is currently manual.

## Release checklist

For a new version (call it `1.2.3` — replace as needed):

### 1. Land all code changes on `main`

- Update `Cargo.toml` `version = "1.2.3"`
- Add a new section at the top of `CHANGELOG.md` describing what changed
- Commit and push to `main`
- Confirm `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo doc --no-deps`, and `cargo build --release` all pass

### 2. Tag and push

```bash
git tag -a v1.2.3 -m "cleader 1.2.3 — <short summary>"
git push origin v1.2.3
```

Pushing the tag triggers `.github/workflows/release.yml`. The
workflow takes about 3–5 minutes and creates a GitHub Release with
the binary archives attached. Watch progress at:

```bash
gh run list --workflow=release.yml --limit=1
```

### 3. Publish to crates.io

Once the binaries are up:

```bash
cargo publish
```

This uses the locally-stored crates.io token from `~/.cargo/credentials.toml`
(established once via `cargo login`). If the token has expired, regenerate
at https://crates.io/settings/tokens with scopes `publish-new`,
`publish-update`, `yank`, restricted to crate `cleader`.

**Publishing is irrevocable.** Once `1.2.3` is on crates.io it cannot be
deleted or re-uploaded; the only mitigation is `cargo yank cleader@1.2.3`,
which marks the version as "do not use for new resolution" without breaking
existing pinned consumers.

Always run `cargo publish --dry-run` first to validate.

### 4. Update the Homebrew tap

In a checkout of `https://github.com/aqiulc/homebrew-cleader`:

1. Compute SHA256s for the new release assets:

```bash
for asset in \
  cleader-aarch64-apple-darwin.tar.gz \
  cleader-aarch64-unknown-linux-gnu.tar.gz \
  cleader-x86_64-unknown-linux-gnu.tar.gz
do
  curl -sL -o /tmp/$asset \
    https://github.com/aqiulc/cleader/releases/download/v1.2.3/$asset
done
curl -sL -o /tmp/cleader-crates-io.tar.gz \
  https://crates.io/api/v1/crates/cleader/1.2.3/download
shasum -a 256 /tmp/cleader-*
```

2. In `Formula/cleader.rb`, bump:
   - `version "1.2.3"`
   - Every `url` line — replace `v1.0.0` with `v1.2.3` (and `1.0.0` in the
     crates.io URL)
   - Every `sha256` line — paste the new values from step 1

3. Smoke test locally:

```bash
brew style ./Formula/cleader.rb     # syntax check (may need ruby gem fix)
brew uninstall cleader               # clear the old install
brew untap aqiulc/cleader            # clear the cached tap
brew tap aqiulc/cleader              # re-add (now points at your local? no — re-clones from GitHub after push)
# Better: commit + push first, then re-tap and install
```

4. Commit and push the formula. Users who have already tapped get the
   update on their next `brew update`.

## Credentials and where they live

| Credential | Where | What it's for |
|---|---|---|
| `gh auth` token | macOS keychain (set via `gh auth login`) | Push commits, create releases, manage the repo |
| crates.io API token | `~/.cargo/credentials.toml` (set via `cargo login`) | `cargo publish` |
| Local git identity | `git config user.email aqiul.c@gmail.com` (`--local` in this repo) | Commit attribution stays on the personal account, not the work email |

If `~/.cargo/credentials.toml` is missing or the token is rejected, regenerate
on crates.io and `cargo login` again.

## Common gotchas

- **`macos-13` (Intel macOS) runner queue is slow.** The release matrix
  dropped Intel macOS for v1.0 because the runner sat queued for 15+
  minutes. The crates.io path covers Intel Mac users; revisit if a
  prebuilt binary becomes worth the wait, or set up a cross-compile
  from the Apple Silicon runner.
- **Force-pushing a tag re-runs the release workflow.** Useful if a
  release needs a re-build, but it doesn't re-publish to crates.io
  (that step is manual) and it doesn't replace the Homebrew formula
  (also manual). Force-pushing rewrites the SHA the tag points to —
  the published crate's `.cargo_vcs_info.json` will reference the
  original SHA, which is informational only.
- **GitHub's `/stats/contributors` endpoint is async after a force-push
  and can take hours to repopulate.** The data is correct (visible via
  `/contributors`); the UI sidebar shows stale data until the cache
  refreshes. Force a refresh by visiting
  `https://github.com/aqiulc/cleader/graphs/contributors` in a browser.
- **Cover cache version.** `COVER_CACHE_VERSION` in `src/cover_cache.rs`
  must be bumped whenever the rendered output bytes change (algorithm,
  dimensions, gradient). Otherwise users see stale covers on relaunch.

## Yank / rollback

If a published version has a serious bug:

```bash
cargo yank cleader@1.2.3                       # mark as do-not-use
# (optionally) cargo unyank cleader@1.2.3      # if the report was wrong
```

Yanking keeps the version downloadable (existing users keep working) but
removes it from the default resolution. Ship a fixed `1.2.4` ASAP.

For Homebrew, push a corrective commit to the tap that pins back to
`1.2.2` (or whatever the last good version is) until the fix ships.

## Future polish (not blocking releases)

- **Auto-bump the Homebrew formula** on every new tag: a GitHub Action
  in the cleader repo that computes new SHAs and opens a PR in
  `homebrew-cleader`. Saves the manual step above.
- **Prebuilt Intel macOS binary**: either wait out the runner queue,
  or add a step on the `macos-latest` (Apple Silicon) runner that
  cross-compiles to `x86_64-apple-darwin` (Apple's toolchain supports
  this natively).
- **CI on PRs.** Add a second workflow that runs `cargo test` and
  `cargo clippy` on every PR push (today there's only the release
  workflow, which fires on tag push). Recommended before accepting
  any external contributions.
- **Submit to homebrew-core.** Once cleader has visible traction (~50+
  GitHub stars is the rough threshold homebrew-core uses), opening a
  PR to add `cleader` to homebrew-core means users no longer need the
  custom tap — they can just `brew install cleader`.
