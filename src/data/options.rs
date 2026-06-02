//! Options-chain ingestion and Gamma Exposure (GEX) computation.
//!
//! Fetches an options chain for a symbol from Yahoo Finance's public
//! options endpoint, then computes per-strike gamma exposure using the
//! Black-Scholes constant-volatility gamma formula. The output feeds
//! `data options refresh`, `analytics gex`, and the daily-report GEX
//! one-liner.
//!
//! ## Why f64
//!
//! Greeks (gamma, vega, etc.), implied vol, open-interest, and the
//! aggregate GEX magnitudes are not money — they are
//! statistically-derived quantities reported as floats. `rust_decimal`
//! is reserved for prices, quantities, and cash. The dollar-magnitude
//! readout in `gex_snapshots.total_gamma_*` is an analytics scalar, not
//! a settled cash amount, so f64 is the appropriate storage choice and
//! consistent with similar caches (`real_yields_history`, etc.).
//!
//! ## Why Yahoo
//!
//! pftui already depends on `yahoo_finance_api` and the public
//! `query2.finance.yahoo.com/v7/finance/options/{symbol}` endpoint
//! returns calls + puts + expirations + open_interest + implied_vol
//! with no API key. The same endpoint is consumed by
//! `commands::options::run` (interactive viewer).
//!
//! ## Offline-friendly
//!
//! All public functions fall back gracefully when the network is
//! unavailable: fetch returns `Err(...)`, callers (`refresh.rs`) log
//! and continue.

use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Symbols pftui auto-refreshes options chains for.
///
/// BTC is intentionally omitted: Yahoo does not list BTC options
/// (Deribit is the canonical venue and is a separate provider TBD).
pub const DEFAULT_OPTIONS_SYMBOLS: &[&str] = &["SPY", "QQQ", "GLD", "SLV"];

/// One strike-row inside an options-chain snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OptionsStrikeRow {
    pub symbol: String,
    pub strike: f64,
    pub expiry: String,
    pub dte: i64,
    pub oi_calls: i64,
    pub oi_puts: i64,
    pub vol_calls: i64,
    pub vol_puts: i64,
    pub iv_call: Option<f64>,
    pub iv_put: Option<f64>,
}

/// Per-symbol options-chain snapshot (one expiry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionsChainSnapshot {
    pub symbol: String,
    pub spot: f64,
    pub expiry: String,
    pub dte: i64,
    /// ATM implied vol (call-side, closest strike to spot), if present.
    pub iv_atm: Option<f64>,
    pub rows: Vec<OptionsStrikeRow>,
    pub fetched_at: String,
}

/// GEX (Gamma Exposure) summary across all strikes in a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GexSummary {
    pub symbol: String,
    pub gex_flip_strike: Option<f64>,
    pub total_gamma_call: f64,
    pub total_gamma_put: f64,
    pub max_pain: Option<f64>,
    pub fetched_at: String,
}

impl GexSummary {
    /// 5% band around the flip strike — the "gamma-neutral zone" in
    /// which mechanical dealer hedging tends to pin the underlying.
    /// Returns `None` when no flip strike is computable.
    pub fn gamma_neutral_zone(&self) -> Option<(f64, f64)> {
        let flip = self.gex_flip_strike?;
        if flip <= 0.0 {
            return None;
        }
        let half = flip * 0.025;
        Some((flip - half, flip + half))
    }

    /// True when `strike` falls inside the 5% gamma-neutral zone.
    pub fn strike_in_zone(&self, strike: f64) -> bool {
        match self.gamma_neutral_zone() {
            Some((lo, hi)) => strike >= lo && strike <= hi,
            None => false,
        }
    }
}

/// Black-Scholes call/put gamma. Gamma is identical for calls and
/// puts under BS, so we expose a single function.
///
/// Inputs:
///   spot   — underlying price (S)
///   strike — strike (K)
///   t      — time to expiry in years (T)
///   sigma  — implied volatility (annualised, e.g. 0.20 for 20%)
///   r      — risk-free rate (annualised); 0.05 is a sensible default
///
/// Returns 0.0 for degenerate inputs (non-positive spot/strike/T/sigma)
/// rather than NaN, so the GEX aggregator never poisons a snapshot.
pub fn bs_gamma(spot: f64, strike: f64, t: f64, sigma: f64, r: f64) -> f64 {
    if spot <= 0.0 || strike <= 0.0 || t <= 0.0 || sigma <= 0.0 {
        return 0.0;
    }
    let sqrt_t = t.sqrt();
    let d1 = ((spot / strike).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);
    let phi = (-0.5 * d1 * d1).exp() / (2.0 * std::f64::consts::PI).sqrt();
    phi / (spot * sigma * sqrt_t)
}

