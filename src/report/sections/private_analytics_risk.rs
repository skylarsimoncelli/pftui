//! Private report section: **Drawdown Survival & Tail Risk** — surfaces the
//! native risk/survival analytics (annualized vol, EVT tail VaR, CDaR-95,
//! Hurst regime, risk-of-ruin, time-under-water) for each HELD asset in one
//! table, so the operator sees the depth+time risk picture inside the report
//! they actually read rather than running `analytics survival`/`risk-dashboard`
//! per asset. Composes the verified primitives; degenerate/thin assets are
//! skipped. Self-suppresses when no held asset has enough history.

use anyhow::Result;
use rust_decimal::prelude::ToPrimitive;

use super::super::build::daily::BuildContext;
use crate::analytics::strategy::resolver::resolve_alias;
use crate::analytics::{drawdown_metrics, evt, hurst_rs, risk, survival};
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// One held asset's risk/survival read for the report table.
#[derive(Debug, Clone, Default)]
pub struct AnalyticsRiskRow {
    pub symbol: String,
    /// Annualized volatility (percent).
    pub vol_pct: Option<f64>,
    /// EVT 1-day 99% Value-at-Risk (percent loss) + tail class.
    pub evt_var99_pct: Option<f64>,
    pub evt_tail_class: Option<String>,
    /// CDaR-95 — mean depth of the worst-5% drawdowns (fraction).
    pub cdar95: Option<f64>,
    /// Risk of ruin vs the budget (fraction) and whether the model is reliable
    /// (μ>0). When `!ruin_reliable` the drift is non-positive → recovery is
    /// unbounded in expectation.
    pub ruin_prob: Option<f64>,
    pub ruin_reliable: bool,
    /// Total time-under-water at 95% (years, i.i.d.).
    pub tuw_years: Option<f64>,
    /// Hurst regime label.
    pub regime: Option<String>,
}

/// Compute the risk/survival rows for a set of (already de-duplicated) held
/// symbols, reading each one's full `price_history`. Skips any symbol with
/// fewer than 31 usable closes. Pure read; no writes.
pub fn compute_rows(backend: &BackendConnection, symbols: &[String], budget_pct: f64) -> Vec<AnalyticsRiskRow> {
    let mut out = Vec::new();
    for raw in symbols {
        let resolved = resolve_alias(raw);
        let hist = match price_history::get_history(backend.sqlite(), &resolved, u32::MAX) {
            Ok(h) => h,
            Err(_) => continue,
        };
        let closes: Vec<f64> = hist.iter().filter_map(|b| b.close.to_f64()).filter(|c| *c > 0.0).collect();
        if closes.len() < 31 {
            continue;
        }
        let returns: Vec<f64> = closes.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
        let log_rets: Vec<f64> = closes.windows(2).map(|w| (w[1] / w[0]).ln()).collect();

        let vol_pct = risk::annualized_volatility_pct(&returns).and_then(|d| d.to_f64());
        let e = evt::fit_evt_tail_risk(&returns, 0.95);
        let dd = drawdown_metrics::compute(&closes, None, 0.0);
        let cdar95 = dd.as_ref().map(|d| d.cdar_95);
        let h = hurst_rs::hurst(&log_rets);
        let s = survival::compute(&log_rets, cdar95, budget_pct, 0.95);

        out.push(AnalyticsRiskRow {
            symbol: raw.clone(),
            vol_pct,
            evt_var99_pct: e.as_ref().map(|x| x.var_99_pct),
            evt_tail_class: e.as_ref().map(|x| x.tail_class.clone()),
            cdar95,
            ruin_prob: s.as_ref().map(|x| x.ruin_prob),
            ruin_reliable: s.as_ref().map(|x| x.reliable).unwrap_or(false),
            tuw_years: s.as_ref().and_then(|x| x.max_tuw_iid_days).map(|d| d / 365.25),
            regime: h.as_ref().map(|x| x.regime.clone()),
        });
    }
    out
}

fn fmt_pct(o: Option<f64>, p: usize) -> String {
    o.map(|v| format!("{v:.p$}%")).unwrap_or_else(|| "—".into())
}

