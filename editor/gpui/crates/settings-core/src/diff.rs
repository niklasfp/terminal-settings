#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOp {
    Context,
    Add,
    Remove,
    /// Skipped unchanged lines between hunks.
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub op: DiffOp,
    pub text: String,
    /// 1-based line number in the new text; 0 for removed lines and gaps.
    pub line: usize,
}

/// Line diff (LCS) rendered as hunks with `context` lines around each change.
pub fn line_diff(before: &str, after: &str, context: usize) -> Vec<DiffLine> {
    let a = split(before);
    let b = split(after);

    // Lines before the first difference always match first in the walk below, so leave them out
    // of the LCS table; it is quadratic in the remaining lines.
    let skip = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let (ra, rb) = (&a[skip..], &b[skip..]);
    let (n, m) = (ra.len(), rb.len());
    let width = m + 1;
    let mut lcs = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * width + j] = if ra[i] == rb[j] {
                lcs[(i + 1) * width + j + 1] + 1
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }

    let mut ops: Vec<(DiffOp, &str, usize)> = Vec::with_capacity(a.len().max(b.len()));
    for (k, line) in a[..skip].iter().enumerate() {
        ops.push((DiffOp::Context, line, k + 1));
    }
    let (mut x, mut y) = (0, 0);
    while x < n || y < m {
        if x < n && y < m && ra[x] == rb[y] {
            ops.push((DiffOp::Context, &ra[x], skip + y + 1));
            x += 1;
            y += 1;
        } else if y < m && (x == n || lcs[x * width + y + 1] >= lcs[(x + 1) * width + y]) {
            ops.push((DiffOp::Add, &rb[y], skip + y + 1));
            y += 1;
        } else {
            ops.push((DiffOp::Remove, &ra[x], 0));
            x += 1;
        }
    }

    let mut keep = vec![false; ops.len()];
    for (i, op) in ops.iter().enumerate() {
        if op.0 == DiffOp::Context {
            continue;
        }
        let lo = i.saturating_sub(context);
        let hi = (i + context).min(ops.len() - 1);
        keep[lo..=hi].iter_mut().for_each(|k| *k = true);
    }

    let mut result = Vec::new();
    let mut gap = false;
    for (i, (op, text, line)) in ops.into_iter().enumerate() {
        if !keep[i] {
            gap = true;
            continue;
        }
        if gap && !result.is_empty() {
            result.push(DiffLine {
                op: DiffOp::Gap,
                text: String::new(),
                line: 0,
            });
        }
        gap = false;
        result.push(DiffLine {
            op,
            text: text.to_string(),
            line,
        });
    }
    result
}

fn split(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text
        .replace("\r\n", "\n")
        .split('\n')
        .map(str::to_string)
        .collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(d: &[DiffLine]) -> String {
        d.iter()
            .map(|l| match l.op {
                DiffOp::Context => format!(" {}", l.text),
                DiffOp::Add => format!("+{}", l.text),
                DiffOp::Remove => format!("-{}", l.text),
                DiffOp::Gap => "…".to_string(),
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    #[test]
    fn identical_texts_have_no_changes() {
        assert!(line_diff("a\nb\n", "a\nb\n", 2).is_empty());
    }

    #[test]
    fn change_is_shown_with_context_and_gaps() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\n";
        let after = "1\n2\n3\n4\nfive\n6\n7\n8\n";
        let d = line_diff(before, after, 1);
        assert_eq!(ops(&d), " 4|+five|-5| 6");
        assert_eq!(d[0].line, 4);
        assert_eq!(d[1].line, 5);
        assert_eq!(d[2].line, 0);
    }

    #[test]
    fn separate_hunks_are_split_by_a_gap() {
        let before = "a\nb\nc\nd\ne\nf\ng\n";
        let after = "A\nb\nc\nd\ne\nf\nG\n";
        assert_eq!(ops(&line_diff(before, after, 1)), "+A|-a| b|…| f|+G|-g");
    }

    #[test]
    fn crlf_and_lf_compare_equal_and_added_lines_are_numbered() {
        let d = line_diff("a\r\nb\r\n", "a\nb\nc\n", 2);
        assert_eq!(ops(&d), " a| b|+c");
        assert_eq!(d[2].line, 3);
    }
}
