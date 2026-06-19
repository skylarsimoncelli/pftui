//! Strategy backtesting — define trade conditions as an expression and
//! evaluate them against the full historical price database.
//!
//! Pipeline: [`parser`] → [`Expr`] → [`eval`] over a [`resolver::Resolver`]
//! (price + indicators + timeframe, lookahead-safe) → [`engine`] (trades or
//! regime segments). See each submodule for detail.
//!
//! Everything here is a pure transform over series the loader supplies; the
//! database-backed loader lives in `commands::strategy`.

pub mod engine;
pub mod eval;
pub mod parser;
pub mod resolver;

use anyhow::{bail, Result};

use engine::{
    buy_hold, segment_stats, simulate_trades, BenchStats, ExitKind, SegmentStats, TradeReport,
};
use parser::Expr;
use resolver::Resolver;

/// Default hold horizon (calendar days) when no exit rule is given.
pub const DEFAULT_HOLD_DAYS: i64 = 90;

/// How the user asked positions to be closed.
pub enum ExitSpec {
    HoldDays(i64),
    When(Expr),
}

/// Parse the `--exit` argument. `None` → hold the default horizon.
/// `"hold 90d"` / `"hold 90"` → fixed horizon. Anything else → an exit
/// condition expression.
pub fn parse_exit(arg: Option<&str>) -> Result<ExitSpec> {
    let Some(raw) = arg else {
        return Ok(ExitSpec::HoldDays(DEFAULT_HOLD_DAYS));
    };
    let trimmed = raw.trim();
    if let Some(rest) = trimmed
        .strip_prefix("hold ")
        .or_else(|| trimmed.strip_prefix("hold"))
    {
        let num = rest.trim().trim_end_matches(['d', 'D']).trim();
        let days: i64 = num
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid hold horizon '{raw}'; use e.g. 'hold 90d'"))?;
        if days < 1 {
            bail!("hold horizon must be >= 1 day");
        }
        return Ok(ExitSpec::HoldDays(days));
    }
    Ok(ExitSpec::When(parser::parse(trimmed)?))
}

/// Optional risk exits checked intra-bar (percentages, 15.0 = 15%).
#[derive(Debug, Clone, Copy, Default)]
pub struct RiskExits {
    pub stop_loss_pct: Option<f64>,
    pub take_profit_pct: Option<f64>,
    pub trailing_pct: Option<f64>,
}

/// Run a trade backtest: entry edge + exit rule (+ optional stop/target/trailing)
/// → trades + stats + benchmark.
pub fn run_backtest(
    resolver: &mut Resolver,
    entry: &Expr,
    exit: &ExitSpec,
    risk: RiskExits,
) -> Result<TradeReport> {
    let dates = resolver.master_dates().to_vec();
    let closes = resolver.field_series(None, parser::PriceField::Close, parser::Timeframe::Daily)?;
    let highs = resolver.field_series(None, parser::PriceField::High, parser::Timeframe::Daily)?;
    let lows = resolver.field_series(None, parser::PriceField::Low, parser::Timeframe::Daily)?;
    let entry_mask = eval::eval_condition(entry, resolver)?;
    let base = match exit {
        ExitSpec::HoldDays(d) => ExitKind::HoldDays(*d),
        ExitSpec::When(e) => ExitKind::Condition(eval::eval_condition(e, resolver)?),
    };
    let mut exit_cfg = engine::ExitConfig::new(base);
    exit_cfg.stop_loss_pct = risk.stop_loss_pct;
    exit_cfg.take_profit_pct = risk.take_profit_pct;
    exit_cfg.trailing_pct = risk.trailing_pct;
    let (trades, open_skipped) =
        simulate_trades(&dates, &closes, &highs, &lows, &entry_mask, &exit_cfg);
    Ok(engine::trade_report(&dates, &closes, trades, open_skipped))
}

#[derive(serde::Serialize)]
pub struct SegmentReport {
    pub on: SegmentStats,
    pub off: SegmentStats,
    pub benchmark_hold: BenchStats,
}

/// Run a regime segmentation: forward returns while `when` is true vs false.
pub fn run_segment(resolver: &mut Resolver, when: &Expr) -> Result<SegmentReport> {
    let dates = resolver.master_dates().to_vec();
    let closes = resolver.field_series(None, parser::PriceField::Close, parser::Timeframe::Daily)?;
    let mask = eval::eval_condition(when, resolver)?;
    Ok(SegmentReport {
        on: segment_stats("in-state", &closes, &mask, true),
        off: segment_stats("out-of-state", &closes, &mask, false),
        benchmark_hold: buy_hold(&dates, &closes),
    })
}

#[derive(serde::Serialize)]
pub struct CompareReport {
    pub a: SegmentStats,
    pub b: SegmentStats,
    pub benchmark_hold: BenchStats,
}

/// Compare forward returns under two independent regime masks (`when` vs
/// `vs`) — e.g. hiking vs cutting. Each mask selects its own bars; they are
/// not complements.
pub fn run_compare(
    resolver: &mut Resolver,
    when: &Expr,
    when_label: &str,
    vs: &Expr,
    vs_label: &str,
) -> Result<CompareReport> {
    let dates = resolver.master_dates().to_vec();
    let closes = resolver.field_series(None, parser::PriceField::Close, parser::Timeframe::Daily)?;
    let mask_a = eval::eval_condition(when, resolver)?;
    let mask_b = eval::eval_condition(vs, resolver)?;
    Ok(CompareReport {
        a: segment_stats(when_label, &closes, &mask_a, true),
        b: segment_stats(vs_label, &closes, &mask_b, true),
        benchmark_hold: buy_hold(&dates, &closes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exit_defaults_to_hold() {
        assert!(matches!(
            parse_exit(None).unwrap(),
            ExitSpec::HoldDays(DEFAULT_HOLD_DAYS)
        ));
    }

    #[test]
    fn parse_exit_hold_forms() {
        assert!(matches!(parse_exit(Some("hold 30d")).unwrap(), ExitSpec::HoldDays(30)));
        assert!(matches!(parse_exit(Some("hold 45")).unwrap(), ExitSpec::HoldDays(45)));
    }

    #[test]
    fn parse_exit_expression() {
        assert!(matches!(
            parse_exit(Some("close crosses_below sma(close, 50)")).unwrap(),
            ExitSpec::When(_)
        ));
    }

    #[test]
    fn parse_exit_rejects_bad_hold() {
        assert!(parse_exit(Some("hold abc")).is_err());
        assert!(parse_exit(Some("hold 0d")).is_err());
    }
}
