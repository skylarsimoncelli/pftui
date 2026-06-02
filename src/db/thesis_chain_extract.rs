//! Deterministic heuristic extractor for `thesis_dependencies` chains.
//!
//! Reads three free-form text sources — `thesis.content`,
//! `prediction_lessons.why_wrong`, and recent `agent_messages.content` —
//! and emits structured `ProposedChain` rows that the `analytics
//! thesis-chains extract` CLI can either dry-run or apply via the existing
//! [`crate::db::thesis_dependencies::insert`] path.
//!
//! v1 is intentionally regex-only — no LLM call is required. Patterns
//! detected include:
//!   "if X then Y", "if X, Y"
//!   "when X, Y", "when X then Y"
//!   "X implies Y" / "X => Y" / "X -> Y" / "X → Y"
//!   "X drives Y" / "X accelerates Y"
//!   "X dampens Y" / "X weakens Y"
//!   "X contradicts Y"
//!   "X is contingent on Y" / "X depends on Y"
//!
//! The classifier maps each pattern to one of the canonical relations in
//! [`crate::db::thesis_dependencies::RELATIONS`]. Phrases that don't match a
//! recognised shape are skipped.

use anyhow::Result;
use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::thesis_dependencies;

/// A chain proposed by the heuristic extractor, before any dedupe or write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedChain {
    pub antecedent_text: String,
    pub relation: String,
    pub consequent_text: String,
    pub conviction: String,
    pub source: ExtractSource,
    /// JSON-friendly source descriptor — a thesis section slug or a
    /// `prediction_lessons.id`. Allows callers to drop the value into the
    /// existing `source_lesson_ids` / `source_thesis_sections` columns.
    pub source_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtractSource {
    Thesis,
    Lessons,
    Messages,
}

impl ExtractSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExtractSource::Thesis => "thesis",
            ExtractSource::Lessons => "lessons",
            ExtractSource::Messages => "messages",
        }
    }
}

/// Apply the heuristic patterns to a single free-form text fragment. Returns
/// the list of [`ProposedChain`] rows recovered, tagged with the supplied
/// `source` and `source_ref`.
pub fn extract_from_text(
    text: &str,
    source: ExtractSource,
    source_ref: &str,
) -> Vec<ProposedChain> {
    let mut out = Vec::new();
    for sentence in split_sentences(text) {
        let s = sentence.trim();
        if s.is_empty() {
            continue;
        }
        if let Some((a, r, c)) = match_relation(s) {
            out.push(ProposedChain {
                antecedent_text: clean_clause(&a),
                relation: r.to_string(),
                consequent_text: clean_clause(&c),
                conviction: "medium".to_string(),
                source,
                source_ref: source_ref.to_string(),
            });
        }
    }
    out
}

/// Detect a relation in a single sentence. Returns
/// `(antecedent_text, relation, consequent_text)` on the first match.
fn match_relation(sentence: &str) -> Option<(String, &'static str, String)> {
    // Patterns are listed most-specific-first so e.g. "is contingent on"
    // wins over the bare "depends" word.
    let patterns: &[(&Regex, &str, bool)] = &[
        (contingent_on_re(), "contingent-on", true),
        (contradicts_re(), "contradicts", false),
        (accelerates_re(), "accelerates", false),
        (dampens_re(), "dampens", false),
        (if_then_re(), "implies", false),
        (when_then_re(), "implies", false),
        (implies_re(), "implies", false),
        (arrow_re(), "implies", false),
    ];
    for (re, relation, swap) in patterns {
        if let Some(c) = re.captures(sentence) {
            let left = c.get(1)?.as_str().to_string();
            let right = c.get(2)?.as_str().to_string();
            return Some(if *swap {
                // "X contingent on Y" — consequent = X, antecedent = Y.
                (right, relation, left)
            } else {
                (left, relation, right)
            });
        }
    }
    None
}

// All regex patterns are case-insensitive on the relation token and require
// a clear separator (comma / "then" / arrow). The "lazy" non-greedy quantifier
// on the left and the trailing punctuation guard keep the matches scoped to a
// single clause boundary.
//
// Each `*_re()` helper lazily compiles once via `OnceLock` so the regex
// objects are reused across extract calls without an external `lazy_static`
// dependency.
use std::sync::OnceLock;

fn re(slot: &'static OnceLock<Regex>, pattern: &'static str) -> &'static Regex {
    slot.get_or_init(|| Regex::new(pattern).expect("static regex pattern must compile"))
}

