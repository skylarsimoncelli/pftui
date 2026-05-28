use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, Duration, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::db::snapshots::PortfolioSnapshot;
use crate::models::asset::AssetCategory;
use crate::models::position::Position;

const TRAILING_DAYS: i64 = 90;

#[derive(Debug, Clone, Serialize)]
pub struct DrawdownSummary {
    pub current_dd_from_local_high_pct: f64,
    pub current_dd_value: f64,
    pub local_high_date: String,
    pub local_high_value: f64,
    pub max_dd_mtd_pct: f64,
    pub max_dd_mtd_trough_date: Option<String>,
    pub max_dd_ytd_pct: f64,
    pub max_dd_ytd_trough_date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrawdownSeriesPoint {
    pub date: String,
    pub total_value: f64,
    pub local_high_date: String,
    pub local_high_value: f64,
    pub drawdown_pct: f64,
    pub drawdown_value: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrawdownReport {
    pub summary: DrawdownSummary,
    pub series: Vec<DrawdownSeriesPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_decomposition: Option<DrawdownDecomposition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrawdownDecomposition {
    pub as_of: String,
    pub previous_date: String,
    pub total_daily_change: f64,
    pub total_daily_change_pct: f64,
    pub positions: Vec<PositionDrawdownContribution>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PositionDrawdownContribution {
    pub symbol: String,
    pub daily_change_pct: f64,
    pub weight_pct: f64,
    pub contribution_pct: f64,
    pub change_value: f64,
    pub previous_value: f64,
    pub current_value: f64,
}

#[derive(Debug, Clone)]
struct ValuePoint {
    date: NaiveDate,
    value: Decimal,
}

#[derive(Debug, Clone)]
struct RawContribution {
    symbol: String,
    daily_change_pct: Decimal,
    previous_value: Decimal,
    current_value: Decimal,
    change_value: Decimal,
}

pub fn compute_drawdown_report(
    snapshots: &[PortfolioSnapshot],
    current_date: Option<&str>,
    current_value: Option<Decimal>,
    latest_decomposition: Option<DrawdownDecomposition>,
) -> Option<DrawdownReport> {
    let points = build_value_points(snapshots, current_date, current_value);
    if points.is_empty() {
        return None;
    }

    let series = compute_trailing_series(&points);
    let summary = compute_summary(&points)?;
    Some(DrawdownReport {
        summary,
        series,
        latest_decomposition,
    })
}

pub fn compute_latest_decomposition(
    positions: &[Position],
    previous_prices: &HashMap<String, Decimal>,
    as_of: &str,
    previous_date: &str,
) -> Option<DrawdownDecomposition> {
    let mut raw = Vec::new();

    for pos in positions {
        let (current_price, current_value) = match (pos.current_price, pos.current_value) {
            (Some(price), Some(value)) => (price, value),
            _ => continue,
        };
        let previous_price = if pos.category == AssetCategory::Cash {
            Some(dec!(1))
        } else {
            previous_prices.get(&pos.symbol).copied()
        };
        let Some(previous_price) = previous_price else {
            continue;
        };
        if previous_price <= dec!(0) {
            continue;
        }

        let fx_rate = pos.fx_rate.unwrap_or(dec!(1));
        let previous_value = previous_price * pos.quantity * fx_rate;
        if previous_value <= dec!(0) {
            continue;
        }

        let change_value = current_value - previous_value;
        let daily_change_pct = ((current_price - previous_price) / previous_price) * dec!(100);

        raw.push(RawContribution {
            symbol: pos.symbol.clone(),
            daily_change_pct,
            previous_value,
            current_value,
            change_value,
        });
    }

    let total_previous: Decimal = raw.iter().map(|row| row.previous_value).sum();
    if total_previous <= dec!(0) {
        return None;
    }
    let total_change: Decimal = raw.iter().map(|row| row.change_value).sum();
    let total_daily_change_pct = (total_change / total_previous) * dec!(100);

    let mut positions: Vec<PositionDrawdownContribution> = raw
        .into_iter()
        .filter(|row| row.change_value != dec!(0))
        .map(|row| {
            let weight_pct = (row.previous_value / total_previous) * dec!(100);
            let contribution_pct = (row.change_value / total_previous) * dec!(100);
            PositionDrawdownContribution {
                symbol: row.symbol,
                daily_change_pct: decimal_to_f64(row.daily_change_pct, 2),
                weight_pct: decimal_to_f64(weight_pct, 2),
                contribution_pct: decimal_to_f64(contribution_pct, 4),
                change_value: decimal_to_f64(row.change_value, 2),
                previous_value: decimal_to_f64(row.previous_value, 2),
                current_value: decimal_to_f64(row.current_value, 2),
            }
        })
        .collect();

    positions.sort_by(|a, b| {
        b.contribution_pct
            .abs()
            .partial_cmp(&a.contribution_pct.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Some(DrawdownDecomposition {
        as_of: as_of.to_string(),
        previous_date: previous_date.to_string(),
        total_daily_change: decimal_to_f64(total_change, 2),
        total_daily_change_pct: decimal_to_f64(total_daily_change_pct, 4),
        positions,
    })
}

fn build_value_points(
    snapshots: &[PortfolioSnapshot],
    current_date: Option<&str>,
    current_value: Option<Decimal>,
) -> Vec<ValuePoint> {
    let mut by_date: BTreeMap<NaiveDate, Decimal> = BTreeMap::new();

    for snap in snapshots {
        let Ok(date) = NaiveDate::parse_from_str(&snap.date, "%Y-%m-%d") else {
            continue;
        };
        by_date.insert(date, snap.total_value);
    }

    if let (Some(date), Some(value)) = (current_date, current_value) {
        if let Ok(parsed) = NaiveDate::parse_from_str(date, "%Y-%m-%d") {
            by_date.insert(parsed, value);
        }
    }

    by_date
        .into_iter()
        .map(|(date, value)| ValuePoint { date, value })
        .collect()
}

fn compute_summary(points: &[ValuePoint]) -> Option<DrawdownSummary> {
    let latest = points.last()?;
    let trailing_start = latest.date - Duration::days(TRAILING_DAYS - 1);
    let trailing: Vec<_> = points
        .iter()
        .filter(|point| point.date >= trailing_start)
        .cloned()
        .collect();
    let local_high = highest_point(&trailing)?;
    let current_dd_value = latest.value - local_high.value;
    let current_dd_pct = pct_change(latest.value, local_high.value).unwrap_or(dec!(0));

    let month_start = latest.date.with_day(1).unwrap_or(latest.date);
    let year_start = NaiveDate::from_ymd_opt(latest.date.year(), 1, 1).unwrap_or(latest.date);
    let (max_dd_mtd_pct, max_dd_mtd_trough_date) = max_drawdown_since(points, month_start);
    let (max_dd_ytd_pct, max_dd_ytd_trough_date) = max_drawdown_since(points, year_start);

    Some(DrawdownSummary {
        current_dd_from_local_high_pct: decimal_to_f64(current_dd_pct, 2),
        current_dd_value: decimal_to_f64(current_dd_value, 2),
        local_high_date: format_date(local_high.date),
        local_high_value: decimal_to_f64(local_high.value, 2),
        max_dd_mtd_pct: decimal_to_f64(max_dd_mtd_pct, 2),
        max_dd_mtd_trough_date,
        max_dd_ytd_pct: decimal_to_f64(max_dd_ytd_pct, 2),
        max_dd_ytd_trough_date,
    })
}

fn compute_trailing_series(points: &[ValuePoint]) -> Vec<DrawdownSeriesPoint> {
    let Some(latest) = points.last() else {
        return Vec::new();
    };
    let trailing_start = latest.date - Duration::days(TRAILING_DAYS - 1);
    let mut peak: Option<ValuePoint> = None;
    let mut out = Vec::new();

    for point in points.iter().filter(|point| point.date >= trailing_start) {
        if peak
            .as_ref()
            .map(|p| point.value >= p.value)
            .unwrap_or(true)
        {
            peak = Some(point.clone());
        }
        let Some(current_peak) = peak.as_ref() else {
            continue;
        };
        let drawdown_value = point.value - current_peak.value;
        let drawdown_pct = pct_change(point.value, current_peak.value).unwrap_or(dec!(0));
        out.push(DrawdownSeriesPoint {
            date: format_date(point.date),
            total_value: decimal_to_f64(point.value, 2),
            local_high_date: format_date(current_peak.date),
            local_high_value: decimal_to_f64(current_peak.value, 2),
            drawdown_pct: decimal_to_f64(drawdown_pct, 2),
            drawdown_value: decimal_to_f64(drawdown_value, 2),
        });
    }

    out
}

fn max_drawdown_since(points: &[ValuePoint], start: NaiveDate) -> (Decimal, Option<String>) {
    let mut peak: Option<ValuePoint> = None;
    let mut max_dd_pct = dec!(0);
    let mut trough_date = None;

    for point in points.iter().filter(|point| point.date >= start) {
        if peak
            .as_ref()
            .map(|p| point.value >= p.value)
            .unwrap_or(true)
        {
            peak = Some(point.clone());
            continue;
        }

        let Some(current_peak) = peak.as_ref() else {
            continue;
        };
        let Some(dd_pct) = pct_change(point.value, current_peak.value) else {
            continue;
        };
        if dd_pct < max_dd_pct {
            max_dd_pct = dd_pct;
            trough_date = Some(format_date(point.date));
        }
    }

    (max_dd_pct, trough_date)
}

fn highest_point(points: &[ValuePoint]) -> Option<ValuePoint> {
    let mut high = points.first()?.clone();
    for point in points.iter().skip(1) {
        if point.value > high.value || (point.value == high.value && point.date > high.date) {
            high = point.clone();
        }
    }
    Some(high)
}

fn pct_change(current: Decimal, base: Decimal) -> Option<Decimal> {
    if base <= dec!(0) {
        return None;
    }
    Some(((current - base) / base) * dec!(100))
}

fn decimal_to_f64(value: Decimal, dp: u32) -> f64 {
    let rounded = value.round_dp(dp);
    let normalized = if rounded == dec!(0) { dec!(0) } else { rounded };
    normalized.to_string().parse::<f64>().unwrap_or(0.0)
}

fn format_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(date: &str, value: Decimal) -> PortfolioSnapshot {
        PortfolioSnapshot {
            date: date.to_string(),
            total_value: value,
            cash_value: dec!(0),
            invested_value: value,
            snapshot_at: format!("{date}T00:00:00Z"),
        }
    }

    fn position(symbol: &str, qty: Decimal, current_price: Decimal) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category: AssetCategory::Equity,
            quantity: qty,
            avg_cost: current_price,
            total_cost: current_price * qty,
            currency: "USD".to_string(),
            current_price: Some(current_price),
            current_value: Some(current_price * qty),
            gain: None,
            gain_pct: None,
            allocation_pct: None,
            native_currency: None,
            fx_rate: None,
        }
    }

    #[test]
    fn summary_finds_current_and_period_drawdowns() {
        let snapshots = vec![
            snap("2026-01-01", dec!(100)),
            snap("2026-01-15", dec!(120)),
            snap("2026-02-01", dec!(130)),
            snap("2026-02-10", dec!(104)),
        ];

        let report = compute_drawdown_report(&snapshots, None, None, None).unwrap();

        assert_eq!(report.summary.local_high_date, "2026-02-01");
        assert_eq!(report.summary.current_dd_from_local_high_pct, -20.0);
        assert_eq!(report.summary.current_dd_value, -26.0);
        assert_eq!(report.summary.max_dd_mtd_pct, -20.0);
        assert_eq!(
            report.summary.max_dd_mtd_trough_date.as_deref(),
            Some("2026-02-10")
        );
        assert_eq!(report.summary.max_dd_ytd_pct, -20.0);
    }

    #[test]
    fn local_high_uses_trailing_90_days() {
        let snapshots = vec![
            snap("2025-10-01", dec!(200)),
            snap("2026-02-01", dec!(100)),
            snap("2026-02-10", dec!(90)),
        ];

        let report = compute_drawdown_report(&snapshots, None, None, None).unwrap();

        assert_eq!(report.summary.local_high_date, "2026-02-01");
        assert_eq!(report.summary.local_high_value, 100.0);
        assert_eq!(report.summary.current_dd_from_local_high_pct, -10.0);
    }

    #[test]
    fn current_value_replaces_snapshot_for_same_date() {
        let snapshots = vec![snap("2026-02-01", dec!(100)), snap("2026-02-10", dec!(90))];

        let report =
            compute_drawdown_report(&snapshots, Some("2026-02-10"), Some(dec!(95)), None).unwrap();

        assert_eq!(report.series.last().unwrap().total_value, 95.0);
        assert_eq!(report.summary.current_dd_from_local_high_pct, -5.0);
    }

    #[test]
    fn latest_decomposition_contributions_sum_to_portfolio_move() {
        let positions = vec![
            position("BTC", dec!(2), dec!(90)),
            position("GC=F", dec!(1), dec!(110)),
            Position {
                category: AssetCategory::Cash,
                ..position("USD", dec!(10), dec!(1))
            },
        ];
        let previous_prices = HashMap::from([
            ("BTC".to_string(), dec!(100)),
            ("GC=F".to_string(), dec!(100)),
        ]);

        let decomp =
            compute_latest_decomposition(&positions, &previous_prices, "2026-02-10", "2026-02-09")
                .unwrap();

        assert_eq!(decomp.total_daily_change, -10.0);
        assert_eq!(decomp.total_daily_change_pct, -3.2258);
        let contribution_sum: f64 = decomp.positions.iter().map(|p| p.contribution_pct).sum();
        assert!((contribution_sum - decomp.total_daily_change_pct).abs() < 0.0001);
        assert_eq!(decomp.positions[0].symbol, "BTC");
        assert_eq!(decomp.positions[0].contribution_pct, -6.4516);
    }
}
