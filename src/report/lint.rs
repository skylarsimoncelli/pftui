//! Render-time lint: catch internal/debug text that must never reach a
//! delivered report.
//!
//! Section renderers and their loaders occasionally embed developer-facing
//! diagnostics (e.g. "no matching row in prediction_lessons — likely a
//! prompt-time ID drift") as fallback prose. Those are notes to a maintainer,
//! not to the operator, and one shipped to a real PDF. [`scan_for_leaks`]
//! flags them so a regression test fails in CI and `assemble_*` can warn at
//! build time.

/// Substrings that should never appear in finished report markdown. Kept
/// deliberately narrow to unambiguous internal/debug phrasing so legitimate
/// prose is never flagged.
pub const FORBIDDEN_SUBSTRINGS: &[&str] = &[
    "no matching row",
    "prompt-time id drift",
    "check that the lesson book",
    "buildcontext",
    "unavailable for this build",
    ".unwrap()",
    "todo(",
    "fixme",
    "fn render_",
    "lessons.id, not prediction_id",
];

/// A single leak finding: 1-based line number, the matched pattern, and a
/// trimmed excerpt of the offending line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeakFinding {
    pub line: usize,
    pub pattern: &'static str,
    pub excerpt: String,
}

/// Scan rendered markdown for forbidden internal/debug substrings
/// (case-insensitive). Returns one finding per (line, pattern) hit.
pub fn scan_for_leaks(markdown: &str) -> Vec<LeakFinding> {
    let mut findings = Vec::new();
    for (idx, line) in markdown.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        for pat in FORBIDDEN_SUBSTRINGS {
            if lower.contains(pat) {
                findings.push(LeakFinding {
                    line: idx + 1,
                    pattern: pat,
                    excerpt: line.trim().chars().take(100).collect(),
                });
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_the_lessons_debug_leak() {
        let bad = "- L-832 x2: Lesson #832 (no matching row in prediction_lessons — \
                   likely a prompt-time ID drift; check that the lesson book uses lessons.id, \
                   not prediction_id)";
        let findings = scan_for_leaks(bad);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.pattern == "no matching row"));
    }

    #[test]
    fn catches_dead_stub_text() {
        let findings = scan_for_leaks("Market snapshot data is unavailable for this build.");
        assert!(findings.iter().any(|f| f.pattern == "unavailable for this build"));
    }

    #[test]
    fn clean_report_prose_passes() {
        let good = "## Bottom Line\n\n- BTC -19.0% to $61,667 led an everything-down week; \
                    the structural floor (CB gold bid, Mayer<0.85) holds.\n- Gold is the anchor.";
        assert!(scan_for_leaks(good).is_empty());
    }
}
