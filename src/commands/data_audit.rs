//! `pftui data audit` — DB-wide false-value audit (read-only).
//!
//! Umbrella over per-table signature checks. Each check carries PER-TABLE
//! judgment, because a generic "weird number" detector condemns real
//! history: April-2020 negative oil is REAL (severity info at most),
//! near-zero ^IRX yields are REAL (yield/index symbols are excluded from
//! the order-of-magnitude checks), portfolio flow events are REAL (jumps
//! are flagged info, never condemned).
//!
//! Severity ladder:
//!   info    — real-but-notable; the operator should know, nothing is wrong
//!   suspect — likely false value (placeholder runs, fat-finger returns)
//!   corrupt — provably wrong (range violations, sign inconsistencies,
//!             cross-population collisions)
//!
//! Read-only by design — repair stays manual:
//! `pftui data decontaminate` purges poisoned L2 rows; L1 repair is an
//! operator-reviewed DELETE. Output lists row KEYS only (symbols, dates,
//! indicator names) — never values from the operator's portfolio tables.

use anyhow::Result;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::price_history::DatedClose;

const SAMPLE_CAP: usize = 5;

/// Severity of one finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Suspect,
    Corrupt,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Suspect => "suspect",
            Severity::Corrupt => "corrupt",
        }
    }
}

/// One audit finding: a (table, check) pair that matched rows.
#[derive(Debug, Clone, Serialize)]
pub struct AuditFinding {
    pub table: String,
    pub check: String,
    pub severity: Severity,
    pub count: usize,
    /// Row KEYS only (symbol/date/indicator) — never stored values from
    /// the operator's own tables.
    pub sample_keys: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct AuditReport {
    pub tables_scanned: Vec<String>,
    pub findings: Vec<AuditFinding>,
    pub info: usize,
    pub suspect: usize,
    pub corrupt: usize,
}

impl AuditReport {
    /// Findings that warrant operator attention (suspect + corrupt).
    pub fn attention_count(&self) -> usize {
        self.suspect + self.corrupt
    }

