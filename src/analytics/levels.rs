use serde::{Deserialize, Serialize};

use crate::db::technical_levels::TechnicalLevelRecord;
use crate::db::technical_snapshots::TechnicalSnapshotRecord;
use crate::models::price::HistoryRecord;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionableLevel {
    pub symbol: String,
    pub level_type: String,
    pub price: f64,
    pub strength: f64,
    pub source_method: String,
    pub timeframe: String,
    pub notes: Option<String>,
    pub computed_at: String,
    pub distance_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ActionableLevelPair {
    pub support: Option<ActionableLevel>,
    pub resistance: Option<ActionableLevel>,
}

/// Compute market structure levels from price history and optional technical snapshot.
///
/// Produces support/resistance via swing pivots, moving-average levels from the snapshot,
/// Bollinger band boundaries, 52-week extremes, and recent swing highs/lows.
pub fn compute_levels(
    symbol: &str,
    timeframe: &str,
    history: &[HistoryRecord],
    snapshot: Option<&TechnicalSnapshotRecord>,
) -> Vec<TechnicalLevelRecord> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut levels: Vec<TechnicalLevelRecord> = Vec::new();

    if history.is_empty() {
        return levels;
    }

    let closes: Vec<f64> = history
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    let latest_close = match closes.last() {
        Some(c) => *c,
        None => return levels,
    };

    // -----------------------------------------------------------------------
    // 1. Moving average levels from technical snapshot
    // -----------------------------------------------------------------------
    if let Some(snap) = snapshot {
        if let Some(sma) = snap.sma_20 {
            levels.push(make_level(
                symbol,
                if latest_close > sma {
                    "support"
                } else {
                    "resistance"
                },
                sma,
                0.6,
                "moving_average",
                timeframe,
                Some("SMA 20"),
                &now,
            ));
        }
        if let Some(sma) = snap.sma_50 {
            levels.push(make_level(
                symbol,
                if latest_close > sma {
                    "support"
                } else {
                    "resistance"
                },
                sma,
                0.75,
                "moving_average",
                timeframe,
                Some("SMA 50"),
                &now,
            ));
        }
        if let Some(sma) = snap.sma_200 {
            levels.push(make_level(
                symbol,
                if latest_close > sma {
                    "support"
                } else {
                    "resistance"
                },
                sma,
                0.9,
                "moving_average",
                timeframe,
                Some("SMA 200"),
                &now,
            ));
        }

        // Bollinger bands
        if let Some(bb_upper) = snap.bollinger_upper {
            levels.push(make_level(
                symbol,
                "bb_upper",
                bb_upper,
                0.5,
                "bollinger",
                timeframe,
                Some("BB upper (20,2)"),
                &now,
            ));
        }
        if let Some(bb_lower) = snap.bollinger_lower {
            levels.push(make_level(
                symbol,
                "bb_lower",
                bb_lower,
                0.5,
                "bollinger",
                timeframe,
                Some("BB lower (20,2)"),
                &now,
            ));
        }

        // 52-week range extremes
        if let Some(low) = snap.range_52w_low {
            levels.push(make_level(
                symbol,
                "range_52w_low",
                low,
                0.85,
                "range",
                timeframe,
                Some("52-week low"),
                &now,
            ));
        }
        if let Some(high) = snap.range_52w_high {
            levels.push(make_level(
                symbol,
                "range_52w_high",
                high,
                0.85,
                "range",
                timeframe,
                Some("52-week high"),
                &now,
            ));
        }
    }

    // -----------------------------------------------------------------------
    // 2. Swing highs and lows (pivot detection on closes)
    // -----------------------------------------------------------------------
    let swing_window = 5; // look 5 bars left/right
    let min_bars = swing_window * 2 + 1;
    if closes.len() >= min_bars {
        // Only scan the most recent 120 bars for relevance
        let scan_start = if closes.len() > 120 {
            closes.len() - 120
        } else {
            0
        };

        let mut swing_highs: Vec<f64> = Vec::new();
        let mut swing_lows: Vec<f64> = Vec::new();

        for i in (scan_start + swing_window)..(closes.len() - swing_window) {
            let val = closes[i];
            let left = &closes[i - swing_window..i];
            let right = &closes[i + 1..=i + swing_window];

            if left.iter().all(|&v| val >= v) && right.iter().all(|&v| val >= v) {
                swing_highs.push(val);
            }
            if left.iter().all(|&v| val <= v) && right.iter().all(|&v| val <= v) {
                swing_lows.push(val);
            }
        }

        // Cluster nearby swings (within 1.5% of each other) and keep strongest
        let clustered_highs = cluster_levels(&swing_highs, 0.015);
        let clustered_lows = cluster_levels(&swing_lows, 0.015);

        for (price, count) in &clustered_highs {
            let strength = (0.5 + (*count as f64 * 0.15)).min(1.0);
            let lt = if latest_close < *price {
                "resistance"
            } else {
                "swing_high"
            };
            levels.push(make_level(
                symbol,
                lt,
                *price,
                strength,
                "swing",
                timeframe,
                Some(&format!("Swing high (tested {}x)", count)),
                &now,
            ));
        }

        for (price, count) in &clustered_lows {
            let strength = (0.5 + (*count as f64 * 0.15)).min(1.0);
            let lt = if latest_close > *price {
                "support"
            } else {
                "swing_low"
            };
            levels.push(make_level(
                symbol,
                lt,
                *price,
                strength,
                "swing",
                timeframe,
                Some(&format!("Swing low (tested {}x)", count)),
                &now,
            ));
        }
    }

    // -----------------------------------------------------------------------
    // 3. Round-number levels near current price (psychological levels)
    // -----------------------------------------------------------------------
    let magnitude = round_number_step(latest_close);
    if magnitude > 0.0 {
        let base = (latest_close / magnitude).floor() * magnitude;
        for i in -2i32..=3 {
            let rn = base + (i as f64) * magnitude;
            if rn > 0.0 {
                let dist = ((rn - latest_close) / latest_close).abs();
                // Only include levels within 10% of current price
                if dist < 0.10 {
                    let lt = if rn < latest_close {
                        "support"
                    } else {
                        "resistance"
                    };
                    levels.push(make_level(
                        symbol,
                        lt,
                        rn,
                        0.4,
                        "pivot",
                        timeframe,
                        Some(&format!("Round number {}", format_price(rn))),
                        &now,
                    ));
                }
            }
        }
    }

    // Sort by price ascending
    levels.sort_by(|a, b| {
        a.price
            .partial_cmp(&b.price)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Deduplicate very close levels (within 0.3% of each other), keeping highest strength
    dedup_close_levels(&mut levels, 0.003);

    levels
}

pub fn nearest_actionable_levels(
    levels: &[TechnicalLevelRecord],
    price: f64,
) -> ActionableLevelPair {
    ActionableLevelPair {
        support: select_actionable_level(levels, price, "support"),
        resistance: select_actionable_level(levels, price, "resistance"),
    }
}

pub fn select_actionable_level(
    levels: &[TechnicalLevelRecord],
    price: f64,
    selector: &str,
) -> Option<ActionableLevel> {
    let normalized = selector.trim().to_lowercase();
    match normalized.as_str() {
        "support" => levels
            .iter()
            .filter(|level| {
                level.price < price
                    && (is_support_type(&level.level_type) || level.level_type.starts_with("sma_"))
            })
            .max_by(|a, b| {
                a.price
                    .partial_cmp(&b.price)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|level| actionable_level(level, price)),
        "resistance" => levels
            .iter()
            .filter(|level| {
                level.price > price
                    && (is_resistance_type(&level.level_type)
                        || level.level_type.starts_with("sma_"))
            })
            .min_by(|a, b| {
                a.price
                    .partial_cmp(&b.price)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|level| actionable_level(level, price)),
        exact_type => levels
            .iter()
            .filter(|level| level.level_type.eq_ignore_ascii_case(exact_type))
            .min_by(|a, b| {
                (a.price - price)
                    .abs()
                    .partial_cmp(&(b.price - price).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|level| actionable_level(level, price)),
    }
}

/// Cluster nearby price points (within `tolerance` fraction of each other).
/// Returns (representative_price, count) pairs.
fn cluster_levels(prices: &[f64], tolerance: f64) -> Vec<(f64, usize)> {
    if prices.is_empty() {
        return Vec::new();
    }
    let mut sorted = prices.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut clusters: Vec<(f64, usize)> = Vec::new();
    let mut cluster_sum = sorted[0];
    let mut cluster_count = 1usize;
    let mut cluster_start = sorted[0];

    for &price in &sorted[1..] {
        if (price - cluster_start) / cluster_start <= tolerance {
            cluster_sum += price;
            cluster_count += 1;
        } else {
            clusters.push((cluster_sum / cluster_count as f64, cluster_count));
            cluster_sum = price;
            cluster_count = 1;
            cluster_start = price;
        }
    }
    clusters.push((cluster_sum / cluster_count as f64, cluster_count));
    clusters
}

/// Remove levels that are within `tolerance` fraction of each other, keeping the higher-strength one.
fn dedup_close_levels(levels: &mut Vec<TechnicalLevelRecord>, tolerance: f64) {
    if levels.len() < 2 {
        return;
    }
    let mut keep = vec![true; levels.len()];
    for i in 0..levels.len() {
        if !keep[i] {
            continue;
        }
        for j in (i + 1)..levels.len() {
            if !keep[j] {
                continue;
            }
            let ref_price = levels[i].price.max(levels[j].price);
            if ref_price == 0.0 {
                continue;
            }
            let dist = (levels[i].price - levels[j].price).abs() / ref_price;
            if dist <= tolerance {
                // Drop the weaker one
                if levels[j].strength > levels[i].strength {
                    keep[i] = false;
                    break;
                } else {
                    keep[j] = false;
                }
            }
        }
    }
    let mut idx = 0;
    levels.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
}

/// Choose round-number step size based on asset price magnitude.
fn round_number_step(price: f64) -> f64 {
    if price >= 10000.0 {
        5000.0
    } else if price >= 1000.0 {
        500.0
    } else if price >= 100.0 {
        50.0
    } else if price >= 10.0 {
        5.0
    } else if price >= 1.0 {
        0.5
    } else {
        0.0 // skip for sub-dollar
    }
}

fn format_price(price: f64) -> String {
    if price >= 1000.0 {
        format!("{:.0}", price)
    } else {
        format!("{:.2}", price)
    }
}

fn actionable_level(level: &TechnicalLevelRecord, price: f64) -> ActionableLevel {
    let distance_pct = if price > 0.0 {
        ((level.price - price).abs() / price) * 100.0
    } else {
        0.0
    };

    ActionableLevel {
        symbol: level.symbol.clone(),
        level_type: level.level_type.clone(),
        price: level.price,
        strength: level.strength,
        source_method: level.source_method.clone(),
        timeframe: level.timeframe.clone(),
        notes: level.notes.clone(),
        computed_at: level.computed_at.clone(),
        distance_pct,
    }
}

fn is_support_type(level_type: &str) -> bool {
    matches!(
        level_type,
        "support" | "swing_low" | "bb_lower" | "range_52w_low"
    )
}

fn is_resistance_type(level_type: &str) -> bool {
    matches!(
        level_type,
        "resistance" | "swing_high" | "bb_upper" | "range_52w_high"
    )
}

#[allow(clippy::too_many_arguments)]
fn make_level(
    symbol: &str,
    level_type: &str,
    price: f64,
    strength: f64,
    source_method: &str,
    timeframe: &str,
    notes: Option<&str>,
    computed_at: &str,
) -> TechnicalLevelRecord {
    TechnicalLevelRecord {
        id: None,
        symbol: symbol.to_string(),
        level_type: level_type.to_string(),
        price,
        strength,
        source_method: source_method.to_string(),
        timeframe: timeframe.to_string(),
        notes: notes.map(|s| s.to_string()),
        computed_at: computed_at.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn build_history(days: usize, base: f64) -> Vec<HistoryRecord> {
        (0..days)
            .map(|i| {
                // Sine wave around base to create natural swings
                let offset = (i as f64 * 0.15).sin() * base * 0.05;
                let close = base + offset + (i as f64 * 0.01);
                HistoryRecord {
                    date: format!("2026-01-{:02}", (i % 28) + 1),
                    close: Decimal::from_f64_retain(close).unwrap_or(dec!(100)),
                    volume: Some(1_000 + (i as u64 * 10)),
                    open: None,
                    high: None,
                    low: None,
                }
            })
            .collect()
    }

    fn make_snapshot(sma_20: f64, sma_50: f64, sma_200: f64) -> TechnicalSnapshotRecord {
        TechnicalSnapshotRecord {
            symbol: "TEST".to_string(),
            timeframe: "1d".to_string(),
            rsi_14: Some(55.0),
            macd: Some(0.5),
            macd_signal: Some(0.3),
            macd_histogram: Some(0.2),
            sma_20: Some(sma_20),
            sma_50: Some(sma_50),
            sma_200: Some(sma_200),
            bollinger_upper: Some(sma_20 * 1.05),
            bollinger_middle: Some(sma_20),
            bollinger_lower: Some(sma_20 * 0.95),
            range_52w_low: Some(sma_200 * 0.8),
            range_52w_high: Some(sma_20 * 1.15),
            range_52w_position: Some(60.0),
            volume_avg_20: Some(5000.0),
            volume_ratio_20: Some(1.1),
            volume_regime: Some("normal".to_string()),
            above_sma_20: Some(true),
            above_sma_50: Some(true),
            above_sma_200: Some(true),
            computed_at: "2026-03-18T16:00:00Z".to_string(),
        }
    }

    #[test]
    fn compute_levels_returns_ma_levels_from_snapshot() {
        let history = build_history(260, 100.0);
        let snapshot = make_snapshot(101.0, 99.0, 95.0);
        let levels = compute_levels("TEST", "1d", &history, Some(&snapshot));

        // Should have MA levels
        let ma_levels: Vec<_> = levels
            .iter()
            .filter(|l| l.source_method == "moving_average")
            .collect();
        assert!(
            ma_levels.len() >= 2,
            "expected at least 2 MA levels, got {}",
            ma_levels.len()
        );
    }

    #[test]
    fn compute_levels_returns_bollinger_and_range() {
        let history = build_history(260, 100.0);
        let snapshot = make_snapshot(101.0, 99.0, 95.0);
        let levels = compute_levels("TEST", "1d", &history, Some(&snapshot));

        assert!(
            levels
                .iter()
                .any(|l| l.level_type == "bb_upper" || l.level_type == "bb_lower"),
            "expected Bollinger levels"
        );
        assert!(
            levels
                .iter()
                .any(|l| l.level_type == "range_52w_low" || l.level_type == "range_52w_high"),
            "expected 52w range levels"
        );
    }

    #[test]
    fn compute_levels_without_snapshot_still_produces_swings() {
        let history = build_history(120, 100.0);
        let levels = compute_levels("TEST", "1d", &history, None);
        // Should have swing and/or round-number levels
        assert!(
            !levels.is_empty(),
            "expected at least some levels from swing/round-number detection"
        );
    }

    #[test]
    fn compute_levels_empty_history_returns_empty() {
        let levels = compute_levels("TEST", "1d", &[], None);
        assert!(levels.is_empty());
    }

    #[test]
    fn cluster_levels_merges_nearby() {
        let prices = vec![100.0, 100.5, 101.0, 110.0, 110.2];
        let clusters = cluster_levels(&prices, 0.015);
        assert_eq!(clusters.len(), 2, "expected 2 clusters");
    }

    #[test]
    fn dedup_keeps_stronger_level() {
        let now = "2026-03-18T16:00:00Z".to_string();
        let mut levels = vec![
            make_level("X", "support", 100.0, 0.5, "pivot", "1d", None, &now),
            make_level("X", "support", 100.2, 0.8, "swing", "1d", None, &now),
        ];
        dedup_close_levels(&mut levels, 0.003);
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].strength, 0.8);
    }

    #[test]
    fn round_number_step_tiers() {
        assert_eq!(round_number_step(85000.0), 5000.0);
        assert_eq!(round_number_step(2700.0), 500.0);
        assert_eq!(round_number_step(170.0), 50.0);
        assert_eq!(round_number_step(32.0), 5.0);
        assert_eq!(round_number_step(0.5), 0.0);
    }

    #[test]
    fn nearest_actionable_levels_selects_support_and_resistance() {
        let now = "2026-03-18T16:00:00Z".to_string();
        let levels = vec![
            make_level("X", "support", 150.0, 0.4, "pivot", "1d", None, &now),
            make_level("X", "support", 155.0, 0.8, "swing", "1d", None, &now),
            make_level("X", "resistance", 165.0, 0.7, "pivot", "1d", None, &now),
            make_level("X", "resistance", 172.0, 0.9, "swing", "1d", None, &now),
        ];

        let pair = nearest_actionable_levels(&levels, 160.0);
        assert_eq!(pair.support.unwrap().price, 155.0);
        assert_eq!(pair.resistance.unwrap().price, 165.0);
    }

    #[test]
    fn select_actionable_level_uses_exact_type_when_requested() {
        let now = "2026-03-18T16:00:00Z".to_string();
        let levels = vec![
            make_level("X", "bb_upper", 72000.0, 0.5, "bollinger", "1d", None, &now),
            make_level("X", "bb_upper", 76000.0, 0.5, "bollinger", "1d", None, &now),
        ];

        let selected = select_actionable_level(&levels, 73000.0, "bb_upper").unwrap();
        assert_eq!(selected.price, 72000.0);
        assert!(selected.distance_pct < 2.0);
    }
}
