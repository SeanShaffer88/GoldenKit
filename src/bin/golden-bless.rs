//! golden-bless — review and promote pending golden updates.
//!
//! A failed golden test leaves `<golden>.actual` next to its
//! golden. This tool finds them, shows each diff, and — only
//! with `--apply` — promotes the sidecars onto their goldens.
//! Blessing is a review, not a reflex.
//!
//! Usage:
//!   golden-bless [--dir PATH]           list pending + diffs (exit 1 if any)
//!   golden-bless [--dir PATH] --apply   promote all pending (exit 0)
//!   golden-bless --help
//!
//! Tip: add `*.actual` to your project's .gitignore — sidecars
//! are working files, not sources.

use std::path::PathBuf;
use std::process::ExitCode;

use golden_kit::{diff, find_pending, promote};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut apply = false;
    let mut dir: PathBuf = PathBuf::from(".");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--apply" => apply = true,
            "--dir" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("golden-bless: --dir needs a path");
                    return ExitCode::from(2);
                }
                dir = PathBuf::from(&args[i]);
            }
            "--help" | "-h" => {
                println!(
                    "golden-bless — review and promote pending golden updates\n\n\
                     USAGE:\n  golden-bless [--dir PATH]           list pending + diffs\n  \
                     golden-bless [--dir PATH] --apply   promote all pending\n\n\
                     Exit codes: 0 = nothing pending (or all applied), 1 = pending found, 2 = usage error"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("golden-bless: unknown argument '{other}' (try --help)");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let pending = find_pending(&dir);
    if pending.is_empty() {
        println!(
            "golden-bless: no pending .actual files under {}",
            dir.display()
        );
        return ExitCode::SUCCESS;
    }

    for p in &pending {
        let status = if p.golden.exists() {
            "update"
        } else {
            "create"
        };
        println!("== {} ({status})", p.golden.display());
        if p.golden.exists() {
            let golden = std::fs::read_to_string(&p.golden).unwrap_or_default();
            let actual = std::fs::read_to_string(&p.actual).unwrap_or_default();
            let d = diff::unified_diff(&golden, &actual, "golden", "actual");
            if d.is_empty() {
                println!("   (content identical to the golden)");
            } else {
                for line in d.lines().skip(2) {
                    println!("   {line}");
                }
            }
        } else {
            match std::fs::read_to_string(&p.actual) {
                Ok(text) => {
                    let preview: Vec<&str> = text.lines().take(10).collect();
                    for line in &preview {
                        println!("   +{line}");
                    }
                    if text.lines().count() > 10 {
                        println!("   +… ({} lines total)", text.lines().count());
                    }
                }
                Err(_) => println!("   (binary or unreadable actual)"),
            }
        }
    }

    if !apply {
        println!(
            "\n{} pending. Review the diffs above, then promote with: golden-bless --apply",
            pending.len()
        );
        return ExitCode::from(1);
    }

    let mut failed = 0;
    for p in &pending {
        match promote(p) {
            Ok(()) => println!("blessed {}", p.golden.display()),
            Err(e) => {
                eprintln!("FAILED {}: {e}", p.golden.display());
                failed += 1;
            }
        }
    }
    if failed > 0 {
        eprintln!("golden-bless: {failed} promotion(s) failed");
        return ExitCode::from(1);
    }
    println!("golden-bless: {} golden(s) updated", pending.len());
    ExitCode::SUCCESS
}