    /// Distinct tables carrying suspect/corrupt findings.
    pub fn attention_tables(&self) -> usize {
        let mut tables: Vec<&str> = self
            .findings
            .iter()
            .filter(|f| f.severity != Severity::Info)
            .map(|f| f.table.as_str())
            .collect();
        tables.sort_unstable();
        tables.dedup();
        tables.len()
    }
}

fn finding(
    table: &str,
    check: &str,
    severity: Severity,
    keys: Vec<String>,
    count: usize,
    detail: &str,
) -> AuditFinding {
    AuditFinding {
        table: table.to_string(),
        check: check.to_string(),
        severity,
        count,
        sample_keys: keys.into_iter().take(SAMPLE_CAP).collect(),
        detail: detail.to_string(),
    }
}

fn table_exists(conn: &rusqlite::Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// Crypto symbols get the wider fat-finger band (±99.9% vs ±95%).
fn is_crypto_symbol(symbol: &str) -> bool {
    let base = symbol.strip_suffix("-USD").unwrap_or(symbol);
    matches!(
        base,
        "BTC" | "ETH" | "SOL" | "XRP" | "ADA" | "DOGE" | "LTC" | "DOT" | "LINK" | "AVAX" | "BNB"
    ) || symbol.ends_with("-USD")
}

fn fat_finger_threshold(symbol: Option<&str>) -> f64 {
    match symbol {
        Some(s) if is_crypto_symbol(s) => 99.9,
        _ => 95.0,
    }
}

// ── price_history ───────────────────────────────────────────────────────────

/// Cross-population bimodality: a series whose closes cluster in two bands
/// separated by >10x — the equity-ticker-collision signature (237 ~$28
/// equity prints inside the ~$60k BTC series). Judgment:
/// - `^`-prefixed symbols (indices, yields) are EXCLUDED: ^IRX legitimately
///   spans near-zero to 5+ across a hiking cycle.
/// - If the two bands partition cleanly in TIME (all low-band rows strictly
///   before all high-band rows or vice versa), it could be a split or
///   redenomination → suspect. Interleaved bands → corrupt (two different
///   instruments are sharing one symbol).
pub fn scan_bimodality(symbol: &str, series: &[DatedClose]) -> Option<AuditFinding> {
    if symbol.starts_with('^') {
        return None;
    }
    let positive: Vec<&DatedClose> = series.iter().filter(|r| r.close > Decimal::ZERO).collect();
    if positive.len() < 20 {
        return None;
    }
    let mut lns: Vec<f64> = positive
        .iter()
        .filter_map(|r| r.close.to_f64())
        .map(f64::ln)
        .collect();
    lns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Largest gap between consecutive sorted ln-closes.
    let mut best_gap = 0.0f64;
    let mut split_at = 0usize;
    for i in 1..lns.len() {
        let gap = lns[i] - lns[i - 1];
        if gap > best_gap {
            best_gap = gap;
            split_at = i;
        }
    }
    let ln10 = std::f64::consts::LN_10;
    if best_gap <= ln10 {
        return None;
    }
    let low_count = split_at;
    let high_count = lns.len() - split_at;
    if low_count < 5 || high_count < 5 {
        return None;
    }

    // Band boundary in price space; classify rows and inspect time order.
    let boundary = ((lns[split_at - 1] + lns[split_at]) / 2.0).exp();
    let boundary_dec = Decimal::from_f64_retain(boundary).unwrap_or(Decimal::ZERO);
    let minority_is_low = low_count <= high_count;
    let minority: Vec<&&DatedClose> = positive
        .iter()
        .filter(|r| (r.close < boundary_dec) == minority_is_low)
        .collect();
    let majority_dates: Vec<&str> = positive
        .iter()
        .filter(|r| (r.close < boundary_dec) != minority_is_low)
        .map(|r| r.date.as_str())
        .collect();
    let interleaved = minority.iter().any(|r| {
        majority_dates.first().is_some_and(|&f| r.date.as_str() > f)
            && majority_dates.last().is_some_and(|&l| r.date.as_str() < l)
    });

    let keys: Vec<String> = minority
        .iter()
        .map(|r| format!("{} {}", symbol, r.date))
        .collect();
    let (severity, shape) = if interleaved {
        (
            Severity::Corrupt,
            "bands interleave in time — two instruments sharing one symbol",
        )
    } else {
        (
            Severity::Suspect,
            "bands partition cleanly in time — possible split/redenomination, review before repair",
        )
    };
    Some(finding(
        "price_history",
        "bimodality",
        severity,
        keys,
        minority.len(),
        &format!(
            "{}: closes cluster in two bands >{:.0}x apart ({} low / {} high rows); {}",
            symbol,
            best_gap.exp(),
            low_count,
            high_count,
            shape
        ),
    ))
}

/// Exact-placeholder runs: >=5 consecutive identical closes (to 4dp) on
/// FX (`=X`) and commodity (`=F`) symbols — the FX 1.0000-placeholder
/// signature. USD/cash rows are exempt (a cash series SHOULD be flat), as
/// is every non-FX/non-commodity symbol (an illiquid equity can
/// legitimately print flat closes; FX/commodities never tick identical to
/// 4dp for a week).
pub fn scan_placeholder_runs(symbol: &str, series: &[DatedClose]) -> Vec<AuditFinding> {
    let fx_or_commodity = symbol.ends_with("=X") || symbol.ends_with("=F");
    let usd_cash = symbol == "USD" || symbol == "USD=X" || symbol.eq_ignore_ascii_case("cash");
    if !fx_or_commodity || usd_cash {
        return Vec::new();
    }
    let mut findings = Vec::new();
    let mut run_start = 0usize;
    let mut i = 1usize;
    let flush = |findings: &mut Vec<AuditFinding>, start: usize, end: usize| {
        let len = end - start + 1;
        if len >= 5 {
            let value = series[start].close.round_dp(4);
            findings.push(finding(
                "price_history",
                "placeholder-run",
                Severity::Suspect,
                vec![
                    format!("{} {}", symbol, series[start].date),
                    format!("{} {}", symbol, series[end].date),
                ],
                len,
                &format!(
                    "{}: {} consecutive identical closes ({}) from {} to {}",
                    symbol, len, value, series[start].date, series[end].date
                ),
            ));
        }
    };
    while i <= series.len() {
        let same = i < series.len()
            && series[i].close.round_dp(4) == series[run_start].close.round_dp(4);
        if !same {
            flush(&mut findings, run_start, i - 1);
            run_start = i;
        }
        i += 1;
    }
    findings
}

/// Nonpositive closes. Judgment: a zero close is garbage anywhere
/// (corrupt); a NEGATIVE close is a real possibility on futures (April
/// 2020 CL=F settled at −$37.63) and on `^` spread/rate symbols → info.
/// Negative equity/crypto/FX closes are corrupt.
pub fn scan_nonpositive_closes(symbol: &str, series: &[DatedClose]) -> Vec<AuditFinding> {
    let negatives: Vec<&DatedClose> =
        series.iter().filter(|r| r.close < Decimal::ZERO).collect();
    let zeros: Vec<&DatedClose> = series.iter().filter(|r| r.close == Decimal::ZERO).collect();
    let mut findings = Vec::new();
    if !negatives.is_empty() {
        let severity = if symbol.ends_with("=F") || symbol.starts_with('^') {
            Severity::Info
        } else {
            Severity::Corrupt
        };
        let detail = if severity == Severity::Info {
            format!(
                "{}: negative close(s) — REAL events exist for futures/rates (Apr-2020 oil); review, do not condemn",
                symbol
            )
        } else {
            format!("{}: negative close(s) on a non-futures symbol", symbol)
        };
        findings.push(finding(
            "price_history",
            "negative-close",
            severity,
            negatives
                .iter()
                .map(|r| format!("{} {}", symbol, r.date))
                .collect(),
            negatives.len(),
            &detail,
        ));
    }
    if !zeros.is_empty() {
        findings.push(finding(
            "price_history",
            "zero-close",
            Severity::Corrupt,
            zeros
                .iter()
                .map(|r| format!("{} {}", symbol, r.date))
                .collect(),
            zeros.len(),
            &format!("{}: zero/unparseable close(s)", symbol),
        ));
    }
    findings
}

fn audit_price_history(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    let mut findings = Vec::new();
    let symbols = crate::db::price_history::get_distinct_symbols(conn)?;
    for symbol in &symbols {
        let series = crate::db::price_history::get_history_with_sources(conn, symbol)?;
        if series.is_empty() {
            continue;
        }
        // Spike-and-revert (reused from `data prices audit`). Judgment:
        // `^`-prefixed symbols (volatility indices, yields) ROUTINELY jump
        // >20% d/d and revert — a VIX spike or a near-zero ^IRX print
        // moving 20% is real market behavior, not corruption → info.
        let spikes = crate::commands::prices::scan_spike_reverts(symbol, &series);
        if !spikes.is_empty() {
            let count = spikes.len();
            let keys = spikes
                .iter()
                .map(|s| format!("{} {}", s.symbol, s.spike_date))
                .collect();
            let (severity, judgment) = if symbol.starts_with('^') {
                (
                    Severity::Info,
                    " (index/rate symbol — large d/d swings are real; review only on other evidence)",
                )
            } else {
                (Severity::Suspect, "")
            };
            findings.push(finding(
                "price_history",
                "spike-revert",
                severity,
                keys,
                count,
                &format!(
                    "{}: bar(s) jumping >20% d/d then reverting >15% next bar{} — detail: pftui data prices audit --symbol {}",
                    symbol, judgment, symbol
                ),
            ));
        }
        if let Some(f) = scan_bimodality(symbol, &series) {
            findings.push(f);
        }
        findings.extend(scan_placeholder_runs(symbol, &series));
        findings.extend(scan_nonpositive_closes(symbol, &series));
    }
    Ok(findings)
}

// ── economic_data ───────────────────────────────────────────────────────────

fn audit_economic_data(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    let mut findings = Vec::new();
    let mut stmt = conn
        .prepare("SELECT indicator, value, quarantined FROM economic_data ORDER BY indicator")?;
    let rows: Vec<(String, String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut unquarantined_bad = Vec::new();
    let mut quarantined = Vec::new();
    for (indicator, value, q) in &rows {
        let passes = value
            .parse::<Decimal>()
            .map(|v| crate::db::economic_data::passes_sanity_check(indicator, v))
            .unwrap_or(false);
        if *q != 0 {
            quarantined.push(indicator.clone());
        } else if !passes {
            unquarantined_bad.push(indicator.clone());
        }
    }
    if !unquarantined_bad.is_empty() {
        let count = unquarantined_bad.len();
        findings.push(finding(
            "economic_data",
            "plausible-range-violation",
            Severity::Corrupt,
            unquarantined_bad,
            count,
            "out-of-band value with quarantined=0 — renders into briefs; the retro-quarantine migration should have swept this (run any pftui command to re-migrate)",
        ));
    }
    if !quarantined.is_empty() {
        let count = quarantined.len();
        findings.push(finding(
            "economic_data",
            "quarantined-rows",
            Severity::Info,
            quarantined,
            count,
            "rows already quarantined — excluded from all readers; awaiting a clean refresh overwrite",
        ));
    }
    Ok(findings)
}

// ── sentiment_history ───────────────────────────────────────────────────────

fn audit_sentiment_history(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    let mut findings = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT index_type, date, value FROM sentiment_history
         WHERE value < 0 OR value > 100 ORDER BY date",
    )?;
    let bad: Vec<String> = stmt
        .query_map([], |row| {
            let it: String = row.get(0)?;
            let date: String = row.get(1)?;
            Ok(format!("{it} {date}"))
        })?
        .filter_map(|r| r.ok())
        .collect();
    if !bad.is_empty() {
        let count = bad.len();
        findings.push(finding(
            "sentiment_history",
            "value-out-of-range",
            Severity::Corrupt,
            bad,
            count,
            "Fear & Greed gauges are defined on 0-100",
        ));
    }

    let mut stmt = conn.prepare(
        "SELECT index_type, date, COUNT(*) FROM sentiment_history
         GROUP BY index_type, date HAVING COUNT(*) > 1",
    )?;
    let dupes: Vec<String> = stmt
        .query_map([], |row| {
            let it: String = row.get(0)?;
            let date: String = row.get(1)?;
            Ok(format!("{it} {date}"))
        })?
        .filter_map(|r| r.ok())
        .collect();
    if !dupes.is_empty() {
        let count = dupes.len();
        findings.push(finding(
            "sentiment_history",
            "duplicate-key",
            Severity::Corrupt,
            dupes,
            count,
            "duplicate (index_type, date) — the PK should make this impossible",
        ));
    }
    Ok(findings)
}

// ── cot_cache ───────────────────────────────────────────────────────────────

fn audit_cot_cache(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    // Schema note: cot_cache stores raw contract counts (no percentile
    // columns — percentiles are computed at read time), so the percentile
    // 0-100 check has nothing to bind to; we check the internal sign/
    // arithmetic invariants instead.
    let mut findings = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT cftc_code, report_date FROM cot_cache
         WHERE managed_money_long < 0 OR managed_money_short < 0
            OR commercial_long < 0 OR commercial_short < 0
            OR open_interest < 0
         ORDER BY report_date",
    )?;
    let negative: Vec<String> = stmt
        .query_map([], |row| {
            let code: String = row.get(0)?;
            let date: String = row.get(1)?;
            Ok(format!("{code} {date}"))
        })?
        .filter_map(|r| r.ok())
        .collect();
    if !negative.is_empty() {
        let count = negative.len();
        findings.push(finding(
            "cot_cache",
            "negative-position-count",
            Severity::Corrupt,
            negative,
            count,
            "long/short/open-interest contract counts cannot be negative",
        ));
    }

    let mut stmt = conn.prepare(
        "SELECT cftc_code, report_date FROM cot_cache
         WHERE managed_money_net != managed_money_long - managed_money_short
            OR commercial_net != commercial_long - commercial_short
         ORDER BY report_date",
    )?;
    let inconsistent: Vec<String> = stmt
        .query_map([], |row| {
            let code: String = row.get(0)?;
            let date: String = row.get(1)?;
            Ok(format!("{code} {date}"))
        })?
        .filter_map(|r| r.ok())
        .collect();
    if !inconsistent.is_empty() {
        let count = inconsistent.len();
        findings.push(finding(
            "cot_cache",
            "net-arithmetic-mismatch",
            Severity::Corrupt,
            inconsistent,
            count,
            "net != long - short — the row was not written from one consistent report",
        ));
    }
    Ok(findings)
}

// ── onchain_cache (incl. etf_flow_* metrics) ────────────────────────────────

fn audit_onchain_cache(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    let mut findings = Vec::new();
    let mut stmt = conn
        .prepare("SELECT metric, date, value FROM onchain_cache ORDER BY metric, date")?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut i = 0usize;
    while i < rows.len() {
        let metric = &rows[i].0;
        let mut j = i;
        while j < rows.len() && &rows[j].0 == metric {
            j += 1;
        }
        // All-zero run >= 5 consecutive dates within one metric.
        let mut run_start: Option<usize> = None;
        for k in i..=j {
            let is_zero = k < j
                && rows[k]
                    .2
                    .parse::<Decimal>()
                    .map(|v| v == Decimal::ZERO)
                    .unwrap_or(false);
            match (is_zero, run_start) {
                (true, None) => run_start = Some(k),
                (false, Some(start)) => {
                    let len = k - start;
                    if len >= 5 {
                        // Judgment: zero-flow days are REAL for minor ETF
                        // funds (info); a flat-zero on-chain metric (fees,
                        // reserves) is a feed failure (suspect).
                        let severity = if metric.starts_with("etf_flow_") {
                            Severity::Info
                        } else {
                            Severity::Suspect
                        };
                        findings.push(finding(
                            "onchain_cache",
                            "all-zero-run",
                            severity,
                            vec![
                                format!("{} {}", metric, rows[start].1),
                                format!("{} {}", metric, rows[k - 1].1),
                            ],
                            len,
                            &format!(
                                "{}: {} consecutive zero values ({}..{}){}",
                                metric,
                                len,
                                rows[start].1,
                                rows[k - 1].1,
                                if metric.starts_with("etf_flow_") {
                                    " — zero-flow runs are real for small funds"
                                } else {
                                    ""
                                }
                            ),
                        ));
                    }
                    run_start = None;
                }
                _ => {}
            }
        }
        i = j;
    }
    Ok(findings)
}

// ── L3 ledgers: forecast_scores / signal_expectancy / recommendations ──────

fn audit_forecast_scores(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    if !table_exists(conn, "forecast_scores") {
        return Ok(Vec::new());
    }
    // Superseded rows are retired ledger history (verification reissue) —
    // only the active corpus is audited.
    let mut stmt = conn.prepare(
        "SELECT analyst, asset, view_date, horizon_days, realized_pct
         FROM forecast_scores
         WHERE realized_pct IS NOT NULL AND status != 'superseded'",
    )?;
    let rows: Vec<(String, String, String, i64, f64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    let bad: Vec<String> = rows
        .iter()
        .filter(|(_, asset, _, _, pct)| pct.abs() > fat_finger_threshold(Some(asset)))
        .map(|(analyst, asset, date, horizon, _)| {
            format!("{analyst} {asset} {date} h{horizon}")
        })
        .collect();
    if bad.is_empty() {
        return Ok(Vec::new());
    }
    let count = bad.len();
    Ok(vec![finding(
        "forecast_scores",
        "implausible-realized-return",
        Severity::Suspect,
        bad,
        count,
        "realized_pct outside ±95% (±99.9% crypto) — fat-finger detection in our own scoring ledger",
    )])
}

fn audit_signal_expectancy(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    if !table_exists(conn, "signal_expectancy") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT signal_id, asset, horizon_days, as_of, mean_pct, median_pct
         FROM signal_expectancy
         WHERE mean_pct IS NOT NULL OR median_pct IS NOT NULL",
    )?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(String, String, i64, String, Option<f64>, Option<f64>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    let bad: Vec<String> = rows
        .iter()
        .filter(|(_, asset, _, _, mean, median)| {
            let th = fat_finger_threshold(Some(asset));
            mean.is_some_and(|v| v.abs() > th) || median.is_some_and(|v| v.abs() > th)
        })
        .map(|(sig, asset, horizon, as_of, _, _)| format!("{sig} {asset} h{horizon} {as_of}"))
        .collect();
    if bad.is_empty() {
        return Ok(Vec::new());
    }
    let count = bad.len();
    Ok(vec![finding(
        "signal_expectancy",
        "implausible-expectancy",
        Severity::Suspect,
        bad,
        count,
        "mean/median forward return outside ±95% (±99.9% crypto) — recompute via pftui research backtest",
    )])
}

fn audit_recommendations(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    if !table_exists(conn, "recommendations") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, report_date, asset, fwd_30d_pct, fwd_90d_pct, fwd_180d_pct
         FROM recommendations
         WHERE fwd_30d_pct IS NOT NULL OR fwd_90d_pct IS NOT NULL OR fwd_180d_pct IS NOT NULL",
    )?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        i64,
        String,
        Option<String>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    let bad: Vec<String> = rows
        .iter()
        .filter(|(_, _, asset, f30, f90, f180)| {
            let th = fat_finger_threshold(asset.as_deref());
            [f30, f90, f180]
                .iter()
                .any(|v| v.is_some_and(|x| x.abs() > th))
        })
        .map(|(id, date, asset, _, _, _)| {
            format!("#{id} {date} {}", asset.as_deref().unwrap_or("-"))
        })
        .collect();
    if bad.is_empty() {
        return Ok(Vec::new());
    }
    let count = bad.len();
    Ok(vec![finding(
        "recommendations",
        "implausible-forward-return",
        Severity::Suspect,
        bad,
        count,
        "scored forward return outside ±95% (±99.9% crypto) — fat-finger detection in our own ledger",
    )])
}

// ── portfolio_snapshots ─────────────────────────────────────────────────────

/// Day-over-day total_value jumps >30% between consecutive snapshots.
/// Judgment: flow events (deposits/withdrawals) are REAL — severity info,
/// flag-don't-condemn. The 7 operator-backfilled historical rows carry
/// cash_value=0 deliberately (cash/invested split unrecorded — journal
/// note #728); comparisons touching that pattern are excluded. Output
/// carries DATES ONLY — never the values.
fn audit_portfolio_snapshots(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    let mut stmt = conn.prepare(
        "SELECT date, total_value, cash_value FROM portfolio_snapshots ORDER BY date",
    )?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut jumps = Vec::new();
    for pair in rows.windows(2) {
        let (prev_date, prev_total, prev_cash) = &pair[0];
        let (date, total, cash) = &pair[1];
        // Known backfill pattern (journal note #728): cash_value=0 rows are
        // deliberate operator backfill with unrecorded cash split — skip.
        let is_backfill = |c: &str| c.parse::<Decimal>().map(|v| v == Decimal::ZERO).unwrap_or(false);
        if is_backfill(prev_cash) || is_backfill(cash) {
            continue;
        }
        let (Ok(prev_v), Ok(v)) = (prev_total.parse::<Decimal>(), total.parse::<Decimal>())
        else {
            continue;
        };
        if prev_v <= Decimal::ZERO {
            continue;
        }
        let change_pct = ((v - prev_v) / prev_v * Decimal::from(100)).abs();
        if change_pct > Decimal::from(30) {
            jumps.push(format!("{prev_date} -> {date}"));
        }
    }
    if jumps.is_empty() {
        return Ok(Vec::new());
    }
    let count = jumps.len();
    Ok(vec![finding(
        "portfolio_snapshots",
        "large-day-over-day-jump",
        Severity::Info,
        jumps,
        count,
        ">30% total_value move between consecutive snapshots — flow events are real; review only if no deposit/withdrawal matches (dates only; values never printed)",
    )])
}

// ── scenario_history ────────────────────────────────────────────────────────

/// The scenario probability LEDGER DISCIPLINE landed on this date
/// (docs/EPISTEMICS.md §3: evidence requirement, 5pp/day delta caps,
/// conflict guard). Wild book sums and uncapped jumps BEFORE it are
/// expected historical findings (info); violations AFTER it mean a writer
/// is bypassing the ledger (suspect).
const SCENARIO_LEDGER_DATE: &str = "2026-06-10";

/// Active-scenario probability book sums outside [60, 110] per recorded
/// date, and single-scenario moves >15pp between consecutive records.
/// Probabilities are on the 0-100 scale. The book sum is "as-of": each
/// active scenario contributes its latest recorded probability on/before
/// the date. Historical active-status isn't tracked, so currently-resolved
/// scenarios are excluded from the sum (noted in the detail).
pub fn audit_scenario_history(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    let mut stmt = conn.prepare(
        "SELECT h.scenario_id, substr(h.recorded_at, 1, 10), h.probability
         FROM scenario_history h
         JOIN scenarios s ON s.id = h.scenario_id
         WHERE s.status != 'resolved'
         ORDER BY h.recorded_at ASC, h.id ASC",
    )?;
    let rows: Vec<(i64, String, f64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut findings = Vec::new();

    // Book-sum check: apply each date's updates, then sum the latest known
    // probability per active scenario at end of that date.
    let mut latest: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    let mut sum_pre: Vec<String> = Vec::new();
    let mut sum_post: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < rows.len() {
        let date = rows[i].1.clone();
        while i < rows.len() && rows[i].1 == date {
            latest.insert(rows[i].0, rows[i].2);
            i += 1;
        }
        let sum: f64 = latest.values().sum();
        if !(60.0..=110.0).contains(&sum) {
            let key = format!("{date} sum={sum:.1}");
            if date.as_str() < SCENARIO_LEDGER_DATE {
                sum_pre.push(key);
            } else {
                sum_post.push(key);
            }
        }
    }
    if !sum_pre.is_empty() {
        let count = sum_pre.len();
        findings.push(finding(
            "scenario_history",
            "book-sum-out-of-band",
            Severity::Info,
            sum_pre,
            count,
            &format!(
                "active-scenario probability book summed outside [60, 110] BEFORE the {SCENARIO_LEDGER_DATE} ledger discipline — expected historical finding (uncoordinated writers; currently-resolved scenarios excluded from the sum)"
            ),
        ));
    }
    if !sum_post.is_empty() {
        let count = sum_post.len();
        findings.push(finding(
            "scenario_history",
            "book-sum-out-of-band",
            Severity::Suspect,
            sum_post,
            count,
            &format!(
                "active-scenario probability book summed outside [60, 110] ON/AFTER the {SCENARIO_LEDGER_DATE} ledger discipline — a writer is bypassing the scenario ledger"
            ),
        ));
    }

    // Jump check: >15pp between consecutive records of the same scenario
    // (all scenarios — the delta caps apply to every ledgered update).
    let mut stmt = conn.prepare(
        "SELECT scenario_id, substr(recorded_at, 1, 10), probability
         FROM scenario_history
         ORDER BY scenario_id ASC, recorded_at ASC, id ASC",
    )?;
    let all_rows: Vec<(i64, String, f64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();
    let mut jump_pre: Vec<String> = Vec::new();
    let mut jump_post: Vec<String> = Vec::new();
    for pair in all_rows.windows(2) {
        let (prev_id, prev_date, prev_p) = &pair[0];
        let (id, date, p) = &pair[1];
        if prev_id != id {
            continue;
        }
        let delta = p - prev_p;
        if delta.abs() > 15.0 {
            let key = format!("scenario#{id} {prev_date} -> {date} ({delta:+.1}pp)");
            if date.as_str() < SCENARIO_LEDGER_DATE {
                jump_pre.push(key);
            } else {
                jump_post.push(key);
            }
        }
    }
    if !jump_pre.is_empty() {
        let count = jump_pre.len();
        findings.push(finding(
            "scenario_history",
            "single-move-gt-15pp",
            Severity::Info,
            jump_pre,
            count,
            ">15pp single-scenario move between consecutive records, pre-ledger — expected historical finding",
        ));
    }
    if !jump_post.is_empty() {
        let count = jump_post.len();
        findings.push(finding(
            "scenario_history",
            "single-move-gt-15pp",
            Severity::Suspect,
            jump_post,
            count,
            ">15pp single-scenario move between consecutive records ON/AFTER the ledger discipline — should have been capped at 5pp/day (--hard-print escape hatch is ledgered, not a bypass)",
        ));
    }
    Ok(findings)
}

// ── transactions ────────────────────────────────────────────────────────────

/// Symbols exempt from the price-vs-history comparison: cash legs SHOULD
/// price at 1.0 and have no market session to compare against.
fn is_cash_symbol(symbol: &str) -> bool {
    symbol.eq_ignore_ascii_case("USD")
        || symbol.eq_ignore_ascii_case("USD=X")
        || symbol.eq_ignore_ascii_case("CASH")
}

/// Operator-entered transaction sanity. Severity is always SUSPECT —
/// transactions are hand-entered; the audit reports, it never auto-fixes.
///
/// PRIVACY: output carries row ids + symbol + date + percent-deviation
/// ONLY. Quantities, prices, and values from the operator's real positions
/// are NEVER printed.
///
/// Checks:
/// - `price-vs-session-range`: buy/sell `price_per` outside ±15% of the
///   symbol's close on the transaction date (or the nearest session within
///   5 days — `price_history` stores closes only, so the close±15% band IS
///   the day_low*0.85..day_high*1.15 fallback). Cash/USD rows and rows in
///   a non-USD currency are exempt; symbols with no nearby session are
///   skipped (unverifiable ≠ wrong).
/// - `nonpositive-quantity`: buy/sell rows store POSITIVE quantities by
///   convention (the writer rejects ≤0); a nonpositive quantity is a
///   hand-edit or import error.
/// - `orphaned-paired-tx`: `paired_tx_id` referencing a missing row.
pub fn audit_transactions(conn: &rusqlite::Connection) -> Result<Vec<AuditFinding>> {
    let mut findings = Vec::new();

    struct TxRow {
        id: i64,
        symbol: String,
        category: String,
        tx_type: String,
        quantity: String,
        price_per: String,
        currency: String,
        date: String,
        paired: Option<i64>,
    }
    let rows: Vec<TxRow> = {
        let mut stmt = conn.prepare(
            "SELECT id, symbol, category, tx_type, quantity, price_per, currency,
                    substr(date, 1, 10), paired_tx_id
             FROM transactions ORDER BY date ASC, id ASC",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok(TxRow {
                id: row.get(0)?,
                symbol: row.get(1)?,
                category: row.get(2)?,
                tx_type: row.get(3)?,
                quantity: row.get(4)?,
                price_per: row.get(5)?,
                currency: row.get(6)?,
                date: row.get(7)?,
                paired: row.get(8)?,
            })
        })?;
        mapped.filter_map(|r| r.ok()).collect()
    };
    let ids: std::collections::HashSet<i64> = rows.iter().map(|r| r.id).collect();

