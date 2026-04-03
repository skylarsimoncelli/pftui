use std::collections::HashMap;

use crate::db::backend::BackendConnection;
use crate::db::price_history::{get_history_batch, get_history_batch_backend};
use crate::db::technical_snapshots::{
    get_latest_snapshots_batch, get_latest_snapshots_batch_backend, TechnicalSnapshotRecord,
};
use crate::indicators::atr::compute_atr;
use crate::indicators::bollinger::compute_bollinger;
use crate::indicators::compute_sma;
use crate::indicators::{compute_macd, compute_rsi};
use crate::models::price::HistoryRecord;
use rusqlite::Connection;

pub const DEFAULT_TIMEFRAME: &str = "1d";

pub fn compute_snapshot(
    symbol: &str,
    timeframe: &str,
    history: &[HistoryRecord],
) -> Option<TechnicalSnapshotRecord> {
    if history.len() < 14 {
        return None;
    }

    let closes: Vec<f64> = history
        .iter()
        .map(|row| row.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();
    if closes.len() < 14 {
        return None;
    }

    let latest_close = *closes.last()?;

    let rsi_14 = compute_rsi(&closes, 14).iter().rev().find_map(|v| *v);
    let macd = compute_macd(&closes, 12, 26, 9)
        .iter()
        .rev()
        .find_map(|v| *v);
    let sma_20 = compute_sma(&closes, 20).iter().rev().find_map(|v| *v);
    let sma_50 = compute_sma(&closes, 50).iter().rev().find_map(|v| *v);
    let sma_200 = compute_sma(&closes, 200).iter().rev().find_map(|v| *v);
    let bollinger = compute_bollinger(&closes, 20, 2.0)
        .iter()
        .rev()
        .find_map(|v| *v);

    let range_slice = if history.len() > 252 {
        &closes[closes.len() - 252..]
    } else {
        &closes[..]
    };
    let range_52w_low = range_slice.iter().copied().reduce(f64::min);
    let range_52w_high = range_slice.iter().copied().reduce(f64::max);
    let range_52w_position = match (range_52w_low, range_52w_high) {
        (Some(low), Some(high)) if high > low => {
            Some(((latest_close - low) / (high - low)) * 100.0)
        }
        _ => None,
    };

    let recent_volume: Vec<f64> = history
        .iter()
        .rev()
        .take(20)
        .filter_map(|row| row.volume.map(|v| v as f64))
        .collect();
    let latest_volume = history.last().and_then(|row| row.volume).map(|v| v as f64);
    let volume_avg_20 = if recent_volume.is_empty() {
        None
    } else {
        Some(recent_volume.iter().sum::<f64>() / recent_volume.len() as f64)
    };
    let volume_ratio_20 = match (latest_volume, volume_avg_20) {
        (Some(latest), Some(avg)) if avg > 0.0 => Some(latest / avg),
        _ => None,
    };
    let volume_regime = volume_ratio_20.map(|ratio| {
        if ratio >= 1.5 {
            "high".to_string()
        } else if ratio <= 0.75 {
            "low".to_string()
        } else {
            "normal".to_string()
        }
    });

    // OHLCV-aware ATR computation (F48 step 2)
    let highs: Vec<Option<f64>> = history
        .iter()
        .map(|row| {
            row.high
                .as_ref()
                .and_then(|d| d.to_string().parse::<f64>().ok())
        })
        .collect();
    let lows: Vec<Option<f64>> = history
        .iter()
        .map(|row| {
            row.low
                .as_ref()
                .and_then(|d| d.to_string().parse::<f64>().ok())
        })
        .collect();

    let atr_series = compute_atr(&highs, &lows, &closes, 14);
    let atr_14 = atr_series.iter().rev().find_map(|v| *v);

    let atr_ratio = atr_14.and_then(|atr| {
        if latest_close > 0.0 {
            Some((atr / latest_close) * 100.0)
        } else {
            None
        }
    });

    // Range expansion: current ATR > 1.5x the 20-period simple average of ATR
    let range_expansion = if atr_series.len() >= 20 {
        let recent_atrs: Vec<f64> = atr_series
            .iter()
            .rev()
            .take(20)
            .filter_map(|v| *v)
            .collect();
        if recent_atrs.len() >= 2 {
            let atr_avg = recent_atrs.iter().sum::<f64>() / recent_atrs.len() as f64;
            atr_14.map(|current| current > atr_avg * 1.5)
        } else {
            None
        }
    } else {
        None
    };

    // Day range ratio: today's (high - low) / ATR
    let day_range_ratio = if let (Some(last_h), Some(last_l), Some(atr)) = (
        highs.last().copied().flatten(),
        lows.last().copied().flatten(),
        atr_14,
    ) {
        if atr > 0.0 {
            Some((last_h - last_l) / atr)
        } else {
            None
        }
    } else {
        None
    };

    Some(TechnicalSnapshotRecord {
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        rsi_14,
        macd: macd.map(|m| m.macd),
        macd_signal: macd.map(|m| m.signal),
        macd_histogram: macd.map(|m| m.histogram),
        sma_20,
        sma_50,
        sma_200,
        bollinger_upper: bollinger.as_ref().map(|b| b.upper),
        bollinger_middle: bollinger.as_ref().map(|b| b.middle),
        bollinger_lower: bollinger.as_ref().map(|b| b.lower),
        range_52w_low,
        range_52w_high,
        range_52w_position,
        volume_avg_20,
        volume_ratio_20,
        volume_regime,
        above_sma_20: sma_20.map(|v| latest_close >= v),
        above_sma_50: sma_50.map(|v| latest_close >= v),
        above_sma_200: sma_200.map(|v| latest_close >= v),
        atr_14,
        atr_ratio,
        range_expansion,
        day_range_ratio,
        computed_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub fn load_or_compute_snapshots(
    conn: &Connection,
    symbols: &[String],
    timeframe: &str,
) -> HashMap<String, TechnicalSnapshotRecord> {
    if symbols.is_empty() {
        return HashMap::new();
    }

    // Batch-fetch all cached snapshots in one query instead of N individual queries
    let mut out = get_latest_snapshots_batch(conn, symbols, timeframe).unwrap_or_default();

    // Identify symbols that weren't in the snapshot cache
    let missing: Vec<String> = symbols
        .iter()
        .filter(|s| !out.contains_key(s.as_str()))
        .cloned()
        .collect();

    if missing.is_empty() {
        return out;
    }

    // Batch-fetch price history for all missing symbols in one query
    let history_map = get_history_batch(conn, &missing, 370).unwrap_or_default();

    for (symbol, history) in history_map {
        if history.is_empty() {
            continue;
        }
        if let Some(snapshot) = compute_snapshot(&symbol, timeframe, &history) {
            out.insert(symbol, snapshot);
        }
    }

    out
}

pub fn load_or_compute_snapshots_backend(
    backend: &BackendConnection,
    symbols: &[String],
    timeframe: &str,
) -> HashMap<String, TechnicalSnapshotRecord> {
    if symbols.is_empty() {
        return HashMap::new();
    }

    // Batch-fetch all cached snapshots in one query instead of N individual queries
    let mut out =
        get_latest_snapshots_batch_backend(backend, symbols, timeframe).unwrap_or_default();

    // Identify symbols that weren't in the snapshot cache
    let missing: Vec<String> = symbols
        .iter()
        .filter(|s| !out.contains_key(s.as_str()))
        .cloned()
        .collect();

    if missing.is_empty() {
        return out;
    }

    // Batch-fetch price history for all missing symbols in one query
    let history_map = get_history_batch_backend(backend, &missing, 370).unwrap_or_default();

    for (symbol, history) in history_map {
        if history.is_empty() {
            continue;
        }
        if let Some(snapshot) = compute_snapshot(&symbol, timeframe, &history) {
            out.insert(symbol, snapshot);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn build_history(days: usize) -> Vec<HistoryRecord> {
        (0..days)
            .map(|i| HistoryRecord {
                date: format!("2026-01-{:02}", (i % 28) + 1),
                close: dec!(100) + rust_decimal::Decimal::from(i as i64),
                volume: Some(1_000 + (i as u64 * 10)),
                open: None,
                high: None,
                low: None,
            })
            .collect()
    }

    fn build_ohlcv_history(days: usize) -> Vec<HistoryRecord> {
        (0..days)
            .map(|i| {
                let base = dec!(100) + rust_decimal::Decimal::from(i as i64);
                HistoryRecord {
                    date: format!("2026-01-{:02}", (i % 28) + 1),
                    close: base,
                    volume: Some(1_000 + (i as u64 * 10)),
                    open: Some(base - dec!(1)),
                    high: Some(base + dec!(3)),
                    low: Some(base - dec!(2)),
                }
            })
            .collect()
    }

    #[test]
    fn compute_snapshot_populates_core_fields() {
        let history = build_history(260);
        let snapshot = compute_snapshot("AAPL", DEFAULT_TIMEFRAME, &history).unwrap();

        assert_eq!(snapshot.symbol, "AAPL");
        assert_eq!(snapshot.timeframe, "1d");
        assert!(snapshot.rsi_14.is_some());
        assert!(snapshot.macd.is_some());
        assert!(snapshot.sma_20.is_some());
        assert!(snapshot.sma_50.is_some());
        assert!(snapshot.sma_200.is_some());
        assert!(snapshot.range_52w_position.is_some());
    }

    #[test]
    fn compute_snapshot_populates_atr_with_ohlcv() {
        let history = build_ohlcv_history(60);
        let snapshot = compute_snapshot("AAPL", DEFAULT_TIMEFRAME, &history).unwrap();

        assert!(
            snapshot.atr_14.is_some(),
            "ATR should be computed from OHLCV"
        );
        let atr = snapshot.atr_14.unwrap();
        assert!(atr > 0.0, "ATR must be positive");

        assert!(snapshot.atr_ratio.is_some(), "ATR ratio should be computed");
        let ratio = snapshot.atr_ratio.unwrap();
        assert!(ratio > 0.0, "ATR ratio must be positive");

        assert!(
            snapshot.day_range_ratio.is_some(),
            "Day range ratio should be computed from OHLCV"
        );
    }

    #[test]
    fn compute_snapshot_atr_fallback_without_ohlcv() {
        // Close-only data (no high/low) — ATR falls back to close-to-close range
        let history = build_history(60);
        let snapshot = compute_snapshot("AAPL", DEFAULT_TIMEFRAME, &history).unwrap();

        // ATR still computes from close-to-close changes
        assert!(
            snapshot.atr_14.is_some(),
            "ATR should still compute from close-only data"
        );
        // Day range ratio requires OHLCV, should be None
        assert!(
            snapshot.day_range_ratio.is_none(),
            "Day range ratio requires OHLCV"
        );
    }

    #[test]
    fn compute_snapshot_requires_enough_history() {
        let history = build_history(10);
        assert!(compute_snapshot("AAPL", DEFAULT_TIMEFRAME, &history).is_none());
    }
}
