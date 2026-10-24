# GoldenKit

## TL;DR

Golden-file testing for Rust, done once: a zero-dependency crate that compares generated output against checked-in golden files with a **readable unified diff** on mismatch, plus `golden-bless` — a review tool for updating goldens deliberately instead of by hand-editing fixtures or deleting tests. Scrubbers tame volatile content (timestamps, temp paths, versions); CRLF/LF differences are normalized away by default.

```rust
// In a test:
golden_kit::assert_golden("tests/golden/help.txt", &render_help());
```

```console
$ cargo test            # a mismatch panics with a diff and leaves help.txt.actual
$ golden-bless          # review every pending change, diffs shown
$ golden-bless --apply  # promote the reviewed .actual files onto their goldens
```

## Why this exists

Every project in the family that grew golden tests re-derived the same machinery by hand — the comparison, the "how do I see what changed", the fixture-updating dance — and the InstallScope settings/job goldens showed what that costs when it's improvised: a default flip changed generated output, the goldens broke, and the failure told nobody *what* had actually changed or whether the change was right (WinDevKit pain point #15). GoldenKit makes the workflow a solved problem:

- **A mismatch shows its work.** The failure is a proper unified diff (context lines, `@@` hunks), not "files differ". You can judge a change in seconds.
- **Blessing is a review, not a reflex.** A failed check leaves the new output in a `<golden>.actual` sidecar. `golden-bless` lists every pending sidecar with its diff — update or create — and promotes them only when you say `--apply`. Nothing updates a golden silently. (For the quick path, `GOLDEN_BLESS=1 cargo test` rewrites goldens in place — same mechanism, no review step; use it when the change is already understood.)
- **Volatile content is scrubbable.** `.scrub("2026-10-23T19:50:43Z", "<TIME>")` replaces timestamps, temp paths, or version strings with stable tokens before comparison; the golden stores the scrubbed form.
- **Bytes are covered too.** `assert_golden_bytes` reports sizes and the first differing offset, and falls back to a text diff when both sides are UTF-8.
- **Cross-platform sane.** Line endings normalize to LF on both sides by default, so a golden written on Windows passes on Linux CI and vice versa.

## The API in one screen

| Need | Call |
|---|---|
| One-shot text check | `golden_kit::assert_golden(path, &text)` |
| One-shot bytes check | `golden_kit::assert_golden_bytes(path, &bytes)` |
| Options + structured result | `Golden::new(path).scrub(a, b).trim_trailing_whitespace(true).check_str(&text)` → `Result<Outcome, Failure>` |
| Bless control | `.bless(true)` per check, or the `GOLDEN_BLESS=1` env var for the whole run |
| The review tool | `golden-bless [--dir PATH]` / `golden-bless --apply` (exit 1 when pendings exist, 0 when clean) |

`Outcome` tells you what a passing check did (`Match`, `Blessed`, `Created`); `Failure` carries the kind (`Missing` / `Mismatch` / `Io`), the diff, and the sidecar path. Add `*.actual` to your `.gitignore` — sidecars are working files, not sources.

## Honest caveats

- **The diff is small-tool grade.** Common affixes are trimmed and an LCS runs over the differing middle; if that middle is enormous (multi-megabyte goldens changing everywhere), it degrades to a single replace block rather than consuming unbounded memory. Golden files are normally small and changes local — that's the case this is built and tested for.
- **Scrubbing is literal replacement**, not regex — zero dependencies is a design rule here (a test tool that drags in a dependency tree defeats its purpose). Order matters: replacements run in the order added.
- **`GOLDEN_BLESS=1` trusts the run.** It rewrites goldens without review; a genuinely wrong change blessed that way becomes the new truth. The sidecar + `golden-bless` flow exists for changes you haven't already judged.

## Verification status

14 tests: the diff engine (identical/single-change/insert/delete, distant changes splitting into two hunks, edge changes), the full check workflow through real files (match clears stale sidecars, mismatch leaves a diff + sidecar, missing-then-bless creates, bless updates, scrub match, CRLF normalization, bytes offset reporting, panic message carries the diff), and the pending-find/promote helpers (including skipping `target/`). The `golden-bless` binary was additionally exercised end-to-end by hand: list shows the diff and exits 1, `--apply` promotes and exits 0. Windows CI runs the full WinDevKit gate — format, clippy, the whole suite, and a real MSVC build.

Building: `cargo build --release` (library + `golden-bless`); `cargo test --locked`. To use it: add `golden-kit` as a dev-dependency of your crate.

License: MIT. Part of the WinDevKit family — see `AGENTS.md` for build discipline and `ROADMAP.md` for what's next.
