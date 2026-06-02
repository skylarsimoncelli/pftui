//! `thesis_dependencies` — formalized cross-asset if-then chains.
//!
//! Stores structured antecedent → consequent triples extracted from the
//! `thesis` table, `prediction_lessons.why_wrong`, and `agent_messages`.
//! Each chain has a relation type (`implies`, `contradicts`, `contingent-on`,
//! `accelerates`, `dampens`), a current evaluation state (`confirmed`,
//! `open`, `disconfirmed`, `stale`), and pointers back to the source lessons
//! and thesis sections that motivated the chain.
//!
//! The first-pass validator (see `validate_chain`) understands simple
//! `<SYMBOL> {<,>,<=,>=,==,!=} <value>` predicates and consults
//! `price_history` for the latest close. Chains whose text does not parse
//! to a checkable predicate are left in `current_state='open'` with
//! `last_validated_at` updated.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::price_history;

pub const RELATIONS: &[&str] = &[
    "implies",
    "contradicts",
    "contingent-on",
    "accelerates",
    "dampens",
];

#[allow(dead_code)]
pub const STATES: &[&str] = &["confirmed", "open", "disconfirmed", "stale"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThesisDependency {
    pub id: i64,
    pub antecedent_id: Option<String>,
    pub antecedent_text: String,
    pub relation: String,
    pub consequent_id: Option<String>,
    pub consequent_text: String,
    pub evidence_count: i64,
    pub conviction: Option<String>,
    /// JSON-encoded list of source `prediction_lessons.id` values.
    pub source_lesson_ids: Option<String>,
    /// JSON-encoded list of `thesis` section identifiers (slug or id).
    pub source_thesis_sections: Option<String>,
    pub current_state: String,
    pub last_validated_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationOutcome {
    pub id: i64,
    pub antecedent_state: PredicateOutcome,
    pub consequent_state: PredicateOutcome,
    pub new_chain_state: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PredicateOutcome {
    /// Predicate parsed and evaluated true against latest data.
    True,
    /// Predicate parsed and evaluated false.
    False,
    /// Predicate referenced a symbol/value the substrate cannot resolve.
    Unknown,
    /// Text did not parse to a checkable predicate at all.
    NotEvaluable,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS thesis_dependencies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            antecedent_id TEXT,
            antecedent_text TEXT NOT NULL,
            relation TEXT NOT NULL CHECK(relation IN
                ('implies','contradicts','contingent-on','accelerates','dampens')),
            consequent_id TEXT,
            consequent_text TEXT NOT NULL,
            evidence_count INTEGER NOT NULL DEFAULT 0,
            conviction TEXT,
            source_lesson_ids TEXT,
            source_thesis_sections TEXT,
            current_state TEXT NOT NULL DEFAULT 'open' CHECK(current_state IN
                ('confirmed','open','disconfirmed','stale')),
            last_validated_at TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_thesis_dependencies_relation
            ON thesis_dependencies(relation);
        CREATE INDEX IF NOT EXISTS idx_thesis_dependencies_state
            ON thesis_dependencies(current_state);
        CREATE INDEX IF NOT EXISTS idx_thesis_dependencies_antecedent_id
            ON thesis_dependencies(antecedent_id);
        CREATE INDEX IF NOT EXISTS idx_thesis_dependencies_consequent_id
            ON thesis_dependencies(consequent_id);",
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert(
    conn: &Connection,
    antecedent_id: Option<&str>,
    antecedent_text: &str,
    relation: &str,
    consequent_id: Option<&str>,
    consequent_text: &str,
    evidence_count: i64,
    conviction: Option<&str>,
    source_lesson_ids: Option<&[i64]>,
    source_thesis_sections: Option<&[String]>,
) -> Result<i64> {
    ensure_table(conn)?;
    if !RELATIONS.contains(&relation) {
        anyhow::bail!(
            "invalid relation '{}', expected one of: {}",
            relation,
            RELATIONS.join(", ")
        );
    }
    let lessons_json = match source_lesson_ids {
        Some(ids) => Some(serde_json::to_string(ids)?),
        None => None,
    };
    let sections_json = match source_thesis_sections {
        Some(secs) => Some(serde_json::to_string(secs)?),
        None => None,
    };
    conn.execute(
        "INSERT INTO thesis_dependencies
            (antecedent_id, antecedent_text, relation, consequent_id,
             consequent_text, evidence_count, conviction, source_lesson_ids,
             source_thesis_sections)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            antecedent_id,
            antecedent_text,
            relation,
            consequent_id,
            consequent_text,
            evidence_count,
            conviction,
            lessons_json,
            sections_json,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<ThesisDependency>> {
    ensure_table(conn)?;
    let row = conn
        .query_row(
            "SELECT id, antecedent_id, antecedent_text, relation, consequent_id,
                    consequent_text, evidence_count, conviction, source_lesson_ids,
                    source_thesis_sections, current_state, last_validated_at,
                    created_at
             FROM thesis_dependencies WHERE id = ?1",
            params![id],
            row_to_chain,
        )
        .optional()?;
    Ok(row)
}

pub fn list(
    conn: &Connection,
    state: Option<&str>,
    node: Option<&str>,
) -> Result<Vec<ThesisDependency>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT id, antecedent_id, antecedent_text, relation, consequent_id,
                consequent_text, evidence_count, conviction, source_lesson_ids,
                source_thesis_sections, current_state, last_validated_at,
                created_at
         FROM thesis_dependencies WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = state {
        sql.push_str(" AND current_state = ?");
        args.push(Box::new(s.to_string()));
    }
    if let Some(n) = node {
        sql.push_str(
            " AND (antecedent_id = ? OR consequent_id = ?
                OR antecedent_text LIKE ? OR consequent_text LIKE ?)",
        );
        args.push(Box::new(n.to_string()));
        args.push(Box::new(n.to_string()));
        let like = format!("%{}%", n);
        args.push(Box::new(like.clone()));
        args.push(Box::new(like));
    }
    sql.push_str(" ORDER BY id ASC");

    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), row_to_chain)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Return chains whose antecedent or consequent text references the given
/// symbol. The match is a case-insensitive substring against the trimmed
/// predicate token (so `BTC` matches `BTC > 100000` and `btc-usd`).
pub fn find_chains_for_symbol(
    conn: &Connection,
    symbol: &str,
) -> Result<Vec<ThesisDependency>> {
    ensure_table(conn)?;
    let needle = format!("%{}%", symbol);
    let mut stmt = conn.prepare(
        "SELECT id, antecedent_id, antecedent_text, relation, consequent_id,
                consequent_text, evidence_count, conviction, source_lesson_ids,
                source_thesis_sections, current_state, last_validated_at,
                created_at
         FROM thesis_dependencies
         WHERE antecedent_text LIKE ?1 COLLATE NOCASE
            OR consequent_text LIKE ?1 COLLATE NOCASE
            OR antecedent_id = ?2
            OR consequent_id = ?2
         ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map(params![needle, symbol], row_to_chain)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn row_to_chain(row: &rusqlite::Row) -> rusqlite::Result<ThesisDependency> {
    Ok(ThesisDependency {
        id: row.get(0)?,
        antecedent_id: row.get(1)?,
        antecedent_text: row.get(2)?,
        relation: row.get(3)?,
        consequent_id: row.get(4)?,
        consequent_text: row.get(5)?,
        evidence_count: row.get(6)?,
        conviction: row.get(7)?,
        source_lesson_ids: row.get(8)?,
        source_thesis_sections: row.get(9)?,
        current_state: row.get(10)?,
        last_validated_at: row.get(11)?,
        created_at: row.get(12)?,
    })
}

/// Parse a simple `<SYMBOL> {operator} <value>` predicate from free-form
/// text. Supports the operators `>`, `>=`, `<`, `<=`, `==`, `=`, `!=`.
/// Symbols may contain alphanumerics, `-`, `=`, `.`, `/`, `^`.
///
/// Examples that parse:
///   "XAU > 4500"
///   "BTC >= 100000"
///   "DXY < 102.5"
///   "BTC-USD > 100000"
///
/// Returns `None` if no such predicate appears.
pub fn parse_predicate(text: &str) -> Option<Predicate> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Scan tokens looking for an operator. The acceptable operator forms
    // are listed longest-first so `>=` is preferred over `>`.
    const OPS: &[(&str, &str)] = &[
        (">=", "gte"),
        ("<=", "lte"),
        ("==", "eq"),
        ("!=", "neq"),
        (">", "gt"),
        ("<", "lt"),
        ("=", "eq"),
    ];
    for (op_str, op_norm) in OPS {
        if let Some(idx) = trimmed.find(op_str) {
            let left = trimmed[..idx].trim();
            let right = trimmed[idx + op_str.len()..].trim();
            let symbol = extract_symbol(left)?;
            let value = extract_number(right)?;
            return Some(Predicate {
                symbol,
                op: (*op_norm).to_string(),
                value,
            });
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub struct Predicate {
    pub symbol: String,
    pub op: String,
    pub value: f64,
}

fn extract_symbol(text: &str) -> Option<String> {
    // Take the last "word"-like token on the left side, allowing common
    // ticker punctuation: -, =, ., /, ^.
    let candidate = text
        .split(|c: char| c.is_whitespace())
        .rfind(|s| !s.is_empty())?;
    let cleaned: String = candidate
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '=' | '.' | '/' | '^'))
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned)
}

fn extract_number(text: &str) -> Option<f64> {
    // Take the first numeric-looking token from the right side, stripping
    // commas and trailing punctuation.
    let candidate = text
        .split(|c: char| c.is_whitespace())
        .find(|s| !s.is_empty())?;
    let cleaned: String = candidate
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-'))
        .collect();
    cleaned.parse::<f64>().ok()
}

/// Evaluate a parsed predicate against `price_history`. Returns
/// `PredicateOutcome::Unknown` if no price is available for the symbol.
pub fn evaluate_predicate_against_prices(
    conn: &Connection,
    predicate: &Predicate,
    as_of_date: &str,
) -> Result<PredicateOutcome> {
    let price = price_history::get_price_at_date(conn, &predicate.symbol, as_of_date)?;
    let Some(price) = price else {
        return Ok(PredicateOutcome::Unknown);
    };
    use rust_decimal::prelude::ToPrimitive;
    let price_f = price.to_f64().unwrap_or(0.0);
    let matched = match predicate.op.as_str() {
        "gt" => price_f > predicate.value,
        "gte" => price_f >= predicate.value,
        "lt" => price_f < predicate.value,
        "lte" => price_f <= predicate.value,
        "eq" => (price_f - predicate.value).abs() < f64::EPSILON,
        "neq" => (price_f - predicate.value).abs() >= f64::EPSILON,
        _ => return Ok(PredicateOutcome::NotEvaluable),
    };
    Ok(if matched {
        PredicateOutcome::True
    } else {
        PredicateOutcome::False
    })
}

/// Evaluate a chain side (antecedent or consequent) against the substrate.
/// First parses a simple symbol+threshold predicate; if no predicate
/// parses, returns `NotEvaluable` so callers can flag the chain as
/// "not yet evaluable".
pub fn evaluate_text(
    conn: &Connection,
    text: &str,
    as_of_date: &str,
) -> Result<PredicateOutcome> {
    let Some(predicate) = parse_predicate(text) else {
        return Ok(PredicateOutcome::NotEvaluable);
    };
    evaluate_predicate_against_prices(conn, &predicate, as_of_date)
}

/// Validate a chain by id. Looks up `as_of_date` defaulting to today
/// (UTC), evaluates antecedent + consequent text, and persists a new
/// `current_state` + `last_validated_at`.
pub fn validate_chain(
    conn: &Connection,
    id: i64,
    as_of_date: Option<&str>,
) -> Result<ValidationOutcome> {
    ensure_table(conn)?;
    let chain = get(conn, id)?
        .ok_or_else(|| anyhow::anyhow!("thesis_dependencies row {} not found", id))?;

    let now = chrono::Utc::now();
    let as_of = as_of_date
        .map(|s| s.to_string())
        .unwrap_or_else(|| now.format("%Y-%m-%d").to_string());

    let antecedent_state = evaluate_text(conn, &chain.antecedent_text, &as_of)?;
    let consequent_state = evaluate_text(conn, &chain.consequent_text, &as_of)?;

    let (new_state, note) = derive_state(&chain.relation, &antecedent_state, &consequent_state);

    let last_validated_at = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "UPDATE thesis_dependencies
         SET current_state = ?1, last_validated_at = ?2
         WHERE id = ?3",
        params![new_state, last_validated_at, id],
    )?;

    Ok(ValidationOutcome {
        id,
        antecedent_state,
        consequent_state,
        new_chain_state: new_state,
        note,
    })
}

/// Decide the new chain state given the relation type and per-side
/// predicate outcomes. Documented inline so the rubric can be reviewed
/// independently of the SQL layer.
///
/// `implies`         — A=>C: confirmed when both sides true, disconfirmed
///                     when A true and C false, open otherwise.
/// `contradicts`     — A contradicts C: confirmed when A true and C false,
///                     disconfirmed when both true, open otherwise.
/// `contingent-on`   — C contingent on A: confirmed when both true,
///                     disconfirmed when C true but A false (consequent
///                     fired without the antecedent), open otherwise.
/// `accelerates` /
/// `dampens`         — directional rather than binary; we only flag
///                     `confirmed` when both sides are true and otherwise
///                     leave the chain `open`.
fn derive_state(
    relation: &str,
    a: &PredicateOutcome,
    c: &PredicateOutcome,
) -> (String, String) {
    use PredicateOutcome::*;
    match relation {
        "implies" => match (a, c) {
            (True, True) => ("confirmed".into(), "antecedent and consequent both true".into()),
            (True, False) => (
                "disconfirmed".into(),
                "antecedent true but consequent false".into(),
            ),
            (NotEvaluable, _) | (_, NotEvaluable) => (
                "open".into(),
                "not yet evaluable (predicate did not parse)".into(),
            ),
            _ => ("open".into(), "insufficient evidence to confirm".into()),
        },
        "contradicts" => match (a, c) {
            (True, False) => (
                "confirmed".into(),
                "antecedent true and consequent false — contradiction holds".into(),
            ),
            (True, True) => (
                "disconfirmed".into(),
                "antecedent and consequent both true — contradiction fails".into(),
            ),
            (NotEvaluable, _) | (_, NotEvaluable) => (
                "open".into(),
                "not yet evaluable (predicate did not parse)".into(),
            ),
            _ => ("open".into(), "insufficient evidence to confirm".into()),
        },
        "contingent-on" => match (a, c) {
            (True, True) => (
                "confirmed".into(),
                "consequent observed with antecedent true".into(),
            ),
            (False, True) => (
                "disconfirmed".into(),
                "consequent fired without antecedent".into(),
            ),
            (NotEvaluable, _) | (_, NotEvaluable) => (
                "open".into(),
                "not yet evaluable (predicate did not parse)".into(),
            ),
            _ => ("open".into(), "insufficient evidence to confirm".into()),
        },
        "accelerates" | "dampens" => match (a, c) {
            (True, True) => (
                "confirmed".into(),
                format!("{} relation: both sides currently true", relation),
            ),
            (NotEvaluable, _) | (_, NotEvaluable) => (
                "open".into(),
                "not yet evaluable (predicate did not parse)".into(),
            ),
            _ => (
                "open".into(),
                format!("{} relation: insufficient evidence", relation),
            ),
        },
        _ => ("open".into(), format!("unknown relation: {}", relation)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        // price_history table for validation tests.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                volume TEXT,
                open TEXT,
                high TEXT,
                low TEXT,
                PRIMARY KEY (symbol, date)
            );",
        )
        .unwrap();
        conn
    }

    fn seed_price(conn: &Connection, symbol: &str, date: &str, close: &str) {
        let _ = Decimal::from_str(close).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO price_history (symbol, date, close)
             VALUES (?1, ?2, ?3)",
            params![symbol, date, close],
        )
        .unwrap();
    }

    #[test]
    fn parse_predicate_basic_forms() {
        let p = parse_predicate("XAU > 4500").unwrap();
        assert_eq!(p.symbol, "XAU");
        assert_eq!(p.op, "gt");
        assert!((p.value - 4500.0).abs() < f64::EPSILON);

        let p = parse_predicate("BTC-USD >= 100000").unwrap();
        assert_eq!(p.symbol, "BTC-USD");
        assert_eq!(p.op, "gte");

        let p = parse_predicate("DXY <= 102.5").unwrap();
        assert_eq!(p.symbol, "DXY");
        assert_eq!(p.op, "lte");

        // Free-form prose should NOT parse.
        assert!(parse_predicate("if real yields back off, gold should grind higher").is_none());
    }

    #[test]
    fn validate_implies_chain_confirmed_when_both_true() {
        let conn = fresh_conn();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        seed_price(&conn, "XAU", &today, "4600");
        seed_price(&conn, "BTC", &today, "120000");
        let id = insert(
            &conn,
            None,
            "XAU > 4500",
            "implies",
            None,
            "BTC > 100000",
            1,
            Some("high"),
            None,
            None,
        )
        .unwrap();
        let outcome = validate_chain(&conn, id, None).unwrap();
        assert_eq!(outcome.antecedent_state, PredicateOutcome::True);
        assert_eq!(outcome.consequent_state, PredicateOutcome::True);
        assert_eq!(outcome.new_chain_state, "confirmed");

        let chain = get(&conn, id).unwrap().unwrap();
        assert_eq!(chain.current_state, "confirmed");
        assert!(chain.last_validated_at.is_some());
    }

    #[test]
    fn validate_implies_chain_disconfirmed_when_antecedent_true_consequent_false() {
        let conn = fresh_conn();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        seed_price(&conn, "XAU", &today, "4600");
        seed_price(&conn, "BTC", &today, "80000");
        let id = insert(
            &conn,
            None,
            "XAU > 4500",
            "implies",
            None,
            "BTC > 100000",
            1,
            None,
            None,
            None,
        )
        .unwrap();
        let outcome = validate_chain(&conn, id, None).unwrap();
        assert_eq!(outcome.antecedent_state, PredicateOutcome::True);
        assert_eq!(outcome.consequent_state, PredicateOutcome::False);
        assert_eq!(outcome.new_chain_state, "disconfirmed");
    }

    #[test]
    fn unparseable_predicate_leaves_state_open() {
        let conn = fresh_conn();
        let id = insert(
            &conn,
            None,
            "BRICS de-dollarisation accelerates",
            "implies",
            None,
            "gold floor rises",
            1,
            None,
            None,
            None,
        )
        .unwrap();
        let outcome = validate_chain(&conn, id, None).unwrap();
        assert_eq!(outcome.antecedent_state, PredicateOutcome::NotEvaluable);
        assert_eq!(outcome.consequent_state, PredicateOutcome::NotEvaluable);
        assert_eq!(outcome.new_chain_state, "open");
        assert!(outcome.note.contains("not yet evaluable"));
    }

    #[test]
    fn contradicts_relation_state_logic() {
        let conn = fresh_conn();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        seed_price(&conn, "DXY", &today, "105");
        seed_price(&conn, "GOLD", &today, "4000");
        let id = insert(
            &conn,
            None,
            "DXY > 104",
            "contradicts",
            None,
            "GOLD > 4500",
            1,
            None,
            None,
            None,
        )
        .unwrap();
        let outcome = validate_chain(&conn, id, None).unwrap();
        assert_eq!(outcome.antecedent_state, PredicateOutcome::True);
        assert_eq!(outcome.consequent_state, PredicateOutcome::False);
        assert_eq!(outcome.new_chain_state, "confirmed");
    }

    #[test]
    fn list_filters_by_state_and_node() {
        let conn = fresh_conn();
        insert(
            &conn,
            Some("xau"),
            "XAU > 4500",
            "implies",
            Some("btc"),
            "BTC > 100000",
            1,
            None,
            None,
            None,
        )
        .unwrap();
        insert(
            &conn,
            Some("dxy"),
            "DXY > 104",
            "contradicts",
            Some("gold"),
            "GOLD > 4500",
            1,
            None,
            None,
            None,
        )
        .unwrap();
        let all = list(&conn, None, None).unwrap();
        assert_eq!(all.len(), 2);
        let only_xau = list(&conn, None, Some("xau")).unwrap();
        assert_eq!(only_xau.len(), 1);
        let by_state = list(&conn, Some("open"), None).unwrap();
        assert_eq!(by_state.len(), 2);
    }

    #[test]
    fn find_chains_for_symbol_matches_substring_and_id() {
        let conn = fresh_conn();
        insert(
            &conn,
            Some("xau-node"),
            "XAU > 4500",
            "implies",
            None,
            "BTC > 100000",
            1,
            None,
            None,
            None,
        )
        .unwrap();
        insert(
            &conn,
            None,
            "DXY > 104",
            "contradicts",
            Some("gold-node"),
            "GOLD > 4500",
            1,
            None,
            None,
            None,
        )
        .unwrap();
        let by_text = find_chains_for_symbol(&conn, "BTC").unwrap();
        assert_eq!(by_text.len(), 1);
        let by_id = find_chains_for_symbol(&conn, "gold-node").unwrap();
        assert_eq!(by_id.len(), 1);
    }

    #[test]
    fn reject_invalid_relation() {
        let conn = fresh_conn();
        let err = insert(
            &conn,
            None,
            "X > 1",
            "magic",
            None,
            "Y > 2",
            1,
            None,
            None,
            None,
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("invalid relation"));
    }

    #[test]
    fn source_lessons_round_trip_as_json() {
        let conn = fresh_conn();
        let id = insert(
            &conn,
            None,
            "X > 1",
            "implies",
            None,
            "Y > 2",
            1,
            None,
            Some(&[42, 99]),
            Some(&["macro-thesis-2026Q1".to_string()]),
        )
        .unwrap();
        let chain = get(&conn, id).unwrap().unwrap();
        let lessons: Vec<i64> =
            serde_json::from_str(chain.source_lesson_ids.as_deref().unwrap()).unwrap();
        assert_eq!(lessons, vec![42, 99]);
        let sections: Vec<String> =
            serde_json::from_str(chain.source_thesis_sections.as_deref().unwrap()).unwrap();
        assert_eq!(sections, vec!["macro-thesis-2026Q1".to_string()]);
    }
}
