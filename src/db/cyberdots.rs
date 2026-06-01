use anyhow::{bail, Result};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

pub const VALID_TIMEFRAMES: &[&str] = &["1h", "4h", "1d", "1w", "1M"];
pub const VALID_DOT_STATES: &[&str] = &["bullish", "bearish", "flat"];
pub const VALID_TRACKLINE_POSITIONS: &[&str] = &["above", "below", "on"];
pub const VALID_SOURCES: &[&str] = &["skylar-manual", "journal-parsed", "tradingview-import"];
#[allow(dead_code)]
pub const VALID_FLIPS: &[&str] = &[
    "flipped-bullish",
    "flipped-bearish",
    "held",
    "flat-confirmed",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CyberdotsSignal {
    pub id: i64,
    pub symbol: String,
    pub timeframe: String,
    pub recorded_at: String,
    pub dot_state: String,
    pub trackline_position: String,
    pub flip_from_prior: Option<String>,
    pub source: String,
    pub notes: Option<String>,
    pub related_transaction_id: Option<i64>,
}

impl CyberdotsSignal {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            symbol: row.get(1)?,
            timeframe: row.get(2)?,
            recorded_at: row.get(3)?,
            dot_state: row.get(4)?,
            trackline_position: row.get(5)?,
            flip_from_prior: row.get(6)?,
            source: row.get(7)?,
            notes: row.get(8)?,
            related_transaction_id: row.get(9)?,
        })
    }
}

pub fn validate_timeframe(tf: &str) -> Result<()> {
    if !VALID_TIMEFRAMES.contains(&tf) {
        bail!(
            "Invalid timeframe '{}'. Must be one of: {}",
            tf,
            VALID_TIMEFRAMES.join(", ")
        );
    }
    Ok(())
}

pub fn validate_dot_state(state: &str) -> Result<()> {
    if !VALID_DOT_STATES.contains(&state) {
        bail!(
            "Invalid dot state '{}'. Must be one of: {}",
            state,
            VALID_DOT_STATES.join(", ")
        );
    }
    Ok(())
}

pub fn validate_trackline_position(pos: &str) -> Result<()> {
    if !VALID_TRACKLINE_POSITIONS.contains(&pos) {
        bail!(
            "Invalid trackline position '{}'. Must be one of: {}",
            pos,
            VALID_TRACKLINE_POSITIONS.join(", ")
        );
    }
    Ok(())
}

pub fn validate_source(src: &str) -> Result<()> {
    if !VALID_SOURCES.contains(&src) {
        bail!(
            "Invalid source '{}'. Must be one of: {}",
            src,
            VALID_SOURCES.join(", ")
        );
    }
    Ok(())
}

/// Compute flip_from_prior by comparing a new (dot_state, trackline_position)
/// to the most-recent prior signal for the same (symbol, timeframe).
///
/// Rules:
///   - If there is no prior row, `None` (this row establishes the baseline).
///   - If the new dot state matches the prior dot state, it's a `held` (or
///     `flat-confirmed` when both are flat).
///   - If the dot state changes from bearish/flat -> bullish: `flipped-bullish`.
///   - If the dot state changes from bullish/flat -> bearish: `flipped-bearish`.
///   - Bullish/bearish -> flat is treated as `flat-confirmed`.
pub fn compute_flip(prior: Option<&CyberdotsSignal>, new_dot_state: &str) -> Option<String> {
    let prior = prior?;
    let prior_state = prior.dot_state.as_str();
    Some(
        match (prior_state, new_dot_state) {
            ("bullish", "bullish") | ("bearish", "bearish") => "held",
            ("flat", "flat") => "flat-confirmed",
            ("bullish", "flat") | ("bearish", "flat") => "flat-confirmed",
            (_, "bullish") => "flipped-bullish",
            (_, "bearish") => "flipped-bearish",
            _ => "held",
        }
        .to_string(),
    )
}