    let mut out_of_band: Vec<String> = Vec::new();
    let mut bad_quantity: Vec<String> = Vec::new();
    let mut orphaned: Vec<String> = Vec::new();

    for TxRow {
        id,
        symbol,
        category,
        tx_type,
        quantity,
        price_per,
        currency,
        date,
        paired,
    } in &rows
    {
        let is_trade = tx_type == "buy" || tx_type == "sell";

        // Orphaned pair reference (any tx_type).
        if let Some(p) = paired {
            if !ids.contains(p) {
                orphaned.push(format!("#{id} {symbol} {date} -> missing #{p}"));
            }
        }
        if !is_trade {
            continue;
        }

        // Quantity sign consistency vs tx_type (positive-by-convention).
        if quantity
            .parse::<Decimal>()
            .map(|q| q <= Decimal::ZERO)
            .unwrap_or(true)
        {
            bad_quantity.push(format!("#{id} {symbol} {date} ({tx_type})"));
        }

        // Price vs the symbol's session range that day (or nearest session).
        if is_cash_symbol(symbol) || category.eq_ignore_ascii_case("cash") {
            continue; // cash legs are exempt
        }
        if !currency.eq_ignore_ascii_case("USD") {
            continue; // closes are USD — a cross-currency band would lie
        }
        let Ok(price) = price_per.parse::<Decimal>() else {
            continue; // unparseable price is caught nowhere else, but keys
                      // without a deviation would be noise; skip
        };
        let Some(close) = nearest_close(conn, symbol, date, 5)? else {
            continue; // no nearby session — unverifiable, not wrong
        };
        if close <= Decimal::ZERO {
            continue;
        }
        let low = close * Decimal::new(85, 2);
        let high = close * Decimal::new(115, 2);
        if price < low || price > high {
            let dev = ((price - close) / close * Decimal::from(100))
                .to_f64()
                .unwrap_or(0.0);
            out_of_band.push(format!("#{id} {symbol} {date} dev{dev:+.1}%"));
        }
    }