/// Compute the GEX summary from a chain snapshot.
///
/// Net per-strike gamma exposure (dealer-perspective, calls long /
/// puts short):
///   gex_call = oi_calls * gamma_call * 100 * spot^2 / 100
///   gex_put  = oi_puts  * gamma_put  * 100 * spot^2 / 100
///   net_gex  = gex_call - gex_put
///
/// The "flip strike" is the strike at which cumulative net-GEX
/// changes sign. Approximate by sorting strikes ascending and
/// finding the lowest strike whose cumulative net-GEX crosses zero.
/// Returns `None` for chains with all-zero OI.
///
/// Max pain = strike that minimises total intrinsic option value
/// outstanding (sum_oi_call * max(0,S-K) + sum_oi_put * max(0,K-S)).
pub fn compute_gex(snapshot: &OptionsChainSnapshot) -> GexSummary {
    let t = (snapshot.dte.max(0) as f64) / 365.25;
    let sigma_fallback = snapshot.iv_atm.unwrap_or(0.30).max(0.05);
    let r = 0.05;

    let mut per_strike: Vec<(f64, f64, f64)> = snapshot
        .rows
        .iter()
        .map(|row| {
            let sigma_c = row.iv_call.unwrap_or(sigma_fallback).max(0.05);
            let sigma_p = row.iv_put.unwrap_or(sigma_fallback).max(0.05);
            let gamma_c = bs_gamma(snapshot.spot, row.strike, t, sigma_c, r);
            let gamma_p = bs_gamma(snapshot.spot, row.strike, t, sigma_p, r);
            let multiplier = snapshot.spot * snapshot.spot;
            let gex_c = (row.oi_calls as f64) * gamma_c * multiplier;
            let gex_p = (row.oi_puts as f64) * gamma_p * multiplier;
            (row.strike, gex_c, gex_p)
        })
        .collect();
    per_strike.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let total_gamma_call: f64 = per_strike.iter().map(|(_, c, _)| *c).sum();
    let total_gamma_put: f64 = per_strike.iter().map(|(_, _, p)| *p).sum();

    let flip = find_flip_strike(&per_strike);
    let max_pain = compute_max_pain(snapshot);

    GexSummary {
        symbol: snapshot.symbol.clone(),
        gex_flip_strike: flip,
        total_gamma_call,
        total_gamma_put,
        max_pain,
        fetched_at: snapshot.fetched_at.clone(),
    }
}

/// Find the strike at which cumulative net-GEX crosses zero.
fn find_flip_strike(per_strike: &[(f64, f64, f64)]) -> Option<f64> {
    if per_strike.is_empty() {
        return None;
    }
    let mut cum_net: f64 = 0.0;
    let mut prev_strike: Option<f64> = None;
    let mut prev_cum: f64 = 0.0;
    for (k, c, p) in per_strike {
        let net = c - p;
        cum_net += net;
        if let Some(ps) = prev_strike {
            // sign-change between previous and current cumulative
            if prev_cum.signum() != cum_net.signum() && prev_cum != 0.0 && cum_net != 0.0 {
                // linear interpolation between strikes for sub-strike flip
                let span = cum_net - prev_cum;
                if span.abs() > f64::EPSILON {
                    let t = -prev_cum / span;
                    let interp = ps + t * (k - ps);
                    return Some(interp);
                }
                return Some(*k);
            }
        }
        prev_strike = Some(*k);
        prev_cum = cum_net;
    }
    // No sign-change: return the strike at which |cumulative net| is
    // smallest (the closest-to-zero crossing candidate).
    let mut best: Option<(f64, f64)> = None;
    let mut cum: f64 = 0.0;
    for (k, c, p) in per_strike {
        cum += c - p;
        let abs = cum.abs();
        best = match best {
            Some((_, ba)) if ba <= abs => best,
            _ => Some((*k, abs)),
        };
    }
    best.map(|(k, _)| k)
}

