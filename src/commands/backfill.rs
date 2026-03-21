use std::time::Duration;

use anyhow::Result;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::price_history::{
    find_symbols_needing_backfill_backend, upsert_history_backend, BackfillSymbolStatus,
};
use crate::price::yahoo;

/// Delay between sequential Yahoo Finance API requests to avoid rate limiting.
const BACKFILL_RATE_LIMIT_DELAY: Duration = Duration::from_millis(200);
/// How many days of history to request from Yahoo for backfill.
const BACKFILL_HISTORY_DAYS: u32 = 365;

/// Result of backfilling a single symbol.
#[derive(Debug)]
struct SymbolBackfillResult {
    symbol: String,
    total_rows: u32,
    rows_before: u32,
    rows_fetched: u32,
    status: &'static str,
    error: Option<String>,
}

/// Run OHLCV backfill: find symbols with missing OHLCV data and re-fetch from Yahoo Finance.
pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let symbols = find_symbols_needing_backfill_backend(backend)?;

    if symbols.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "ok",
                    "message": "All symbols already have full OHLCV data",
                    "symbols_checked": 0,
                    "symbols_backfilled": 0,
                    "symbols_failed": 0,
                    "results": []
                }))?
            );
        } else {
            println!("✅ All symbols already have full OHLCV data — nothing to backfill.");
        }
        return Ok(());
    }

    if !json_output {
        println!(
            "Found {} symbol(s) with missing OHLCV data. Starting backfill...\n",
            symbols.len()
        );
    }

    let results = backfill_symbols(backend, &symbols, json_output)?;

    let succeeded = results.iter().filter(|r| r.status == "ok").count();
    let failed = results.iter().filter(|r| r.status == "error").count();
    let skipped = results.iter().filter(|r| r.status == "skipped").count();
    let total_fetched: u32 = results.iter().map(|r| r.rows_fetched).sum();

    if json_output {
        let result_json: Vec<_> = results
            .iter()
            .map(|r| {
                json!({
                    "symbol": r.symbol,
                    "total_rows": r.total_rows,
                    "missing_rows_before": r.rows_before,
                    "rows_fetched": r.rows_fetched,
                    "status": r.status,
                    "error": r.error,
                })
            })
            .collect();

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": if failed == 0 { "ok" } else { "partial" },
                "symbols_checked": symbols.len(),
                "symbols_backfilled": succeeded,
                "symbols_failed": failed,
                "symbols_skipped": skipped,
                "total_rows_fetched": total_fetched,
                "results": result_json,
            }))?
        );
    } else {
        println!("\n--- Backfill Summary ---");
        println!("  Symbols checked:    {}", symbols.len());
        println!("  Backfilled:         {}", succeeded);
        println!("  Failed:             {}", failed);
        if skipped > 0 {
            println!("  Skipped:            {}", skipped);
        }
        println!("  Total rows fetched: {}", total_fetched);
    }

    Ok(())
}