    if !out_of_band.is_empty() {
        let count = out_of_band.len();
        findings.push(finding(
            "transactions",
            "price-vs-session-range",
            Severity::Suspect,
            out_of_band,
            count,
            "possible entry error: fill price >15% from the nearest session close (deviation only — quantities/values never printed); operator-entered, review by hand",
        ));
    }
    if !bad_quantity.is_empty() {
        let count = bad_quantity.len();
        findings.push(finding(
            "transactions",
            "nonpositive-quantity",
            Severity::Suspect,
            bad_quantity,
            count,
            "buy/sell rows store positive quantities by convention (direction lives in tx_type) — nonpositive quantity is a hand-edit or import error",
        ));
    }
    if !orphaned.is_empty() {
        let count = orphaned.len();
        findings.push(finding(
            "transactions",
            "orphaned-paired-tx",
            Severity::Suspect,
            orphaned,
            count,
            "paired_tx_id references a transaction row that no longer exists — the cash leg and asset leg have come apart",
        ));
    }
    Ok(findings)
}

/// The symbol's close ON `date`, else the nearest session within
/// `window_days` either side (ties go to the earlier/prior session).
fn nearest_close(
    conn: &rusqlite::Connection,
    symbol: &str,
    date: &str,
    window_days: i64,
) -> Result<Option<Decimal>> {
    let mut stmt = conn.prepare_cached(
        "SELECT close FROM price_history
         WHERE symbol = ?1
           AND date BETWEEN date(?2, ?3) AND date(?2, ?4)
         ORDER BY abs(julianday(date) - julianday(?2)) ASC, date ASC
         LIMIT 1",
    )?;
    let close: Option<String> = stmt
        .query_row(
            rusqlite::params![
                symbol,
                date,
                format!("-{window_days} days"),
                format!("+{window_days} days")
            ],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|e| {
            if e == rusqlite::Error::QueryReturnedNoRows {
                Ok(None)
            } else {
                Err(e)
            }
        })?;
    Ok(close.and_then(|c| std::str::FromStr::from_str(&c).ok()))
}