fn if_then_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"(?i)\bif\s+(.+?),\s*(?:then\s+)?(.+?)(?:[.;!?]|$)")
}

fn when_then_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"(?i)\bwhen\s+(.+?),\s*(?:then\s+)?(.+?)(?:[.;!?]|$)")
}

fn implies_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"(?i)(.+?)\s+implies\s+(.+?)(?:[.;!?]|$)")
}

fn arrow_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"(.+?)\s*(?:=>|->|→)\s*(.+?)(?:[.;!?]|$)")
}

fn accelerates_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"(?i)(.+?)\s+(?:accelerates|drives|amplifies)\s+(.+?)(?:[.;!?]|$)")
}

fn dampens_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"(?i)(.+?)\s+(?:dampens|weakens|suppresses|caps)\s+(.+?)(?:[.;!?]|$)")
}

fn contradicts_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"(?i)(.+?)\s+contradicts\s+(.+?)(?:[.;!?]|$)")
}

fn contingent_on_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(
        &SLOT,
        r"(?i)(.+?)\s+(?:is\s+contingent\s+on|depends\s+on|is\s+conditional\s+on)\s+(.+?)(?:[.;!?]|$)",
    )
}

/// Split a free-form text fragment into rough sentence-shaped pieces. The
/// split keeps sentence boundaries simple (period / question mark /
/// exclamation / newline) — adequate for the heuristic extractor and
/// faster + more predictable than a full sentence tokenizer.
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in text.chars() {
        buf.push(ch);
        if matches!(ch, '\n' | '.' | '!' | '?') {
            let candidate = buf.trim().to_string();
            if !candidate.is_empty() {
                out.push(candidate);
            }
            buf.clear();
        }
    }
    let remainder = buf.trim().to_string();
    if !remainder.is_empty() {
        out.push(remainder);
    }
    out
}

/// Trim quoting punctuation and stray whitespace from a candidate clause.
fn clean_clause(text: &str) -> String {
    text.trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '(' | ')'))
        .trim()
        .to_string()
}

/// Read thesis content for extraction. Returns `(section_slug, content)` pairs.
pub fn collect_thesis_sources(conn: &Connection) -> Result<Vec<(String, String)>> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='thesis'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT section, content FROM thesis ORDER BY section ASC")?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Read prediction_lessons.why_wrong for extraction. Returns
/// `(lesson_id, why_wrong)` pairs.
pub fn collect_lesson_sources(conn: &Connection) -> Result<Vec<(i64, String)>> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type='table' AND name='prediction_lessons'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, why_wrong FROM prediction_lessons
         WHERE why_wrong IS NOT NULL AND why_wrong != ''
         ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Read recent agent_messages.content. `since_date` is matched as a
/// `YYYY-MM-DD` prefix; only rows on/after that anchor are returned.
pub fn collect_message_sources(
    conn: &Connection,
    since_date: &str,
) -> Result<Vec<(i64, String)>> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='agent_messages'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, content FROM agent_messages
         WHERE content IS NOT NULL AND content != ''
           AND created_at >= ?1
         ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![since_date], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Persist a proposed chain via [`thesis_dependencies::insert`], unless an
/// equivalent chain already exists (matched on
/// `lower(antecedent_text) + relation + lower(consequent_text)`).
///
/// Returns `Some(id)` for a newly-inserted row, `None` when the chain was
/// deduped.
pub fn apply_proposed(conn: &Connection, proposed: &ProposedChain) -> Result<Option<i64>> {
    thesis_dependencies::ensure_table(conn)?;
    let existing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM thesis_dependencies
         WHERE LOWER(antecedent_text) = LOWER(?1)
           AND LOWER(consequent_text) = LOWER(?2)
           AND relation = ?3",
        rusqlite::params![
            proposed.antecedent_text,
            proposed.consequent_text,
            proposed.relation,
        ],
        |r| r.get(0),
    )?;
    if existing > 0 {
        return Ok(None);
    }
    let lessons: Option<Vec<i64>> = if proposed.source == ExtractSource::Lessons {
        proposed.source_ref.parse::<i64>().ok().map(|id| vec![id])
    } else {
        None
    };
    let lessons_slice: Option<&[i64]> = lessons.as_deref();
    let sections: Option<Vec<String>> = if proposed.source == ExtractSource::Thesis {
        Some(vec![proposed.source_ref.clone()])
    } else {
        None
    };
    let sections_slice: Option<&[String]> = sections.as_deref();
    let id = thesis_dependencies::insert(
        conn,
        None,
        &proposed.antecedent_text,
        &proposed.relation,
        None,
        &proposed.consequent_text,
        1,
        Some(&proposed.conviction),
        lessons_slice,
        sections_slice,
    )?;
    Ok(Some(id))
}

