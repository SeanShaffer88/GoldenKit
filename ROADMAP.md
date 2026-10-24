# GoldenKit Roadmap

## v0.1.0 (current)
Golden-file checks for text and bytes: unified-diff failures, `.actual` sidecars, `GOLDEN_BLESS=1` in-place blessing, literal scrubbers, CRLF normalization, and the `golden-bless` review/promote tool.

## Next, in order
1. **Built-in volatile scrubs**: one-call helpers for the patterns every project scrubs by hand — ISO-8601 timestamps, UUIDs, temp-dir paths — implemented as small hand-rolled matchers (still zero-dependency).
2. **Directory goldens**: compare a whole generated tree (docs sites, codegen output) against a golden directory with a per-file report, reusing the sidecar/bless flow per file.
3. **`golden-bless --check` for CI**: fail a build when stale `.actual` sidecars are lying around unreviewed (today that's a find_pending one-liner; make it a first-class mode).

## Explicitly not planned
- Regex scrubbing: it would break the zero-dependency rule, and literal replacement has covered every real case so far.
- A test-framework plugin/runner: GoldenKit is a library your tests call, not a harness that owns them.
