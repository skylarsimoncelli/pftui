//! `research_evidence` — L3 append-only ledger of structured web-research
//! findings and their sources.
//!
//! Report-run analysts (high/macro timeframe routines, phase-2c external-TA,
//! phase-2d antithesis, deepdive) do deep web research — IMF COFER, SIPRI/WGC,
//! settlement volumes, on-chain dashboards, sell-side notes. Today those
//! sources, URLs and findings melt into free-text prose and evaporate. This
//! ledger captures them as first-class rows so they survive and become
//! queryable / citeable.
//!
//! Contract: append-only. The writer NEVER updates or deletes a row. Rows are
//! system output (the research record), never mutated after insert.
//!
//! Consumer: `pftui research evidence list` and the competence dossier
//! (`pftui research dossier`), which surfaces the relevant captured sources so
//! the research record is actually consumed in output.

use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// One captured research finding and its source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceRow {
    pub id: i64,
    pub created_at: String,
    /// Report/research date the finding belongs to (YYYY-MM-DD).
    pub run_date: String,
    /// Analyst origin: high|macro|medium|low|external-ta|antithesis|deepdive
    /// (free text — the layer that captured the source).
    pub layer: String,
    /// Symbol the finding concerns, or NULL for macro-wide findings.
    pub asset: Option<String>,
    /// The assertion the evidence supports.
    pub claim: String,
    /// Source name (institution / outlet / dashboard).
    pub source_name: String,
    /// Source URL (nullable).
    pub source_url: Option<String>,
    /// When the source was published, YYYY-MM-DD (nullable).
    pub source_date: Option<String>,
    /// The actual extracted finding / quote.
    pub finding: String,
    /// supports | refutes | context (optional).
    pub stance: Option<String>,
    /// Optional confidence, decimal-as-TEXT in storage.
    pub confidence: Option<String>,
}

const VALID_STANCES: &[&str] = &["supports", "refutes", "context"];

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS research_evidence (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            run_date TEXT NOT NULL,
            layer TEXT NOT NULL,
            asset TEXT,
            claim TEXT NOT NULL,
            source_name TEXT NOT NULL,
            source_url TEXT,
            source_date TEXT,
            finding TEXT NOT NULL,
            stance TEXT,
            confidence TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_research_evidence_asset ON research_evidence(asset);
        CREATE INDEX IF NOT EXISTS idx_research_evidence_layer ON research_evidence(layer);
        CREATE INDEX IF NOT EXISTS idx_research_evidence_run_date ON research_evidence(run_date);",
    )?;
    Ok(())
}