/// Summary returned from an extraction pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractSummary {
    pub proposed: usize,
    pub applied: usize,
    pub deduped: usize,
    pub by_source: ExtractBySource,
    pub chains: Vec<ProposedChain>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractBySource {
    pub thesis: usize,
    pub lessons: usize,
    pub messages: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_if_then_implication() {
        let text = "If real yields back off, gold should grind higher.";
        let chains = extract_from_text(text, ExtractSource::Thesis, "macro-2026Q1");
        assert_eq!(chains.len(), 1);
        let c = &chains[0];
        assert_eq!(c.relation, "implies");
        assert!(c.antecedent_text.contains("real yields back off"));
        assert!(c.consequent_text.contains("gold"));
        assert_eq!(c.conviction, "medium");
        assert_eq!(c.source, ExtractSource::Thesis);
        assert_eq!(c.source_ref, "macro-2026Q1");
    }

    #[test]
    fn extracts_when_clause_as_implies() {
        let text = "When DXY breaks 102, equities rally.";
        let chains = extract_from_text(text, ExtractSource::Messages, "42");
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].relation, "implies");
        assert!(chains[0].antecedent_text.contains("DXY"));
        assert!(chains[0].consequent_text.contains("equities"));
    }

    #[test]
    fn extracts_implies_keyword() {
        let text = "A weaker dollar implies stronger gold.";
        let chains = extract_from_text(text, ExtractSource::Lessons, "99");
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].relation, "implies");
        assert!(chains[0].antecedent_text.to_lowercase().contains("weaker dollar"));
        assert!(chains[0].consequent_text.to_lowercase().contains("stronger gold"));
    }

    #[test]
    fn extracts_arrow_form() {
        let text = "Liquidity tightening -> equity multiple compression.";
        let chains = extract_from_text(text, ExtractSource::Thesis, "liquidity-2026");
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].relation, "implies");
        assert!(chains[0].antecedent_text.to_lowercase().contains("liquidity"));
    }

    #[test]
    fn extracts_accelerates() {
        let text = "BTC ETF inflows accelerates price discovery.";
        let chains = extract_from_text(text, ExtractSource::Messages, "100");
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].relation, "accelerates");
    }

    #[test]
    fn extracts_dampens() {
        let text = "Higher real yields dampens gold demand.";
        let chains = extract_from_text(text, ExtractSource::Lessons, "55");
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].relation, "dampens");
    }

    #[test]
    fn extracts_contradicts() {
        let text = "Strong dollar contradicts rising commodity prices.";
        let chains = extract_from_text(text, ExtractSource::Thesis, "fx-2026");
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].relation, "contradicts");
    }

    #[test]
    fn extracts_contingent_on_swaps_sides() {
        let text = "A gold rally is contingent on real yields breaking lower.";
        let chains = extract_from_text(text, ExtractSource::Thesis, "gold-thesis");
        assert_eq!(chains.len(), 1);
        let c = &chains[0];
        assert_eq!(c.relation, "contingent-on");
        // antecedent is the "real yields" clause, consequent is the "gold rally".
        assert!(c.antecedent_text.to_lowercase().contains("real yields"));
        assert!(c.consequent_text.to_lowercase().contains("gold rally"));
    }

    #[test]
    fn skips_unrelated_prose() {
        let text = "BTC is currently consolidating near the 95k zone.";
        assert!(extract_from_text(text, ExtractSource::Messages, "1").is_empty());
    }

    #[test]
    fn apply_then_reapply_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        thesis_dependencies::ensure_table(&conn).unwrap();
        let proposed = ProposedChain {
            antecedent_text: "real yields back off".to_string(),
            relation: "implies".to_string(),
            consequent_text: "gold should grind higher".to_string(),
            conviction: "medium".to_string(),
            source: ExtractSource::Thesis,
            source_ref: "macro-2026Q1".to_string(),
        };
        let first = apply_proposed(&conn, &proposed).unwrap();
        assert!(first.is_some());
        let second = apply_proposed(&conn, &proposed).unwrap();
        assert!(second.is_none(), "second insert should dedupe");
    }
}
