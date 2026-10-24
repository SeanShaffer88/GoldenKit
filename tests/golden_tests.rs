//! Integration tests: the full golden workflow through real
//! files — match, mismatch + sidecar, bless, scrub, bytes, and
//! the pending/promote helpers the golden-bless tool runs on.

use std::fs;
use std::path::PathBuf;

use golden_kit::{assert_golden, find_pending, promote, FailureKind, Golden, Outcome};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "goldenkit-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn match_passes_and_clears_stale_sidecar() {
    let dir = temp_dir("match");
    let golden = dir.join("out.txt");
    fs::write(&golden, "hello\n").unwrap();
    fs::write(dir.join("out.txt.actual"), "stale\n").unwrap();
    let outcome = Golden::new(&golden).check_str("hello\n").unwrap();
    assert_eq!(outcome, Outcome::Match);
    assert!(
        !dir.join("out.txt.actual").exists(),
        "stale sidecar cleared"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn mismatch_fails_with_diff_and_sidecar() {
    let dir = temp_dir("mismatch");
    let golden = dir.join("out.txt");
    fs::write(&golden, "line1\nline2\n").unwrap();
    let failure = Golden::new(&golden)
        .check_str("line1\nCHANGED\n")
        .unwrap_err();
    assert_eq!(failure.kind, FailureKind::Mismatch);
    let diff = failure.diff.expect("text mismatch carries a diff");
    assert!(diff.contains("-line2"), "{diff}");
    assert!(diff.contains("+CHANGED"), "{diff}");
    // The sidecar holds the actual for golden-bless to review.
    assert_eq!(
        fs::read_to_string(dir.join("out.txt.actual")).unwrap(),
        "line1\nCHANGED\n"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn missing_golden_fails_then_bless_creates() {
    let dir = temp_dir("missing");
    let golden = dir.join("new.txt");
    let failure = Golden::new(&golden).check_str("fresh\n").unwrap_err();
    assert_eq!(failure.kind, FailureKind::Missing);
    let outcome = Golden::new(&golden)
        .bless(true)
        .check_str("fresh\n")
        .unwrap();
    assert_eq!(outcome, Outcome::Created);
    assert_eq!(fs::read_to_string(&golden).unwrap(), "fresh\n");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bless_updates_existing() {
    let dir = temp_dir("bless");
    let golden = dir.join("out.txt");
    fs::write(&golden, "old\n").unwrap();
    let outcome = Golden::new(&golden).bless(true).check_str("new\n").unwrap();
    assert_eq!(outcome, Outcome::Blessed);
    assert_eq!(fs::read_to_string(&golden).unwrap(), "new\n");
    assert!(!dir.join("out.txt.actual").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn scrub_replaces_volatile_content() {
    let dir = temp_dir("scrub");
    let golden = dir.join("log.txt");
    fs::write(&golden, "at <TIME> wrote C:\\Users\\<USER>\\f.txt\n").unwrap();
    let outcome = Golden::new(&golden)
        .scrub("2026-10-23T19:50:43Z", "<TIME>")
        .scrub("C:\\Users\\sean\\f.txt", "C:\\Users\\<USER>\\f.txt")
        .check_str("at 2026-10-23T19:50:43Z wrote C:\\Users\\sean\\f.txt\n")
        .unwrap();
    assert_eq!(outcome, Outcome::Match);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn newline_normalization_default_on() {
    let dir = temp_dir("crlf");
    let golden = dir.join("out.txt");
    fs::write(&golden, "a\nb\n").unwrap();
    // Actual arrives with Windows line endings; the LF golden passes.
    let outcome = Golden::new(&golden).check_str("a\r\nb\r\n").unwrap();
    assert_eq!(outcome, Outcome::Match);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bytes_mismatch_reports_offset() {
    let dir = temp_dir("bytes");
    let golden = dir.join("blob.bin");
    fs::write(&golden, [1u8, 2, 3, 4]).unwrap();
    let failure = Golden::new(&golden)
        .check_bytes(&[1u8, 2, 9, 4])
        .unwrap_err();
    assert_eq!(failure.kind, FailureKind::Mismatch);
    assert!(failure.message.contains("offset 2"), "{}", failure.message);
    let ok = Golden::new(&golden).check_bytes(&[1u8, 2, 3, 4]).unwrap();
    assert_eq!(ok, Outcome::Match);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pending_find_and_promote() {
    let dir = temp_dir("pending");
    let sub = dir.join("tests/golden");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("a.txt"), "old\n").unwrap();
    fs::write(sub.join("a.txt.actual"), "new\n").unwrap();
    fs::write(sub.join("b.txt.actual"), "created\n").unwrap();
    // Sidecars under target/ are build artifacts, not pendings.
    let tgt = dir.join("target/x");
    fs::create_dir_all(&tgt).unwrap();
    fs::write(tgt.join("c.txt.actual"), "ignored\n").unwrap();

    let pending = find_pending(&dir);
    assert_eq!(pending.len(), 2, "{pending:?}");
    for p in &pending {
        promote(p).unwrap();
    }
    assert_eq!(fs::read_to_string(sub.join("a.txt")).unwrap(), "new\n");
    assert_eq!(fs::read_to_string(sub.join("b.txt")).unwrap(), "created\n");
    assert!(find_pending(&dir).is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn assert_golden_panics_with_diff() {
    let dir = temp_dir("panic");
    let golden = dir.join("out.txt");
    fs::write(&golden, "expected\n").unwrap();
    let result = std::panic::catch_unwind(|| {
        assert_golden(&golden, "different\n");
    });
    let err = result.expect_err("mismatch must panic");
    let msg = err
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| err.downcast_ref::<&str>().unwrap_or(&"").to_string());
    assert!(msg.contains("-expected"), "{msg}");
    assert!(msg.contains("+different"), "{msg}");
    fs::remove_dir_all(&dir).unwrap();
}
