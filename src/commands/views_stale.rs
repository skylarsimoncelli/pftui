//! `pftui analytics views stale` — stale-view detector.
//!
//! For each held asset (net positive transaction quantity) and each
//! canonical analyst layer (low/medium/high/macro): if the layer's latest
//! view on that asset is older than `--days` AND the asset's price has
//! moved more than `--move-pct` percent since the view's `updated_at`
//! (per `price_history`), the view is flagged: evidence moved, conviction
//! didn't. Missing views are a coverage problem, not a staleness problem,
//! and are not flagged here (see `analytics views portfolio-matrix`).

use anyhow::Result;
use chrono::{NaiveDate, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;

use crate::db::analyst_views;
use crate::db::backend::BackendConnection;
use crate::db::price_history;
use crate::models::asset::AssetCategory;
use crate::models::position::compute_positions;

/// Canonical timeframe layers checked by the detector.
const CANONICAL_LAYERS: [&str; 4] = ["low", "medium", "high", "macro"];

#[derive(Debug, Serialize)]
struct StaleView {
    asset: String,
    layer: String,
    direction: String,
    conviction: i64,
    view_date: String,
    view_age_days: i64,
    price_at_view: String,
    price_now: String,
    price_date_now: String,
    move_pct: f64,
}

/// Resolve the price-history symbol for an asset: the asset symbol itself,
/// falling back to `<ASSET>-USD` (crypto positions are often held as `BTC`
/// while the deep history series is `BTC-USD`).
fn resolve_history_symbol(conn: &Connection, asset: &str) -> Result<Option<String>> {
    for candidate in [asset.to_string(), format!("{asset}-USD")] {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM price_history WHERE symbol = ?1",
            [candidate.as_str()],
            |row| row.get(0),
        )?;
        if n > 0 {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

struct StaleReport {
    stale: Vec<StaleView>,
    held: Vec<String>,
    views_checked: usize,
}

fn detect_stale(backend: &BackendConnection, days: i64, move_pct: f64) -> Result<StaleReport> {
    let conn = backend.sqlite_native().ok_or_else(|| {
        anyhow::anyhow!("`analytics views stale` requires the SQLite backend (price_history scan)")
    })?;
    let today = Utc::now().date_naive();

    // Held assets: net positive transaction quantity, cash excluded.
    let txs = crate::db::transactions::list_transactions_backend(backend)?;
    let positions = compute_positions(&txs, &HashMap::new(), &HashMap::new());
    let mut held: Vec<String> = positions
        .iter()
        .filter(|p| p.category != AssetCategory::Cash)
        .map(|p| p.symbol.to_uppercase())
        .collect();
    held.sort();
    held.dedup();

    let mut stale: Vec<StaleView> = Vec::new();
    let mut views_checked = 0usize;

    for asset in &held {
        for layer in CANONICAL_LAYERS {
            let Some(view) = analyst_views::get_view_backend(backend, layer, asset)? else {
                continue;
            };
            views_checked += 1;

            // updated_at is "YYYY-MM-DD HH:MM:SS" (SQLite datetime('now')).
            let view_date_str: String = view.updated_at.chars().take(10).collect();
            let Ok(view_date) = NaiveDate::parse_from_str(&view_date_str, "%Y-%m-%d") else {
                continue;
            };
            let age_days = (today - view_date).num_days();
            if age_days <= days {
                continue;
            }

            let Some(history_symbol) = resolve_history_symbol(conn, asset)? else {
                continue;
            };
            let Some(price_then) =
                price_history::get_price_at_date(conn, &history_symbol, &view_date_str)?
            else {
                continue;
            };
            if price_then == Decimal::ZERO {
                continue;
            }
            let latest = price_history::get_history(conn, &history_symbol, 1)?;
            let Some(latest) = latest.last() else {
                continue;
            };

            let change_pct =
                ((latest.close - price_then) / price_then * Decimal::from(100)).round_dp(2);
            let change_pct_f = change_pct.to_f64().unwrap_or(0.0);
            if change_pct_f.abs() > move_pct {
                stale.push(StaleView {
                    asset: asset.clone(),
                    layer: layer.to_string(),
                    direction: view.direction.clone(),
                    conviction: view.conviction,
                    view_date: view_date_str,
                    view_age_days: age_days,
                    price_at_view: price_then.round_dp(4).to_string(),
                    price_now: latest.close.round_dp(4).to_string(),
                    price_date_now: latest.date.clone(),
                    move_pct: change_pct_f,
                });
            }
        }
    }

    Ok(StaleReport {
        stale,
        held,
        views_checked,
    })
}

/// Count stale views at the detector's default thresholds (21d / 10% move).
/// Used by the `data refresh` housekeeping summary line.
pub fn count_stale_for_refresh(backend: &BackendConnection) -> Result<usize> {
    Ok(detect_stale(backend, 21, 10.0)?.stale.len())
}

pub fn run(backend: &BackendConnection, days: i64, move_pct: f64, json_output: bool) -> Result<()> {
    let days = days.max(1);
    let move_pct = move_pct.abs();
    let StaleReport {
        stale,
        held,
        views_checked,
    } = detect_stale(backend, days, move_pct)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "stale_views": stale,
                "stale_count": stale.len(),
                "held_assets": held,
                "views_checked": views_checked,
                "days_threshold": days,
                "move_pct_threshold": move_pct,
            }))?
        );
    } else if stale.is_empty() {
        println!(
            "no stale views ({} view(s) checked across {} held asset(s); thresholds: >{}d old AND >{:.1}% price move)",
            views_checked,
            held.len(),
            days,
            move_pct
        );
    } else {
        println!(
            "Stale analyst views — view may be stale: evidence moved, conviction didn't (> {}d old, > {:.1}% move):",
            days, move_pct
        );
        println!(
            "{:<10} {:<8} {:<9} {:>5} {:>9} {:>12} {:>12} {:>8}",
            "Asset", "Layer", "Direction", "Conv", "Age(d)", "Px@view", "PxNow", "Move%"
        );
        println!("{}", "─".repeat(80));
        for row in &stale {
            println!(
                "{:<10} {:<8} {:<9} {:>+5} {:>9} {:>12} {:>12} {:>+7.1}%",
                row.asset,
                row.layer,
                row.direction,
                row.conviction,
                row.view_age_days,
                row.price_at_view,
                row.price_now,
                row.move_pct
            );
        }
        println!(
            "\n{} stale view(s). Refresh with `analytics views set` or downgrade conviction.",
            stale.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn setup_backend() -> BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    fn add_buy(backend: &BackendConnection, symbol: &str, qty: &str, price: &str) {
        let tx = crate::models::transaction::NewTransaction {
            symbol: symbol.to_string(),
            category: AssetCategory::Crypto,
            tx_type: crate::models::transaction::TxType::Buy,
            quantity: qty.parse().unwrap(),
            price_per: price.parse().unwrap(),
            currency: "USD".to_string(),
            date: "2026-01-01".to_string(),
            notes: None,
        };
        crate::db::transactions::insert_transaction_backend(backend, &tx).unwrap();
    }

    fn put_history(backend: &BackendConnection, symbol: &str, date: &str, close: Decimal) {
        let conn = backend.sqlite_native().unwrap();
        crate::db::price_history::upsert_history(
            conn,
            symbol,
            "test",
            &[crate::models::price::HistoryRecord {
                date: date.to_string(),
                close,
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();
    }

    fn put_old_view(backend: &BackendConnection, layer: &str, asset: &str, days_ago: i64) {
        analyst_views::upsert_view_backend(
            backend, layer, asset, "bull", 3, "synthetic", None, None, None,
        )
        .unwrap();
        let stamp = (Utc::now() - chrono::Duration::days(days_ago))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let conn = backend.sqlite_native().unwrap();
        conn.execute(
            "UPDATE analyst_views SET updated_at = ?1 WHERE analyst = ?2 AND asset = ?3",
            rusqlite::params![stamp, layer, asset],
        )
        .unwrap();
    }

    #[test]
    fn flags_old_view_with_big_move() {
        let backend = setup_backend();
        add_buy(&backend, "BTC", "1", "50000");
        put_old_view(&backend, "medium", "BTC", 40);

        let view_date = (Utc::now() - chrono::Duration::days(40))
            .format("%Y-%m-%d")
            .to_string();
        let today = Utc::now().format("%Y-%m-%d").to_string();
        // Price moved +25% since the view.
        put_history(&backend, "BTC", &view_date, dec!(80000));
        put_history(&backend, "BTC", &today, dec!(100000));

        let report = detect_stale(&backend, 21, 10.0).unwrap();
        assert_eq!(report.held, vec!["BTC".to_string()]);
        assert_eq!(report.views_checked, 1);
        assert_eq!(report.stale.len(), 1);
        let row = &report.stale[0];
        assert_eq!(row.asset, "BTC");
        assert_eq!(row.layer, "medium");
        assert_eq!(row.view_age_days, 40);
        assert!((row.move_pct - 25.0).abs() < 0.01, "got {}", row.move_pct);

        // A high move threshold suppresses the flag.
        assert!(detect_stale(&backend, 21, 50.0).unwrap().stale.is_empty());
        // A larger age threshold suppresses the flag too.
        assert!(detect_stale(&backend, 60, 10.0).unwrap().stale.is_empty());

        // Smoke both render paths.
        run(&backend, 21, 10.0, false).unwrap();
        run(&backend, 21, 10.0, true).unwrap();
    }

    #[test]
    fn fresh_view_or_small_move_not_flagged() {
        let backend = setup_backend();
        add_buy(&backend, "BTC", "1", "50000");
        // Fresh view (today) — never stale regardless of move.
        analyst_views::upsert_view_backend(
            &backend, "low", "BTC", "bull", 2, "synthetic", None, None, None,
        )
        .unwrap();
        let today = Utc::now().format("%Y-%m-%d").to_string();
        put_history(&backend, "BTC", &today, dec!(100000));
        let report = detect_stale(&backend, 21, 10.0).unwrap();
        assert_eq!(report.views_checked, 1);
        assert!(report.stale.is_empty());
        run(&backend, 21, 10.0, false).unwrap();
        run(&backend, 21, 10.0, true).unwrap();
    }

    #[test]
    fn falls_back_to_usd_suffixed_series() {
        let backend = setup_backend();
        let conn = backend.sqlite_native().unwrap();
        // Only BTC-USD history exists; asset is held as BTC.
        let view_date = (Utc::now() - chrono::Duration::days(40))
            .format("%Y-%m-%d")
            .to_string();
        put_history(&backend, "BTC-USD", &view_date, dec!(80000));
        assert_eq!(
            resolve_history_symbol(conn, "BTC").unwrap(),
            Some("BTC-USD".to_string())
        );
        assert_eq!(resolve_history_symbol(conn, "ETH").unwrap(), None);
    }
}
