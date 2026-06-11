//! Price-ingest plausibility guard for the canonical `price_history` series.
//!
//! `price_history` is L1 — THE durable series every downstream layer trusts
//! (technicals, cycle engines, research harness, reports). A single corrupt
//! dated close poisons all of them at once. This module gates every
//! automated insert path with a day-over-day plausibility check:
//!
//! - A new close within [`MAX_DD_CHANGE_PCT`] of the symbol's previous
//!   stored close is accepted normally.
//! - A larger jump is SUSPECT. If the caller supplies a corroborating
//!   secondary-source quote within [`CORROBORATION_TOLERANCE_PCT`] of the
//!   candidate, the print is accepted (a real move). If the secondary
//!   contradicts the candidate — or no secondary exists — the print is
//!   REJECTED and never written.
//! - Genuine >20% events (crashes, halts) with no secondary source are
//!   admitted via the explicit operator override:
//!   `pftui data refresh --accept-outlier <SYM>`.
//!
//! The guard compares each record against the latest stored close strictly
//! before that record's date, so intraday re-stamps of today's close are
//! always measured against yesterday's bar, and batch backfills are checked
//! bar-by-bar as they extend the series.

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::db::backend::BackendConnection;
use crate::db::price_history::{get_latest_close_before_backend, upsert_history_backend};
use crate::models::price::HistoryRecord;
use crate::report::format::group_thousands;

/// Maximum day-over-day % change before a print is considered SUSPECT.
pub const MAX_DD_CHANGE_PCT: Decimal = dec!(20);

/// A suspect print is accepted when a secondary source confirms it within
/// this % tolerance.
pub const CORROBORATION_TOLERANCE_PCT: Decimal = dec!(5);

/// Verdict for one candidate print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardDecision {
    /// Within threshold (or no prior close to compare against).
    Accept,
    /// Suspect jump, but a secondary source confirmed the move.
    AcceptCorroborated { secondary: Decimal },
    /// Suspect jump admitted via explicit operator override.
    AcceptOverride,
    /// Suspect jump with no corroboration — do not write.
    Reject { secondary: Option<Decimal> },
}

/// Signed day-over-day % change of `candidate` vs `prev` (prev must be > 0).
pub fn signed_change_pct(candidate: Decimal, prev: Decimal) -> Decimal {
    ((candidate - prev) / prev) * dec!(100)
}

/// Pure guard evaluation for a single candidate close.
///
/// `prev_close` is the latest stored close strictly before the candidate's
/// date; `secondary` is an independent same-day quote from another source
/// (only consulted when the print is suspect).
pub fn evaluate_print(
    candidate: Decimal,
    prev_close: Option<Decimal>,
    secondary: Option<Decimal>,
    accept_outlier: bool,
) -> GuardDecision {
    let prev = match prev_close {
        Some(p) if p > Decimal::ZERO => p,
        // No baseline (first bar for the symbol) — nothing to guard against.
        _ => return GuardDecision::Accept,
    };
    let change_pct = signed_change_pct(candidate, prev);
    if change_pct.abs() <= MAX_DD_CHANGE_PCT {
        return GuardDecision::Accept;
    }
    if accept_outlier {
        return GuardDecision::AcceptOverride;
    }
    match secondary {
        Some(sec) if sec > Decimal::ZERO => {
            let divergence = signed_change_pct(candidate, sec).abs();
            if divergence <= CORROBORATION_TOLERANCE_PCT {
                GuardDecision::AcceptCorroborated { secondary: sec }
            } else {
                GuardDecision::Reject { secondary: Some(sec) }
            }
        }
        _ => GuardDecision::Reject { secondary: None },
    }
}

/// One rejected print, with everything needed for a loud warning line.
#[derive(Debug, Clone)]
pub struct RejectedPrint {
    pub symbol: String,
    pub date: String,
    pub candidate: Decimal,
    pub source: String,
    pub prev_date: String,
    pub prev_close: Decimal,
    /// Signed d/d % change vs prev_close.
    pub change_pct: Decimal,
    /// Secondary-source quote, when one was available but contradicted.
    pub secondary: Option<Decimal>,
}

