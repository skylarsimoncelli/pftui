//! Private report section: **Risk-Parity Allocation Check** — for the held
//! basket, compares the operator's CURRENT allocation against the
//! risk-contribution-equalized weights (risk-parity on the full covariance, and
//! downside-risk-parity on the co-crash semicovariance). Surfaces where the
//! book is over/under-weight *risk* (not capital) — the single most actionable
//! read for accumulation sizing: "you're 60% BTC; equal-risk wants 11%".
//! Self-suppresses when fewer than two held assets have enough common history.

use anyhow::Result;

use super::super::build::daily::BuildContext;
use crate::analytics::basket::Method;
use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// One held asset's current-vs-suggested allocation.
#[derive(Debug, Clone, Default)]
pub struct BasketAllocRow {
    pub symbol: String,
    /// Current portfolio weight (percent).
    pub current_pct: f64,
    /// Risk-parity (ERC, full covariance) suggested weight (percent).
    pub rp_pct: f64,
    /// Downside-risk-parity (ERC on co-crash semicovariance) weight (percent).
    pub downside_pct: f64,
    /// current − risk-parity (percentage points; positive = overweight RISK).
    pub gap_pp: f64,
}

/// Compute the allocation-check rows from the held `(symbol, current_pct)`
/// pairs. Filters to assets with enough price history, runs risk-parity +
/// downside-risk-parity over that sub-basket, and pairs each suggested weight
/// with the operator's current weight. `None` when fewer than two held assets
/// are priceable or the basket can't be aligned (section self-suppresses).
pub fn compute_rows(backend: &BackendConnection, held: &[(String, f64)]) -> Option<Vec<BasketAllocRow>> {
    // Keep only held assets that have enough history to enter a covariance.
    let priceable: Vec<(String, f64)> = held
        .iter()
        .filter(|(s, _)| {
            price_history::get_history(backend.sqlite(), &resolve_alias(s), 64)
                .map(|h| h.len() >= 21)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    if priceable.len() < 2 {
        return None;
    }
    let symbols: Vec<String> = priceable.iter().map(|(s, _)| s.clone()).collect();

    let (rp, _) = crate::commands::basket::compute(backend, &symbols, Method::RiskParity, 0).ok()?;
    let (drp, _) =
        crate::commands::basket::compute(backend, &symbols, Method::DownsideRiskParity, 0).ok()?;

    // Map resolved symbol → suggested weight for each method.
    let rp_w = |resolved: &str| rp.weights.iter().find(|w| w.symbol == resolved).map(|w| w.weight);
    let drp_w = |resolved: &str| drp.weights.iter().find(|w| w.symbol == resolved).map(|w| w.weight);

    let mut rows = Vec::new();
    for (sym, current) in &priceable {
        let resolved = resolve_alias(sym);
        let (Some(rpw), Some(drpw)) = (rp_w(&resolved), drp_w(&resolved)) else {
            continue;
        };
        let rp_pct = rpw * 100.0;
        rows.push(BasketAllocRow {
            symbol: sym.clone(),
            current_pct: *current,
            rp_pct,
            downside_pct: drpw * 100.0,
            gap_pp: current - rp_pct,
        });
    }
    if rows.len() < 2 {
        return None;
    }
    // Largest overweight-risk first.
    rows.sort_by(|a, b| b.gap_pp.partial_cmp(&a.gap_pp).unwrap_or(std::cmp::Ordering::Equal));
    Some(rows)
}

/// Render the section from the precomputed rows on the context.
pub fn render_private_basket_allocation(ctx: &BuildContext) -> Result<String> {
    let rows = &ctx.private_basket_allocation;
    if rows.is_empty() {
        return Ok(super::suppressed("fewer than two held assets with common history"));
    }

    let mut out = String::from("## Risk-Parity Allocation Check\n\n");
    out.push_str(
        "Current book weight vs the **risk-equalized** weights — risk-parity (ERC on the full \
         covariance) and downside-risk-parity (ERC on the co-crash semicovariance). The gap is in \
         *risk*, not capital: a positive gap means the position carries more of the portfolio's \
         risk budget than an equal-risk book would give it.\n\n",
    );
    out.push_str("| Asset | Current | Risk-parity | Downside-RP | Gap (cur−RP) |\n");
    out.push_str("|---|---:|---:|---:|---:|\n");
    for r in rows {
        out.push_str(&format!(
            "| {} | {:.0}% | {:.0}% | {:.0}% | {:+.0} pp |\n",
            r.symbol, r.current_pct, r.rp_pct, r.downside_pct, r.gap_pp,
        ));
    }

    // One-line read: the single most over- and under-weight-risk position.
    if let (Some(over), Some(under)) = (rows.first(), rows.last()) {
        if over.gap_pp > 1.0 || under.gap_pp < -1.0 {
            out.push_str(&format!(
                "\nMost overweight risk: **{}** ({:+.0} pp vs equal-risk) · most underweight: **{}** ({:+.0} pp).\n",
                over.symbol, over.gap_pp, under.symbol, under.gap_pp,
            ));
        }
    }
    out.push_str("\n*Suggested weights are computed from price history only (no view on direction); they answer \"how would I split RISK equally\", not \"what should I buy\".*\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(sym: &str, cur: f64, rp: f64) -> BasketAllocRow {
        BasketAllocRow {
            symbol: sym.to_string(),
            current_pct: cur,
            rp_pct: rp,
            downside_pct: rp,
            gap_pp: cur - rp,
        }
    }

    #[test]
    fn suppresses_with_no_rows() {
        let s = render_private_basket_allocation(&BuildContext::default()).unwrap();
        assert!(s.contains("SUPPRESS") || s.contains("suppress"));
    }

    #[test]
    fn renders_table_and_overweight_read() {
        let ctx = BuildContext {
            private_basket_allocation: vec![row("BTC", 60.0, 11.0), row("gold", 40.0, 47.0)],
            ..BuildContext::default()
        };
        let s = render_private_basket_allocation(&ctx).unwrap();
        assert!(s.contains("## Risk-Parity Allocation Check"));
        assert!(s.contains("| BTC |") && s.contains("| gold |"));
        // BTC is overweight risk (+49pp), gold underweight (−7pp).
        assert!(s.contains("Most overweight risk: **BTC**"));
        assert!(s.contains("most underweight: **gold**"));
    }
}
