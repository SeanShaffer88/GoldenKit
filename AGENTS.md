# GoldenKit — agent notes

- Zero dependencies is a hard rule for this crate (it's in the README and the Cargo.toml comment): scrubbing stays literal replacement, the diff stays hand-rolled. A PR adding a dependency needs a written justification.
- Bless semantics are the product: a failed check MUST leave a `.actual` sidecar; `golden-bless` promotes only with `--apply`; `GOLDEN_BLESS=1` is the only in-test rewrite path. Don't add auto-bless-on-failure behaviour.
- The golden stores the SCRUBBED form; scrubbing applies to the actual only, before comparison, and the sidecar holds the scrubbed actual so promotion is a pure rename. Keep that invariant.
- The diff engine's LCS caps at ~4M cells and degrades to a replace block — that cap is deliberate; don't raise it casually.
- Gate: WinDevKit tools/gate.sh (fmt + clippy both targets + full tests + doctests). CI's real build is the link gate.