/// Backfill each symbol sequentially with rate limiting.
fn backfill_symbols(
    backend: &BackendConnection,
    symbols: &[BackfillSymbolStatus],
    human_output: bool,
) -> Result<Vec<SymbolBackfillResult>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let mut results = Vec::with_capacity(symbols.len());

    for (i, sym_status) in symbols.iter().enumerate() {
        if human_output {
            print!(
                "  [{}/{}] {} ({} rows missing OHLCV)... ",
                i + 1,
                symbols.len(),
                sym_status.symbol,
                sym_status.missing_ohlcv_rows,
            );
        }

        // Rate limit between requests
        if i > 0 {
            std::thread::sleep(BACKFILL_RATE_LIMIT_DELAY);
        }

        let result = rt.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(30),
                yahoo::fetch_history(&sym_status.symbol, BACKFILL_HISTORY_DAYS),
            )
            .await
        });

        match result {
            Ok(Ok(records)) => {
                let count = records.len() as u32;
                // Only keep records that actually have OHLCV data
                let ohlcv_records: Vec<_> = records
                    .into_iter()
                    .filter(|r| r.open.is_some() && r.high.is_some() && r.low.is_some())
                    .collect();
                let ohlcv_count = ohlcv_records.len() as u32;

                if ohlcv_records.is_empty() {
                    if human_output {
                        println!("skipped (fetched {} rows but none had OHLCV)", count);
                    }
                    results.push(SymbolBackfillResult {
                        symbol: sym_status.symbol.clone(),
                        total_rows: sym_status.total_rows,
                        rows_before: sym_status.missing_ohlcv_rows,
                        rows_fetched: 0,
                        status: "skipped",
                        error: Some(format!(
                            "Fetched {} rows but none contained OHLCV data",
                            count
                        )),
                    });
                    continue;
                }

                match upsert_history_backend(backend, &sym_status.symbol, "yahoo", &ohlcv_records) {
                    Ok(()) => {
                        if human_output {
                            println!("✅ {} rows with OHLCV", ohlcv_count);
                        }
                        results.push(SymbolBackfillResult {
                            symbol: sym_status.symbol.clone(),
                            total_rows: sym_status.total_rows,
                            rows_before: sym_status.missing_ohlcv_rows,
                            rows_fetched: ohlcv_count,
                            status: "ok",
                            error: None,
                        });
                    }
                    Err(e) => {
                        if human_output {
                            println!("❌ DB error: {}", e);
                        }
                        results.push(SymbolBackfillResult {
                            symbol: sym_status.symbol.clone(),
                            total_rows: sym_status.total_rows,
                            rows_before: sym_status.missing_ohlcv_rows,
                            rows_fetched: 0,
                            status: "error",
                            error: Some(format!("DB write failed: {}", e)),
                        });
                    }
                }
            }
            Ok(Err(e)) => {
                if human_output {
                    println!("❌ fetch error: {}", e);
                }
                results.push(SymbolBackfillResult {
                    symbol: sym_status.symbol.clone(),
                    total_rows: sym_status.total_rows,
                    rows_before: sym_status.missing_ohlcv_rows,
                    rows_fetched: 0,
                    status: "error",
                    error: Some(format!("Yahoo fetch failed: {}", e)),
                });
            }
            Err(_) => {
                if human_output {
                    println!("❌ timeout");
                }
                results.push(SymbolBackfillResult {
                    symbol: sym_status.symbol.clone(),
                    total_rows: sym_status.total_rows,
                    rows_before: sym_status.missing_ohlcv_rows,
                    rows_fetched: 0,
                    status: "error",
                    error: Some("Request timed out after 30s".to_string()),
                });
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use crate::db::open_in_memory;
    use crate::db::price_history::{find_symbols_needing_backfill, upsert_history};
    use crate::models::price::HistoryRecord;
    use rust_decimal_macros::dec;

    #[test]
    fn test_find_symbols_needing_backfill_empty_db() {
        let conn = open_in_memory();
        let result = find_symbols_needing_backfill(&conn).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_symbols_needing_backfill_all_complete() {
        let conn = open_in_memory();
        let records = vec![HistoryRecord {
            date: "2025-01-01".into(),
            close: dec!(100),
            volume: Some(1_000_000),
            open: Some(dec!(99)),
            high: Some(dec!(102)),
            low: Some(dec!(98)),
        }];
        upsert_history(&conn, "AAPL", "yahoo", &records).unwrap();

        let result = find_symbols_needing_backfill(&conn).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_symbols_needing_backfill_partial() {
        let conn = open_in_memory();
        // AAPL: has close only (missing OHLCV)
        let records = vec![HistoryRecord {
            date: "2025-01-01".into(),
            close: dec!(100),
            volume: None,
            open: None,
            high: None,
            low: None,
        }];
        upsert_history(&conn, "AAPL", "yahoo", &records).unwrap();

        // BTC: has full OHLCV (should not appear)
        let btc_records = vec![HistoryRecord {
            date: "2025-01-01".into(),
            close: dec!(42000),
            volume: Some(50_000_000),
            open: Some(dec!(41500)),
            high: Some(dec!(42500)),
            low: Some(dec!(41000)),
        }];
        upsert_history(&conn, "BTC", "yahoo", &btc_records).unwrap();

        let result = find_symbols_needing_backfill(&conn).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].symbol, "AAPL");
        assert_eq!(result[0].total_rows, 1);
        assert_eq!(result[0].missing_ohlcv_rows, 1);
    }

    #[test]
    fn test_find_symbols_needing_backfill_mixed_rows() {
        let conn = open_in_memory();
        // 2 rows: one with OHLCV, one without
        let records = vec![
            HistoryRecord {
                date: "2025-01-01".into(),
                close: dec!(100),
                volume: Some(1_000_000),
                open: Some(dec!(99)),
                high: Some(dec!(102)),
                low: Some(dec!(98)),
            },
            HistoryRecord {
                date: "2025-01-02".into(),
                close: dec!(105),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];
        upsert_history(&conn, "AAPL", "yahoo", &records).unwrap();

        let result = find_symbols_needing_backfill(&conn).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].symbol, "AAPL");
        assert_eq!(result[0].total_rows, 2);
        assert_eq!(result[0].missing_ohlcv_rows, 1);
    }
}
