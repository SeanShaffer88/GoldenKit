//! A small unified-diff engine for text — enough of `diff -u` to
//! make a golden mismatch readable: common prefix/suffix trimmed,
//! an LCS over the differing middle, hunks with 3 lines of context
//! in the standard `@@ -a,b +c,d @@` shape.
//!
//! Zero dependencies means no external diff crate; the algorithm
//! is kept deliberately simple and is unit-tested on the shapes
//! golden files actually produce (small local changes in long
//! files). If the differing middle is huge, the LCS table is
//! capped and the middle degrades to a single replace block
//! rather than consuming unbounded memory.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Eq,
    Del,
    Ins,
}

/// Build a unified diff between `expected` and `actual`.
/// Returns an empty string when the inputs are line-identical.
/// Labels appear in the `---` / `+++` header lines.
pub fn unified_diff(
    expected: &str,
    actual: &str,
    expected_label: &str,
    actual_label: &str,
) -> String {
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    unified_diff_lines(&a, &b, expected_label, actual_label)
}

pub fn unified_diff_lines(
    a: &[&str],
    b: &[&str],
    expected_label: &str,
    actual_label: &str,
) -> String {
    // Trim common prefix and suffix.
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < (a.len() - pre)
        && suf < (b.len() - pre)
        && a[a.len() - 1 - suf] == b[b.len() - 1 - suf]
    {
        suf += 1;
    }
    let am = &a[pre..a.len() - suf];
    let bm = &b[pre..b.len() - suf];
    if am.is_empty() && bm.is_empty() {
        return String::new();
    }

    // Op list for the middle.
    let mid_ops = middle_ops(am, bm);
    // Full op stream: (op, a_index, b_index).
    let mut ops: Vec<(Op, usize, usize)> = Vec::new();
    for i in 0..pre {
        ops.push((Op::Eq, i, i));
    }
    let (mut ai, mut bi) = (pre, pre);
    for op in mid_ops {
        match op {
            Op::Eq => {
                ops.push((Op::Eq, ai, bi));
                ai += 1;
                bi += 1;
            }
            Op::Del => {
                ops.push((Op::Del, ai, bi));
                ai += 1;
            }
            Op::Ins => {
                ops.push((Op::Ins, ai, bi));
                bi += 1;
            }
        }
    }
    for i in 0..suf {
        ops.push((Op::Eq, a.len() - suf + i, b.len() - suf + i));
    }

    // Group into hunks: changes separated by <= 2*CONTEXT equal
    // lines merge into one hunk.
    const CONTEXT: usize = 3;
    let change_idx: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, (op, _, _))| *op != Op::Eq)
        .map(|(i, _)| i)
        .collect();
    if change_idx.is_empty() {
        return String::new();
    }
    let mut hunks: Vec<(usize, usize)> = Vec::new(); // [start, end) into ops
    let mut start = change_idx[0].saturating_sub(CONTEXT);
    let mut prev = change_idx[0];
    for &ci in &change_idx[1..] {
        if ci - prev - 1 > 2 * CONTEXT {
            let end = (prev + 1 + CONTEXT).min(ops.len());
            hunks.push((start, end));
            start = ci.saturating_sub(CONTEXT);
        }
        prev = ci;
    }
    hunks.push((start, (prev + 1 + CONTEXT).min(ops.len())));

    let mut out = String::new();
    out.push_str(&format!("--- {expected_label}\n+++ {actual_label}\n"));
    for (hs, he) in hunks {
        let hunk = &ops[hs..he];
        let a_start = hunk[0].1;
        let b_start = hunk[0].2;
        let a_len = hunk.iter().filter(|(op, _, _)| *op != Op::Ins).count();
        let b_len = hunk.iter().filter(|(op, _, _)| *op != Op::Del).count();
        // Unified headers use the line BEFORE the range when the
        // range is empty; otherwise the 1-based first line.
        let a_disp = if a_len == 0 { a_start } else { a_start + 1 };
        let b_disp = if b_len == 0 { b_start } else { b_start + 1 };
        out.push_str(&format!("@@ -{a_disp},{a_len} +{b_disp},{b_len} @@\n"));
        for (op, ai, bi) in hunk {
            match op {
                Op::Eq => {
                    out.push(' ');
                    out.push_str(a[*ai]);
                    out.push('\n');
                }
                Op::Del => {
                    out.push('-');
                    out.push_str(a[*ai]);
                    out.push('\n');
                }
                Op::Ins => {
                    out.push('+');
                    out.push_str(b[*bi]);
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// Ops for the differing middle. LCS via dynamic programming;
/// if the table would exceed ~4M cells the middle degrades to
/// delete-all + insert-all (still correct, just less minimal).
fn middle_ops(a: &[&str], b: &[&str]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return vec![Op::Ins; m];
    }
    if m == 0 {
        return vec![Op::Del; n];
    }
    if n * m > 4_000_000 {
        let mut ops = vec![Op::Del; n];
        ops.extend(vec![Op::Ins; m]);
        return ops;
    }
    // dp[i][j] = LCS length of a[i..], b[j..].
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Eq);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(Op::Del);
            i += 1;
        } else {
            ops.push(Op::Ins);
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Del);
        i += 1;
    }
    while j < m {
        ops.push(Op::Ins);
        j += 1;
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_empty() {
        assert_eq!(unified_diff("a\nb\n", "a\nb\n", "g", "a"), "");
    }

    #[test]
    fn single_line_change() {
        let d = unified_diff("one\ntwo\nthree\n", "one\nTWO\nthree\n", "golden", "actual");
        assert!(d.starts_with("--- golden\n+++ actual\n"), "{d}");
        assert!(d.contains("@@ -1,3 +1,3 @@"), "{d}");
        assert!(d.contains("-two\n"), "{d}");
        assert!(d.contains("+TWO\n"), "{d}");
        assert!(d.contains(" one\n"), "{d}");
    }

    #[test]
    fn insertion_and_deletion() {
        let d = unified_diff("a\nc\n", "a\nb\nc\n", "g", "a");
        assert!(d.contains("+b\n"), "{d}");
        assert!(!d.contains("-b"), "{d}");
        let d2 = unified_diff("a\nb\nc\n", "a\nc\n", "g", "a");
        assert!(d2.contains("-b\n"), "{d2}");
    }

    #[test]
    fn distant_changes_make_two_hunks() {
        let mut a = String::new();
        let mut b = String::new();
        for i in 0..30 {
            a.push_str(&format!("line{i}\n"));
            b.push_str(&format!("line{i}\n"));
        }
        let b = b
            .replace("line2\n", "CHANGED2\n")
            .replace("line27\n", "CHANGED27\n");
        let d = unified_diff(&a, &b, "g", "a");
        assert_eq!(d.matches("@@ ").count(), 2, "{d}");
        assert!(d.contains("-line2\n"), "{d}");
        assert!(d.contains("-line27\n"), "{d}");
    }

    #[test]
    fn change_at_edges() {
        let d = unified_diff("x\na\nb\n", "y\na\nb\n", "g", "a");
        assert!(d.contains("-x\n+y\n"), "{d}");
        let d2 = unified_diff("a\nb\nx\n", "a\nb\ny\n", "g", "a");
        assert!(d2.contains("-x\n+y\n"), "{d2}");
    }
}
