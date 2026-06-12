//! `pftui data decontaminate` — purge L2 derived rows computed from a
//! corrupt L1 price series.
//!
//! When `price_history` is repaired after a corruption incident (e.g. the
//! 2026-06 BTC equity-ticker collision and FX 1.0000 placeholders), the L2
//! layers computed from the corrupt closes do NOT self-heal: technical and
//! correlation snapshots are stamped per-refresh-run (`computed_at`/
//! `recorded_at` = now), so historical rows poisoned during the corruption
//! window persist forever and keep feeding range_52w reads, correlation
//! grids, and signal stats.
//!
//! Catalog honesty note: `rebuildable = true` on these tables means the
//! CURRENT state regrows on the next refresh — deleted HISTORICAL rows do
//! not regrow (there is no backfill path that recomputes a past refresh
//! day). Decontamination therefore trades poisoned history for an honest
//! gap, which downstream readers already tolerate (they read latest-N).
//!
//! Per-table scope decisions (survey of L2 tables fed by price_history):
//!
//! | table                 | in scope? | why |
//! |---|---|---|
//! | technical_snapshots   | YES | per-symbol rows; range_52w columns carry the corrupt prints |
//! | correlation_snapshots | YES | pairwise; delete where EITHER side matches — pair rows are independent, no aggregate skew |
//! | technical_levels      | YES | per-symbol; wholesale-replaced per refresh anyway (DELETE+reinsert), stale rows are pure liability |
//! | technical_signals     | YES | per-symbol detection events computed from the corrupt series |
//! | signal_expectancy     | YES | per-asset stats; genuinely rebuildable via `pftui research backtest` |
//! | timeframe_signals     | NO  | `assets` is a cross-asset JSON list — partial deletion would skew multi-asset signals |
//! | regime_snapshots/_history | NO | cross-asset regime classification — single-symbol deletion skews the composite |
//! | portfolio/position_snapshots | NO | operator performance history; partial per-symbol deletion skews portfolio totals; repaired L1 + snapshot rebuild is the correct path |
//! | mobile_timeframe_scores | NO | tiny, wholesale-upserted each serve — self-heals |
//! | calibration_* / news_silence_baselines / failure_correlations | NO | not derived from price_history |
//!
//! UX contract: dry-run by default — running without `--confirm` only
//! prints per-table counts. `--confirm` executes the deletes inside a
//! transaction and writes a journal-note audit trail (author `system`,
//! section `system`).

use anyhow::Result;
use serde::Serialize;

use crate::db::backend::BackendConnection;

/// One in-scope table: (name, WHERE clause with ?1 = symbol and optional
/// ?2 = before-date bound on the table's timestamp column).
struct ScopeTable {
    table: &'static str,
    symbol_predicate: &'static str,
    time_column: &'static str,
}

