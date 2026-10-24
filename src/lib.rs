//! GoldenKit — golden-file testing as a solved problem.
//!
//! The pattern (WinDevKit PAIN_POINTS #15): a test generates some
//! output — a settings file, a report, a rendered document — and
//! compares it against a checked-in "golden" file. Every project
//! in the family re-derived this by hand: the comparison, the
//! diff, the "how do I update the goldens" dance. GoldenKit does
//! it once:
//!
//! ```rust,ignore
//! golden_kit::assert_golden("tests/golden/help.txt", &render_help());
//! ```
//!
//! - **Mismatch → a readable unified diff**, in the panic message
//!   and in a `<golden>.actual` sidecar file left next to the
//!   golden for inspection.
//! - **Blessing is a review, not a reflex.** `golden-bless`
//!   (the companion tool) lists every pending `.actual`, shows
//!   its diff against the golden, and promotes them only with
//!   `--apply`. For the quick path, running tests with
//!   `GOLDEN_BLESS=1` rewrites goldens in place.
//! - **Volatile bits are scrubbable.** Timestamps, temp paths and
//!   version strings can be replaced with stable tokens before
//!   comparison, on both the text and bytes surfaces.
//! - **Line endings are normalized** (CRLF → LF) on both sides by
//!   default, so a golden checked in on one OS passes on another.
//!
//! Zero dependencies: a test tool that drags in a dependency
//! tree defeats its own purpose.

pub mod diff;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const VERSION: &str = "0.1.0";

/// What a successful check did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Actual matched the golden.
    Match,
    /// Bless mode rewrote an existing golden.
    Blessed,
    /// Bless mode created a golden that did not exist.
    Created,
}

