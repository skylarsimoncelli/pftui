//! Small text helpers shared across the TUI and CLI.
//!
//! The headline reason this module exists: truncating strings by BYTE index
//! (`&s[..n]`) panics whenever `n` lands inside a multi-byte UTF-8 character —
//! e.g. an em dash `—` (3 bytes), smart quotes, or accented letters. That has
//! repeatedly crashed views that display free-form user content (journal notes,
//! news titles, error messages). Always truncate by CHARACTER instead.

/// Truncate `s` to at most `max` characters (Unicode scalar values), appending
/// a single-character ellipsis `…` when truncation occurred. Never panics on
/// multi-byte content because it operates on char boundaries.
///
/// `max` is the budget for the visible head text; the ellipsis is added on top,
/// so the returned string is at most `max + 1` characters.
pub fn truncate_ellipsis(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_string_is_unchanged() {
        assert_eq!(truncate_ellipsis("hello", 10), "hello");
        assert_eq!(truncate_ellipsis("hello", 5), "hello");
    }

    #[test]
    fn long_ascii_is_truncated_with_ellipsis() {
        assert_eq!(truncate_ellipsis("abcdefgh", 5), "abcde…");
    }

    #[test]
    fn does_not_panic_on_multibyte_boundary() {
        // The exact crash class: an em dash straddling the cut point. Byte
        // slicing `&s[..7]` would panic here ("—" is 3 bytes); char slicing
        // must not.
        let s = "gold — silver split, chose 40% silver over recommended 30%";
        let out = truncate_ellipsis(s, 7);
        assert_eq!(out.chars().count(), 8); // 7 head chars + ellipsis
        assert!(out.ends_with('…'));
        // And a cut that lands right on/after the em dash.
        for n in 0..s.chars().count() + 2 {
            let _ = truncate_ellipsis(s, n); // must never panic
        }
    }

    #[test]
    fn counts_chars_not_bytes() {
        // 4 em dashes = 4 chars but 12 bytes. Budget of 5 keeps all 4 (≤ 5).
        let s = "————";
        assert_eq!(truncate_ellipsis(s, 5), "————");
        assert_eq!(truncate_ellipsis(s, 2), "——…");
    }
}
