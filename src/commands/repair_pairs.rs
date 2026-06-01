//! Heuristic backfill for `paired_tx_id` on historical transactions.
//!
//! For each unpaired buy on a non-cash symbol, search for the closest USD
//! sell transaction within ±`max_days` days and within ±`max_notional_pct`
//! of the buy notional. Idempotent: only proposes pairs where BOTH legs
//! currently have `paired_tx_id = NULL`. `--dry-run` (default) preview;
//! `--confirm` applies; `--skip <id>` excludes a transaction id from
//! consideration for tricky cases that need manual review.
//!
//! See TODO: "Historical-data backfill for newly-introduced feature
//! tables" for the original spec.

use anyhow::{bail, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::db::backend::BackendConnection;
use crate::db::transactions::{list_transactions_backend, set_paired_transaction_backend};
use crate::models::asset::AssetCategory;
use crate::models::transaction::{Transaction, TxType};

pub struct Options {
    pub dry_run: bool,
    pub confirm: bool,
    pub skip: Vec<i64>,
    pub max_days: i64,
    pub max_notional_pct: f64,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposedPair {
    pub buy_id: i64,
    pub sell_id: i64,
    pub symbol: String,
    pub buy_date: String,
    pub sell_date: String,
    pub day_delta: i64,
    pub buy_notional: String,
    pub sell_notional: String,
    pub notional_delta_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairReport {
    pub scanned_unpaired_buys: usize,
    pub proposed: Vec<ProposedPair>,
    pub applied: usize,
    pub dry_run: bool,
    pub max_days: i64,
    pub max_notional_pct: f64,
    pub skipped_ids: Vec<i64>,
}

pub fn run(backend: &BackendConnection, opts: Options) -> Result<()> {
    // `--dry-run` is the default safe mode. `--confirm` is required to
    // actually mutate. `--dry-run --confirm` is a contradiction.
    if opts.dry_run && opts.confirm {
        bail!("--dry-run and --confirm are mutually exclusive");
    }
    let mutate = opts.confirm;
    let report = compute_report(
        backend,
        opts.max_days,
        opts.max_notional_pct,
        &opts.skip,
        mutate,
    )?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    if report.proposed.is_empty() {
        println!(
            "repair-pairs: no unpaired buy/sell candidates within ±{}d, ±{:.1}% notional.",
            report.max_days, report.max_notional_pct
        );
        return Ok(());
    }

    let header = if mutate {
        format!("repair-pairs: APPLIED {} pair(s)", report.applied)
    } else {
        format!(
            "repair-pairs: DRY RUN — {} proposed pair(s) (re-run with --confirm to apply)",
            report.proposed.len()
        )
    };
    println!("{header}");
    println!("{}", "-".repeat(header.len()));
    println!(
        "{:<6} {:<6} {:<8} {:<12} {:<12} {:>5} {:>14} {:>14} {:>8}",
        "buy", "sell", "symbol", "buy_date", "sell_date", "Δd", "buy_notional", "sell_notional", "Δ%"
    );
    for p in &report.proposed {
        println!(
            "{:<6} {:<6} {:<8} {:<12} {:<12} {:>5} {:>14} {:>14} {:>7.2}%",
            p.buy_id,
            p.sell_id,
            p.symbol,
            p.buy_date,
            p.sell_date,
            p.day_delta,
            p.buy_notional,
            p.sell_notional,
            p.notional_delta_pct,
        );
    }
    Ok(())
}

pub fn compute_report(
    backend: &BackendConnection,
    max_days: i64,
    max_notional_pct: f64,
    skip: &[i64],
    mutate: bool,
) -> Result<RepairReport> {
    let txs = list_transactions_backend(backend)?;
    let proposals = propose_pairs(&txs, max_days, max_notional_pct, skip)?;
    let mut applied = 0usize;
    if mutate {
        for p in &proposals {
            // Idempotency re-check: only apply when both legs still NULL.
            // (List was materialized above; pairs proposed earlier in this
            // pass do not collide because `propose_pairs` consumes both
            // sides per pair.)
            set_paired_transaction_backend(backend, p.buy_id, Some(p.sell_id))?;
            set_paired_transaction_backend(backend, p.sell_id, Some(p.buy_id))?;
            applied += 1;
        }
    }

    let scanned = txs
        .iter()
        .filter(|t| {
            t.tx_type == TxType::Buy
                && t.category != AssetCategory::Cash
                && t.paired_tx_id.is_none()
                && !skip.contains(&t.id)
        })
        .count();

    Ok(RepairReport {
        scanned_unpaired_buys: scanned,
        proposed: proposals,
        applied,
        dry_run: !mutate,
        max_days,
        max_notional_pct,
        skipped_ids: skip.to_vec(),
    })
}

/// Pure pairing heuristic — extracted for unit testability.
pub fn propose_pairs(
    txs: &[Transaction],
    max_days: i64,
    max_notional_pct: f64,
    skip: &[i64],
) -> Result<Vec<ProposedPair>> {
    let skip_set: HashSet<i64> = skip.iter().copied().collect();

    let mut unpaired_buys: Vec<&Transaction> = txs
        .iter()
        .filter(|t| {
            t.tx_type == TxType::Buy
                && t.category != AssetCategory::Cash
                && t.paired_tx_id.is_none()
                && !skip_set.contains(&t.id)
        })
        .collect();
    // Deterministic ordering: oldest buys first so they get first claim.
    unpaired_buys.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));

    let mut available_sells: Vec<&Transaction> = txs
        .iter()
        .filter(|t| {
            t.tx_type == TxType::Sell
                && t.currency.eq_ignore_ascii_case("USD")
                && t.paired_tx_id.is_none()
                && !skip_set.contains(&t.id)
        })
        .collect();
    available_sells.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
    let mut consumed: HashSet<i64> = HashSet::new();

    let mut out = Vec::new();
    for buy in &unpaired_buys {
        let buy_date = parse_date(&buy.date)?;
        let buy_notional = buy.quantity * buy.price_per;
        if buy_notional == Decimal::ZERO {
            continue;
        }
        let buy_notional_f64 = decimal_to_f64(buy_notional);
        let mut best: Option<(&Transaction, i64, f64)> = None;
        for sell in &available_sells {
            if consumed.contains(&sell.id) {
                continue;
            }
            let sell_date = parse_date(&sell.date)?;
            let day_delta = (sell_date - buy_date).num_days();
            if day_delta.abs() > max_days {
                continue;
            }
            let sell_notional = sell.quantity * sell.price_per;
            let sell_notional_f64 = decimal_to_f64(sell_notional);
            if buy_notional_f64.abs() < f64::EPSILON {
                continue;
            }
            let pct = ((sell_notional_f64 - buy_notional_f64) / buy_notional_f64).abs() * 100.0;
            if pct > max_notional_pct {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, best_day, best_pct)) => {
                    // Prefer smaller |day_delta|; tiebreak on smaller pct.
                    let bd = day_delta.abs();
                    let bbd = best_day.abs();
                    bd < bbd || (bd == bbd && pct < best_pct)
                }
            };
            if better {
                best = Some((sell, day_delta, pct));
            }
        }
        if let Some((sell, day_delta, pct)) = best {
            consumed.insert(sell.id);
            let sell_notional = sell.quantity * sell.price_per;
            out.push(ProposedPair {
                buy_id: buy.id,
                sell_id: sell.id,
                symbol: buy.symbol.clone(),
                buy_date: buy.date.clone(),
                sell_date: sell.date.clone(),
                day_delta,
                buy_notional: buy_notional.to_string(),
                sell_notional: sell_notional.to_string(),
                notional_delta_pct: round2(pct),
            });
        }
    }
    Ok(out)
}