/// Validated input for an append. All trimming/validation happens here so both
/// the CLI and tests share one contract.
#[allow(clippy::too_many_arguments)]
pub fn add(
    conn: &Connection,
    run_date: &str,
    layer: &str,
    asset: Option<&str>,
    claim: &str,
    source_name: &str,
    source_url: Option<&str>,
    source_date: Option<&str>,
    finding: &str,
    stance: Option<&str>,
    confidence: Option<&str>,
) -> Result<EvidenceRow> {
    ensure_table(conn)?;

    let run_date = run_date.trim();
    let layer = layer.trim();
    let claim = claim.trim();
    let source_name = source_name.trim();
    let finding = finding.trim();

    if run_date.is_empty() {
        bail!("--run-date must not be empty");
    }
    if layer.is_empty() {
        bail!("--layer must not be empty");
    }
    if claim.is_empty() {
        bail!("--claim must not be empty");
    }
    if source_name.is_empty() {
        bail!("--source must not be empty");
    }
    if finding.is_empty() {
        bail!("--finding must not be empty");
    }

    let asset = asset
        .map(|a| a.trim().to_uppercase())
        .filter(|a| !a.is_empty());

    let stance = match stance.map(|s| s.trim().to_ascii_lowercase()) {
        None => None,
        Some(s) if s.is_empty() => None,
        Some(s) => {
            if !VALID_STANCES.contains(&s.as_str()) {
                bail!(
                    "invalid --stance '{s}'; must be one of supports|refutes|context"
                );
            }
            Some(s)
        }
    };

    // Confidence stored as a canonical decimal string (decimal-as-TEXT).
    let confidence = match confidence.map(|c| c.trim().to_string()) {
        None => None,
        Some(c) if c.is_empty() => None,
        Some(c) => {
            let dec = Decimal::from_str(&c)
                .map_err(|_| anyhow::anyhow!("invalid --confidence '{c}'; must be a decimal"))?;
            Some(dec.normalize().to_string())
        }
    };

    let source_url = source_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    let source_date = source_date
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());

    conn.execute(
        "INSERT INTO research_evidence
            (run_date, layer, asset, claim, source_name, source_url, source_date,
             finding, stance, confidence)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            run_date,
            layer,
            asset,
            claim,
            source_name,
            source_url,
            source_date,
            finding,
            stance,
            confidence,
        ],
    )?;
    let id = conn.last_insert_rowid();
    get(conn, id)?.ok_or_else(|| anyhow::anyhow!("inserted row {id} vanished"))
}

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<EvidenceRow> {
    Ok(EvidenceRow {
        id: row.get(0)?,
        created_at: row.get(1)?,
        run_date: row.get(2)?,
        layer: row.get(3)?,
        asset: row.get(4)?,
        claim: row.get(5)?,
        source_name: row.get(6)?,
        source_url: row.get(7)?,
        source_date: row.get(8)?,
        finding: row.get(9)?,
        stance: row.get(10)?,
        confidence: row.get(11)?,
    })
}

const SELECT_COLS: &str = "id, created_at, run_date, layer, asset, claim, source_name,
     source_url, source_date, finding, stance, confidence";

pub fn get(conn: &Connection, id: i64) -> Result<Option<EvidenceRow>> {
    ensure_table(conn)?;
    let sql = format!("SELECT {SELECT_COLS} FROM research_evidence WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], map_row)?;
    Ok(rows.next().transpose()?)
}

/// Filters for `list`. Any field None = unfiltered.
#[derive(Debug, Default, Clone)]
pub struct EvidenceFilter<'a> {
    pub asset: Option<&'a str>,
    pub layer: Option<&'a str>,
    /// Inclusive lower bound on run_date (YYYY-MM-DD).
    pub since: Option<&'a str>,
    pub source: Option<&'a str>,
    pub limit: Option<i64>,
}

