//! Regime Intelligence — Risk-On / Risk-Off composite scoring.
//!
//! Computes a composite regime score from 9 cross-asset signals, all sourced
//! from Yahoo Finance data already fetched for the Markets view. Each signal
//! contributes +1 (risk-on) or -1 (risk-off). The composite score ranges
//! from -9 (extreme risk-off) to +9 (extreme risk-on).
//!
//! Signals:
//! 1. VIX level:      < 20 → risk-on, ≥ 20 → risk-off
//! 2. VIX 5D trend:   falling → risk-on, rising → risk-off
//! 3. 10Y yield trend: rising → risk-on, falling → risk-off
//! 4. 2Y-10Y spread:  positive → risk-on (normal), negative → risk-off (inverted)
//! 5. DXY 5D trend:   falling → risk-on, rising → risk-off
//! 6. Gold/SPX 5D:    falling → risk-on, rising → risk-off
//! 7. BTC/SPX corr:   positive → risk-on, negative → risk-off
//! 8. HY credit (HYG/LQD): rising → risk-on (spreads tightening), falling → risk-off
//! 9. Copper/Gold:    rising → risk-on, falling → risk-off

use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::models::price::HistoryRecord;

/// Number of days for short-term directional signals.
const LOOKBACK_DAYS: usize = 5;

/// Number of days for correlation calculation.
const CORR_DAYS: usize = 20;

/// A single regime signal with its computed value.
#[derive(Debug, Clone)]
pub struct RegimeSignal {
    /// Signal identifier (e.g. "VIX level"). Used for help/detail views.
    #[allow(dead_code)]
    pub name: &'static str,
    /// Short label for compact display (e.g. "VIX 23.7↑").
    pub label: String,
    /// +1 = risk-on, -1 = risk-off, 0 = no data.
    pub score: i8,
}

/// Full regime assessment.
#[derive(Debug, Clone)]
pub struct RegimeScore {
    pub signals: Vec<RegimeSignal>,
    /// Sum of all signal scores (-9 to +9).
    pub total: i8,
    /// How many signals had data (out of 9).
    pub active_count: u8,
}

impl RegimeScore {
    /// Human-readable regime label.
    pub fn label(&self) -> &'static str {
        if self.active_count == 0 {
            return "NO DATA";
        }
        match self.total {
            5..=9 => "RISK-ON",
            2..=4 => "LEAN RISK-ON",
            -1..=1 => "NEUTRAL",
            -4..=-2 => "LEAN RISK-OFF",
            _ => "RISK-OFF",
        }
    }

    /// Returns true if there is enough data to display the regime bar.
    pub fn has_data(&self) -> bool {
        self.active_count >= 3
    }
}

/// Yahoo symbols used by regime signals (for targeted fetching).
#[allow(dead_code)]
pub const REGIME_YAHOO_SYMBOLS: &[&str] = &[
    "^VIX",     // VIX
    "^TNX",     // 10Y Treasury yield
    "^IRX",     // 2Y (actually 13-week T-bill, proxy for short rates)
    "DX-Y.NYB", // DXY Dollar Index
    "GC=F",     // Gold Futures
    "^GSPC",    // S&P 500
    "BTC-USD",  // Bitcoin
    "HYG",      // High Yield Bond ETF
    "LQD",      // Investment Grade Bond ETF
    "HG=F",     // Copper Futures
];