fn parse_date(raw: &str) -> Result<NaiveDate> {
    // Accept "YYYY-MM-DD" first; fall back to the leading 10 chars for
    // ISO-with-time strings encountered in older rows.
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .or_else(|_| {
            let head = raw.get(..10).unwrap_or("");
            NaiveDate::parse_from_str(head, "%Y-%m-%d")
        })
        .map_err(|_| anyhow::anyhow!("invalid transaction date '{}'", raw))
}

fn decimal_to_f64(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn tx(
        id: i64,
        symbol: &str,
        category: AssetCategory,
        tx_type: TxType,
        qty: Decimal,
        price: Decimal,
        currency: &str,
        date: &str,
        paired_tx_id: Option<i64>,
    ) -> Transaction {
        Transaction {
            id,
            symbol: symbol.to_string(),
            category,
            tx_type,
            quantity: qty,
            price_per: price,
            currency: currency.to_string(),
            date: date.to_string(),
            notes: None,
            paired_tx_id,
            created_at: date.to_string(),
        }
    }

    #[test]
    fn pairs_buy_with_same_day_usd_sell_within_notional() {
        let txs = vec![
            tx(1, "AAPL", AssetCategory::Equity, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", None),
            tx(2, "USD", AssetCategory::Cash, TxType::Sell, dec!(1500), dec!(1), "USD", "2024-01-10", None),
        ];
        let proposals = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].buy_id, 1);
        assert_eq!(proposals[0].sell_id, 2);
        assert_eq!(proposals[0].day_delta, 0);
    }

    #[test]
    fn rejects_outside_day_window() {
        let txs = vec![
            tx(1, "AAPL", AssetCategory::Equity, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", None),
            tx(2, "USD", AssetCategory::Cash, TxType::Sell, dec!(1500), dec!(1), "USD", "2024-01-13", None),
        ];
        let proposals = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        assert!(proposals.is_empty());
    }

    #[test]
    fn rejects_outside_notional_window() {
        let txs = vec![
            tx(1, "AAPL", AssetCategory::Equity, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", None),
            tx(2, "USD", AssetCategory::Cash, TxType::Sell, dec!(2000), dec!(1), "USD", "2024-01-10", None),
        ];
        // |2000-1500|/1500 = 33.3% > 10%
        let proposals = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        assert!(proposals.is_empty());
    }

    #[test]
    fn rejects_non_usd_sell() {
        let txs = vec![
            tx(1, "AAPL", AssetCategory::Equity, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", None),
            tx(2, "EUR", AssetCategory::Cash, TxType::Sell, dec!(1500), dec!(1), "EUR", "2024-01-10", None),
        ];
        let proposals = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        assert!(proposals.is_empty());
    }

    #[test]
    fn skips_already_paired_legs() {
        let txs = vec![
            tx(1, "AAPL", AssetCategory::Equity, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", Some(99)),
            tx(2, "USD", AssetCategory::Cash, TxType::Sell, dec!(1500), dec!(1), "USD", "2024-01-10", None),
        ];
        let proposals = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        assert!(proposals.is_empty());
    }

    #[test]
    fn skips_cash_buy_legs() {
        let txs = vec![
            tx(1, "USD", AssetCategory::Cash, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", None),
            tx(2, "USD", AssetCategory::Cash, TxType::Sell, dec!(1500), dec!(1), "USD", "2024-01-10", None),
        ];
        let proposals = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        assert!(proposals.is_empty());
    }

    #[test]
    fn manual_skip_list_excludes_id() {
        let txs = vec![
            tx(1, "AAPL", AssetCategory::Equity, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", None),
            tx(2, "USD", AssetCategory::Cash, TxType::Sell, dec!(1500), dec!(1), "USD", "2024-01-10", None),
        ];
        let proposals = propose_pairs(&txs, 2, 10.0, &[1]).unwrap();
        assert!(proposals.is_empty());
    }

    #[test]
    fn prefers_closest_day_then_smallest_pct() {
        let txs = vec![
            tx(1, "AAPL", AssetCategory::Equity, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", None),
            // Day-delta 2, pct ~0
            tx(2, "USD", AssetCategory::Cash, TxType::Sell, dec!(1500), dec!(1), "USD", "2024-01-12", None),
            // Day-delta 0, pct 6.7%
            tx(3, "USD", AssetCategory::Cash, TxType::Sell, dec!(1600), dec!(1), "USD", "2024-01-10", None),
        ];
        let proposals = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        assert_eq!(proposals.len(), 1);
        // Closest day wins
        assert_eq!(proposals[0].sell_id, 3);
    }

    #[test]
    fn idempotent_each_sell_paired_only_once() {
        let txs = vec![
            tx(1, "AAPL", AssetCategory::Equity, TxType::Buy, dec!(10), dec!(150), "USD", "2024-01-10", None),
            tx(2, "MSFT", AssetCategory::Equity, TxType::Buy, dec!(5), dec!(300), "USD", "2024-01-10", None),
            tx(3, "USD", AssetCategory::Cash, TxType::Sell, dec!(1500), dec!(1), "USD", "2024-01-10", None),
        ];
        let proposals = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        // Both buys have notional 1500 — sell can only attach to one.
        assert_eq!(proposals.len(), 1);
        // The earlier-sorted (id 1) wins because both share the same date.
        assert_eq!(proposals[0].buy_id, 1);
    }

    #[test]
    fn run_twice_is_idempotent_on_empty_pool() {
        let txs: Vec<Transaction> = vec![];
        let a = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        let b = propose_pairs(&txs, 2, 10.0, &[]).unwrap();
        assert_eq!(a, b);
    }

    mod backend_apply {
        use super::*;
        use crate::db::backend::BackendConnection;
        use crate::db::open_in_memory;
        use crate::db::transactions::{
            insert_transaction_backend, list_transactions_backend,
        };
        use crate::models::transaction::NewTransaction;

        fn backend() -> BackendConnection {
            BackendConnection::Sqlite {
                conn: open_in_memory(),
            }
        }

        fn insert(
            backend: &BackendConnection,
            symbol: &str,
            category: AssetCategory,
            tx_type: TxType,
            qty: Decimal,
            price: Decimal,
            currency: &str,
            date: &str,
        ) -> i64 {
            insert_transaction_backend(
                backend,
                &NewTransaction {
                    symbol: symbol.to_string(),
                    category,
                    tx_type,
                    quantity: qty,
                    price_per: price,
                    currency: currency.to_string(),
                    date: date.to_string(),
                    notes: None,
                },
            )
            .unwrap()
        }

        #[test]
        fn dry_run_does_not_mutate() {
            let backend = backend();
            insert(
                &backend,
                "AAPL",
                AssetCategory::Equity,
                TxType::Buy,
                dec!(10),
                dec!(150),
                "USD",
                "2024-01-10",
            );
            insert(
                &backend,
                "USD",
                AssetCategory::Cash,
                TxType::Sell,
                dec!(1500),
                dec!(1),
                "USD",
                "2024-01-10",
            );
            let report = compute_report(&backend, 2, 10.0, &[], false).unwrap();
            assert_eq!(report.proposed.len(), 1);
            assert_eq!(report.applied, 0);
            // DB unchanged
            let txs = list_transactions_backend(&backend).unwrap();
            assert!(txs.iter().all(|t| t.paired_tx_id.is_none()));
        }

        #[test]
        fn confirm_applies_and_is_idempotent() {
            let backend = backend();
            let buy_id = insert(
                &backend,
                "AAPL",
                AssetCategory::Equity,
                TxType::Buy,
                dec!(10),
                dec!(150),
                "USD",
                "2024-01-10",
            );
            let sell_id = insert(
                &backend,
                "USD",
                AssetCategory::Cash,
                TxType::Sell,
                dec!(1500),
                dec!(1),
                "USD",
                "2024-01-10",
            );
            // First apply: one pair.
            let r1 = compute_report(&backend, 2, 10.0, &[], true).unwrap();
            assert_eq!(r1.applied, 1);
            let txs = list_transactions_backend(&backend).unwrap();
            let buy = txs.iter().find(|t| t.id == buy_id).unwrap();
            let sell = txs.iter().find(|t| t.id == sell_id).unwrap();
            assert_eq!(buy.paired_tx_id, Some(sell_id));
            assert_eq!(sell.paired_tx_id, Some(buy_id));

            // Re-running is a no-op: both legs already paired so they are
            // filtered before pairing.
            let r2 = compute_report(&backend, 2, 10.0, &[], true).unwrap();
            assert_eq!(r2.proposed.len(), 0);
            assert_eq!(r2.applied, 0);
        }
    }
}