const SCOPE: &[ScopeTable] = &[
    ScopeTable {
        table: "technical_snapshots",
        symbol_predicate: "symbol = ?1",
        time_column: "computed_at",
    },
    ScopeTable {
        table: "correlation_snapshots",
        symbol_predicate: "(symbol_a = ?1 OR symbol_b = ?1)",
        time_column: "recorded_at",
    },
    ScopeTable {
        table: "technical_levels",
        symbol_predicate: "symbol = ?1",
        time_column: "computed_at",
    },
    ScopeTable {
        table: "technical_signals",
        symbol_predicate: "symbol = ?1",
        time_column: "detected_at",
    },
    ScopeTable {
        table: "signal_expectancy",
        symbol_predicate: "asset = ?1",
        time_column: "computed_at",
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct TableCount {
    pub table: String,
    pub rows: usize,
}

#[derive(Debug, Serialize)]
pub struct DecontaminateReport {
    pub symbol: String,
    pub before: Option<String>,
    pub executed: bool,
    pub tables: Vec<TableCount>,
    pub total_rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journal_note_id: Option<i64>,
}

fn where_clause(scope: &ScopeTable, before: Option<&str>) -> String {
    match before {
        Some(_) => format!(
            "{} AND {} < ?2",
            scope.symbol_predicate, scope.time_column
        ),
        None => scope.symbol_predicate.to_string(),
    }
}

fn count_rows(
    conn: &rusqlite::Connection,
    scope: &ScopeTable,
    symbol: &str,
    before: Option<&str>,
) -> Result<usize> {
    let sql = format!(
        "SELECT COUNT(*) FROM {} WHERE {}",
        scope.table,
        where_clause(scope, before)
    );
    let count: i64 = match before {
        Some(b) => conn.query_row(&sql, rusqlite::params![symbol, b], |row| row.get(0))?,
        None => conn.query_row(&sql, rusqlite::params![symbol], |row| row.get(0))?,
    };
    Ok(count as usize)
}

fn delete_rows(
    conn: &rusqlite::Connection,
    scope: &ScopeTable,
    symbol: &str,
    before: Option<&str>,
) -> Result<usize> {
    let sql = format!(
        "DELETE FROM {} WHERE {}",
        scope.table,
        where_clause(scope, before)
    );
    let deleted = match before {
        Some(b) => conn.execute(&sql, rusqlite::params![symbol, b])?,
        None => conn.execute(&sql, rusqlite::params![symbol])?,
    };
    Ok(deleted)
}

/// Core decontamination over a native SQLite connection. `confirm = false`
/// is the dry run: counts only, nothing deleted, no journal note.
pub fn decontaminate(
    conn: &rusqlite::Connection,
    symbol: &str,
    before: Option<&str>,
    confirm: bool,
) -> Result<DecontaminateReport> {
    let mut tables = Vec::new();
    let mut total = 0usize;

    if confirm {
        // All-or-nothing: a partial decontamination is worse than none.
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            for scope in SCOPE {
                let deleted = delete_rows(conn, scope, symbol, before)?;
                total += deleted;
                tables.push(TableCount {
                    table: scope.table.to_string(),
                    rows: deleted,
                });
            }
            Ok(())
        })();
        match result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
    } else {
        for scope in SCOPE {
            let rows = count_rows(conn, scope, symbol, before)?;
            total += rows;
            tables.push(TableCount {
                table: scope.table.to_string(),
                rows,
            });
        }
    }

    // Audit trail: a journal note (author system, section system) recording
    // exactly what was purged, written only on execution.
    let journal_note_id = if confirm {
        let today = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
        let per_table: Vec<String> = tables
            .iter()
            .filter(|t| t.rows > 0)
            .map(|t| format!("{}={}", t.table, t.rows))
            .collect();
        let content = format!(
            "[decontaminate {}] Purged {} L2 derived row(s) computed from the corrupt L1 series for {}{}: {}. \
             Scope: per-symbol L2 tables only (cross-asset aggregates excluded — see src/commands/decontaminate.rs). \
             Historical L2 rows do NOT regrow; downstream readers see an honest gap until new refresh runs accumulate.",
            today,
            total,
            symbol,
            before
                .map(|b| format!(" (computed before {b})"))
                .unwrap_or_default(),
            if per_table.is_empty() {
                "no matching rows".to_string()
            } else {
                per_table.join(", ")
            },
        );
        Some(crate::db::daily_notes::add_note(
            conn, &today, "system", &content, "system",
        )?)
    } else {
        None
    };

    Ok(DecontaminateReport {
        symbol: symbol.to_string(),
        before: before.map(str::to_string),
        executed: confirm,
        tables,
        total_rows: total,
        journal_note_id,
    })
}