/// Return the most-recent signal for (symbol, timeframe), if any.
pub fn latest_for(
    conn: &Connection,
    symbol: &str,
    timeframe: &str,
) -> Result<Option<CyberdotsSignal>> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, timeframe, recorded_at, dot_state, trackline_position,
                flip_from_prior, source, notes, related_transaction_id
         FROM cyberdots_signals
         WHERE symbol = ?1 AND timeframe = ?2
         ORDER BY recorded_at DESC, id DESC
         LIMIT 1",
    )?;
    let result = stmt
        .query_row(params![symbol, timeframe], CyberdotsSignal::from_row)
        .ok();
    Ok(result)
}

pub struct AddSignal<'a> {
    pub symbol: &'a str,
    pub timeframe: &'a str,
    pub dot_state: &'a str,
    pub trackline_position: &'a str,
    pub source: &'a str,
    pub notes: Option<&'a str>,
    pub related_transaction_id: Option<i64>,
}

/// Insert a new signal, auto-computing `flip_from_prior` from the most-recent
/// prior row for (symbol, timeframe). Returns the newly inserted row id.
pub fn add_signal(conn: &Connection, args: &AddSignal<'_>) -> Result<(i64, Option<String>)> {
    validate_timeframe(args.timeframe)?;
    validate_dot_state(args.dot_state)?;
    validate_trackline_position(args.trackline_position)?;
    validate_source(args.source)?;

    let prior = latest_for(conn, args.symbol, args.timeframe)?;
    let flip = compute_flip(prior.as_ref(), args.dot_state);

    conn.execute(
        "INSERT INTO cyberdots_signals
            (symbol, timeframe, dot_state, trackline_position,
             flip_from_prior, source, notes, related_transaction_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            args.symbol,
            args.timeframe,
            args.dot_state,
            args.trackline_position,
            flip,
            args.source,
            args.notes,
            args.related_transaction_id
        ],
    )?;

    Ok((conn.last_insert_rowid(), flip))
}

#[derive(Default, Debug, Clone)]
pub struct ListFilter<'a> {
    pub symbol: Option<&'a str>,
    pub timeframe: Option<&'a str>,
    pub since: Option<&'a str>,
    pub flips_only: bool,
    pub limit: Option<usize>,
}