/// Compute the composite regime score from available price data.
pub fn compute_regime(
    prices: &HashMap<String, Decimal>,
    history: &HashMap<String, Vec<HistoryRecord>>,
) -> RegimeScore {
    let signals = vec![
        // 1. VIX level
        signal_vix_level(prices),
        // 2. VIX 5D direction
        signal_direction(history, "^VIX", "VIX dir", true),
        // 3. 10Y yield direction (rising = risk-on)
        signal_direction(history, "^TNX", "10Y dir", false),
        // 4. 2Y-10Y spread
        signal_yield_spread(prices),
        // 5. DXY 5D direction (falling = risk-on → invert)
        signal_direction(history, "DX-Y.NYB", "DXY dir", true),
        // 6. Gold/SPX 5D ratio trend (falling = risk-on → invert)
        signal_ratio_direction(history, "GC=F", "^GSPC", "Au/SPX", true),
        // 7. BTC/SPX correlation
        signal_correlation(history, "BTC-USD", "^GSPC"),
        // 8. HYG/LQD ratio (rising = tightening spreads = risk-on)
        signal_ratio_direction(history, "HYG", "LQD", "HY sprd", false),
        // 9. Copper/Gold ratio (rising = risk-on)
        signal_ratio_direction(history, "HG=F", "GC=F", "Cu/Au", false),
    ];

    let total: i8 = signals.iter().map(|s| s.score).sum();
    let active_count = signals.iter().filter(|s| s.score != 0).count() as u8;

    RegimeScore {
        signals,
        total,
        active_count,
    }
}

// --- Individual signal functions ---

/// Signal 1: VIX level. < 20 = risk-on, ≥ 20 = risk-off.
fn signal_vix_level(prices: &HashMap<String, Decimal>) -> RegimeSignal {
    match prices.get("^VIX") {
        Some(&vix) => {
            let vix_f: f64 = decimal_to_f64(vix);
            let score: i8 = if vix < dec!(20) { 1 } else { -1 };
            let arrow = if score > 0 { "✓" } else { "!" };
            RegimeSignal {
                name: "VIX level",
                label: format!("VIX {:.1}{}", vix_f, arrow),
                score,
            }
        }
        None => RegimeSignal {
            name: "VIX level",
            label: "VIX ---".into(),
            score: 0,
        },
    }
}

/// Directional signal: compares latest price to price N days ago.
/// If `invert` is true, falling = risk-on (+1); otherwise rising = risk-on (+1).
fn signal_direction(
    history: &HashMap<String, Vec<HistoryRecord>>,
    symbol: &str,
    name: &'static str,
    invert: bool,
) -> RegimeSignal {
    match direction_pct(history, symbol, LOOKBACK_DAYS) {
        Some((pct, latest_f)) => {
            let rising = pct > 0.0;
            let score: i8 = if invert {
                if rising { -1 } else { 1 }
            } else if rising {
                1
            } else {
                -1
            };
            let arrow = if rising { "↑" } else { "↓" };
            // Shorten symbol for display
            let short = short_label(symbol);
            RegimeSignal {
                name,
                label: format!("{} {:.1}{}", short, latest_f, arrow),
                score,
            }
        }
        None => RegimeSignal {
            name,
            label: format!("{} ---", short_label(symbol)),
            score: 0,
        },
    }
}

/// Signal for the 2Y-10Y yield spread. Positive (normal curve) = risk-on.
fn signal_yield_spread(prices: &HashMap<String, Decimal>) -> RegimeSignal {
    match (prices.get("^TNX"), prices.get("^IRX")) {
        (Some(&ten_y), Some(&two_y)) => {
            let spread = ten_y - two_y;
            let spread_f = decimal_to_f64(spread);
            let score: i8 = if spread > dec!(0) { 1 } else { -1 };
            let sign = if spread_f >= 0.0 { "+" } else { "" };
            RegimeSignal {
                name: "2Y-10Y",
                label: format!("2s10s {}{:.2}", sign, spread_f),
                score,
            }
        }
        _ => RegimeSignal {
            name: "2Y-10Y",
            label: "2s10s ---".into(),
            score: 0,
        },
    }
}