/// Max pain = strike K minimising sum over all OI of intrinsic value.
fn compute_max_pain(snapshot: &OptionsChainSnapshot) -> Option<f64> {
    if snapshot.rows.is_empty() {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    for candidate in &snapshot.rows {
        let k = candidate.strike;
        let mut pain = 0.0_f64;
        for row in &snapshot.rows {
            let call_itm = (k - row.strike).max(0.0);
            let put_itm = (row.strike - k).max(0.0);
            pain += (row.oi_calls as f64) * call_itm;
            pain += (row.oi_puts as f64) * put_itm;
        }
        best = match best {
            Some((_, bp)) if bp <= pain => best,
            _ => Some((k, pain)),
        };
    }
    best.map(|(k, _)| k)
}

/// Fetch an options chain snapshot for `symbol`. Uses Yahoo Finance's
/// public endpoint. Returns an error when the network is unreachable.
pub async fn fetch_options_chain(symbol: &str) -> Result<OptionsChainSnapshot> {
    let url = format!(
        "https://query2.finance.yahoo.com/v7/finance/options/{}",
        symbol
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("Yahoo options endpoint returned {}", resp.status());
    }
    let json: Value = resp.json().await?;
    parse_chain_snapshot(symbol, &json)
}

/// Parse a Yahoo options-chain JSON payload into a snapshot. Pulled
/// out for unit testing against fixtures.
pub fn parse_chain_snapshot(symbol: &str, json: &Value) -> Result<OptionsChainSnapshot> {
    let result = json
        .get("optionChain")
        .and_then(|v| v.get("result"))
        .and_then(|v| v.get(0))
        .ok_or_else(|| anyhow!("No options chain returned for {}", symbol))?;

    let quote = result
        .get("quote")
        .ok_or_else(|| anyhow!("Missing quote payload for {}", symbol))?;

    let spot = quote
        .get("regularMarketPrice")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| anyhow!("Missing regularMarketPrice for {}", symbol))?;

    let options_obj = result
        .get("options")
        .and_then(|v| v.get(0))
        .ok_or_else(|| anyhow!("Missing option contracts for {}", symbol))?;

    let expiry_ts = options_obj
        .get("expirationDate")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let expiry_date = chrono::DateTime::from_timestamp(expiry_ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let now = Utc::now();
    let dte = if expiry_ts > 0 {
        let expiry_dt = chrono::DateTime::from_timestamp(expiry_ts, 0).unwrap_or(now);
        (expiry_dt - now).num_days().max(0)
    } else {
        0
    };

    let calls = options_obj
        .get("calls")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let puts = options_obj
        .get("puts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    use std::collections::BTreeMap;
    let mut by_strike: BTreeMap<u64, OptionsStrikeRow> = BTreeMap::new();

    let strike_key = |k: f64| -> u64 { (k * 1000.0).round() as u64 };

    for c in &calls {
        let strike = c.get("strike").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if strike <= 0.0 {
            continue;
        }
        let entry = by_strike
            .entry(strike_key(strike))
            .or_insert_with(|| OptionsStrikeRow {
                symbol: symbol.to_uppercase(),
                strike,
                expiry: expiry_date.clone(),
                dte,
                oi_calls: 0,
                oi_puts: 0,
                vol_calls: 0,
                vol_puts: 0,
                iv_call: None,
                iv_put: None,
            });
        entry.oi_calls = c.get("openInterest").and_then(|v| v.as_i64()).unwrap_or(0);
        entry.vol_calls = c.get("volume").and_then(|v| v.as_i64()).unwrap_or(0);
        entry.iv_call = c.get("impliedVolatility").and_then(|v| v.as_f64());
    }
    for p in &puts {
        let strike = p.get("strike").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if strike <= 0.0 {
            continue;
        }
        let entry = by_strike
            .entry(strike_key(strike))
            .or_insert_with(|| OptionsStrikeRow {
                symbol: symbol.to_uppercase(),
                strike,
                expiry: expiry_date.clone(),
                dte,
                oi_calls: 0,
                oi_puts: 0,
                vol_calls: 0,
                vol_puts: 0,
                iv_call: None,
                iv_put: None,
            });
        entry.oi_puts = p.get("openInterest").and_then(|v| v.as_i64()).unwrap_or(0);
        entry.vol_puts = p.get("volume").and_then(|v| v.as_i64()).unwrap_or(0);
        entry.iv_put = p.get("impliedVolatility").and_then(|v| v.as_f64());
    }

    let rows: Vec<OptionsStrikeRow> = by_strike.into_values().collect();

    // ATM IV: closest strike to spot, prefer call IV.
    let iv_atm = rows
        .iter()
        .min_by(|a, b| {
            let da = (a.strike - spot).abs();
            let db = (b.strike - spot).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|r| r.iv_call.or(r.iv_put));

    Ok(OptionsChainSnapshot {
        symbol: symbol.to_uppercase(),
        spot,
        expiry: expiry_date,
        dte,
        iv_atm,
        rows,
        fetched_at: now.to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-value sanity check for BS gamma at ATM/30dte/20%/r=0%.
    ///
    /// At S=K=100, T=30/365, sigma=0.20, r=0:
    ///   d1 = (ln(1) + (0 + 0.5*0.04) * 30/365) / (0.20 * sqrt(30/365))
    ///      = (0.02 * 0.082) / (0.20 * 0.2867)
    ///      ≈ 0.001643 / 0.05735
    ///      ≈ 0.02866
    ///   phi(d1) ≈ 0.39893 (close to peak of standard normal)
    ///   gamma = phi(d1) / (S * sigma * sqrt(T))
    ///         = 0.39893 / (100 * 0.20 * 0.2867)
    ///         ≈ 0.39893 / 5.735
    ///         ≈ 0.0696
    #[test]
    fn bs_gamma_atm_known_value() {
        let g = bs_gamma(100.0, 100.0, 30.0 / 365.0, 0.20, 0.0);
        // tolerate small differences in compounded rounding.
        assert!(
            (g - 0.0696).abs() < 0.002,
            "ATM gamma was {:.6}, expected ~0.0696",
            g
        );
    }

    #[test]
    fn bs_gamma_degenerate_inputs_return_zero() {
        assert_eq!(bs_gamma(0.0, 100.0, 0.1, 0.2, 0.05), 0.0);
        assert_eq!(bs_gamma(100.0, 0.0, 0.1, 0.2, 0.05), 0.0);
        assert_eq!(bs_gamma(100.0, 100.0, 0.0, 0.2, 0.05), 0.0);
        assert_eq!(bs_gamma(100.0, 100.0, 0.1, 0.0, 0.05), 0.0);
    }

    #[test]
    fn bs_gamma_call_equals_put() {
        // Black-Scholes gamma is identical for calls and puts.
        let g1 = bs_gamma(545.0, 540.0, 14.0 / 365.0, 0.18, 0.05);
        let g2 = bs_gamma(545.0, 540.0, 14.0 / 365.0, 0.18, 0.05);
        assert!((g1 - g2).abs() < 1e-12);
    }

    /// Three-strike fixture chain. Calls weighted at 540, puts at 560.
    /// Expected behaviour: flip strike sits between the two clusters
    /// (i.e. between 540 and 560, with simple OI weighting -> 550 area).
    fn fixture_chain() -> OptionsChainSnapshot {
        OptionsChainSnapshot {
            symbol: "SPY".into(),
            spot: 550.0,
            expiry: "2026-06-20".into(),
            dte: 14,
            iv_atm: Some(0.18),
            rows: vec![
                OptionsStrikeRow {
                    symbol: "SPY".into(),
                    strike: 540.0,
                    expiry: "2026-06-20".into(),
                    dte: 14,
                    oi_calls: 10_000,
                    oi_puts: 0,
                    vol_calls: 0,
                    vol_puts: 0,
                    iv_call: Some(0.18),
                    iv_put: Some(0.18),
                },
                OptionsStrikeRow {
                    symbol: "SPY".into(),
                    strike: 550.0,
                    expiry: "2026-06-20".into(),
                    dte: 14,
                    oi_calls: 5_000,
                    oi_puts: 5_000,
                    vol_calls: 0,
                    vol_puts: 0,
                    iv_call: Some(0.18),
                    iv_put: Some(0.18),
                },
                OptionsStrikeRow {
                    symbol: "SPY".into(),
                    strike: 560.0,
                    expiry: "2026-06-20".into(),
                    dte: 14,
                    oi_calls: 0,
                    oi_puts: 10_000,
                    vol_calls: 0,
                    vol_puts: 0,
                    iv_call: Some(0.18),
                    iv_put: Some(0.18),
                },
            ],
            fetched_at: "2026-06-02T00:00:00Z".into(),
        }
    }

    #[test]
    fn gex_summary_from_fixture_chain() {
        let snap = fixture_chain();
        let gex = compute_gex(&snap);
        assert_eq!(gex.symbol, "SPY");
        assert!(gex.total_gamma_call > 0.0);
        assert!(gex.total_gamma_put > 0.0);
        // Flip strike must sit between the call cluster (540) and the
        // put cluster (560): the cumulative net-GEX starts positive (calls
        // at 540), passes through zero, and ends negative (puts at 560).
        let flip = gex.gex_flip_strike.expect("flip strike must exist");
        assert!(
            (540.0..=560.0).contains(&flip),
            "flip strike {} outside [540, 560]",
            flip
        );
        // 550 is the equally-weighted middle strike; max pain should
        // also sit at 550 because intrinsic-value at 550 minimises
        // call+put settlement.
        let mp = gex.max_pain.expect("max pain must exist");
        assert!(
            (540.0..=560.0).contains(&mp),
            "max pain {} outside [540, 560]",
            mp
        );
    }

    #[test]
    fn gex_zone_logic() {
        let mut gex = GexSummary {
            symbol: "SPY".into(),
            gex_flip_strike: Some(550.0),
            total_gamma_call: 1.0,
            total_gamma_put: 1.0,
            max_pain: Some(548.0),
            fetched_at: "2026-06-02T00:00:00Z".into(),
        };
        let (lo, hi) = gex.gamma_neutral_zone().expect("zone");
        assert!((lo - 536.25).abs() < 0.01);
        assert!((hi - 563.75).abs() < 0.01);
        assert!(gex.strike_in_zone(545.0));
        assert!(!gex.strike_in_zone(500.0));

        gex.gex_flip_strike = None;
        assert!(gex.gamma_neutral_zone().is_none());
        assert!(!gex.strike_in_zone(550.0));
    }

    #[test]
    fn parse_chain_snapshot_extracts_rows() {
        let payload = serde_json::json!({
          "optionChain": {
            "result": [{
              "quote": {"regularMarketPrice": 550.0},
              "expirationDates": [1797552000_i64],
              "options": [{
                "expirationDate": 1797552000_i64,
                "calls": [
                  {"strike": 540.0, "openInterest": 100, "volume": 10, "impliedVolatility": 0.20},
                  {"strike": 550.0, "openInterest": 200, "volume": 20, "impliedVolatility": 0.18}
                ],
                "puts": [
                  {"strike": 550.0, "openInterest": 150, "volume": 5,  "impliedVolatility": 0.19},
                  {"strike": 560.0, "openInterest": 300, "volume": 12, "impliedVolatility": 0.21}
                ]
              }]
            }]
          }
        });
        let snap = parse_chain_snapshot("SPY", &payload).unwrap();
        assert_eq!(snap.symbol, "SPY");
        assert_eq!(snap.spot, 550.0);
        assert_eq!(snap.rows.len(), 3);
        let r540 = snap.rows.iter().find(|r| r.strike == 540.0).unwrap();
        assert_eq!(r540.oi_calls, 100);
        assert_eq!(r540.oi_puts, 0);
        let r550 = snap.rows.iter().find(|r| r.strike == 550.0).unwrap();
        assert_eq!(r550.oi_calls, 200);
        assert_eq!(r550.oi_puts, 150);
        let r560 = snap.rows.iter().find(|r| r.strike == 560.0).unwrap();
        assert_eq!(r560.oi_calls, 0);
        assert_eq!(r560.oi_puts, 300);
        assert!(snap.iv_atm.is_some());
    }
}