/// `pftui data decontaminate --symbol SYM [--before DATE] [--dry-run|--confirm] [--json]`
pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    before: Option<&str>,
    confirm: bool,
    json: bool,
) -> Result<()> {
    let Some(conn) = backend.sqlite_native() else {
        anyhow::bail!(
            "data decontaminate currently supports the SQLite backend only \
             (L2 decontamination is a local-cache repair)"
        );
    };

    // Validate the date bound early — a typo'd date silently matching
    // nothing (or everything) would defeat the dry-run contract.
    if let Some(b) = before {
        if chrono::NaiveDate::parse_from_str(b, "%Y-%m-%d").is_err() {
            anyhow::bail!("--before must be a YYYY-MM-DD date (got '{b}')");
        }
    }

    let report = decontaminate(conn, symbol, before, confirm)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    let window = report
        .before
        .as_deref()
        .map(|b| format!(" computed before {b}"))
        .unwrap_or_default();
    if report.executed {
        println!(
            "Decontaminated L2 derived rows for {}{}:",
            report.symbol, window
        );
    } else {
        println!(
            "DRY RUN — L2 derived rows for {}{} that would be deleted:",
            report.symbol, window
        );
    }
    for t in &report.tables {
        println!("  {:<24} {:>8}", t.table, t.rows);
    }
    println!("  {:<24} {:>8}", "TOTAL", report.total_rows);
    if report.executed {
        if let Some(id) = report.journal_note_id {
            println!("Audit trail: journal note #{id} (author system, section system).");
        }
        println!(
            "Note: historical L2 rows do not regrow — readers see an honest gap until\nnew refresh runs accumulate. signal_expectancy fully rebuilds via\n`pftui research backtest`."
        );
    } else {
        println!(
            "Nothing deleted. Re-run with --confirm to execute (writes a journal-note\naudit trail). Excluded by design: timeframe_signals, regime_*,\nportfolio/position_snapshots (cross-asset aggregates / operator history)."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn seeded_conn() -> rusqlite::Connection {
        let conn = open_in_memory();
        // technical_snapshots: 2 BTC rows (one early, one late), 1 AAPL row.
        conn.execute_batch(
            "INSERT INTO technical_snapshots (symbol, timeframe, rsi_14, computed_at)
             VALUES ('BTC', 'daily', 55.0, '2026-06-01T00:00:00Z'),
                    ('BTC', 'daily', 60.0, '2026-06-15T00:00:00Z'),
                    ('AAPL', 'daily', 50.0, '2026-06-01T00:00:00Z');
             INSERT INTO correlation_snapshots (symbol_a, symbol_b, correlation, period, recorded_at)
             VALUES ('BTC', 'GC=F', 0.4, '30d', '2026-06-01T00:00:00Z'),
                    ('SPY', 'BTC', 0.2, '30d', '2026-06-01T00:00:00Z'),
                    ('SPY', 'GC=F', 0.1, '30d', '2026-06-01T00:00:00Z');
             INSERT INTO technical_levels (symbol, level_type, price, source_method, computed_at)
             VALUES ('BTC', 'support', 60000.0, 'swing', '2026-06-01T00:00:00Z');
             INSERT INTO technical_signals (symbol, signal_type, direction, severity, description, timeframe, detected_at)
             VALUES ('BTC', 'rsi_oversold', 'bullish', 'medium', 'x', '1d', '2026-06-01T00:00:00Z');
             INSERT INTO signal_expectancy (signal_id, signal_version, asset, horizon_days, as_of,
                                            n_total, n_evaluable, n_nonoverlap, computed_at)
             VALUES ('sig', 'v1', 'BTC', 30, '2026-06-01', 5, 5, 5, '2026-06-01T00:00:00Z');",
        )
        .expect("seed");
        conn
    }

    fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).expect("count")
    }

    #[test]
    fn dry_run_counts_without_deleting() {
        let conn = seeded_conn();
        let report = decontaminate(&conn, "BTC", Some("2026-06-12"), false).expect("dry run");
        assert!(!report.executed);
        assert_eq!(report.total_rows, 6); // 1 tech snap (early only) + 2 corr + 1 level + 1 signal + 1 expectancy
        assert!(report.journal_note_id.is_none());
        // Nothing was deleted.
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM technical_snapshots"), 3);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM correlation_snapshots"), 3);
        // No audit note written on dry run.
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM daily_notes"), 0);
    }

    #[test]
    fn confirm_deletes_in_scope_rows_and_writes_journal_trail() {
        let conn = seeded_conn();
        let report = decontaminate(&conn, "BTC", Some("2026-06-12"), true).expect("confirm");
        assert!(report.executed);
        assert_eq!(report.total_rows, 6);

        // The late BTC snapshot (after --before) and the AAPL row survive.
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM technical_snapshots"), 2);
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM technical_snapshots WHERE symbol='BTC'"
            ),
            1
        );
        // Either-side matching for correlations: only the non-BTC pair survives.
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM correlation_snapshots"), 1);
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM correlation_snapshots WHERE symbol_a='SPY' AND symbol_b='GC=F'"
            ),
            1
        );
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM technical_levels"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM technical_signals"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM signal_expectancy"), 0);

        // Journal audit trail: author system, section system.
        let id = report.journal_note_id.expect("journal note id");
        let (section, author, content): (String, String, String) = conn
            .query_row(
                "SELECT section, author, content FROM daily_notes WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("note row");
        assert_eq!(section, "system");
        assert_eq!(author, "system");
        assert!(content.contains("BTC"), "{content}");
        assert!(content.contains("technical_snapshots=1"), "{content}");
        assert!(content.contains("correlation_snapshots=2"), "{content}");
    }

    #[test]
    fn before_bound_is_respected_and_no_bound_deletes_all() {
        let conn = seeded_conn();
        // No --before: both BTC technical_snapshots rows are in scope.
        let report = decontaminate(&conn, "BTC", None, false).expect("dry run");
        let tech = report
            .tables
            .iter()
            .find(|t| t.table == "technical_snapshots")
            .expect("tech row");
        assert_eq!(tech.rows, 2);
    }

    #[test]
    fn unknown_symbol_is_a_clean_zero() {
        let conn = seeded_conn();
        let report = decontaminate(&conn, "NOPE", None, true).expect("confirm");
        assert_eq!(report.total_rows, 0);
        // Even a zero-row execution leaves an audit note (the operator asked
        // for a destructive action; the trail records it happened).
        assert!(report.journal_note_id.is_some());
    }
}