/// Ratio direction signal: (A/B) trend over LOOKBACK_DAYS.
/// If `invert`, falling ratio = risk-on.
fn signal_ratio_direction(
    history: &HashMap<String, Vec<HistoryRecord>>,
    num_symbol: &str,
    den_symbol: &str,
    name: &'static str,
    invert: bool,
) -> RegimeSignal {
    let num_history = match history.get(num_symbol) {
        Some(h) if h.len() > LOOKBACK_DAYS => h,
        _ => {
            return RegimeSignal {
                name,
                label: format!("{} ---", name),
                score: 0,
            }
        }
    };
    let den_history = match history.get(den_symbol) {
        Some(h) if h.len() > LOOKBACK_DAYS => h,
        _ => {
            return RegimeSignal {
                name,
                label: format!("{} ---", name),
                score: 0,
            }
        }
    };

    // Compute ratio at latest and N days ago
    let n_len = num_history.len();
    let d_len = den_history.len();
    let latest_num = decimal_to_f64(num_history[n_len - 1].close);
    let latest_den = decimal_to_f64(den_history[d_len - 1].close);
    let past_num = decimal_to_f64(num_history[n_len - 1 - LOOKBACK_DAYS].close);
    let past_den = decimal_to_f64(den_history[d_len - 1 - LOOKBACK_DAYS].close);

    if latest_den.abs() < f64::EPSILON || past_den.abs() < f64::EPSILON {
        return RegimeSignal {
            name,
            label: format!("{} ---", name),
            score: 0,
        };
    }

    let latest_ratio = latest_num / latest_den;
    let past_ratio = past_num / past_den;
    let rising = latest_ratio > past_ratio;

    let score: i8 = if invert {
        if rising { -1 } else { 1 }
    } else if rising {
        1
    } else {
        -1
    };

    let arrow = if rising { "↑" } else { "↓" };
    RegimeSignal {
        name,
        label: format!("{}{}", name, arrow),
        score,
    }
}

/// BTC/SPX correlation signal over CORR_DAYS. Positive correlation = risk-on.
fn signal_correlation(
    history: &HashMap<String, Vec<HistoryRecord>>,
    sym_a: &str,
    sym_b: &str,
) -> RegimeSignal {
    let corr = compute_correlation(history, sym_a, sym_b, CORR_DAYS);
    match corr {
        Some(r) => {
            let score: i8 = if r > 0.0 { 1 } else { -1 };
            RegimeSignal {
                name: "BTC/SPX",
                label: format!("BTC/SPX {:.2}", r),
                score,
            }
        }
        None => RegimeSignal {
            name: "BTC/SPX",
            label: "BTC/SPX ---".into(),
            score: 0,
        },
    }
}

// --- Helpers ---

/// Compute percentage change over `days` for a symbol.
/// Returns (pct_change, latest_value).
fn direction_pct(
    history: &HashMap<String, Vec<HistoryRecord>>,
    symbol: &str,
    days: usize,
) -> Option<(f64, f64)> {
    let records = history.get(symbol)?;
    if records.len() <= days {
        return None;
    }
    let latest = decimal_to_f64(records[records.len() - 1].close);
    let past = decimal_to_f64(records[records.len() - 1 - days].close);
    if past.abs() < f64::EPSILON {
        return None;
    }
    Some(((latest - past) / past * 100.0, latest))
}

/// Pearson correlation of daily returns over `days`.
fn compute_correlation(
    history: &HashMap<String, Vec<HistoryRecord>>,
    sym_a: &str,
    sym_b: &str,
    days: usize,
) -> Option<f64> {
    let hist_a = history.get(sym_a)?;
    let hist_b = history.get(sym_b)?;
    if hist_a.len() < days + 1 || hist_b.len() < days + 1 {
        return None;
    }

    // Compute daily returns for last `days` days
    let returns_a: Vec<f64> = hist_a
        .iter()
        .rev()
        .take(days + 1)
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| {
            let prev = decimal_to_f64(w[1].close); // reversed order
            let curr = decimal_to_f64(w[0].close);
            if prev.abs() < f64::EPSILON {
                0.0
            } else {
                (curr - prev) / prev
            }
        })
        .collect();

    let returns_b: Vec<f64> = hist_b
        .iter()
        .rev()
        .take(days + 1)
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| {
            let prev = decimal_to_f64(w[1].close);
            let curr = decimal_to_f64(w[0].close);
            if prev.abs() < f64::EPSILON {
                0.0
            } else {
                (curr - prev) / prev
            }
        })
        .collect();

    if returns_a.len() < 2 || returns_a.len() != returns_b.len() {
        return None;
    }

    let n = returns_a.len() as f64;
    let mean_a: f64 = returns_a.iter().sum::<f64>() / n;
    let mean_b: f64 = returns_b.iter().sum::<f64>() / n;

    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for i in 0..returns_a.len() {
        let da = returns_a[i] - mean_a;
        let db = returns_b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }

    let denom = (var_a * var_b).sqrt();
    if denom < f64::EPSILON {
        return None;
    }

    Some(cov / denom)
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