pub fn list_signals(conn: &Connection, filter: &ListFilter<'_>) -> Result<Vec<CyberdotsSignal>> {
    let mut sql = String::from(
        "SELECT id, symbol, timeframe, recorded_at, dot_state, trackline_position,
                flip_from_prior, source, notes, related_transaction_id
         FROM cyberdots_signals",
    );

    let mut where_parts: Vec<String> = Vec::new();
    let mut bind: Vec<String> = Vec::new();

    if let Some(sym) = filter.symbol {
        where_parts.push(format!("symbol = ?{}", bind.len() + 1));
        bind.push(sym.to_string());
    }
    if let Some(tf) = filter.timeframe {
        validate_timeframe(tf)?;
        where_parts.push(format!("timeframe = ?{}", bind.len() + 1));
        bind.push(tf.to_string());
    }
    if let Some(since) = filter.since {
        where_parts.push(format!("recorded_at >= ?{}", bind.len() + 1));
        bind.push(since.to_string());
    }
    if filter.flips_only {
        where_parts.push("flip_from_prior IN ('flipped-bullish','flipped-bearish')".to_string());
    }

    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }

    sql.push_str(" ORDER BY recorded_at DESC, id DESC");
    if let Some(n) = filter.limit {
        sql.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> =
        bind.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(params_refs.as_slice(), CyberdotsSignal::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Return the most-recent signal per (symbol, timeframe), optionally filtered
/// to a single symbol. Sorted by symbol, timeframe.
pub fn current_signals(conn: &Connection, symbol: Option<&str>) -> Result<Vec<CyberdotsSignal>> {
    let mut sql = String::from(
        "SELECT id, symbol, timeframe, recorded_at, dot_state, trackline_position,
                flip_from_prior, source, notes, related_transaction_id
         FROM cyberdots_signals AS c
         WHERE recorded_at = (
             SELECT MAX(recorded_at) FROM cyberdots_signals
             WHERE symbol = c.symbol AND timeframe = c.timeframe
         )",
    );
    let mut bind: Vec<String> = Vec::new();
    if let Some(sym) = symbol {
        sql.push_str(" AND c.symbol = ?1");
        bind.push(sym.to_string());
    }
    sql.push_str(" ORDER BY c.symbol ASC, c.timeframe ASC");

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> =
        bind.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(params_refs.as_slice(), CyberdotsSignal::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    // Collapse to one row per (symbol, timeframe) — when two rows share the
    // same MAX(recorded_at) timestamp, prefer the highest id.
    let mut seen = std::collections::HashSet::new();
    out.sort_by(|a, b| {
        a.symbol
            .cmp(&b.symbol)
            .then(a.timeframe.cmp(&b.timeframe))
            .then(b.id.cmp(&a.id))
    });
    out.retain(|s| seen.insert((s.symbol.clone(), s.timeframe.clone())));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_db() -> Connection {
        crate::db::open_in_memory()
    }

    #[test]
    fn insert_and_query_roundtrip() {
        let conn = fresh_db();
        let (id, flip) = add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "1d",
                dot_state: "bullish",
                trackline_position: "above",
                source: "skylar-manual",
                notes: Some("opening signal"),
                related_transaction_id: None,
            },
        )
        .unwrap();
        assert!(id > 0);
        // First row has no prior → flip is None.
        assert!(flip.is_none());

        let rows = list_signals(
            &conn,
            &ListFilter {
                symbol: Some("BTC-USD"),
                ..ListFilter::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].dot_state, "bullish");
        assert_eq!(rows[0].trackline_position, "above");
        assert_eq!(rows[0].source, "skylar-manual");
        assert_eq!(rows[0].flip_from_prior, None);
        assert_eq!(rows[0].notes.as_deref(), Some("opening signal"));
    }

    #[test]
    fn flip_from_prior_is_computed_against_latest_row() {
        let conn = fresh_db();
        // 1) baseline bullish
        let (_, flip1) = add_signal(
            &conn,
            &AddSignal {
                symbol: "GC=F",
                timeframe: "1d",
                dot_state: "bullish",
                trackline_position: "above",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        assert!(flip1.is_none());

        // 2) bearish → flipped-bearish
        let (_, flip2) = add_signal(
            &conn,
            &AddSignal {
                symbol: "GC=F",
                timeframe: "1d",
                dot_state: "bearish",
                trackline_position: "below",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        assert_eq!(flip2.as_deref(), Some("flipped-bearish"));

        // 3) still bearish → held
        let (_, flip3) = add_signal(
            &conn,
            &AddSignal {
                symbol: "GC=F",
                timeframe: "1d",
                dot_state: "bearish",
                trackline_position: "below",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        assert_eq!(flip3.as_deref(), Some("held"));

        // 4) flat → flat-confirmed
        let (_, flip4) = add_signal(
            &conn,
            &AddSignal {
                symbol: "GC=F",
                timeframe: "1d",
                dot_state: "flat",
                trackline_position: "on",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        assert_eq!(flip4.as_deref(), Some("flat-confirmed"));

        // 5) bullish from flat → flipped-bullish
        let (_, flip5) = add_signal(
            &conn,
            &AddSignal {
                symbol: "GC=F",
                timeframe: "1d",
                dot_state: "bullish",
                trackline_position: "above",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        assert_eq!(flip5.as_deref(), Some("flipped-bullish"));
    }

    #[test]
    fn flip_history_separates_by_symbol_and_timeframe() {
        let conn = fresh_db();
        // BTC 1d bullish
        add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "1d",
                dot_state: "bullish",
                trackline_position: "above",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        // BTC 4h bearish — different timeframe, so no prior
        let (_, flip_btc_4h) = add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "4h",
                dot_state: "bearish",
                trackline_position: "below",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        assert!(flip_btc_4h.is_none());
        // BTC 1d bearish — flips relative to the BTC 1d bullish, not BTC 4h
        let (_, flip_btc_1d) = add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "1d",
                dot_state: "bearish",
                trackline_position: "below",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        assert_eq!(flip_btc_1d.as_deref(), Some("flipped-bearish"));
    }

    #[test]
    fn current_returns_one_row_per_symbol_timeframe() {
        let conn = fresh_db();
        for state in ["bullish", "bearish", "bullish"] {
            add_signal(
                &conn,
                &AddSignal {
                    symbol: "BTC-USD",
                    timeframe: "1d",
                    dot_state: state,
                    trackline_position: "above",
                    source: "skylar-manual",
                    notes: None,
                    related_transaction_id: None,
                },
            )
            .unwrap();
        }
        add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "4h",
                dot_state: "bearish",
                trackline_position: "below",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        let current = current_signals(&conn, Some("BTC-USD")).unwrap();
        assert_eq!(current.len(), 2);
        let one_d = current.iter().find(|s| s.timeframe == "1d").unwrap();
        assert_eq!(one_d.dot_state, "bullish");
        let four_h = current.iter().find(|s| s.timeframe == "4h").unwrap();
        assert_eq!(four_h.dot_state, "bearish");
    }

    #[test]
    fn list_filters_to_flips_only() {
        let conn = fresh_db();
        add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "1d",
                dot_state: "bullish",
                trackline_position: "above",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "1d",
                dot_state: "bullish",
                trackline_position: "above",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "1d",
                dot_state: "bearish",
                trackline_position: "below",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap();
        let flips = list_signals(
            &conn,
            &ListFilter {
                flips_only: true,
                ..ListFilter::default()
            },
        )
        .unwrap();
        assert_eq!(flips.len(), 1);
        assert_eq!(flips[0].flip_from_prior.as_deref(), Some("flipped-bearish"));
    }

    #[test]
    fn invalid_timeframe_is_rejected() {
        let conn = fresh_db();
        let err = add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "2d",
                dot_state: "bullish",
                trackline_position: "above",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap_err();
        assert!(format!("{err}").contains("Invalid timeframe"));
    }

    #[test]
    fn invalid_dot_state_is_rejected() {
        let conn = fresh_db();
        let err = add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "1d",
                dot_state: "neutral",
                trackline_position: "above",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap_err();
        assert!(format!("{err}").contains("Invalid dot state"));
    }

    #[test]
    fn invalid_trackline_position_is_rejected() {
        let conn = fresh_db();
        let err = add_signal(
            &conn,
            &AddSignal {
                symbol: "BTC-USD",
                timeframe: "1d",
                dot_state: "bullish",
                trackline_position: "across",
                source: "skylar-manual",
                notes: None,
                related_transaction_id: None,
            },
        )
        .unwrap_err();
        assert!(format!("{err}").contains("Invalid trackline position"));
    }

    #[test]
    fn check_constraint_rejects_invalid_dot_state_directly() {
        // Verify the SQLite CHECK constraint catches direct writes too,
        // not just the Rust-side validation.
        let conn = fresh_db();
        let res = conn.execute(
            "INSERT INTO cyberdots_signals
                (symbol, timeframe, dot_state, trackline_position, source)
             VALUES ('BTC-USD', '1d', 'neutral', 'above', 'skylar-manual')",
            [],
        );
        assert!(res.is_err());
    }

    #[test]
    fn check_constraint_rejects_invalid_timeframe_directly() {
        let conn = fresh_db();
        let res = conn.execute(
            "INSERT INTO cyberdots_signals
                (symbol, timeframe, dot_state, trackline_position, source)
             VALUES ('BTC-USD', '2d', 'bullish', 'above', 'skylar-manual')",
            [],
        );
        assert!(res.is_err());
    }
}