fn fmt_price(v: Decimal) -> String {
    group_thousands(&v.round_dp(if v.abs() >= dec!(1000) { 0 } else { 2 }).to_string())
}

impl RejectedPrint {
    /// The loud refresh-output warning, e.g.
    /// `⚠ price guard: BTC-USD print 77,414 rejected — +24.7% d/d, secondary says 62,580`
    pub fn warning_line(&self) -> String {
        let sign = if self.change_pct >= Decimal::ZERO { "+" } else { "" };
        let tail = match self.secondary {
            Some(sec) => format!("secondary says {}", fmt_price(sec)),
            None => format!(
                "no secondary corroboration (override: pftui data refresh --accept-outlier {})",
                self.symbol
            ),
        };
        format!(
            "⚠ price guard: {} print {} rejected for {} (source {}) — {}{}% d/d vs {} ({}), {}",
            self.symbol,
            fmt_price(self.candidate),
            self.date,
            self.source,
            sign,
            self.change_pct.round_dp(1),
            fmt_price(self.prev_close),
            self.prev_date,
            tail
        )
    }
}

/// Result of one guarded upsert batch.
#[derive(Debug, Default)]
pub struct GuardOutcome {
    /// Number of records written.
    pub accepted: usize,
    /// Records written only because a secondary source confirmed them.
    pub corroborated: Vec<String>,
    /// Records written only via the --accept-outlier override.
    pub overridden: Vec<String>,
    /// Records refused — never written.
    pub rejections: Vec<RejectedPrint>,
}

