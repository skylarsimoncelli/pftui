use anyhow::Result;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct EconomicDataEntry {
    pub indicator: String,
    pub value: Decimal,
    pub previous: Option<Decimal>,
    pub change: Option<Decimal>,
    pub source_url: String,
    pub source: String,
    pub confidence: String,
    pub fetched_at: String,
    /// True when the stored value failed the per-indicator plausibility
    /// sanity check (`plausible_range`). Quarantined rows are excluded from
    /// `get_all`/`get_all_backend` so they never reach reports, briefs, the
    /// TUI economy tab, or the `data economy` CLI table. They remain in the
    /// database (visible via `get_quarantined`) for diagnostics.
    ///
    /// Note: this flag is computed at write time inside `upsert_entry` from
    /// the indicator/value pair — callers constructing an entry should set
    /// it to `false`; any out-of-band value is quarantined regardless.
    pub quarantined: bool,
}

/// Hardcoded per-indicator plausible value ranges (inclusive), used to
/// quarantine garbage values produced by lossy web extraction (e.g. a
/// scraped year "2024" stored as an NFP print, or "14" as PPI y/y).
///
/// Indicators not listed here have no sanity check.
pub fn plausible_range(indicator: &str) -> Option<(Decimal, Decimal)> {
    let range = match indicator {
        // CPI y/y %: deflation to high-inflation regimes
        "cpi" => (Decimal::from(-2), Decimal::from(15)),
        // PPI y/y %
        "ppi" => (Decimal::from(-10), Decimal::from(20)),
        // NFP monthly change (jobs): COVID-scale shocks to boom prints
        "nfp" => (Decimal::from(-1_000_000), Decimal::from(1_500_000)),
        // Unemployment rate %
        "unemployment_rate" => (Decimal::from(2), Decimal::from(25)),
        // Fed funds rate %
        "fed_funds_rate" => (Decimal::ZERO, Decimal::from(12)),
        // Weekly initial jobless claims
        "initial_jobless_claims" => (Decimal::from(100_000), Decimal::from(7_000_000)),
        // ISM PMI index values
        "pmi_manufacturing" | "pmi_services" => (Decimal::from(20), Decimal::from(75)),
        _ => return None,
    };
    Some(range)
}

/// True when `value` is within the plausible band for `indicator` (or the
/// indicator has no configured band).
pub fn passes_sanity_check(indicator: &str, value: Decimal) -> bool {
    match plausible_range(indicator) {
        Some((min, max)) => value >= min && value <= max,
        None => true,
    }
}

pub fn upsert_entry(conn: &Connection, entry: &EconomicDataEntry) -> Result<()> {
    let quarantined = entry.quarantined || !passes_sanity_check(&entry.indicator, entry.value);
    if quarantined {
        eprintln!(
            "warning: economic_data sanity check failed for '{}' (value {}); row quarantined",
            entry.indicator, entry.value
        );
    }
    conn.execute(
        "INSERT INTO economic_data (indicator, value, previous, change, source_url, source, confidence, fetched_at, quarantined)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(indicator) DO UPDATE SET
            value = excluded.value,
            previous = excluded.previous,
            change = excluded.change,
            source_url = excluded.source_url,
            source = excluded.source,
            confidence = excluded.confidence,
            fetched_at = excluded.fetched_at,
            quarantined = excluded.quarantined",
        params![
            entry.indicator,
            entry.value.to_string(),
            entry.previous.map(|v| v.to_string()),
            entry.change.map(|v| v.to_string()),
            entry.source_url,
            entry.source,
            entry.confidence,
            entry.fetched_at,
            quarantined as i64,
        ],
    )?;
    Ok(())
}

pub fn upsert_entry_backend(backend: &BackendConnection, entry: &EconomicDataEntry) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_entry(conn, entry),
        |pool| upsert_entry_postgres(pool, entry),
    )
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<EconomicDataEntry> {
    let value: String = row.get(1)?;
    let previous: Option<String> = row.get(2)?;
    let change: Option<String> = row.get(3)?;
    let quarantined: i64 = row.get(8)?;
    Ok(EconomicDataEntry {
        indicator: row.get(0)?,
        value: value.parse().unwrap_or(Decimal::ZERO),
        previous: previous.and_then(|v| v.parse().ok()),
        change: change.and_then(|v| v.parse().ok()),
        source_url: row.get(4)?,
        source: row.get(5)?,
        confidence: row.get(6)?,
        fetched_at: row.get(7)?,
        quarantined: quarantined != 0,
    })
}

