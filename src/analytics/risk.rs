use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const TRADING_DAYS_PER_YEAR: f64 = 252.0;
const Z95: f64 = 0.05;

#[derive(Debug, Clone)]
pub struct RiskMetrics {
    pub annualized_volatility_pct: Option<Decimal>,
    pub max_drawdown_pct: Option<Decimal>,
    pub sharpe_ratio: Option<Decimal>,
    pub historical_var_95_pct: Option<Decimal>,
    pub herfindahl_index: Option<Decimal>,
}

/// Compute all F4.1 risk metrics from portfolio values and current position values.
///
/// - `portfolio_values`: ordered oldest -> newest
/// - `position_values`: current market values per position
/// - `ffr_pct`: annualized Fed Funds Rate in percent (e.g. 4.50)
pub fn compute_risk_metrics(
    portfolio_values: &[Decimal],
    position_values: &[Decimal],
    ffr_pct: Option<Decimal>,
) -> RiskMetrics {
    let returns = daily_returns(portfolio_values);
    RiskMetrics {
        annualized_volatility_pct: annualized_volatility_pct(&returns),
        max_drawdown_pct: max_drawdown_pct(portfolio_values),
        sharpe_ratio: sharpe_ratio_vs_ffr(&returns, ffr_pct.unwrap_or(dec!(0))),
        historical_var_95_pct: historical_var_95_pct(&returns),
        herfindahl_index: herfindahl_index(position_values),
    }
}

/// Daily simple returns: `v_t / v_{t-1} - 1`.
pub fn daily_returns(values: &[Decimal]) -> Vec<f64> {
    values
        .windows(2)
        .filter_map(|w| {
            let prev = w[0].to_f64()?;
            let curr = w[1].to_f64()?;
            if prev <= 0.0 || !prev.is_finite() || !curr.is_finite() {
                return None;
            }
            Some((curr / prev) - 1.0)
        })
        .collect()
}

/// Annualized volatility in percent using sample standard deviation of daily returns.
pub fn annualized_volatility_pct(returns: &[f64]) -> Option<Decimal> {
    let std = sample_stddev(returns)?;
    let vol_pct = std * TRADING_DAYS_PER_YEAR.sqrt() * 100.0;
    Decimal::from_f64(vol_pct).map(|d| d.round_dp(4))
}

/// Max drawdown in percent (negative or zero).
pub fn max_drawdown_pct(values: &[Decimal]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }

    let mut peak = values[0].to_f64()?;
    if peak <= 0.0 || !peak.is_finite() {
        return None;
    }
    let mut worst = 0.0_f64;

    for value in values {
        let v = value.to_f64()?;
        if !v.is_finite() || v <= 0.0 {
            continue;
        }
        if v > peak {
            peak = v;
        }
        let dd = ((v - peak) / peak) * 100.0;
        if dd < worst {
            worst = dd;
        }
    }

    Decimal::from_f64(worst).map(|d| d.round_dp(4))
}

/// Annualized Sharpe ratio using Fed Funds Rate as annual risk-free rate.
pub fn sharpe_ratio_vs_ffr(returns: &[f64], ffr_pct: Decimal) -> Option<Decimal> {
    if returns.len() < 2 {
        return None;
    }

    let annual_rf = (ffr_pct / dec!(100)).to_f64()?;
    let daily_rf = (1.0 + annual_rf).powf(1.0 / TRADING_DAYS_PER_YEAR) - 1.0;

    let excess: Vec<f64> = returns.iter().map(|r| r - daily_rf).collect();
    let mean = mean(&excess)?;
    let std = sample_stddev(&excess)?;
    if std <= 0.0 {
        return None;
    }
    Decimal::from_f64((mean / std) * TRADING_DAYS_PER_YEAR.sqrt()).map(|d| d.round_dp(4))
}