/// Guarded insert into price_history. Records are processed in
/// chronological order; each accepted record immediately extends the stored
/// series, so the next record in the batch is checked against it.
///
/// `secondary` is an independent same-day spot quote used to corroborate
/// suspect single-print stamps (it is only meaningful for "today" stamps —
/// multi-day backfill batches should pass None).
pub fn upsert_history_guarded_backend(
    backend: &BackendConnection,
    symbol: &str,
    source: &str,
    records: &[HistoryRecord],
    secondary: Option<Decimal>,
    accept_outlier: bool,
) -> Result<GuardOutcome> {
    let mut ordered: Vec<&HistoryRecord> = records.iter().collect();
    ordered.sort_by(|a, b| a.date.cmp(&b.date));

    let mut outcome = GuardOutcome::default();
    for rec in ordered {
        let prev = get_latest_close_before_backend(backend, symbol, &rec.date)?;
        let decision = evaluate_print(
            rec.close,
            prev.as_ref().map(|(_, c)| *c),
            secondary,
            accept_outlier,
        );
        match decision {
            GuardDecision::Accept => {
                upsert_history_backend(backend, symbol, source, std::slice::from_ref(rec))?;
                outcome.accepted += 1;
            }
            GuardDecision::AcceptCorroborated { secondary: sec } => {
                upsert_history_backend(backend, symbol, source, std::slice::from_ref(rec))?;
                outcome.accepted += 1;
                outcome.corroborated.push(format!(
                    "{} {} confirmed by secondary {}",
                    symbol,
                    fmt_price(rec.close),
                    fmt_price(sec)
                ));
            }
            GuardDecision::AcceptOverride => {
                upsert_history_backend(backend, symbol, source, std::slice::from_ref(rec))?;
                outcome.accepted += 1;
                outcome
                    .overridden
                    .push(format!("{} {} (--accept-outlier)", symbol, fmt_price(rec.close)));
            }
            GuardDecision::Reject { secondary: sec } => {
                let (prev_date, prev_close) =
                    prev.unwrap_or_else(|| ("?".to_string(), Decimal::ZERO));
                let change_pct = if prev_close > Decimal::ZERO {
                    signed_change_pct(rec.close, prev_close)
                } else {
                    Decimal::ZERO
                };
                outcome.rejections.push(RejectedPrint {
                    symbol: symbol.to_string(),
                    date: rec.date.clone(),
                    candidate: rec.close,
                    source: source.to_string(),
                    prev_date,
                    prev_close,
                    change_pct,
                    secondary: sec,
                });
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn backend() -> BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    fn rec(date: &str, close: Decimal) -> HistoryRecord {
        HistoryRecord {
            date: date.to_string(),
            close,
            volume: None,
            open: None,
            high: None,
            low: None,
        }
    }

    fn close_on(backend: &BackendConnection, symbol: &str, date: &str) -> Option<Decimal> {
        let conn = backend.sqlite_native().unwrap();
        conn.query_row(
            "SELECT close FROM price_history WHERE symbol=?1 AND date=?2",
            rusqlite::params![symbol, date],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok())
    }

    // ── evaluate_print ──────────────────────────────────────────────────

    #[test]
    fn accepts_when_no_prior_close() {
        assert_eq!(evaluate_print(dec!(100), None, None, false), GuardDecision::Accept);
        assert_eq!(
            evaluate_print(dec!(100), Some(Decimal::ZERO), None, false),
            GuardDecision::Accept
        );
    }

    #[test]
    fn accepts_within_threshold() {
        // +19.9% — just inside
        assert_eq!(
            evaluate_print(dec!(119.9), Some(dec!(100)), None, false),
            GuardDecision::Accept
        );
        // -20% exactly — boundary is inclusive
        assert_eq!(
            evaluate_print(dec!(80), Some(dec!(100)), None, false),
            GuardDecision::Accept
        );
    }

    #[test]
    fn rejects_suspect_jump_without_secondary() {
        assert_eq!(
            evaluate_print(dec!(125), Some(dec!(100)), None, false),
            GuardDecision::Reject { secondary: None }
        );
        // The original bug shape: stale 77,414 print vs true ~62,000 close
        assert_eq!(
            evaluate_print(dec!(77414), Some(dec!(62064)), None, false),
            GuardDecision::Reject { secondary: None }
        );
    }

    #[test]
    fn corroborated_suspect_jump_is_accepted() {
        // Real crash: -30% d/d, secondary agrees within 5%
        assert_eq!(
            evaluate_print(dec!(70), Some(dec!(100)), Some(dec!(71)), false),
            GuardDecision::AcceptCorroborated { secondary: dec!(71) }
        );
    }

    #[test]
    fn contradicting_secondary_rejects() {
        // Corrupt print +24.7%, secondary says the old level is right
        assert_eq!(
            evaluate_print(dec!(77414), Some(dec!(62064)), Some(dec!(62580)), false),
            GuardDecision::Reject { secondary: Some(dec!(62580)) }
        );
    }

    #[test]
    fn override_accepts_suspect_jump() {
        assert_eq!(
            evaluate_print(dec!(50), Some(dec!(100)), None, true),
            GuardDecision::AcceptOverride
        );
    }

    // ── guarded upsert ──────────────────────────────────────────────────

    #[test]
    fn guarded_upsert_writes_normal_prints() {
        let b = backend();
        let out = upsert_history_guarded_backend(
            &b,
            "BTC-USD",
            "coingecko",
            &[rec("2026-06-05", dec!(62064)), rec("2026-06-06", dec!(62500))],
            None,
            false,
        )
        .unwrap();
        assert_eq!(out.accepted, 2);
        assert!(out.rejections.is_empty());
        assert_eq!(close_on(&b, "BTC-USD", "2026-06-06"), Some(dec!(62500)));
    }

    #[test]
    fn guarded_upsert_rejects_suspect_print_and_writes_nothing() {
        let b = backend();
        upsert_history_guarded_backend(
            &b,
            "BTC-USD",
            "coingecko",
            &[rec("2026-06-05", dec!(62064))],
            None,
            false,
        )
        .unwrap();
        // The corrupt stamp: 6-day-old 77,414 onto a new date
        let out = upsert_history_guarded_backend(
            &b,
            "BTC-USD",
            "cache",
            &[rec("2026-06-11", dec!(77414))],
            None,
            false,
        )
        .unwrap();
        assert_eq!(out.accepted, 0);
        assert_eq!(out.rejections.len(), 1);
        assert_eq!(close_on(&b, "BTC-USD", "2026-06-11"), None);
        let line = out.rejections[0].warning_line();
        assert!(line.contains("price guard"), "{line}");
        assert!(line.contains("77,414"), "{line}");
        assert!(line.contains("rejected"), "{line}");
        assert!(line.contains("d/d"), "{line}");
        assert!(line.contains("--accept-outlier BTC-USD"), "{line}");
    }

    #[test]
    fn guarded_upsert_accepts_with_corroborating_secondary() {
        let b = backend();
        upsert_history_guarded_backend(
            &b,
            "BTC-USD",
            "coingecko",
            &[rec("2026-06-10", dec!(100000))],
            None,
            false,
        )
        .unwrap();
        // Real -35% crash, secondary confirms
        let out = upsert_history_guarded_backend(
            &b,
            "BTC-USD",
            "coingecko",
            &[rec("2026-06-11", dec!(65000))],
            Some(dec!(64500)),
            false,
        )
        .unwrap();
        assert_eq!(out.accepted, 1);
        assert_eq!(out.corroborated.len(), 1);
        assert!(out.rejections.is_empty());
        assert_eq!(close_on(&b, "BTC-USD", "2026-06-11"), Some(dec!(65000)));
    }

    #[test]
    fn guarded_upsert_rejects_with_contradicting_secondary() {
        let b = backend();
        upsert_history_guarded_backend(
            &b,
            "BTC-USD",
            "coingecko",
            &[rec("2026-06-05", dec!(62064))],
            None,
            false,
        )
        .unwrap();
        let out = upsert_history_guarded_backend(
            &b,
            "BTC-USD",
            "cache",
            &[rec("2026-06-11", dec!(77414))],
            Some(dec!(62580)),
            false,
        )
        .unwrap();
        assert_eq!(out.accepted, 0);
        assert_eq!(out.rejections.len(), 1);
        let line = out.rejections[0].warning_line();
        assert!(line.contains("secondary says 62,580"), "{line}");
        assert_eq!(close_on(&b, "BTC-USD", "2026-06-11"), None);
    }

    #[test]
    fn guarded_upsert_override_writes_genuine_gap() {
        let b = backend();
        upsert_history_guarded_backend(
            &b,
            "HALTED",
            "yahoo",
            &[rec("2026-06-10", dec!(100))],
            None,
            false,
        )
        .unwrap();
        let out = upsert_history_guarded_backend(
            &b,
            "HALTED",
            "yahoo",
            &[rec("2026-06-11", dec!(40))],
            None,
            true,
        )
        .unwrap();
        assert_eq!(out.accepted, 1);
        assert_eq!(out.overridden.len(), 1);
        assert_eq!(close_on(&b, "HALTED", "2026-06-11"), Some(dec!(40)));
    }

    #[test]
    fn guarded_upsert_checks_batches_bar_by_bar() {
        let b = backend();
        // A backfill batch carrying one corrupt spike in the middle:
        // 100 → 101 → 130 (corrupt) → 102. The spike is rejected; the
        // following bar is checked against the last ACCEPTED close (101).
        let out = upsert_history_guarded_backend(
            &b,
            "AAPL",
            "yahoo",
            &[
                rec("2026-06-08", dec!(100)),
                rec("2026-06-09", dec!(101)),
                rec("2026-06-10", dec!(130)),
                rec("2026-06-11", dec!(102)),
            ],
            None,
            false,
        )
        .unwrap();
        assert_eq!(out.accepted, 3);
        assert_eq!(out.rejections.len(), 1);
        assert_eq!(out.rejections[0].date, "2026-06-10");
        assert_eq!(close_on(&b, "AAPL", "2026-06-10"), None);
        assert_eq!(close_on(&b, "AAPL", "2026-06-11"), Some(dec!(102)));
    }
}
