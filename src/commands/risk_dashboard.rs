//! `analytics risk-dashboard <SYM>` — the risk-side capstone, analogous to
//! `positioning` for direction. Composes the measured risk primitives (EVT
//! tail-risk, tail-dependence / co-crash, Hurst regime, vol, drawdown) into one
//! auditable view + a plain-language risk read. Each line is the same
//! computation as its dedicated command.

use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;

use crate::analytics::changepoint::detect_regime_breaks;
use crate::analytics::copula::tail_dependence;
use crate::analytics::evt::fit_evt_tail_risk;
use crate::analytics::hurst_rs::hurst;
use crate::analytics::risk;
use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// Default co-crash partner: the operator's BTC↔gold diversification pair, with
/// gold as the generic diversifier for everything else.
fn default_partner(resolved: &str) -> &'static str {
    match resolved {
        "GC=F" => "BTC-USD",
        _ => "GC=F",
    }
}

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    vs: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let resolved = resolve_alias(symbol);
    let hist = price_history::get_history(backend.sqlite(), &resolved, u32::MAX)?;
    if hist.len() < 101 {
        bail!("not enough price history for '{symbol}' (resolved '{resolved}') — need ≥101 bars");
    }
    let as_of = hist.last().map(|h| h.date.clone()).unwrap_or_default();
    let closes_dec: Vec<rust_decimal::Decimal> = hist.iter().map(|b| b.close).collect();
    let closes: Vec<f64> = hist.iter().filter_map(|b| b.close.to_f64()).collect();
    // Pair each daily return with its date in one pass (so regime-break dates
    // align even if a mid-series close is missing/non-positive — a tail slice
    // would mis-assign change-point dates).
    let mut returns: Vec<f64> = Vec::with_capacity(hist.len());
    let mut ret_dates: Vec<String> = Vec::with_capacity(hist.len());
    let mut log_rets: Vec<f64> = Vec::with_capacity(hist.len());
    for w in hist.windows(2) {
        if let (Some(p0), Some(p1)) = (w[0].close.to_f64(), w[1].close.to_f64()) {
            if p0 > 0.0 {
                returns.push(p1 / p0 - 1.0);
                ret_dates.push(w[1].date.clone());
                if p1 > 0.0 {
                    log_rets.push((p1 / p0).ln());
                }
            }
        }
    }

    // --- measured risk primitives ---
    let vol = risk::annualized_volatility_pct(&returns).and_then(|d| d.to_f64());
    let max_dd = risk::max_drawdown_pct(&closes_dec).and_then(|d| d.to_f64());
    let price = *closes.last().unwrap();
    let ath = closes.iter().cloned().fold(f64::MIN, f64::max);
    let dd_from_ath = if ath > 0.0 { (price / ath - 1.0) * 100.0 } else { 0.0 };
    let evt = fit_evt_tail_risk(&returns, 0.95);
    let hurst_res = hurst(&log_rets);
    let regime = detect_regime_breaks(&ret_dates, &returns, 0.5, 5.0);
    // Drawdown-path risk on the price series as a long-hold equity curve: CDaR
    // (tail of the drawdown distribution) + duration-aware Ulcer/Omega. This is
    // the buy-and-hold risk the operator actually sits through accumulating.
    let dd_metrics = crate::analytics::drawdown_metrics::compute(&closes, None, 0.0);

    // Co-crash partner — reuse the SHARED aligned-returns helper that
    // `tail-dependence` uses (intersect dates first, difference over consecutive
    // common dates) so the two commands produce identical λ_L.
    let partner = vs.map(resolve_alias).unwrap_or_else(|| default_partner(&resolved).to_string());
    let td = if partner != resolved {
        crate::commands::tail_dependence::aligned_common_returns(backend, &resolved, &partner)
            .and_then(|(x, y)| tail_dependence(&x, &y, 0.05))
    } else {
        None
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "risk-dashboard",
                "asset": symbol,
                "resolved_symbol": resolved,
                "as_of": as_of,
                "annualized_vol_pct": vol,
                "max_drawdown_pct": max_dd,
                "drawdown_from_ath_pct": (dd_from_ath * 100.0).round() / 100.0,
                "tail_risk": evt,
                "drawdown_path": dd_metrics,
                "hurst": hurst_res,
                "regime_break": regime,
                "co_crash": td.as_ref().map(|t| json!({ "vs": partner, "tail_dependence": t })),
            }))?
        );
        return Ok(());
    }

    println!("═══ Risk Dashboard — {symbol} ({resolved}) ═══");
    println!("As of {as_of} · price {price:.2}\n");
    println!(
        "Volatility:  {} annualized | max drawdown {} | now {:+.1}% from ATH",
        vol.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "—".into()),
        max_dd.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "—".into()),
        dd_from_ath,
    );
    match &evt {
        Some(e) => println!(
            "Tail risk:   ξ {:+.2} ({}) | 1d VaR 99% {:.1}% · 99.9% {:.1}% | ES99 {:.1}%",
            e.xi, e.tail_class, e.var_99_pct, e.var_999_pct, e.es_99_pct
        ),
        None => println!("Tail risk:   (insufficient data)"),
    }
    if let Some(d) = &dd_metrics {
        println!(
            "Drawdown:    CDaR-95 {:.1}% (worst-5% mean depth) | CDaR-90 {:.1}% | Ulcer {:.1}% | Omega(τ=0) {}",
            d.cdar_95 * 100.0,
            d.cdar_90 * 100.0,
            d.ulcer_index_pct,
            d.omega_ratio.map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".into()),
        );
    }
    match &hurst_res {
        Some(h) => println!(
            "Regime:      Hurst {:.2} ({}) | DFA {} | {}",
            h.h,
            h.regime,
            h.dfa_alpha.map(|a| format!("{a:.2}")).unwrap_or_else(|| "—".into()),
            regime
                .as_ref()
                .map(|rb| rb.interpretation.clone())
                .unwrap_or_else(|| "no regime-break data".into()),
        ),
        None => println!("Regime:      (insufficient data)"),
    }
    if let Some(t) = &td {
        println!(
            "Co-crash:    vs {} — lower-tail λ_L {:.2} ({})",
            partner,
            t.emp_lower_tail_dep,
            if t.emp_lower_tail_dep >= 0.40 {
                "STRONG — diversification largely fails in a crash"
            } else if t.emp_lower_tail_dep >= 0.20 {
                "MODERATE — partial joint downside"
            } else {
                "WEAK — tails near independence, diversification holds"
            },
        );
    }
    println!("\n{}", risk_verdict(&evt, vol, &td));
    Ok(())
}

/// One-line composite risk read from the measured pieces.
fn risk_verdict(
    evt: &Option<crate::analytics::evt::EvtTailRisk>,
    vol: Option<f64>,
    td: &Option<crate::analytics::copula::TailDependence>,
) -> String {
    let mut notes = Vec::new();
    if let Some(e) = evt {
        if e.xi >= 0.25 {
            notes.push(format!("FAT left tail (ξ={:.2}) — crashes deeper than normal", e.xi));
        }
    }
    if let Some(v) = vol {
        if v >= 50.0 {
            notes.push(format!("high vol ({v:.0}%/yr)"));
        }
    }
    if let Some(t) = td {
        if t.emp_lower_tail_dep < 0.20 {
            notes.push("co-crash risk LOW (diversification intact)".to_string());
        } else if t.emp_lower_tail_dep >= 0.40 {
            notes.push("co-crash risk HIGH (diversification fails when needed)".to_string());
        }
    }
    if notes.is_empty() {
        "Composite: risk profile within normal bounds on the measured dimensions.".to_string()
    } else {
        format!("Composite: {}.", notes.join("; "))
    }
}