/// Historical Value-at-Risk (95%) as a positive loss percentage.
pub fn historical_var_95_pct(returns: &[f64]) -> Option<Decimal> {
    if returns.is_empty() {
        return None;
    }
    let mut sorted = returns.to_vec();
    sorted.sort_by(f64::total_cmp);
    let idx = ((sorted.len() as f64) * Z95).floor() as usize;
    let quantile = sorted[idx.min(sorted.len().saturating_sub(1))];
    let var_pct = (-quantile).max(0.0) * 100.0;
    Decimal::from_f64(var_pct).map(|d| d.round_dp(4))
}

/// Herfindahl-Hirschman concentration index (sum of squared weights), range [0, 1].
pub fn herfindahl_index(position_values: &[Decimal]) -> Option<Decimal> {
    if position_values.is_empty() {
        return None;
    }
    let clean: Vec<f64> = position_values
        .iter()
        .filter_map(|v| v.to_f64())
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    if clean.is_empty() {
        return None;
    }
    let total: f64 = clean.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let hhi: f64 = clean.iter().map(|v| (v / total).powi(2)).sum();
    Decimal::from_f64(hhi).map(|d| d.round_dp(6))
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

fn sample_stddev(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mu = mean(values)?;
    let var = values
        .iter()
        .map(|v| {
            let d = v - mu;
            d * d
        })
        .sum::<f64>()
        / (values.len() - 1) as f64;
    Some(var.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn assert_close(actual: Decimal, expected: f64, tol: f64) {
        let actual_f = actual.to_f64().unwrap();
        assert!(
            (actual_f - expected).abs() <= tol,
            "actual={} expected={} tol={}",
            actual_f,
            expected,
            tol
        );
    }

    #[test]
    fn computes_daily_returns() {
        let values = vec![dec!(100), dec!(105), dec!(102.9)];
        let ret = daily_returns(&values);
        assert_eq!(ret.len(), 2);
        assert!((ret[0] - 0.05).abs() < 1e-9);
        assert!((ret[1] + 0.02).abs() < 1e-9);
    }

    #[test]
    fn computes_max_drawdown() {
        let values = vec![dec!(100), dec!(120), dec!(110), dec!(90), dec!(95)];
        let dd = max_drawdown_pct(&values).unwrap();
        assert_close(dd, -25.0, 1e-6);
    }

    #[test]
    fn computes_annualized_volatility() {
        let values = vec![dec!(100), dec!(101), dec!(99), dec!(102), dec!(100)];
        let ret = daily_returns(&values);
        let vol = annualized_volatility_pct(&ret).unwrap();
        assert!(vol > dec!(0));
    }

    #[test]
    fn computes_sharpe_vs_ffr() {
        // Deterministic mildly positive return stream.
        let returns = vec![0.01, 0.005, -0.002, 0.008, 0.004, -0.001];
        let sharpe = sharpe_ratio_vs_ffr(&returns, dec!(4.5)).unwrap();
        assert!(sharpe > dec!(0));
    }

    #[test]
    fn computes_historical_var_95() {
        let returns = vec![-0.04, -0.02, -0.01, 0.0, 0.01, 0.02];
        let var95 = historical_var_95_pct(&returns).unwrap();
        assert_close(var95, 4.0, 1e-6);
    }

    #[test]
    fn computes_herfindahl() {
        let hhi = herfindahl_index(&[dec!(50), dec!(30), dec!(20)]).unwrap();
        assert_close(hhi, 0.38, 1e-6); // 0.5^2 + 0.3^2 + 0.2^2
    }

    #[test]
    fn computes_full_metrics_bundle() {
        let metrics = compute_risk_metrics(
            &[dec!(100), dec!(102), dec!(99), dec!(101), dec!(98)],
            &[dec!(60000), dec!(30000), dec!(10000)],
            Some(dec!(4.25)),
        );

        assert!(metrics.annualized_volatility_pct.is_some());
        assert!(metrics.max_drawdown_pct.is_some());
        assert!(metrics.sharpe_ratio.is_some());
        assert!(metrics.historical_var_95_pct.is_some());
        assert!(metrics.herfindahl_index.is_some());
    }
}
