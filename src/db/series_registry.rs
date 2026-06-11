//! Canonical-series registry (R3) — registration, not physical migration.
//!
//! `series_registry` is the L1 meta-table: one row per canonical time
//! series the system depends on, naming WHERE the series physically lives
//! (`storage_table` + `storage_filter` + `date_column`) and the freshness
//! SLA it must meet. Downstream freshness machinery (`pftui data series
//! status`, the `system doctor` staleness check) is driven entirely by
//! these rows — no more ad-hoc per-table staleness logic for registered
//! series.
//!
//! Physical consolidation of the underlying tables is explicitly deferred:
//! registration now, physical merge when a consumer needs it (see
//! docs/DATA-ARCHITECTURE.md).

use anyhow::{bail, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

// Valid `kind` values are enforced by the table's CHECK constraint:
// price | sentiment | economic | flow | positioning | onchain |
// derived_external.

#[derive(Debug, Clone, Serialize)]
pub struct SeriesEntry {
    pub series_id: String,
    pub kind: String,
    pub storage_table: String,
    pub storage_filter: Option<String>,
    pub date_column: String,
    pub canonical_symbol: Option<String>,
    pub deep_alias: Option<String>,
    pub source: Option<String>,
    pub units: Option<String>,
    pub freshness_sla_hours: i64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeriesStatus {
    #[serde(flatten)]
    pub entry: SeriesEntry,
    /// MAX(date_column) for the series, as stored (date or datetime text).
    pub last_datapoint: Option<String>,
    /// Hours since the last datapoint (None when the series is empty or
    /// its storage table is missing).
    pub age_hours: Option<f64>,
    /// True when age exceeds the SLA, or the series has no data at all.
    pub stale: bool,
    /// True when age exceeds 2× the SLA (doctor warning threshold), or the
    /// series has no data at all.
    pub past_2x_sla: bool,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS series_registry (
            series_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL CHECK (kind IN
                ('price','sentiment','economic','flow','positioning','onchain','derived_external')),
            storage_table TEXT NOT NULL,
            storage_filter TEXT,
            date_column TEXT NOT NULL,
            canonical_symbol TEXT,
            deep_alias TEXT,
            source TEXT,
            units TEXT,
            freshness_sla_hours INTEGER NOT NULL,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    Ok(())
}

/// Seed the core series. `INSERT OR IGNORE` so operator edits (e.g. a
/// tightened SLA) survive restarts; re-running is a no-op.
pub fn seed(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR IGNORE INTO series_registry
         (series_id, kind, storage_table, storage_filter, date_column,
          canonical_symbol, deep_alias, source, units, freshness_sla_hours, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for e in seed_entries() {
        stmt.execute(params![
            e.series_id,
            e.kind,
            e.storage_table,
            e.storage_filter,
            e.date_column,
            e.canonical_symbol,
            e.deep_alias,
            e.source,
            e.units,
            e.freshness_sla_hours,
            e.notes,
        ])?;
    }
    upgrade_fallback_chain_sources(conn)?;
    Ok(())
}

/// One-time upgrade for pre-fallback DBs: rewrite the btc/gold `source`
/// from the old single-source default ('yahoo') to the full fallback chain.
/// Only fires while the row still carries the old default, so operator
/// edits survive (same contract as the INSERT OR IGNORE seed).
fn upgrade_fallback_chain_sources(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE series_registry SET source = ?1
         WHERE series_id = 'btc' AND source = 'yahoo'",
        params![BTC_SOURCE_CHAIN],
    )?;
    conn.execute(
        "UPDATE series_registry SET source = ?1
         WHERE series_id = 'gold' AND source = 'yahoo'",
        params![GOLD_SOURCE_CHAIN],
    )?;
    Ok(())
}

pub fn ensure_and_seed(conn: &Connection) -> Result<()> {
    ensure_table(conn)?;
    seed(conn)
}

/// Spot-price source chains for series with redundant fallback sources
/// (primary→…→last resort, as wired in `commands/refresh.rs`).
pub const BTC_SOURCE_CHAIN: &str = "coingecko→yahoo→mempool.space";
pub const GOLD_SOURCE_CHAIN: &str = "yahoo→geckoterminal-xaut";

fn price(series_id: &str, symbol: &str, sla: i64, notes: Option<&str>) -> SeriesEntry {
    SeriesEntry {
        series_id: series_id.to_string(),
        kind: "price".to_string(),
        storage_table: "price_history".to_string(),
        storage_filter: Some(format!("symbol='{symbol}'")),
        date_column: "date".to_string(),
        canonical_symbol: Some(symbol.to_string()),
        deep_alias: None,
        source: Some("yahoo".to_string()),
        units: Some("USD".to_string()),
        freshness_sla_hours: sla,
        notes: notes.map(|s| s.to_string()),
    }
}

fn econ(indicator: &str) -> SeriesEntry {
    SeriesEntry {
        series_id: format!("econ-{}", indicator.replace('_', "-")),
        kind: "economic".to_string(),
        storage_table: "economic_data".to_string(),
        storage_filter: Some(format!("indicator='{indicator}' AND quarantined = 0")),
        date_column: "fetched_at".to_string(),
        canonical_symbol: None,
        deep_alias: None,
        source: Some("brave".to_string()),
        units: None,
        freshness_sla_hours: 72,
        notes: Some(
            "Head row per indicator; freshness = fetched_at. Plausible-range \
             quarantined rows excluded."
                .to_string(),
        ),
    }
}

fn cot(series_id: &str, cftc_code: &str, symbol: &str, name: &str) -> SeriesEntry {
    SeriesEntry {
        series_id: series_id.to_string(),
        kind: "positioning".to_string(),
        storage_table: "cot_cache".to_string(),
        storage_filter: Some(format!("cftc_code='{cftc_code}'")),
        date_column: "report_date".to_string(),
        canonical_symbol: Some(symbol.to_string()),
        deep_alias: None,
        source: Some("cftc".to_string()),
        units: Some("contracts".to_string()),
        freshness_sla_hours: 192,
        notes: Some(format!("CFTC COT {name} (weekly report)")),
    }
}

/// The canonical seed set: every held/major price symbol, both sentiment
/// gauges, every plausible-range economic indicator, ETF flows, exchange
/// reserves, and the four COT contracts.
pub fn seed_entries() -> Vec<SeriesEntry> {
    let mut gold = price(
        "gold",
        "GC=F",
        72,
        Some(
            "Spot fallback: GeckoTerminal XAUt/USDT pool (on-chain proxy, \
             divergence-guarded at 5%)",
        ),
    );
    gold.source = Some(GOLD_SOURCE_CHAIN.to_string());
    let mut entries = vec![
        gold,
        price("silver", "SI=F", 72, None),
        price("gld", "GLD", 72, None),
        price("spy", "SPY", 72, None),
        price("sp500", "^GSPC", 72, None),
        price("vix", "^VIX", 72, Some("Index points, not USD")),
        price("dxy", "DX-Y.NYB", 72, Some("Index points, not USD")),
        price("us10y", "^TNX", 72, Some("Yield x10, not USD")),
        price("wti", "CL=F", 72, None),
    ];
    let mut btc = price(
        "btc",
        "BTC",
        72,
        Some(
            "Spot series; BTC-USD is the deep Yahoo series (doctor guards divergence). \
             Last-resort spot fallback: mempool.space (divergence-guarded at 5%)",
        ),
    );
    btc.deep_alias = Some("BTC-USD".to_string());
    btc.source = Some(BTC_SOURCE_CHAIN.to_string());
    entries.push(btc);

    entries.push(SeriesEntry {
        series_id: "crypto-fear-greed".to_string(),
        kind: "sentiment".to_string(),
        storage_table: "sentiment_history".to_string(),
        storage_filter: Some("index_type='crypto'".to_string()),
        date_column: "date".to_string(),
        canonical_symbol: None,
        deep_alias: None,
        source: Some("alternative.me".to_string()),
        units: Some("index 0-100".to_string()),
        freshness_sla_hours: 72,
        notes: None,
    });
    entries.push(SeriesEntry {
        series_id: "traditional-fear-greed".to_string(),
        kind: "sentiment".to_string(),
        storage_table: "sentiment_history".to_string(),
        storage_filter: Some("index_type='traditional'".to_string()),
        date_column: "date".to_string(),
        canonical_symbol: None,
        deep_alias: None,
        source: Some("cnn".to_string()),
        units: Some("index 0-100".to_string()),
        freshness_sla_hours: 72,
        notes: None,
    });

    // Every indicator in db::economic_data::plausible_range's list.
    for indicator in [
        "cpi",
        "ppi",
        "nfp",
        "unemployment_rate",
        "fed_funds_rate",
        "initial_jobless_claims",
        "pmi_manufacturing",
        "pmi_services",
    ] {
        entries.push(econ(indicator));
    }

    entries.push(SeriesEntry {
        series_id: "btc-etf-flows".to_string(),
        kind: "flow".to_string(),
        storage_table: "onchain_cache".to_string(),
        storage_filter: Some("metric LIKE 'etf_flow_%'".to_string()),
        date_column: "date".to_string(),
        canonical_symbol: Some("BTC".to_string()),
        deep_alias: None,
        source: Some("btcetffundflow.com".to_string()),
        units: Some("BTC".to_string()),
        freshness_sla_hours: 72,
        notes: Some("Per-fund daily net flows; stored per metric=etf_flow_<FUND>".to_string()),
    });
    entries.push(SeriesEntry {
        series_id: "btc-exchange-reserves".to_string(),
        kind: "onchain".to_string(),
        storage_table: "onchain_cache".to_string(),
        storage_filter: Some("metric='exchange_reserve_proxy_btc'".to_string()),
        date_column: "date".to_string(),
        canonical_symbol: Some("BTC".to_string()),
        deep_alias: None,
        source: Some("onchain".to_string()),
        units: Some("BTC".to_string()),
        freshness_sla_hours: 72,
        notes: None,
    });

    entries.push(cot("cot-gold", "088691", "GC=F", "Gold Futures"));
    entries.push(cot("cot-silver", "084691", "SI=F", "Silver Futures"));
    entries.push(cot("cot-wti", "067411", "CL=F", "WTI Crude Oil Futures"));
    entries.push(cot("cot-btc", "133741", "BTC", "Bitcoin Futures"));

    entries
}

pub fn list(conn: &Connection) -> Result<Vec<SeriesEntry>> {
    let mut stmt = conn.prepare(
        "SELECT series_id, kind, storage_table, storage_filter, date_column,
                canonical_symbol, deep_alias, source, units, freshness_sla_hours, notes
         FROM series_registry
         ORDER BY kind, series_id",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SeriesEntry {
                series_id: row.get(0)?,
                kind: row.get(1)?,
                storage_table: row.get(2)?,
                storage_filter: row.get(3)?,
                date_column: row.get(4)?,
                canonical_symbol: row.get(5)?,
                deep_alias: row.get(6)?,
                source: row.get(7)?,
                units: row.get(8)?,
                freshness_sla_hours: row.get(9)?,
                notes: row.get(10)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn valid_ident(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `MAX(date_column)` for a registered series, or None when the storage
/// table is missing or the filter matches no rows.
pub fn last_datapoint(conn: &Connection, entry: &SeriesEntry) -> Result<Option<String>> {
    if !valid_ident(&entry.storage_table) || !valid_ident(&entry.date_column) {
        bail!(
            "series {} has invalid storage_table/date_column identifiers",
            entry.series_id
        );
    }
    if !crate::db::archive::table_exists(conn, &entry.storage_table)? {
        return Ok(None);
    }
    let where_clause = entry
        .storage_filter
        .as_deref()
        .filter(|f| !f.trim().is_empty())
        .map(|f| format!(" WHERE {f}"))
        .unwrap_or_default();
    let sql = format!(
        "SELECT MAX(\"{}\") FROM \"{}\"{}",
        entry.date_column, entry.storage_table, where_clause
    );
    let value: Option<String> = conn
        .query_row(&sql, [], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten();
    Ok(value)
}

/// Parse the stored date/datetime text formats used across pftui tables:
/// RFC3339, `YYYY-MM-DD HH:MM:SS`, `YYYY-MM-DDTHH:MM:SS`, bare `YYYY-MM-DD`
/// (treated as midnight UTC).
pub fn parse_datapoint_time(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(raw, fmt) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    // Bare date — also handles a leading date in longer non-standard strings.
    let head: String = raw.chars().take(10).collect();
    if let Ok(d) = NaiveDate::parse_from_str(&head, "%Y-%m-%d") {
        return Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?));
    }
    None
}

/// Compute freshness status for one entry at `now` (parameterized for
/// deterministic tests).
pub fn status_for(
    conn: &Connection,
    entry: &SeriesEntry,
    now: DateTime<Utc>,
) -> Result<SeriesStatus> {
    let last = last_datapoint(conn, entry)?;
    let age_hours = last
        .as_deref()
        .and_then(parse_datapoint_time)
        .map(|t| (now - t).num_seconds() as f64 / 3600.0);
    let sla = entry.freshness_sla_hours as f64;
    let (stale, past_2x) = match age_hours {
        Some(age) => (age > sla, age > 2.0 * sla),
        // Empty series or missing table: loudly stale.
        None => (true, true),
    };
    Ok(SeriesStatus {
        entry: entry.clone(),
        last_datapoint: last,
        age_hours,
        stale,
        past_2x_sla: past_2x,
    })
}

/// Status for every registered series.
pub fn status_all(conn: &Connection, now: DateTime<Utc>) -> Result<Vec<SeriesStatus>> {
    ensure_and_seed(conn)?;
    let entries = list(conn)?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in &entries {
        out.push(status_for(conn, entry, now)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn conn_with_registry() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_and_seed(&conn).unwrap();
        conn
    }

    #[test]
    fn seed_is_idempotent_and_covers_core_series() {
        let conn = conn_with_registry();
        let first = list(&conn).unwrap().len();
        assert!(first >= 26, "expected >= 26 seeded series, got {first}");
        ensure_and_seed(&conn).unwrap();
        assert_eq!(list(&conn).unwrap().len(), first);

        let entries = list(&conn).unwrap();
        let btc = entries.iter().find(|e| e.series_id == "btc").unwrap();
        assert_eq!(btc.deep_alias.as_deref(), Some("BTC-USD"));
        let cot = entries.iter().find(|e| e.series_id == "cot-gold").unwrap();
        assert_eq!(cot.freshness_sla_hours, 192);
        assert_eq!(cot.storage_table, "cot_cache");
    }

    #[test]
    fn seed_records_fallback_chains_for_btc_and_gold() {
        let conn = conn_with_registry();
        let entries = list(&conn).unwrap();
        let btc = entries.iter().find(|e| e.series_id == "btc").unwrap();
        assert_eq!(btc.source.as_deref(), Some(BTC_SOURCE_CHAIN));
        let gold = entries.iter().find(|e| e.series_id == "gold").unwrap();
        assert_eq!(gold.source.as_deref(), Some(GOLD_SOURCE_CHAIN));
        assert!(gold.notes.as_deref().unwrap_or("").contains("GeckoTerminal"));
    }

    #[test]
    fn legacy_yahoo_source_rows_upgrade_to_fallback_chain() {
        let conn = conn_with_registry();
        // Simulate a pre-fallback DB: rows still carry the old default.
        conn.execute(
            "UPDATE series_registry SET source = 'yahoo'
             WHERE series_id IN ('btc', 'gold')",
            [],
        )
        .unwrap();
        ensure_and_seed(&conn).unwrap();
        let entries = list(&conn).unwrap();
        let btc = entries.iter().find(|e| e.series_id == "btc").unwrap();
        assert_eq!(btc.source.as_deref(), Some(BTC_SOURCE_CHAIN));
        let gold = entries.iter().find(|e| e.series_id == "gold").unwrap();
        assert_eq!(gold.source.as_deref(), Some(GOLD_SOURCE_CHAIN));
    }

    #[test]
    fn operator_customized_source_survives_chain_upgrade() {
        let conn = conn_with_registry();
        conn.execute(
            "UPDATE series_registry SET source = 'my-custom-feed' WHERE series_id = 'btc'",
            [],
        )
        .unwrap();
        ensure_and_seed(&conn).unwrap();
        let entries = list(&conn).unwrap();
        let btc = entries.iter().find(|e| e.series_id == "btc").unwrap();
        assert_eq!(btc.source.as_deref(), Some("my-custom-feed"));
    }

    #[test]
    fn seed_survives_operator_edits() {
        let conn = conn_with_registry();
        conn.execute(
            "UPDATE series_registry SET freshness_sla_hours = 24 WHERE series_id = 'gold'",
            [],
        )
        .unwrap();
        ensure_and_seed(&conn).unwrap();
        let sla: i64 = conn
            .query_row(
                "SELECT freshness_sla_hours FROM series_registry WHERE series_id = 'gold'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sla, 24);
    }

    #[test]
    fn status_math_against_synthetic_sla() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn.execute_batch(
            "CREATE TABLE price_history (symbol TEXT, date TEXT, close TEXT);
             INSERT INTO price_history VALUES ('GC=F', '2026-06-01', '3300');
             INSERT INTO price_history VALUES ('GC=F', '2026-06-08', '3350');
             INSERT INTO price_history VALUES ('SI=F', '2026-04-01', '33');",
        )
        .unwrap();
        let entry = |id: &str, sym: &str, sla: i64| SeriesEntry {
            series_id: id.to_string(),
            kind: "price".to_string(),
            storage_table: "price_history".to_string(),
            storage_filter: Some(format!("symbol='{sym}'")),
            date_column: "date".to_string(),
            canonical_symbol: Some(sym.to_string()),
            deep_alias: None,
            source: None,
            units: None,
            freshness_sla_hours: sla,
            notes: None,
        };
        let now = Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap();

        // Fresh: last point 36h ago, SLA 72h.
        let s = status_for(&conn, &entry("gold", "GC=F", 72), now).unwrap();
        assert_eq!(s.last_datapoint.as_deref(), Some("2026-06-08"));
        let age = s.age_hours.unwrap();
        assert!((age - 36.0).abs() < 0.01, "age {age}");
        assert!(!s.stale && !s.past_2x_sla);

        // Stale but under 2x: same data with a 24h SLA (36h > 24, < 48).
        let s = status_for(&conn, &entry("gold-tight", "GC=F", 24), now).unwrap();
        assert!(s.stale && !s.past_2x_sla);

        // Past 2x: silver last point ~69 days old, SLA 72h.
        let s = status_for(&conn, &entry("silver", "SI=F", 72), now).unwrap();
        assert!(s.stale && s.past_2x_sla);

        // No data at all -> loudly stale.
        let s = status_for(&conn, &entry("plat", "PL=F", 72), now).unwrap();
        assert!(s.last_datapoint.is_none() && s.age_hours.is_none());
        assert!(s.stale && s.past_2x_sla);
    }

    #[test]
    fn status_handles_missing_storage_table() {
        let conn = Connection::open_in_memory().unwrap();
        let entry = SeriesEntry {
            series_id: "ghost".to_string(),
            kind: "price".to_string(),
            storage_table: "table_that_does_not_exist".to_string(),
            storage_filter: None,
            date_column: "date".to_string(),
            canonical_symbol: None,
            deep_alias: None,
            source: None,
            units: None,
            freshness_sla_hours: 72,
            notes: None,
        };
        let s = status_for(&conn, &entry, Utc::now()).unwrap();
        assert!(s.last_datapoint.is_none() && s.stale && s.past_2x_sla);
    }

    #[test]
    fn parses_all_stored_date_formats() {
        let now = Utc::now();
        for raw in [
            "2026-06-08",
            "2026-06-08 14:30:00",
            "2026-06-08T14:30:00",
            "2026-06-08T14:30:00Z",
            "2026-06-08T14:30:00+00:00",
        ] {
            let parsed = parse_datapoint_time(raw)
                .unwrap_or_else(|| panic!("failed to parse {raw:?}"));
            assert!(now - parsed < Duration::days(365 * 10));
        }
        assert!(parse_datapoint_time("garbage").is_none());
    }
}