// ── runner ──────────────────────────────────────────────────────────────────

type CheckFn = fn(&rusqlite::Connection) -> Result<Vec<AuditFinding>>;

const CHECKS: &[(&str, CheckFn)] = &[
    ("price_history", audit_price_history),
    ("economic_data", audit_economic_data),
    ("sentiment_history", audit_sentiment_history),
    ("cot_cache", audit_cot_cache),
    ("onchain_cache", audit_onchain_cache),
    ("forecast_scores", audit_forecast_scores),
    ("signal_expectancy", audit_signal_expectancy),
    ("recommendations", audit_recommendations),
    ("portfolio_snapshots", audit_portfolio_snapshots),
    ("scenario_history", audit_scenario_history),
    ("transactions", audit_transactions),
];

/// Run all (or one table's) checks. Read-only.
pub fn run_checks(
    conn: &rusqlite::Connection,
    table: Option<&str>,
) -> Result<AuditReport> {
    if let Some(t) = table {
        if !CHECKS.iter().any(|(name, _)| *name == t) {
            let known: Vec<&str> = CHECKS.iter().map(|(n, _)| *n).collect();
            anyhow::bail!(
                "unknown audit table '{t}' — audited tables: {}",
                known.join(", ")
            );
        }
    }
    let mut findings = Vec::new();
    let mut tables_scanned = Vec::new();
    for (name, check) in CHECKS {
        if table.is_some_and(|t| t != *name) {
            continue;
        }
        if !table_exists(conn, name) {
            continue;
        }
        tables_scanned.push(name.to_string());
        findings.extend(check(conn)?);
    }
    let info = findings.iter().filter(|f| f.severity == Severity::Info).count();
    let suspect = findings
        .iter()
        .filter(|f| f.severity == Severity::Suspect)
        .count();
    let corrupt = findings
        .iter()
        .filter(|f| f.severity == Severity::Corrupt)
        .count();
    Ok(AuditReport {
        tables_scanned,
        findings,
        info,
        suspect,
        corrupt,
    })
}