/// List evidence rows, newest-first (run_date DESC, id DESC).
pub fn list(conn: &Connection, filter: &EvidenceFilter) -> Result<Vec<EvidenceRow>> {
    ensure_table(conn)?;
    let mut sql = format!("SELECT {SELECT_COLS} FROM research_evidence WHERE 1=1");
    let mut binds: Vec<String> = Vec::new();
    if let Some(asset) = filter.asset {
        binds.push(asset.trim().to_uppercase());
        sql.push_str(&format!(" AND asset = ?{}", binds.len()));
    }
    if let Some(layer) = filter.layer {
        binds.push(layer.trim().to_string());
        sql.push_str(&format!(" AND layer = ?{}", binds.len()));
    }
    if let Some(since) = filter.since {
        binds.push(since.trim().to_string());
        sql.push_str(&format!(" AND run_date >= ?{}", binds.len()));
    }
    if let Some(source) = filter.source {
        binds.push(format!("%{}%", source.trim()));
        sql.push_str(&format!(" AND source_name LIKE ?{}", binds.len()));
    }
    sql.push_str(" ORDER BY run_date DESC, id DESC");
    if let Some(limit) = filter.limit {
        sql.push_str(&format!(" LIMIT {limit}"));
    }
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::ToSql> =
        binds.iter().map(|b| b as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(params.as_slice(), map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[allow(clippy::too_many_arguments)]
    fn seed(
        conn: &Connection,
        run_date: &str,
        layer: &str,
        asset: Option<&str>,
        source: &str,
        confidence: Option<&str>,
    ) -> EvidenceRow {
        add(
            conn,
            run_date,
            layer,
            asset,
            "claim text",
            source,
            Some("https://example.test/x"),
            Some("2026-06-01"),
            "the extracted finding",
            Some("supports"),
            confidence,
        )
        .unwrap()
    }

    #[test]
    fn add_then_get_roundtrips() {
        let conn = fresh();
        let row = seed(&conn, "2026-06-23", "high", Some("btc"), "Glassnode", Some("3.50"));
        assert_eq!(row.layer, "high");
        assert_eq!(row.asset.as_deref(), Some("BTC")); // uppercased
        assert_eq!(row.source_name, "Glassnode");
        assert_eq!(row.confidence.as_deref(), Some("3.5")); // normalized decimal
        assert_eq!(row.stance.as_deref(), Some("supports"));
        let fetched = get(&conn, row.id).unwrap().unwrap();
        assert_eq!(fetched, row);
    }

    #[test]
    fn list_filters_and_orders_newest_first() {
        let conn = fresh();
        seed(&conn, "2026-06-20", "high", Some("BTC"), "Glassnode", None);
        seed(&conn, "2026-06-22", "macro", None, "IMF COFER", None);
        seed(&conn, "2026-06-23", "high", Some("BTC"), "TradingView", None);

        // Unfiltered: newest run_date first.
        let all = list(&conn, &EvidenceFilter::default()).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].run_date, "2026-06-23");

        // Asset filter (case-insensitive).
        let btc = list(
            &conn,
            &EvidenceFilter {
                asset: Some("btc"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(btc.len(), 2);
        assert!(btc.iter().all(|r| r.asset.as_deref() == Some("BTC")));

        // Layer filter.
        let macro_rows = list(
            &conn,
            &EvidenceFilter {
                layer: Some("macro"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(macro_rows.len(), 1);
        assert_eq!(macro_rows[0].asset, None);

        // Since filter (inclusive).
        let recent = list(
            &conn,
            &EvidenceFilter {
                since: Some("2026-06-22"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(recent.len(), 2);

        // Source substring filter.
        let imf = list(
            &conn,
            &EvidenceFilter {
                source: Some("IMF"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(imf.len(), 1);
        assert_eq!(imf[0].source_name, "IMF COFER");
    }

    #[test]
    fn append_only_no_mutation_path() {
        // The module exposes no update or delete function. A row's content is
        // fixed after insert; a second add is a NEW row, never a mutation.
        let conn = fresh();
        let a = seed(&conn, "2026-06-23", "high", Some("BTC"), "Glassnode", None);
        let b = seed(&conn, "2026-06-23", "high", Some("BTC"), "Glassnode", None);
        assert_ne!(a.id, b.id);
        assert_eq!(list(&conn, &EvidenceFilter::default()).unwrap().len(), 2);
    }

    #[test]
    fn rejects_blank_required_fields() {
        let conn = fresh();
        let err = add(
            &conn, "2026-06-23", "high", None, "   ", "src", None, None, "finding", None, None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("--claim"));
    }

    #[test]
    fn rejects_invalid_stance_and_confidence() {
        let conn = fresh();
        let err = add(
            &conn, "2026-06-23", "high", None, "c", "s", None, None, "f", Some("maybe"), None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("stance"));

        let err = add(
            &conn, "2026-06-23", "high", None, "c", "s", None, None, "f", None, Some("abc"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("confidence"));
    }

    #[test]
    fn empty_optionals_become_none() {
        let conn = fresh();
        let row = add(
            &conn, "2026-06-23", "high", Some("  "), "c", "s", Some(""), Some("  "), "f", Some(""),
            Some(""),
        )
        .unwrap();
        assert_eq!(row.asset, None);
        assert_eq!(row.source_url, None);
        assert_eq!(row.source_date, None);
        assert_eq!(row.stance, None);
        assert_eq!(row.confidence, None);
    }
}
