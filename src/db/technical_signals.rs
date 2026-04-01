use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use sqlx::PgPool;

use super::backend::BackendConnection;
use super::query;

#[derive(Debug, Clone, Serialize)]
pub struct TechnicalSignalRecord {
    pub id: i64,
    pub symbol: String,
    pub signal_type: String,
    pub direction: String,
    pub severity: String,
    pub trigger_price: Option<f64>,
    pub description: String,
    pub timeframe: String,
    pub detected_at: String,
}

/// Input for creating a new technical signal.
pub struct NewSignal<'a> {
    pub symbol: &'a str,
    pub signal_type: &'a str,
    pub direction: &'a str,
    pub severity: &'a str,
    pub trigger_price: Option<f64>,
    pub description: &'a str,
    pub timeframe: &'a str,
}

// ── SQLite ────────────────────────────────────────────────────────────

pub fn add_signal(conn: &Connection, sig: &NewSignal<'_>) -> Result<i64> {
    conn.execute(
        "INSERT INTO technical_signals (symbol, signal_type, direction, severity, trigger_price, description, timeframe)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![sig.symbol, sig.signal_type, sig.direction, sig.severity, sig.trigger_price, sig.description, sig.timeframe],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_signals(
    conn: &Connection,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSignalRecord>> {
    list_signals_filtered(conn, symbol, signal_type, None, None, limit)
}

/// List signals with optional severity and direction filters.
pub fn list_signals_filtered(
    conn: &Connection,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    direction: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSignalRecord>> {
    let mut sql = String::from(
        "SELECT id, symbol, signal_type, direction, severity, trigger_price, description, timeframe, detected_at
         FROM technical_signals WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(sym) = symbol {
        sql.push_str(" AND symbol = ?");
        params.push(Box::new(sym.to_string()));
    }
    if let Some(st) = signal_type {
        sql.push_str(" AND signal_type = ?");
        params.push(Box::new(st.to_string()));
    }
    if let Some(sev) = severity {
        sql.push_str(" AND severity = ?");
        params.push(Box::new(sev.to_string()));
    }
    if let Some(dir) = direction {
        sql.push_str(" AND direction = ?");
        params.push(Box::new(dir.to_string()));
    }
    sql.push_str(" ORDER BY detected_at DESC");
    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(TechnicalSignalRecord {
            id: row.get(0)?,
            symbol: row.get(1)?,
            signal_type: row.get(2)?,
            direction: row.get(3)?,
            severity: row.get(4)?,
            trigger_price: row.get(5)?,
            description: row.get(6)?,
            timeframe: row.get(7)?,
            detected_at: row.get(8)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn prune_signals(conn: &Connection, hours: i64) -> Result<u64> {
    let count = conn.execute(
        "DELETE FROM technical_signals WHERE detected_at < datetime('now', ?1)",
        rusqlite::params![format!("-{} hours", hours)],
    )?;
    Ok(count as u64)
}

// ── PostgreSQL ────────────────────────────────────────────────────────

fn add_signal_postgres(pool: &PgPool, sig: &NewSignal<'_>) -> Result<i64> {
    let row = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO technical_signals (symbol, signal_type, direction, severity, trigger_price, description, timeframe)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind(sig.symbol)
        .bind(sig.signal_type)
        .bind(sig.direction)
        .bind(sig.severity)
        .bind(sig.trigger_price)
        .bind(sig.description)
        .bind(sig.timeframe)
        .fetch_one(pool)
        .await
    })?;
    Ok(row)
}

fn list_signals_postgres(
    pool: &PgPool,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSignalRecord>> {
    list_signals_filtered_postgres(pool, symbol, signal_type, None, None, limit)
}

fn list_signals_filtered_postgres(
    pool: &PgPool,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    direction: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSignalRecord>> {
    let lim = limit.unwrap_or(100) as i64;
    let mut sql = String::from(
        "SELECT id, symbol, signal_type, direction, severity, trigger_price, description, timeframe, detected_at::TEXT as detected_at
         FROM technical_signals WHERE 1=1",
    );
    let mut param_idx = 1u32;
    let mut binds: Vec<String> = Vec::new();

    if let Some(sym) = symbol {
        sql.push_str(&format!(" AND symbol = ${}", param_idx));
        param_idx += 1;
        binds.push(sym.to_string());
    }
    if let Some(st) = signal_type {
        sql.push_str(&format!(" AND signal_type = ${}", param_idx));
        param_idx += 1;
        binds.push(st.to_string());
    }
    if let Some(sev) = severity {
        sql.push_str(&format!(" AND severity = ${}", param_idx));
        param_idx += 1;
        binds.push(sev.to_string());
    }
    if let Some(dir) = direction {
        sql.push_str(&format!(" AND direction = ${}", param_idx));
        param_idx += 1;
        binds.push(dir.to_string());
    }
    sql.push_str(&format!(" ORDER BY detected_at DESC LIMIT ${}", param_idx));

    let rows = crate::db::pg_runtime::block_on(async {
        let mut q = sqlx::query(&sql);
        for b in &binds {
            q = q.bind(b);
        }
        q = q.bind(lim);
        q.fetch_all(pool).await
    })?;

    Ok(rows
        .iter()
        .map(|row| {
            use sqlx::Row;
            TechnicalSignalRecord {
                id: row.get("id"),
                symbol: row.get("symbol"),
                signal_type: row.get("signal_type"),
                direction: row.get("direction"),
                severity: row.get("severity"),
                trigger_price: row.get("trigger_price"),
                description: row.get("description"),
                timeframe: row.get("timeframe"),
                detected_at: row.get("detected_at"),
            }
        })
        .collect())
}

fn prune_signals_postgres(pool: &PgPool, hours: i64) -> Result<u64> {
    let result = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM technical_signals WHERE detected_at < NOW() - $1::INTERVAL")
            .bind(format!("{} hours", hours))
            .execute(pool)
            .await
    })?;
    Ok(result.rows_affected())
}

// ── Backend dispatch ──────────────────────────────────────────────────

pub fn add_signal_backend(backend: &BackendConnection, sig: &NewSignal<'_>) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_signal(conn, sig),
        |pool| add_signal_postgres(pool, sig),
    )
}

pub fn list_signals_backend(
    backend: &BackendConnection,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSignalRecord>> {
    query::dispatch(
        backend,
        |conn| list_signals(conn, symbol, signal_type, limit),
        |pool| list_signals_postgres(pool, symbol, signal_type, limit),
    )
}

/// List signals with optional severity and direction filters (used by `analytics signals`).
pub fn list_signals_filtered_backend(
    backend: &BackendConnection,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    direction: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSignalRecord>> {
    query::dispatch(
        backend,
        |conn| list_signals_filtered(conn, symbol, signal_type, severity, direction, limit),
        |pool| {
            list_signals_filtered_postgres(pool, symbol, signal_type, severity, direction, limit)
        },
    )
}

/// Prune signals older than `hours` to prevent unbounded growth.
pub fn prune_signals_backend(backend: &BackendConnection, hours: i64) -> Result<u64> {
    query::dispatch(
        backend,
        |conn| prune_signals(conn, hours),
        |pool| prune_signals_postgres(pool, hours),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE technical_signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                signal_type TEXT NOT NULL,
                direction TEXT NOT NULL,
                severity TEXT NOT NULL,
                trigger_price REAL,
                description TEXT NOT NULL,
                timeframe TEXT NOT NULL DEFAULT '1d',
                detected_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .unwrap();
        conn
    }

    fn sig<'a>(
        symbol: &'a str,
        signal_type: &'a str,
        direction: &'a str,
        severity: &'a str,
        trigger_price: Option<f64>,
        description: &'a str,
        timeframe: &'a str,
    ) -> NewSignal<'a> {
        NewSignal {
            symbol,
            signal_type,
            direction,
            severity,
            trigger_price,
            description,
            timeframe,
        }
    }

    #[test]
    fn add_and_list_signals() {
        let conn = setup_db();
        let id = add_signal(
            &conn,
            &sig(
                "AAPL",
                "rsi_overbought",
                "bearish",
                "notable",
                Some(195.0),
                "RSI 14 crossed above 70 (currently 74.2)",
                "1d",
            ),
        )
        .unwrap();
        assert!(id > 0);

        let all = list_signals(&conn, None, None, None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].symbol, "AAPL");
        assert_eq!(all[0].signal_type, "rsi_overbought");
        assert_eq!(all[0].direction, "bearish");
    }

    #[test]
    fn list_filters_by_symbol() {
        let conn = setup_db();
        add_signal(
            &conn,
            &sig(
                "AAPL",
                "rsi_overbought",
                "bearish",
                "notable",
                None,
                "test",
                "1d",
            ),
        )
        .unwrap();
        add_signal(
            &conn,
            &sig(
                "BTC",
                "macd_bull_cross",
                "bullish",
                "notable",
                None,
                "test",
                "1d",
            ),
        )
        .unwrap();

        let aapl = list_signals(&conn, Some("AAPL"), None, None).unwrap();
        assert_eq!(aapl.len(), 1);

        let btc = list_signals(&conn, Some("BTC"), None, None).unwrap();
        assert_eq!(btc.len(), 1);
        assert_eq!(btc[0].signal_type, "macd_bull_cross");
    }

    #[test]
    fn list_filters_by_signal_type() {
        let conn = setup_db();
        add_signal(
            &conn,
            &sig(
                "AAPL",
                "rsi_overbought",
                "bearish",
                "notable",
                None,
                "test",
                "1d",
            ),
        )
        .unwrap();
        add_signal(
            &conn,
            &sig(
                "AAPL",
                "macd_bull_cross",
                "bullish",
                "notable",
                None,
                "test",
                "1d",
            ),
        )
        .unwrap();

        let rsi = list_signals(&conn, None, Some("rsi_overbought"), None).unwrap();
        assert_eq!(rsi.len(), 1);
    }

    #[test]
    fn list_respects_limit() {
        let conn = setup_db();
        for i in 0..5 {
            let sym = format!("SYM{}", i);
            add_signal(
                &conn,
                &sig(
                    &sym,
                    "rsi_overbought",
                    "bearish",
                    "notable",
                    None,
                    "test",
                    "1d",
                ),
            )
            .unwrap();
        }

        let limited = list_signals(&conn, None, None, Some(3)).unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn filtered_by_severity() {
        let conn = setup_db();
        add_signal(
            &conn,
            &sig("AAPL", "rsi_overbought", "bearish", "critical", None, "crit test", "1d"),
        )
        .unwrap();
        add_signal(
            &conn,
            &sig("BTC", "macd_bull_cross", "bullish", "notable", None, "notable test", "1d"),
        )
        .unwrap();
        add_signal(
            &conn,
            &sig("GC=F", "sma200_reclaim", "bullish", "critical", None, "crit gold", "1d"),
        )
        .unwrap();

        let critical = list_signals_filtered(&conn, None, None, Some("critical"), None, None).unwrap();
        assert_eq!(critical.len(), 2);
        assert!(critical.iter().all(|s| s.severity == "critical"));

        let notable = list_signals_filtered(&conn, None, None, Some("notable"), None, None).unwrap();
        assert_eq!(notable.len(), 1);
        assert_eq!(notable[0].symbol, "BTC");
    }

    #[test]
    fn filtered_by_direction() {
        let conn = setup_db();
        add_signal(
            &conn,
            &sig("AAPL", "rsi_overbought", "bearish", "notable", None, "bear test", "1d"),
        )
        .unwrap();
        add_signal(
            &conn,
            &sig("BTC", "macd_bull_cross", "bullish", "notable", None, "bull test", "1d"),
        )
        .unwrap();
        add_signal(
            &conn,
            &sig("GC=F", "sma200_reclaim", "bullish", "notable", None, "bull gold", "1d"),
        )
        .unwrap();

        let bullish = list_signals_filtered(&conn, None, None, None, Some("bullish"), None).unwrap();
        assert_eq!(bullish.len(), 2);
        assert!(bullish.iter().all(|s| s.direction == "bullish"));

        let bearish = list_signals_filtered(&conn, None, None, None, Some("bearish"), None).unwrap();
        assert_eq!(bearish.len(), 1);
        assert_eq!(bearish[0].symbol, "AAPL");
    }

    #[test]
    fn filtered_by_severity_and_direction_combined() {
        let conn = setup_db();
        add_signal(
            &conn,
            &sig("AAPL", "rsi_overbought", "bearish", "critical", None, "test", "1d"),
        )
        .unwrap();
        add_signal(
            &conn,
            &sig("BTC", "macd_bull_cross", "bullish", "critical", None, "test", "1d"),
        )
        .unwrap();
        add_signal(
            &conn,
            &sig("GC=F", "sma200_reclaim", "bullish", "notable", None, "test", "1d"),
        )
        .unwrap();

        let crit_bull =
            list_signals_filtered(&conn, None, None, Some("critical"), Some("bullish"), None).unwrap();
        assert_eq!(crit_bull.len(), 1);
        assert_eq!(crit_bull[0].symbol, "BTC");

        // All filters combined: symbol + severity + direction
        let specific = list_signals_filtered(
            &conn, Some("AAPL"), None, Some("critical"), Some("bearish"), None,
        )
        .unwrap();
        assert_eq!(specific.len(), 1);
        assert_eq!(specific[0].symbol, "AAPL");

        // No match
        let empty = list_signals_filtered(
            &conn, None, None, Some("notable"), Some("bearish"), None,
        )
        .unwrap();
        assert_eq!(empty.len(), 0);
    }
}