/// One-line summary for `pftui system doctor`.
pub fn doctor_summary(conn: &rusqlite::Connection) -> Result<(bool, String)> {
    let report = run_checks(conn, None)?;
    let attention = report.attention_count();
    if attention == 0 {
        Ok((
            true,
            format!(
                "Data audit: no suspect findings across {} tables ({} info)",
                report.tables_scanned.len(),
                report.info
            ),
        ))
    } else {
        Ok((
            false,
            format!(
                "Data audit: {} suspect findings across {} tables — pftui data audit",
                attention,
                report.attention_tables()
            ),
        ))
    }
}

/// `pftui data audit [--table X] [--json]`
pub fn run(backend: &BackendConnection, table: Option<&str>, json: bool) -> Result<()> {
    let Some(conn) = backend.sqlite_native() else {
        anyhow::bail!("data audit currently supports the SQLite backend only");
    };
    let report = run_checks(conn, table)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!(
        "DB-wide false-value audit — {} table(s) scanned (read-only; repair is manual: pftui data decontaminate / operator-reviewed DELETE)",
        report.tables_scanned.len()
    );
    if report.findings.is_empty() {
        println!("✓ No findings.");
        return Ok(());
    }
    println!(
        "{} finding(s): {} corrupt, {} suspect, {} info",
        report.findings.len(),
        report.corrupt,
        report.suspect,
        report.info
    );
    println!();
    println!(
        "  {:<20} {:<28} {:<8} {:>6}  SAMPLE KEYS",
        "TABLE", "CHECK", "SEV", "ROWS"
    );
    for f in &report.findings {
        println!(
            "  {:<20} {:<28} {:<8} {:>6}  {}",
            f.table,
            f.check,
            f.severity.label(),
            f.count,
            f.sample_keys.join(", ")
        );
        println!("  {:<20} {}", "", f.detail);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    fn dc(date: &str, close: Decimal) -> DatedClose {
        DatedClose {
            date: date.to_string(),
            close,
            source: "test".to_string(),
        }
    }

    // ── bimodality ─────────────────────────────────────────────────────

    #[test]
    fn bimodality_flags_synthetic_two_band_series() {
        // The BTC equity-collision shape: ~$60k closes with interleaved
        // ~$28 equity prints.
        let mut series = Vec::new();
        for day in 1..=30 {
            series.push(dc(
                &format!("2026-05-{day:02}"),
                dec!(60000) + Decimal::from(day * 10),
            ));
        }
        for day in [3, 7, 11, 15, 19, 23] {
            series.push(dc(&format!("2026-05-{day:02}"), dec!(28.4)));
        }
        series.sort_by(|a, b| a.date.cmp(&b.date));
        let f = scan_bimodality("BTC", &series).expect("finding");
        assert_eq!(f.severity, Severity::Corrupt, "{}", f.detail);
        assert_eq!(f.count, 6);
        assert!(f.detail.contains("interleave"), "{}", f.detail);
    }

    #[test]
    fn bimodality_ignores_trending_series() {
        // 10 -> 200 over 60 bars: a real 20x trend is continuous in log
        // space — no gap.
        let mut series = Vec::new();
        let mut price = 10.0f64;
        for day in 0..60 {
            series.push(dc(
                &format!("2026-{:02}-{:02}", 1 + day / 28, 1 + day % 28),
                Decimal::from_f64_retain(price).unwrap(),
            ));
            price *= 1.052;
        }
        assert!(scan_bimodality("GROW", &series).is_none());
    }

    #[test]
    fn bimodality_skips_rate_symbols() {
        // ^IRX-like: near-zero yields then 5+ after a hiking cycle. Real.
        let mut series = Vec::new();
        for day in 1..=15 {
            series.push(dc(&format!("2021-06-{day:02}"), dec!(0.03)));
        }
        for day in 1..=15 {
            series.push(dc(&format!("2023-06-{day:02}"), dec!(5.3)));
        }
        assert!(scan_bimodality("^IRX", &series).is_none());
    }

    #[test]
    fn bimodality_clean_time_partition_is_suspect_not_corrupt() {
        // All low rows strictly before all high rows: split/redenomination
        // shape — review, don't condemn.
        let mut series = Vec::new();
        for day in 1..=10 {
            series.push(dc(&format!("2026-01-{day:02}"), dec!(2)));
        }
        for day in 1..=10 {
            series.push(dc(&format!("2026-02-{day:02}"), dec!(40)));
        }
        let f = scan_bimodality("SPLIT", &series).expect("finding");
        assert_eq!(f.severity, Severity::Suspect, "{}", f.detail);
    }

    // ── placeholder runs ───────────────────────────────────────────────

    #[test]
    fn placeholder_run_flags_fx_series() {
        let mut series = Vec::new();
        for day in 1..=7 {
            series.push(dc(&format!("2026-06-{day:02}"), dec!(1.0000)));
        }
        series.push(dc("2026-06-08", dec!(157.32)));
        let findings = scan_placeholder_runs("JPY=X", &series);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].count, 7);
        assert_eq!(findings[0].severity, Severity::Suspect);
    }

    #[test]
    fn placeholder_run_exempts_usd_cash_row() {
        let mut series = Vec::new();
        for day in 1..=10 {
            series.push(dc(&format!("2026-06-{day:02}"), dec!(1.0000)));
        }
        assert!(scan_placeholder_runs("USD", &series).is_empty());
        assert!(scan_placeholder_runs("USD=X", &series).is_empty());
        // Equities are out of scope for this check entirely.
        assert!(scan_placeholder_runs("AAPL", &series).is_empty());
    }

    #[test]
    fn placeholder_run_requires_five_consecutive() {
        let series = vec![
            dc("2026-06-01", dec!(1.0850)),
            dc("2026-06-02", dec!(1.0850)),
            dc("2026-06-03", dec!(1.0850)),
            dc("2026-06-04", dec!(1.0850)),
            dc("2026-06-05", dec!(1.0851)),
        ];
        assert!(scan_placeholder_runs("EUR=X", &series).is_empty());
    }

    // ── negative oil regression ────────────────────────────────────────

    #[test]
    fn april_2020_negative_oil_is_not_condemned() {
        // CL=F around 2020-04-20: 18.27 -> -37.63 -> 10.01. Real history.
        let series = vec![
            dc("2020-04-17", dec!(18.27)),
            dc("2020-04-20", dec!(-37.63)),
            dc("2020-04-21", dec!(10.01)),
            dc("2020-04-22", dec!(13.78)),
        ];
        let findings = scan_nonpositive_closes("CL=F", &series);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].severity,
            Severity::Info,
            "negative futures close must be info at most: {}",
            findings[0].detail
        );
        // And the spike-revert scanner skips nonpositive bars by design.
        assert!(crate::commands::prices::scan_spike_reverts("CL=F", &series).is_empty());
        // Bimodality must not condemn it either (negative closes excluded).
        assert!(scan_bimodality("CL=F", &series).is_none());
    }

    #[test]
    fn index_symbol_spike_reverts_are_info_not_suspect() {
        // ^VIX/^IRX-shape: >20% d/d jump reverting next bar is REAL market
        // behavior for volatility indices and near-zero yields.
        let conn = open_in_memory();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close, source) VALUES
                ('^VIX','2026-06-01','20','yahoo'),
                ('^VIX','2026-06-02','28','yahoo'),
                ('^VIX','2026-06-03','21','yahoo'),
                ('AAPL','2026-06-01','100','yahoo'),
                ('AAPL','2026-06-02','130','yahoo'),
                ('AAPL','2026-06-03','102','yahoo');",
        )
        .expect("seed");
        let report = run_checks(&conn, Some("price_history")).expect("run");
        let vix = report
            .findings
            .iter()
            .find(|f| f.check == "spike-revert" && f.detail.starts_with("^VIX"))
            .expect("vix finding");
        assert_eq!(vix.severity, Severity::Info, "{}", vix.detail);
        let aapl = report
            .findings
            .iter()
            .find(|f| f.check == "spike-revert" && f.detail.starts_with("AAPL"))
            .expect("aapl finding");
        assert_eq!(aapl.severity, Severity::Suspect, "{}", aapl.detail);
    }

    #[test]
    fn negative_close_on_equity_is_corrupt() {
        let series = vec![dc("2026-06-01", dec!(-5))];
        let findings = scan_nonpositive_closes("AAPL", &series);
        assert_eq!(findings[0].severity, Severity::Corrupt);
    }

    // ── full runner + JSON shape ───────────────────────────────────────

    #[test]
    fn audit_json_shape() {
        let conn = open_in_memory();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close, source) VALUES
                ('JPY=X','2026-06-01','1.0000','yahoo'),
                ('JPY=X','2026-06-02','1.0000','yahoo'),
                ('JPY=X','2026-06-03','1.0000','yahoo'),
                ('JPY=X','2026-06-04','1.0000','yahoo'),
                ('JPY=X','2026-06-05','1.0000','yahoo');
             INSERT INTO sentiment_history (index_type, date, value, classification)
             VALUES ('crypto','2026-06-01',230,'broken');
             INSERT INTO cot_cache (cftc_code, report_date, open_interest,
                managed_money_long, managed_money_short, managed_money_net,
                commercial_long, commercial_short, commercial_net)
             VALUES ('088691','2026-06-03',1000, 500, 200, 999, 300, 100, 200);",
        )
        .expect("seed");

        let report = run_checks(&conn, None).expect("run");
        assert!(report.tables_scanned.len() >= 5);
        assert!(report.suspect >= 1, "placeholder run should be suspect");
        assert!(report.corrupt >= 2, "sentiment + cot rows are corrupt");

        let json = serde_json::to_value(&report).expect("json");
        assert!(json.get("tables_scanned").is_some());
        assert!(json.get("findings").is_some());
        for key in ["info", "suspect", "corrupt"] {
            assert!(json.get(key).is_some(), "missing summary key {key}");
        }
        let first = &json["findings"][0];
        for key in ["table", "check", "severity", "count", "sample_keys", "detail"] {
            assert!(first.get(key).is_some(), "missing finding key {key}");
        }
    }

    #[test]
    fn table_filter_limits_scope_and_rejects_unknown() {
        let conn = open_in_memory();
        let report = run_checks(&conn, Some("price_history")).expect("run");
        assert_eq!(report.tables_scanned, vec!["price_history".to_string()]);
        assert!(run_checks(&conn, Some("not_a_table")).is_err());
    }

    #[test]
    fn portfolio_snapshot_jump_is_info_and_backfill_rows_excluded() {
        let conn = open_in_memory();
        // Synthetic demo values only. Two backfill rows (cash_value=0,
        // journal note #728 pattern) with a huge jump between them must NOT
        // flag; a real >30% jump between native rows flags as info.
        conn.execute_batch(
            "INSERT INTO portfolio_snapshots (date, total_value, cash_value, invested_value) VALUES
                ('2025-11-01','100000','0','100000'),
                ('2025-12-01','250000','0','250000'),
                ('2026-06-01','100000','5000','95000'),
                ('2026-06-02','140000','5000','135000'),
                ('2026-06-03','141000','5000','136000');",
        )
        .expect("seed");
        let report = run_checks(&conn, Some("portfolio_snapshots")).expect("run");
        assert_eq!(report.findings.len(), 1);
        let f = &report.findings[0];
        assert_eq!(f.severity, Severity::Info);
        assert_eq!(f.count, 1);
        assert_eq!(f.sample_keys, vec!["2026-06-01 -> 2026-06-02".to_string()]);
        // Keys only — no values in output.
        assert!(!f.detail.contains("140000"));
    }

    #[test]
    fn economic_data_unquarantined_violation_is_corrupt() {
        let conn = open_in_memory();
        conn.execute(
            "INSERT INTO economic_data (indicator, value, source_url, source, confidence, fetched_at, quarantined)
             VALUES ('nfp', '2026', 'https://example.invalid', 'test', 'medium', '2026-06-01T00:00:00Z', 0)",
            [],
        )
        .expect("seed");
        let report = run_checks(&conn, Some("economic_data")).expect("run");
        assert_eq!(report.corrupt, 1);
        assert!(report.findings[0].sample_keys.contains(&"nfp".to_string()));
    }

    // ── scenario_history book sums + jumps ─────────────────────────────

    #[test]
    fn scenario_book_sum_severity_splits_at_ledger_date() {
        let conn = open_in_memory();
        conn.execute_batch(
            "INSERT INTO scenarios (id, name, probability, status) VALUES
                (9101, 'audit-A', 25.0, 'active'),
                (9102, 'audit-B', 90.0, 'active'),
                (9103, 'audit-D', 99.0, 'resolved');
             INSERT INTO scenario_history (scenario_id, probability, recorded_at) VALUES
                (9101, 30.0, '2026-05-01 09:00:00'),   -- sum 30  (<60, pre  → info)
                (9102, 90.0, '2026-05-02 09:00:00'),   -- sum 120 (>110, pre → info)
                (9103, 99.0, '2026-05-02 10:00:00'),   -- resolved: excluded from sums
                (9101, 20.0, '2026-05-03 09:00:00'),   -- sum 110 (boundary → clean)
                (9101, 25.0, '2026-06-15 09:00:00');   -- sum 115 (>110, post → suspect)",
        )
        .expect("seed");
        let report = run_checks(&conn, Some("scenario_history")).expect("run");
        let sums: Vec<&AuditFinding> = report
            .findings
            .iter()
            .filter(|f| f.check == "book-sum-out-of-band")
            .collect();
        assert_eq!(sums.len(), 2, "one pre-ledger info + one post-ledger suspect");
        let pre = sums.iter().find(|f| f.severity == Severity::Info).expect("pre");
        assert_eq!(pre.count, 2);
        assert!(pre.sample_keys.iter().any(|k| k.starts_with("2026-05-01")));
        assert!(
            pre.sample_keys.iter().any(|k| k.contains("sum=120.0")),
            "{:?} — resolved scenario must not inflate the sum",
            pre.sample_keys
        );
        let post = sums
            .iter()
            .find(|f| f.severity == Severity::Suspect)
            .expect("post");
        assert_eq!(post.count, 1);
        assert!(post.sample_keys[0].starts_with("2026-06-15"));
        // The exactly-110 boundary date is clean.
        assert!(!sums
            .iter()
            .flat_map(|f| &f.sample_keys)
            .any(|k| k.starts_with("2026-05-03")));
    }

    #[test]
    fn scenario_jump_gt_15pp_pre_info_post_suspect() {
        let conn = open_in_memory();
        conn.execute_batch(
            "INSERT INTO scenarios (id, name, probability, status) VALUES
                (9201, 'audit-C', 80.0, 'active');
             INSERT INTO scenario_history (scenario_id, probability, recorded_at) VALUES
                (9201, 10.0, '2026-05-01 09:00:00'),
                (9201, 30.0, '2026-05-02 09:00:00'),   -- +20pp pre  → info
                (9201, 45.0, '2026-05-03 09:00:00'),   -- +15pp exactly → clean
                (9201, 80.0, '2026-06-11 09:00:00');   -- +35pp post → suspect (cap was 5pp/day)",
        )
        .expect("seed");
        let report = run_checks(&conn, Some("scenario_history")).expect("run");
        let jumps: Vec<&AuditFinding> = report
            .findings
            .iter()
            .filter(|f| f.check == "single-move-gt-15pp")
            .collect();
        assert_eq!(jumps.len(), 2);
        let pre = jumps.iter().find(|f| f.severity == Severity::Info).expect("pre");
        assert_eq!(pre.count, 1);
        assert!(pre.sample_keys[0].contains("scenario#9201"), "{:?}", pre.sample_keys);
        assert!(pre.sample_keys[0].contains("+20.0pp"), "{:?}", pre.sample_keys);
        let post = jumps
            .iter()
            .find(|f| f.severity == Severity::Suspect)
            .expect("post");
        assert_eq!(post.count, 1);
        assert!(post.sample_keys[0].contains("2026-06-11"), "{:?}", post.sample_keys);
    }

    // ── transactions sanity ────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn insert_tx(
        conn: &rusqlite::Connection,
        symbol: &str,
        category: &str,
        tx_type: &str,
        quantity: &str,
        price_per: &str,
        currency: &str,
        date: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO transactions (symbol, category, tx_type, quantity, price_per, currency, date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![symbol, category, tx_type, quantity, price_per, currency, date],
        )
        .expect("insert tx");
        conn.last_insert_rowid()
    }

    #[test]
    fn transaction_price_outside_band_is_suspect_with_privacy_shaped_keys() {
        let conn = open_in_memory();
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('AAPL', '2026-06-02', '100', 'yahoo')",
            [],
        )
        .expect("close");
        // Inside the ±15% band → clean.
        insert_tx(&conn, "AAPL", "equity", "buy", "2", "114.99", "USD", "2026-06-02");
        // Outside → suspect. Distinctive quantity/price to assert privacy.
        let id = insert_tx(&conn, "AAPL", "equity", "buy", "7777", "133.33", "USD", "2026-06-02");
        let report = run_checks(&conn, Some("transactions")).expect("run");
        assert_eq!(report.findings.len(), 1);
        let f = &report.findings[0];
        assert_eq!(f.check, "price-vs-session-range");
        assert_eq!(f.severity, Severity::Suspect);
        assert_eq!(f.count, 1);
        assert_eq!(f.sample_keys[0], format!("#{id} AAPL 2026-06-02 dev+33.3%"));
        // PRIVACY: no quantity, no fill price, no close anywhere in output.
        let serialized = serde_json::to_string(&f).expect("json");
        assert!(!serialized.contains("7777"), "quantity leaked: {serialized}");
        assert!(!serialized.contains("133.33"), "fill price leaked: {serialized}");
    }

    #[test]
    fn transaction_price_uses_nearest_session_when_day_missing() {
        let conn = open_in_memory();
        // Friday close only; the trade is dated Sunday.
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('AAPL', '2026-06-05', '100', 'yahoo')",
            [],
        )
        .expect("close");
        insert_tx(&conn, "AAPL", "equity", "sell", "1", "150", "USD", "2026-06-07");
        // And a trade with NO session within 5 days → unverifiable, skipped.
        insert_tx(&conn, "AAPL", "equity", "buy", "1", "999", "USD", "2026-07-15");
        let report = run_checks(&conn, Some("transactions")).expect("run");
        assert_eq!(report.findings.len(), 1);
        let f = &report.findings[0];
        assert_eq!(f.count, 1, "only the nearest-session row flags");
        assert!(f.sample_keys[0].contains("2026-06-07"), "{:?}", f.sample_keys);
        assert!(f.sample_keys[0].contains("dev+50.0%"), "{:?}", f.sample_keys);
    }

    #[test]
    fn transaction_cash_and_non_usd_rows_are_exempt() {
        let conn = open_in_memory();
        // No USD price history at all — would flag if not exempt.
        insert_tx(&conn, "USD", "cash", "sell", "100", "1", "USD", "2026-06-02");
        insert_tx(&conn, "CASH", "cash", "buy", "100", "1", "USD", "2026-06-02");
        // Non-USD currency: close comparison would lie → skipped.
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('VWRL.L', '2026-06-02', '100', 'yahoo')",
            [],
        )
        .expect("close");
        insert_tx(&conn, "VWRL.L", "fund", "buy", "10", "8500", "EUR", "2026-06-02");
        let report = run_checks(&conn, Some("transactions")).expect("run");
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    #[test]
    fn transaction_nonpositive_quantity_and_orphaned_pair_are_suspect() {
        let conn = open_in_memory();
        insert_tx(&conn, "AAPL", "equity", "sell", "-3", "100", "USD", "2026-06-02");
        let id = insert_tx(&conn, "BTC", "crypto", "buy", "1", "50000", "USD", "2026-06-02");
        // Orphans form when rows are deleted by external tools (FK pragma
        // off) — reproduce that shape directly.
        conn.execute_batch("PRAGMA foreign_keys = OFF;").expect("pragma");
        conn.execute(
            "UPDATE transactions SET paired_tx_id = 99999 WHERE id = ?1",
            rusqlite::params![id],
        )
        .expect("orphan");
        let report = run_checks(&conn, Some("transactions")).expect("run");
        let qty = report
            .findings
            .iter()
            .find(|f| f.check == "nonpositive-quantity")
            .expect("quantity finding");
        assert_eq!(qty.severity, Severity::Suspect);
        assert!(qty.sample_keys[0].contains("AAPL"), "{:?}", qty.sample_keys);
        let orphan = report
            .findings
            .iter()
            .find(|f| f.check == "orphaned-paired-tx")
            .expect("orphan finding");
        assert_eq!(orphan.severity, Severity::Suspect);
        assert!(
            orphan.sample_keys[0].contains("missing #99999"),
            "{:?}",
            orphan.sample_keys
        );
    }

    #[test]
    fn doctor_summary_line() {
        let conn = open_in_memory();
        let (passed, msg) = doctor_summary(&conn).expect("summary");
        assert!(passed, "{msg}");
        assert!(msg.contains("no suspect findings"), "{msg}");

        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close, source) VALUES
                ('EUR=X','2026-06-01','1.0000','yahoo'),
                ('EUR=X','2026-06-02','1.0000','yahoo'),
                ('EUR=X','2026-06-03','1.0000','yahoo'),
                ('EUR=X','2026-06-04','1.0000','yahoo'),
                ('EUR=X','2026-06-05','1.0000','yahoo');",
        )
        .expect("seed");
        let (passed, msg) = doctor_summary(&conn).expect("summary");
        assert!(!passed);
        assert!(msg.contains("pftui data audit"), "{msg}");
    }
}
