//! `pftui analytics alignment current|history|compute` — operator-vs-analyst
//! daily alignment score.
//!
//! Sees:
//!   - portfolio holdings (allocation pct per held asset, via `portfolio status`
//!     data path)
//!   - analyst convergence per asset (`db::analyst_views::convergence_report_backend`)
//!   - operator views (journal entries authored 'skylar' last 14d, plus the
//!     optional `operator_replies` table)
//!
//! Writes:
//!   - `alignment_score_history` (one row per day)
//!   - `agent_messages` (when alignment drops below 50 for 2+ consecutive days)
//!
//! Reuses the existing convergence engine — no convergence logic is
//! reimplemented here.

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::HashMap;

use crate::config::{Config, PortfolioMode};
use crate::db::alignment_score::{
    compute_for_date, get_row_backend, history_backend, maybe_emit_drift_alert_backend,
    parse_since_token, upsert_row_backend, AlignmentScoreRow, HeldAsset,
};
use crate::db::allocations::list_allocations_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations};

fn dec_to_f64(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

/// Build the list of held assets above 1% allocation from the same data path
/// that powers `pftui portfolio status`.
pub fn collect_held_assets(backend: &BackendConnection, config: &Config) -> Result<Vec<HeldAsset>> {
    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> = cached.into_iter().map(|q| (q.symbol, q.price)).collect();

    let positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let txs = list_transactions_backend(backend)?;
            // Cash assets price at 1.0
            for tx in &txs {
                if tx.category == AssetCategory::Cash {
                    prices.insert(tx.symbol.clone(), Decimal::ONE);
                }
            }
            let fx_rates =
                crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
            compute_positions(&txs, &prices, &fx_rates)
        }
        PortfolioMode::Percentage => {
            let allocs = list_allocations_backend(backend)?;
            let fx_rates =
                crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
            compute_positions_from_allocations(&allocs, &prices, &fx_rates)
        }
    };

    let mut out = Vec::new();
    for p in positions {
        let pct = p
            .allocation_pct
            .map(dec_to_f64)
            .unwrap_or(0.0);
        if pct >= 1.0 {
            out.push(HeldAsset {
                symbol: p.symbol.clone(),
                allocation_pct: pct,
            });
        }
    }
    Ok(out)
}

/// Default convergence lookback window used when scoring a day. Matches the
/// analyst-views convergence default (a wide-ish window so analyst-views
/// reports include all four timeframe layers).
const DEFAULT_CONVERGENCE_WINDOW: &str = "14d";

// ---------------------------------------------------------------------------
// Subcommand: `current`
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CurrentEnvelope<'a> {
    row: &'a AlignmentScoreRow,
    stored: bool,
}

pub fn run_current(backend: &BackendConnection, config: &Config, json: bool) -> Result<()> {
    let today = Utc::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();

    // If today's row is already stored, return it; otherwise compute on demand
    // (no store — `compute --store` is the explicit write path).
    let (row, stored) = if let Some(existing) = get_row_backend(backend, &today_str)? {
        (existing, true)
    } else {
        let held = collect_held_assets(backend, config)?;
        let row = compute_for_date(backend, &held, today, Some(DEFAULT_CONVERGENCE_WINDOW))?;
        (row, false)
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&CurrentEnvelope { row: &row, stored })?
        );
    } else {
        print_row_pretty(&row, stored);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand: `history`
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct HistoryEnvelope<'a> {
    since: &'a str,
    count: usize,
    rows: &'a [AlignmentScoreRow],
}

pub fn run_history(backend: &BackendConnection, since: &str, json: bool) -> Result<()> {
    let since_date = parse_since_token(since)
        .with_context(|| format!("invalid --since '{}'", since))?;
    let rows = history_backend(backend, Some(&since_date))?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&HistoryEnvelope {
                since: &since_date,
                count: rows.len(),
                rows: &rows,
            })?
        );
    } else if rows.is_empty() {
        println!(
            "No alignment-score rows since {} — run `pftui analytics alignment compute --store`.",
            since_date
        );
    } else {
        println!(
            "Alignment-score history since {} ({} rows):",
            since_date,
            rows.len()
        );
        println!("  {:12} {:>8} {:>15} Divergent", "Date", "Score", "Regime");
        println!("  {}", "-".repeat(60));
        for r in &rows {
            let divergent = if r.divergent_assets.is_empty() {
                String::new()
            } else {
                r.divergent_assets.join(", ")
            };
            println!(
                "  {:12} {:>7.1} {:>15} {}",
                r.date, r.total_alignment_score, r.regime_state, divergent
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand: `compute`
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ComputeEnvelope<'a> {
    row: &'a AlignmentScoreRow,
    stored: bool,
    alert_id: Option<i64>,
}

pub fn run_compute(
    backend: &BackendConnection,
    config: &Config,
    date: Option<&str>,
    store: bool,
    json: bool,
) -> Result<()> {
    let date_val = match date {
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .with_context(|| format!("invalid --date '{}': expected YYYY-MM-DD", d))?,
        None => Utc::now().date_naive(),
    };

    let held = collect_held_assets(backend, config)?;
    let row = compute_for_date(backend, &held, date_val, Some(DEFAULT_CONVERGENCE_WINDOW))?;

    let mut alert_id: Option<i64> = None;
    if store {
        upsert_row_backend(backend, &row)?;
        // After storing, check the recent history for a drift alert.
        let recent = history_backend(backend, Some(&parse_since_token("14d")?))?;
        alert_id = maybe_emit_drift_alert_backend(backend, &recent)?;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ComputeEnvelope {
                row: &row,
                stored: store,
                alert_id,
            })?
        );
    } else {
        print_row_pretty(&row, store);
        if let Some(id) = alert_id {
            println!();
            println!("  -> emitted drift alert agent_messages id={}", id);
        }
    }
    Ok(())
}