/// Why a check failed.
#[derive(Debug)]
pub struct Failure {
    pub path: PathBuf,
    pub kind: FailureKind,
    /// Unified diff (text checks with a readable golden).
    pub diff: Option<String>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// No golden file exists and bless mode is off.
    Missing,
    /// The golden exists but differs.
    Mismatch,
    /// The golden or sidecar could not be read/written.
    Io,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(d) = &self.diff {
            write!(f, "\n{d}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Failure {}

fn io_failure(path: &Path, what: &str, e: &io::Error) -> Failure {
    Failure {
        path: path.to_path_buf(),
        kind: FailureKind::Io,
        diff: None,
        message: format!("golden {what} failed for {}: {e}", path.display()),
    }
}

/// Is bless mode on via the environment? `GOLDEN_BLESS=1`
/// (also `true` / `yes`, case-insensitive).
pub fn bless_from_env() -> bool {
    match std::env::var("GOLDEN_BLESS") {
        Ok(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => false,
    }
}

/// The path of the `.actual` sidecar for a golden path.
pub fn actual_path(golden: &Path) -> PathBuf {
    let mut s = golden.as_os_str().to_owned();
    s.push(".actual");
    PathBuf::from(s)
}

/// A golden check with options. `Golden::new(path).check_str(x)`
/// or the one-shot `assert_golden(path, x)`.
#[derive(Clone, Debug)]
pub struct Golden {
    path: PathBuf,
    bless: Option<bool>,
    normalize_newlines: bool,
    trim_trailing_whitespace: bool,
    replacements: Vec<(String, String)>,
}

impl Golden {
    pub fn new(path: impl AsRef<Path>) -> Golden {
        Golden {
            path: path.as_ref().to_path_buf(),
            bless: None,
            normalize_newlines: true,
            trim_trailing_whitespace: false,
            replacements: Vec::new(),
        }
    }

    /// Override bless mode for this check (default: the
    /// `GOLDEN_BLESS` environment variable).
    pub fn bless(mut self, yes: bool) -> Golden {
        self.bless = Some(yes);
        self
    }

    /// Normalize CRLF/CR line endings to LF on both sides
    /// (default: on).
    pub fn normalize_newlines(mut self, yes: bool) -> Golden {
        self.normalize_newlines = yes;
        self
    }

    /// Trim trailing spaces/tabs on every line, both sides
    /// (default: off — exact comparison).
    pub fn trim_trailing_whitespace(mut self, yes: bool) -> Golden {
        self.trim_trailing_whitespace = yes;
        self
    }

    /// Replace every occurrence of `from` with `to` in the
    /// actual output before comparison — the scrubber for
    /// volatile content (timestamps, temp paths, versions).
    /// Replacements run in the order added, on the actual only;
    /// the golden holds the scrubbed (`to`) form.
    pub fn scrub(mut self, from: &str, to: &str) -> Golden {
        self.replacements.push((from.to_string(), to.to_string()));
        self
    }

    fn bless_on(&self) -> bool {
        self.bless.unwrap_or_else(bless_from_env)
    }

    fn prepare_text(&self, text: &str, is_actual: bool) -> String {
        let mut out = text.to_string();
        if is_actual {
            for (from, to) in &self.replacements {
                out = out.replace(from.as_str(), to.as_str());
            }
        }
        if self.normalize_newlines {
            out = out.replace("\r\n", "\n").replace('\r', "\n");
        }
        if self.trim_trailing_whitespace {
            out = out
                .lines()
                .map(|l| l.trim_end_matches([' ', '\t']))
                .collect::<Vec<_>>()
                .join("\n");
            if text.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    fn prepare_bytes(&self, data: &[u8], is_actual: bool) -> Vec<u8> {
        // Scrubbing on bytes: replacements are byte-level (the
        // patterns callers scrub — paths, versions — are ASCII).
        let mut out = data.to_vec();
        if is_actual {
            for (from, to) in &self.replacements {
                out = replace_bytes(&out, from.as_bytes(), to.as_bytes());
            }
        }
        if self.normalize_newlines {
            let mut norm = Vec::with_capacity(out.len());
            let mut i = 0;
            while i < out.len() {
                if out[i] == b'\r' {
                    norm.push(b'\n');
                    if i + 1 < out.len() && out[i + 1] == b'\n' {
                        i += 1;
                    }
                } else {
                    norm.push(out[i]);
                }
                i += 1;
            }
            out = norm;
        }
        out
    }

    /// Compare text against the golden. See the crate docs for
    /// the bless / sidecar behaviour.
    pub fn check_str(self, actual: &str) -> Result<Outcome, Failure> {
        let prepared = self.prepare_text(actual, true);
        self.check_prepared(&prepared)
    }

    /// Compare bytes against the golden. On mismatch the report
    /// names the sizes and first differing offset; when both
    /// sides are UTF-8 text it also carries a unified diff.
    pub fn check_bytes(self, actual: &[u8]) -> Result<Outcome, Failure> {
        let prepared = self.prepare_bytes(actual, true);
        self.check_prepared_bytes(&prepared)
    }

    /// `check_str` that panics with the diff on failure — the
    /// form most tests want.
    pub fn assert_str(self, actual: &str) {
        if let Err(f) = self.check_str(actual) {
            panic!("{f}");
        }
    }

    /// `check_bytes` that panics on failure.
    pub fn assert_bytes(self, actual: &[u8]) {
        if let Err(f) = self.check_bytes(actual) {
            panic!("{f}");
        }
    }

    fn bless_write(&self, prepared: &[u8], existed: bool) -> Result<Outcome, Failure> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| io_failure(&self.path, "create directory", &e))?;
            }
        }
        fs::write(&self.path, prepared).map_err(|e| io_failure(&self.path, "write", &e))?;
        // A blessed golden leaves no pending sidecar behind.
        let _ = fs::remove_file(actual_path(&self.path));
        Ok(if existed {
            Outcome::Blessed
        } else {
            Outcome::Created
        })
    }

    fn write_sidecar(&self, prepared: &[u8]) {
        let _ = fs::write(actual_path(&self.path), prepared);
    }

    fn read_golden(&self) -> Result<Option<Vec<u8>>, Failure> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_failure(&self.path, "read", &e)),
        }
    }

    fn check_prepared(self, prepared: &str) -> Result<Outcome, Failure> {
        let golden_raw = self.read_golden()?;
        match golden_raw {
            None => {
                if self.bless_on() {
                    self.bless_write(prepared.as_bytes(), false)
                } else {
                    self.write_sidecar(prepared.as_bytes());
                    Err(Failure {
                        path: self.path.clone(),
                        kind: FailureKind::Missing,
                        diff: None,
                        message: format!(
                            "golden missing: {} does not exist. The actual output was \
                             written to {} — review it, then bless with GOLDEN_BLESS=1 \
                             or `golden-bless --apply`.",
                            self.path.display(),
                            actual_path(&self.path).display()
                        ),
                    })
                }
            }
            Some(bytes) => {
                let golden_text = String::from_utf8_lossy(&bytes).into_owned();
                let golden_prepared = self.prepare_text(&golden_text, false);
                if golden_prepared == prepared {
                    let _ = fs::remove_file(actual_path(&self.path));
                    return Ok(Outcome::Match);
                }
                if self.bless_on() {
                    return self.bless_write(prepared.as_bytes(), true);
                }
                self.write_sidecar(prepared.as_bytes());
                let d = diff::unified_diff(
                    &golden_prepared,
                    prepared,
                    &format!("golden {}", self.path.display()),
                    "actual",
                );
                Err(Failure {
                    path: self.path.clone(),
                    kind: FailureKind::Mismatch,
                    diff: Some(d),
                    message: format!(
                        "golden mismatch: {}\nActual written to {} — if the change is \
                         intended, bless with GOLDEN_BLESS=1 or `golden-bless --apply`.",
                        self.path.display(),
                        actual_path(&self.path).display()
                    ),
                })
            }
        }
    }

    fn check_prepared_bytes(self, prepared: &[u8]) -> Result<Outcome, Failure> {
        let golden_raw = self.read_golden()?;
        match golden_raw {
            None => {
                if self.bless_on() {
                    self.bless_write(prepared, false)
                } else {
                    self.write_sidecar(prepared);
                    Err(Failure {
                        path: self.path.clone(),
                        kind: FailureKind::Missing,
                        diff: None,
                        message: format!(
                            "golden missing: {} does not exist ({} bytes actual). \
                             Actual written to {} — review, then bless.",
                            self.path.display(),
                            prepared.len(),
                            actual_path(&self.path).display()
                        ),
                    })
                }
            }
            Some(bytes) => {
                let golden_prepared = self.prepare_bytes(&bytes, false);
                if golden_prepared == prepared {
                    let _ = fs::remove_file(actual_path(&self.path));
                    return Ok(Outcome::Match);
                }
                if self.bless_on() {
                    return self.bless_write(prepared, true);
                }
                self.write_sidecar(prepared);
                let first_diff = golden_prepared
                    .iter()
                    .zip(prepared.iter())
                    .position(|(x, y)| x != y)
                    .unwrap_or(golden_prepared.len().min(prepared.len()));
                let text_diff = match (
                    std::str::from_utf8(&golden_prepared),
                    std::str::from_utf8(prepared),
                ) {
                    (Ok(g), Ok(a)) => Some(diff::unified_diff(
                        g,
                        a,
                        &format!("golden {}", self.path.display()),
                        "actual",
                    )),
                    _ => None,
                };
                Err(Failure {
                    path: self.path.clone(),
                    kind: FailureKind::Mismatch,
                    diff: text_diff,
                    message: format!(
                        "golden bytes mismatch: {} (golden {} bytes, actual {} bytes, \
                         first difference at offset {first_diff}). Actual written to {}.",
                        self.path.display(),
                        golden_prepared.len(),
                        prepared.len(),
                        actual_path(&self.path).display()
                    ),
                })
            }
        }
    }
}