fn short_label(symbol: &str) -> &str {
    match symbol {
        "^VIX" => "VIX",
        "^TNX" => "10Y",
        "^IRX" => "2Y",
        "DX-Y.NYB" => "DXY",
        _ => symbol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_history(prices: &[f64]) -> Vec<HistoryRecord> {
        prices
            .iter()
            .enumerate()
            .map(|(i, &p)| HistoryRecord {
                date: format!("2026-02-{:02}", i + 1),
                close: Decimal::from_str_exact(&format!("{:.2}", p))
                    .unwrap_or_else(|_| Decimal::new((p * 100.0) as i64, 2)),
                volume: None,
            })
            .collect()
    }

    #[test]
    fn regime_score_label_extremes() {
        let score = RegimeScore {
            signals: vec![],
            total: 7,
            active_count: 9,
        };
        assert_eq!(score.label(), "RISK-ON");

        let score = RegimeScore {
            signals: vec![],
            total: -6,
            active_count: 9,
        };
        assert_eq!(score.label(), "RISK-OFF");
    }

    #[test]
    fn regime_score_label_neutral() {
        let score = RegimeScore {
            signals: vec![],
            total: 0,
            active_count: 5,
        };
        assert_eq!(score.label(), "NEUTRAL");
    }

    #[test]
    fn regime_score_no_data() {
        let score = RegimeScore {
            signals: vec![],
            total: 0,
            active_count: 0,
        };
        assert_eq!(score.label(), "NO DATA");
        assert!(!score.has_data());
    }

    #[test]
    fn regime_score_has_data_threshold() {
        let score = RegimeScore {
            signals: vec![],
            total: 2,
            active_count: 3,
        };
        assert!(score.has_data());

        let score = RegimeScore {
            signals: vec![],
            total: 1,
            active_count: 2,
        };
        assert!(!score.has_data());
    }

    #[test]
    fn signal_vix_level_low() {
        let mut prices = HashMap::new();
        prices.insert("^VIX".to_string(), dec!(15.5));
        let sig = signal_vix_level(&prices);
        assert_eq!(sig.score, 1);
        assert!(sig.label.contains("15.5"));
    }

    #[test]
    fn signal_vix_level_high() {
        let mut prices = HashMap::new();
        prices.insert("^VIX".to_string(), dec!(25.3));
        let sig = signal_vix_level(&prices);
        assert_eq!(sig.score, -1);
    }

    #[test]
    fn signal_vix_level_missing() {
        let prices = HashMap::new();
        let sig = signal_vix_level(&prices);
        assert_eq!(sig.score, 0);
    }

    #[test]
    fn signal_direction_rising() {
        let mut history = HashMap::new();
        // 10 days of rising prices
        let prices: Vec<f64> = (1..=10).map(|i| 100.0 + i as f64).collect();
        history.insert("^TNX".to_string(), make_history(&prices));

        // Rising 10Y = risk-on (invert=false)
        let sig = signal_direction(&history, "^TNX", "10Y dir", false);
        assert_eq!(sig.score, 1);
    }

    #[test]
    fn signal_direction_falling_inverted() {
        let mut history = HashMap::new();
        // Falling VIX
        let prices: Vec<f64> = (0..10).rev().map(|i| 20.0 + i as f64).collect();
        history.insert("^VIX".to_string(), make_history(&prices));

        // Falling VIX with invert=true = risk-on
        let sig = signal_direction(&history, "^VIX", "VIX dir", true);
        assert_eq!(sig.score, 1);
    }

    #[test]
    fn signal_yield_spread_positive() {
        let mut prices = HashMap::new();
        prices.insert("^TNX".to_string(), dec!(4.50)); // 10Y
        prices.insert("^IRX".to_string(), dec!(4.00)); // 2Y proxy
        let sig = signal_yield_spread(&prices);
        assert_eq!(sig.score, 1); // Normal curve = risk-on
        assert!(sig.label.contains("+0.50"));
    }

    #[test]
    fn signal_yield_spread_inverted() {
        let mut prices = HashMap::new();
        prices.insert("^TNX".to_string(), dec!(3.80));
        prices.insert("^IRX".to_string(), dec!(4.20));
        let sig = signal_yield_spread(&prices);
        assert_eq!(sig.score, -1); // Inverted = risk-off
    }

    #[test]
    fn signal_ratio_direction_rising() {
        let mut history = HashMap::new();
        let num_prices: Vec<f64> = (1..=10).map(|i| 100.0 + i as f64 * 2.0).collect();
        let den_prices: Vec<f64> = (1..=10).map(|i| 100.0 + i as f64).collect();
        history.insert("HG=F".to_string(), make_history(&num_prices));
        history.insert("GC=F".to_string(), make_history(&den_prices));

        // Copper/Gold rising, invert=false → risk-on
        let sig = signal_ratio_direction(&history, "HG=F", "GC=F", "Cu/Au", false);
        assert_eq!(sig.score, 1);
    }

    #[test]
    fn correlation_positive() {
        let mut history = HashMap::new();
        // Both rising together → positive correlation
        let prices_a: Vec<f64> = (0..25).map(|i| 100.0 + i as f64).collect();
        let prices_b: Vec<f64> = (0..25).map(|i| 50.0 + i as f64 * 0.5).collect();
        history.insert("BTC-USD".to_string(), make_history(&prices_a));
        history.insert("^GSPC".to_string(), make_history(&prices_b));

        let corr = compute_correlation(&history, "BTC-USD", "^GSPC", 20);
        assert!(corr.is_some());
        assert!(corr.unwrap() > 0.5);
    }

    #[test]
    fn correlation_negative() {
        let mut history = HashMap::new();
        // Anti-correlated: when A goes up, B goes down, alternating.
        let prices_a: Vec<f64> = (0..25)
            .map(|i| if i % 2 == 0 { 100.0 + i as f64 } else { 100.0 - i as f64 })
            .collect();
        let prices_b: Vec<f64> = (0..25)
            .map(|i| if i % 2 == 0 { 100.0 - i as f64 } else { 100.0 + i as f64 })
            .collect();
        history.insert("BTC-USD".to_string(), make_history(&prices_a));
        history.insert("^GSPC".to_string(), make_history(&prices_b));

        let corr = compute_correlation(&history, "BTC-USD", "^GSPC", 20);
        assert!(corr.is_some());
        assert!(corr.unwrap() < -0.5, "expected negative correlation, got {}", corr.unwrap());
    }

    #[test]
    fn correlation_insufficient_data() {
        let history = HashMap::new();
        let corr = compute_correlation(&history, "BTC-USD", "^GSPC", 20);
        assert!(corr.is_none());
    }

    #[test]
    fn compute_regime_empty_data() {
        let prices = HashMap::new();
        let history = HashMap::new();
        let regime = compute_regime(&prices, &history);
        assert_eq!(regime.total, 0);
        assert_eq!(regime.active_count, 0);
        assert!(!regime.has_data());
    }

    #[test]
    fn compute_regime_full_risk_on() {
        let mut prices = HashMap::new();
        let mut history = HashMap::new();

        // VIX low
        prices.insert("^VIX".to_string(), dec!(14));
        // VIX falling
        let vix_prices: Vec<f64> = (0..10).rev().map(|i| 14.0 + i as f64).collect();
        history.insert("^VIX".to_string(), make_history(&vix_prices));
        // 10Y rising
        let tny_prices: Vec<f64> = (0..10).map(|i| 4.0 + i as f64 * 0.1).collect();
        history.insert("^TNX".to_string(), make_history(&tny_prices));
        // Normal yield curve
        prices.insert("^TNX".to_string(), dec!(4.50));
        prices.insert("^IRX".to_string(), dec!(4.00));
        // DXY falling
        let dxy_prices: Vec<f64> = (0..10).rev().map(|i| 100.0 + i as f64).collect();
        history.insert("DX-Y.NYB".to_string(), make_history(&dxy_prices));
        // Gold/SPX falling (Gold underperforming)
        let gold_prices: Vec<f64> = (0..10).map(|_| 2000.0).collect();
        let spx_prices: Vec<f64> = (0..10).map(|i| 5000.0 + i as f64 * 50.0).collect();
        history.insert("GC=F".to_string(), make_history(&gold_prices));
        history.insert("^GSPC".to_string(), make_history(&spx_prices));
        // BTC/SPX positive correlation
        let btc_prices: Vec<f64> = (0..25).map(|i| 50000.0 + i as f64 * 500.0).collect();
        let spx_long: Vec<f64> = (0..25).map(|i| 5000.0 + i as f64 * 20.0).collect();
        history.insert("BTC-USD".to_string(), make_history(&btc_prices));
        // Overwrite SPX with longer history for correlation
        history.insert("^GSPC".to_string(), make_history(&spx_long));
        // HYG/LQD rising
        let hyg_prices: Vec<f64> = (0..10).map(|i| 75.0 + i as f64 * 0.3).collect();
        let lqd_prices: Vec<f64> = (0..10).map(|_| 110.0).collect();
        history.insert("HYG".to_string(), make_history(&hyg_prices));
        history.insert("LQD".to_string(), make_history(&lqd_prices));
        // Copper/Gold rising
        let cu_prices: Vec<f64> = (0..10).map(|i| 4.0 + i as f64 * 0.1).collect();
        history.insert("HG=F".to_string(), make_history(&cu_prices));

        let regime = compute_regime(&prices, &history);
        assert!(regime.has_data());
        // Should be mostly or fully risk-on
        assert!(regime.total >= 5, "Expected risk-on, got total={}", regime.total);
    }

    #[test]
    fn regime_signal_count_is_nine() {
        let prices = HashMap::new();
        let history = HashMap::new();
        let regime = compute_regime(&prices, &history);
        assert_eq!(regime.signals.len(), 9);
    }

    #[test]
    fn regime_yahoo_symbols_count() {
        assert_eq!(REGIME_YAHOO_SYMBOLS.len(), 10);
    }

    #[test]
    fn direction_pct_insufficient_data() {
        let mut history = HashMap::new();
        // Only 3 records, need > 5
        history.insert("^VIX".to_string(), make_history(&[20.0, 21.0, 22.0]));
        let result = direction_pct(&history, "^VIX", 5);
        assert!(result.is_none());
    }

    #[test]
    fn direction_pct_correct_value() {
        let mut history = HashMap::new();
        // 10 prices: 100.0, 101.0, ... 109.0
        let prices: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        history.insert("TEST".to_string(), make_history(&prices));
        let result = direction_pct(&history, "TEST", 5);
        assert!(result.is_some());
        let (pct, latest) = result.unwrap();
        assert!((latest - 109.0).abs() < 0.01);
        // (109 - 104) / 104 * 100 ≈ 4.81%
        assert!((pct - 4.81).abs() < 0.1);
    }
}