fn print_row_pretty(row: &AlignmentScoreRow, stored: bool) {
    println!(
        "Alignment score — {} ({})",
        row.date,
        if stored { "stored" } else { "computed (not stored)" }
    );
    println!();
    println!(
        "  Total: {:.1}/100 -> {}",
        row.total_alignment_score, row.regime_state
    );
    println!();
    if row.components.is_empty() {
        println!("  No held assets above 1% — nothing to align on.");
        return;
    }
    println!(
        "  {:10} {:>8} {:>18} {:>22} {:>10}",
        "Symbol", "Alloc%", "Operator", "Analyst", "Class"
    );
    println!("  {}", "-".repeat(74));
    for c in &row.components {
        let op = c
            .operator_view
            .as_ref()
            .map(|o| format!("{}/{}", o.direction, o.conviction_magnitude))
            .unwrap_or_else(|| "—".to_string());
        let analyst = format!("{} ({:+.1})", c.analyst_summary, c.analyst_avg_conviction);
        println!(
            "  {:10} {:>7.1}% {:>18} {:>22} {:>10}",
            c.symbol, c.allocation_weight, op, analyst, c.alignment_class
        );
    }
    if !row.divergent_assets.is_empty() {
        println!();
        println!("  Divergent: {}", row.divergent_assets.join(", "));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::analyst_views::upsert_view_backend;
    use crate::db::journal::{add_entry, NewJournalEntry};

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    fn seed_analyst_views(backend: &BackendConnection, symbol: &str, direction: &str, conv: i64) {
        for analyst in ["low", "medium", "high", "macro"] {
            upsert_view_backend(
                backend,
                analyst,
                symbol,
                direction,
                conv,
                "test reasoning",
                None,
                None,
                None,
            )
            .unwrap();
        }
    }

    fn seed_skylar_journal(backend: &BackendConnection, symbol: &str, content: &str) {
        let BackendConnection::Sqlite { conn } = backend else {
            panic!("expected sqlite");
        };
        let entry = NewJournalEntry {
            timestamp: Utc::now().to_rfc3339(),
            content: content.to_string(),
            tag: None,
            symbol: Some(symbol.to_string()),
            conviction: None,
            status: "open".to_string(),
            author: "skylar".to_string(),
        };
        add_entry(conn, &entry).unwrap();
    }

    #[test]
    fn compute_for_date_returns_aligned_when_both_bullish() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        seed_analyst_views(&backend, "BTC", "bull", 2);
        seed_skylar_journal(&backend, "BTC", "Strongly bullish on BTC into the rest of the month.");

        let today = Utc::now().date_naive();
        let held = vec![HeldAsset {
            symbol: "BTC".to_string(),
            allocation_pct: 30.0,
        }];
        let row = compute_for_date(&backend, &held, today, Some("14d")).unwrap();
        assert_eq!(row.components.len(), 1);
        let comp = &row.components[0];
        assert_eq!(comp.alignment_class, "aligned");
        assert_eq!(row.total_alignment_score, 100.0);
        assert_eq!(row.regime_state, "high-alignment");
        assert!(row.divergent_assets.is_empty());
    }

    #[test]
    fn compute_for_date_flags_divergent_direction() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        seed_analyst_views(&backend, "BTC", "bear", -3);
        seed_skylar_journal(&backend, "BTC", "Buying BTC aggressively — bullish.");

        let today = Utc::now().date_naive();
        let held = vec![HeldAsset {
            symbol: "BTC".to_string(),
            allocation_pct: 25.0,
        }];
        let row = compute_for_date(&backend, &held, today, Some("14d")).unwrap();
        assert_eq!(row.components[0].alignment_class, "divergent-direction");
        assert_eq!(row.total_alignment_score, 0.0);
        assert_eq!(row.regime_state, "divergent");
        assert_eq!(row.divergent_assets, vec!["BTC"]);
    }

    #[test]
    fn compute_for_date_skips_below_1pct_allocation() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        seed_analyst_views(&backend, "DUST", "bull", 3);
        seed_skylar_journal(&backend, "DUST", "Bullish on DUST.");

        let today = Utc::now().date_naive();
        let held = vec![HeldAsset {
            symbol: "DUST".to_string(),
            allocation_pct: 0.5,
        }];
        let row = compute_for_date(&backend, &held, today, Some("14d")).unwrap();
        assert!(row.components.is_empty());
    }

    #[test]
    fn compute_for_date_weights_by_allocation() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        // BTC: aligned (score 100, weight 30)
        seed_analyst_views(&backend, "BTC", "bull", 2);
        seed_skylar_journal(&backend, "BTC", "Bullish on BTC.");

        // GLD: divergent-direction (score 0, weight 10)
        seed_analyst_views(&backend, "GLD", "bear", -3);
        seed_skylar_journal(&backend, "GLD", "Bullish on GLD.");

        let today = Utc::now().date_naive();
        let held = vec![
            HeldAsset {
                symbol: "BTC".to_string(),
                allocation_pct: 30.0,
            },
            HeldAsset {
                symbol: "GLD".to_string(),
                allocation_pct: 10.0,
            },
        ];
        let row = compute_for_date(&backend, &held, today, Some("14d")).unwrap();
        // 30*100 / (30+10) = 75
        assert!((row.total_alignment_score - 75.0).abs() < 0.001);
        assert_eq!(row.regime_state, "mixed");
        assert_eq!(row.divergent_assets, vec!["GLD"]);
    }

    #[test]
    fn regime_transitions_classify_correctly() {
        // High-alignment -> mixed -> divergent thresholds verified together with
        // a known weight pattern.
        for (score, expected) in [
            (95.0, "high-alignment"),
            (80.0, "high-alignment"),
            (79.9, "mixed"),
            (50.0, "mixed"),
            (49.0, "divergent"),
            (10.0, "divergent"),
        ] {
            assert_eq!(
                crate::db::alignment_score::regime_state(score),
                expected,
                "regime mismatch at score={}",
                score
            );
        }
    }

    #[test]
    fn threshold_alert_fires_after_two_consecutive_low_days() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        let mk = |d: &str, score: f64| AlignmentScoreRow {
            date: d.to_string(),
            total_alignment_score: score,
            components: vec![],
            divergent_assets: vec!["BTC".to_string()],
            regime_state: crate::db::alignment_score::regime_state(score).to_string(),
            computed_at: "x".to_string(),
        };

        // Persist two rows, both below 50.
        let rows = vec![mk("2026-05-30", 40.0), mk("2026-05-31", 30.0)];
        for r in &rows {
            crate::db::alignment_score::upsert_row_backend(&backend, r).unwrap();
        }

        let alert = maybe_emit_drift_alert_backend(&backend, &rows).unwrap();
        assert!(alert.is_some(), "expected an alert when 2 consecutive low days");
        // Idempotency: a second call on the same data should NOT double-emit.
        let again = maybe_emit_drift_alert_backend(&backend, &rows).unwrap();
        assert!(
            again.is_none(),
            "expected idempotent no-op when alert already exists for the latest date"
        );
    }

    #[test]
    fn threshold_alert_does_not_fire_on_single_low_day() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        let rows = vec![
            AlignmentScoreRow {
                date: "2026-05-31".to_string(),
                total_alignment_score: 30.0,
                components: vec![],
                divergent_assets: vec![],
                regime_state: "divergent".to_string(),
                computed_at: "x".to_string(),
            },
        ];
        let alert = maybe_emit_drift_alert_backend(&backend, &rows).unwrap();
        assert!(alert.is_none());
    }

    #[test]
    fn history_envelope_orders_by_date_ascending() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);
        for d in ["2026-05-30", "2026-05-29", "2026-05-31"] {
            crate::db::alignment_score::upsert_row_backend(
                &backend,
                &AlignmentScoreRow {
                    date: d.to_string(),
                    total_alignment_score: 70.0,
                    components: vec![],
                    divergent_assets: vec![],
                    regime_state: "mixed".to_string(),
                    computed_at: "x".to_string(),
                },
            )
            .unwrap();
        }
        let rows = history_backend(&backend, None).unwrap();
        let dates: Vec<String> = rows.iter().map(|r| r.date.clone()).collect();
        assert_eq!(dates, vec!["2026-05-29", "2026-05-30", "2026-05-31"]);
    }
}