/// One-shot text check: `assert_golden(path, &output)`.
/// Panics with a unified diff on mismatch. Bless with
/// `GOLDEN_BLESS=1` or the `golden-bless` tool.
pub fn assert_golden(path: impl AsRef<Path>, actual: &str) {
    Golden::new(path).assert_str(actual);
}

/// One-shot bytes check.
pub fn assert_golden_bytes(path: impl AsRef<Path>, actual: &[u8]) {
    Golden::new(path).assert_bytes(actual);
}

fn replace_bytes(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() || haystack.len() < from.len() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

// ----------------------------------------------------------------
// Bless-side helpers, shared by the golden-bless tool and tests.
// ----------------------------------------------------------------

/// A pending bless: a `.actual` sidecar and the golden it would
/// become.
#[derive(Clone, Debug)]
pub struct Pending {
    pub golden: PathBuf,
    pub actual: PathBuf,
}

/// Find every `*.actual` sidecar under `root` (recursive, depth 8,
/// skipping `.git` and `target` directories), sorted by path.
pub fn find_pending(root: &Path) -> Vec<Pending> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if p.is_dir() {
                if depth < 8 && name != ".git" && name != "target" {
                    stack.push((p, depth + 1));
                }
            } else if name.ends_with(".actual") {
                let s = p.to_string_lossy();
                let golden = PathBuf::from(s.strip_suffix(".actual").unwrap_or(&s).to_string());
                out.push(Pending { golden, actual: p });
            }
        }
    }
    out.sort_by(|x, y| x.actual.cmp(&y.actual));
    out
}

/// Promote one pending sidecar onto its golden path (creating
/// parent directories as needed).
pub fn promote(pending: &Pending) -> io::Result<()> {
    if let Some(parent) = pending.golden.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    // fs::rename will not replace an existing file on Windows.
    if pending.golden.exists() {
        fs::remove_file(&pending.golden)?;
    }
    fs::rename(&pending.actual, &pending.golden)
}