/// All non-quarantined entries. Rows that failed the write-time sanity check
/// are deliberately excluded so no reader surfaces garbage values.
pub fn get_all(conn: &Connection) -> Result<Vec<EconomicDataEntry>> {
    let mut stmt = conn.prepare(
        "SELECT indicator, value, previous, change, source_url, source, confidence, fetched_at, quarantined
         FROM economic_data
         WHERE quarantined = 0
         ORDER BY indicator",
    )?;
    let rows = stmt.query_map([], row_to_entry)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Quarantined entries only (values that failed the plausibility sanity
/// check at write time). Surfaces should render these as
/// "unavailable (failed sanity check)" rather than showing the value.
pub fn get_quarantined(conn: &Connection) -> Result<Vec<EconomicDataEntry>> {
    let mut stmt = conn.prepare(
        "SELECT indicator, value, previous, change, source_url, source, confidence, fetched_at, quarantined
         FROM economic_data
         WHERE quarantined != 0
         ORDER BY indicator",
    )?;
    let rows = stmt.query_map([], row_to_entry)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_all_backend(backend: &BackendConnection) -> Result<Vec<EconomicDataEntry>> {
    query::dispatch(backend, get_all, get_all_postgres)
}

pub fn get_quarantined_backend(backend: &BackendConnection) -> Result<Vec<EconomicDataEntry>> {
    query::dispatch(backend, get_quarantined, get_quarantined_postgres)
}

fn get_all_postgres(pool: &PgPool) -> Result<Vec<EconomicDataEntry>> {
    get_postgres_filtered(pool, false)
}

fn get_quarantined_postgres(pool: &PgPool) -> Result<Vec<EconomicDataEntry>> {
    get_postgres_filtered(pool, true)
}

fn get_postgres_filtered(pool: &PgPool, quarantined: bool) -> Result<Vec<EconomicDataEntry>> {
    let sql = if quarantined {
        "SELECT indicator, value, previous, change, source_url,
                COALESCE(source, 'unknown'), COALESCE(confidence, 'medium'), fetched_at
         FROM economic_data
         WHERE COALESCE(quarantined, FALSE)
         ORDER BY indicator"
    } else {
        "SELECT indicator, value, previous, change, source_url,
                COALESCE(source, 'unknown'), COALESCE(confidence, 'medium'), fetched_at
         FROM economic_data
         WHERE NOT COALESCE(quarantined, FALSE)
         ORDER BY indicator"
    };
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                String,
                String,
                String,
            ),
        >(sql)
        .fetch_all(pool)
        .await
    })?;

    Ok(rows
        .into_iter()
        .map(
            |(indicator, value, previous, change, source_url, source, confidence, fetched_at)| {
                EconomicDataEntry {
                    indicator,
                    value: value.parse().unwrap_or(Decimal::ZERO),
                    previous: previous.and_then(|v| v.parse().ok()),
                    change: change.and_then(|v| v.parse().ok()),
                    source_url,
                    source,
                    confidence,
                    fetched_at,
                    quarantined,
                }
            },
        )
        .collect())
}