/// Render the section from the precomputed rows on the context.
pub fn render_private_analytics_risk(ctx: &BuildContext) -> Result<String> {
    let rows = &ctx.private_analytics_risk;
    if rows.is_empty() {
        return Ok(super::suppressed("no held asset has enough price history for a risk read"));
    }

    let mut out = String::from("## Drawdown Survival & Tail Risk\n\n");
    out.push_str(
        "Native risk read per held asset — *depth* (EVT tail VaR, CDaR-95) and *time/solvency* \
         (risk-of-ruin vs a 25% drawdown budget, total time-under-water). The complement to the \
         direction view: not where it goes, but how far it falls and how long you wait.\n\n",
    );
    out.push_str("| Asset | Vol/yr | 99% VaR | Tail | CDaR-95 | Ruin@25% | Time-underwater | Regime |\n");
    out.push_str("|---|---:|---:|---|---:|---:|---:|---|\n");
    for r in rows {
        let ruin = if !r.ruin_reliable {
            "n/a*".to_string()
        } else {
            r.ruin_prob.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "—".into())
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.symbol,
            fmt_pct(r.vol_pct, 0),
            fmt_pct(r.evt_var99_pct, 1),
            r.evt_tail_class.clone().unwrap_or_else(|| "—".into()),
            r.cdar95.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "—".into()),
            ruin,
            r.tuw_years.map(|v| format!("{v:.1}y")).unwrap_or_else(|| "—".into()),
            r.regime.clone().unwrap_or_else(|| "—".into()),
        ));
    }

    // One-line read: most vs least survivable by ruin (reliable rows only).
    let reliable: Vec<&AnalyticsRiskRow> =
        rows.iter().filter(|r| r.ruin_reliable && r.ruin_prob.is_some()).collect();
    if reliable.len() >= 2 {
        let safest = reliable.iter().min_by(|a, b| a.ruin_prob.partial_cmp(&b.ruin_prob).unwrap()).unwrap();
        let riskiest = reliable.iter().max_by(|a, b| a.ruin_prob.partial_cmp(&b.ruin_prob).unwrap()).unwrap();
        out.push_str(&format!(
            "\nMost survivable: **{}** ({:.0}% ruin) · least: **{}** ({:.0}% ruin).\n",
            safest.symbol,
            safest.ruin_prob.unwrap() * 100.0,
            riskiest.symbol,
            riskiest.ruin_prob.unwrap() * 100.0,
        ));
    }
    if rows.iter().any(|r| !r.ruin_reliable) {
        out.push_str("\n\\* ruin n/a = no positive drift in the window (an asset at a cycle low); recovery is unbounded in expectation — hold only with cycle conviction.\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(sym: &str, ruin: f64, reliable: bool) -> AnalyticsRiskRow {
        AnalyticsRiskRow {
            symbol: sym.to_string(),
            vol_pct: Some(55.0),
            evt_var99_pct: Some(9.8),
            evt_tail_class: Some("moderate".into()),
            cdar95: Some(0.78),
            ruin_prob: Some(ruin),
            ruin_reliable: reliable,
            tuw_years: Some(6.9),
            regime: Some("random-walk".into()),
        }
    }

    #[test]
    fn suppresses_with_no_rows() {
        let ctx = BuildContext::default();
        let s = render_private_analytics_risk(&ctx).unwrap();
        assert!(s.contains("SUPPRESS") || s.trim().is_empty() || s.contains("suppress"), "expected suppression marker, got: {s}");
    }

    #[test]
    fn renders_table_and_survivability_read() {
        let ctx = BuildContext {
            private_analytics_risk: vec![row("BTC", 0.58, true), row("gold", 0.15, true)],
            ..BuildContext::default()
        };
        let s = render_private_analytics_risk(&ctx).unwrap();
        assert!(s.contains("## Drawdown Survival & Tail Risk"));
        assert!(s.contains("| BTC |") && s.contains("| gold |"));
        // Most survivable = gold (lower ruin), least = BTC.
        assert!(s.contains("Most survivable: **gold**"));
        assert!(s.contains("least: **BTC**"));
    }

    #[test]
    fn non_positive_drift_marked_na_with_footnote() {
        let ctx = BuildContext {
            private_analytics_risk: vec![row("BTC", 0.58, true), row("XYZ", 1.0, false)],
            ..BuildContext::default()
        };
        let s = render_private_analytics_risk(&ctx).unwrap();
        assert!(s.contains("n/a*"));
        assert!(s.contains("no positive drift"));
    }
}