fn upsert_entry_postgres(pool: &PgPool, entry: &EconomicDataEntry) -> Result<()> {
    let quarantined = entry.quarantined || !passes_sanity_check(&entry.indicator, entry.value);
    if quarantined {
        eprintln!(
            "warning: economic_data sanity check failed for '{}' (value {}); row quarantined",
            entry.indicator, entry.value
        );
    }
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO economic_data (indicator, value, previous, change, source_url, source, confidence, fetched_at, quarantined)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (indicator) DO UPDATE SET
               value = EXCLUDED.value,
               previous = EXCLUDED.previous,
               change = EXCLUDED.change,
               source_url = EXCLUDED.source_url,
               source = EXCLUDED.source,
               confidence = EXCLUDED.confidence,
               fetched_at = EXCLUDED.fetched_at,
               quarantined = EXCLUDED.quarantined",
        )
        .bind(&entry.indicator)
        .bind(entry.value.to_string())
        .bind(entry.previous.map(|v| v.to_string()))
        .bind(entry.change.map(|v| v.to_string()))
        .bind(&entry.source_url)
        .bind(&entry.source)
        .bind(&entry.confidence)
        .bind(&entry.fetched_at)
        .bind(quarantined)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        crate::db::schema::run_migrations(&conn).expect("migrations");
        conn
    }

    fn entry(indicator: &str, value: Decimal) -> EconomicDataEntry {
        EconomicDataEntry {
            indicator: indicator.to_string(),
            value,
            previous: None,
            change: None,
            source_url: "https://example.invalid/test".to_string(),
            source: "test".to_string(),
            confidence: "medium".to_string(),
            fetched_at: "2026-06-10T00:00:00Z".to_string(),
            quarantined: false,
        }
    }

    #[test]
    fn plausible_range_table() {
        assert_eq!(
            plausible_range("cpi"),
            Some((Decimal::from(-2), Decimal::from(15)))
        );
        assert_eq!(
            plausible_range("nfp"),
            Some((Decimal::from(-1_000_000), Decimal::from(1_500_000)))
        );
        // Unlisted indicator: no check.
        assert_eq!(plausible_range("treasury_10y"), None);
        assert!(passes_sanity_check("treasury_10y", dec!(99999)));
    }

    #[test]
    fn in_band_value_stored_unquarantined() {
        let conn = test_conn();
        upsert_entry(&conn, &entry("cpi", dec!(3.8))).expect("upsert");
        let rows = get_all(&conn).expect("get_all");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].indicator, "cpi");
        assert!(!rows[0].quarantined);
        assert!(get_quarantined(&conn).expect("quarantined").is_empty());
    }

    #[test]
    fn out_of_band_value_quarantined_and_skipped_by_readers() {
        let conn = test_conn();
        // Live-observed garbage: a scraped year stored as the NFP print.
        upsert_entry(&conn, &entry("nfp", dec!(2024000000))).expect("upsert");
        // And "14" as PPI y/y is in-band (-10..20), but 25 is not.
        upsert_entry(&conn, &entry("ppi", dec!(25))).expect("upsert");
        // fed_funds_rate above plausible band.
        upsert_entry(&conn, &entry("fed_funds_rate", dec!(14.5))).expect("upsert");

        // Readers must not see any of these.
        assert!(get_all(&conn).expect("get_all").is_empty());

        let q = get_quarantined(&conn).expect("quarantined");
        assert_eq!(q.len(), 3);
        assert!(q.iter().all(|e| e.quarantined));
    }

    #[test]
    fn requarantine_clears_when_good_value_arrives() {
        let conn = test_conn();
        upsert_entry(&conn, &entry("cpi", dec!(99))).expect("bad upsert");
        assert!(get_all(&conn).expect("get_all").is_empty());
        // A subsequent in-band refresh replaces the quarantined row.
        upsert_entry(&conn, &entry("cpi", dec!(3.8))).expect("good upsert");
        let rows = get_all(&conn).expect("get_all");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value, dec!(3.8));
        assert!(get_quarantined(&conn).expect("quarantined").is_empty());
    }

    #[test]
    fn boundary_values_are_in_band() {
        // Inclusive bounds.
        assert!(passes_sanity_check("cpi", dec!(-2)));
        assert!(passes_sanity_check("cpi", dec!(15)));
        assert!(!passes_sanity_check("cpi", dec!(15.01)));
        assert!(passes_sanity_check("unemployment_rate", dec!(2)));
        assert!(!passes_sanity_check("unemployment_rate", dec!(1.9)));
        assert!(passes_sanity_check("initial_jobless_claims", dec!(100000)));
        assert!(!passes_sanity_check("initial_jobless_claims", dec!(99999)));
        assert!(passes_sanity_check("pmi_manufacturing", dec!(49.5)));
        assert!(!passes_sanity_check("pmi_services", dec!(14)));
        assert!(passes_sanity_check("fed_funds_rate", dec!(0)));
        assert!(!passes_sanity_check("fed_funds_rate", dec!(-0.25)));
    }
}
