use std::collections::{HashMap, HashSet};

use crate::db::backend::BackendConnection;
use crate::db::price_history::{get_history, get_history_backend};
use crate::db::technical_snapshots::{
    get_latest_snapshot, get_latest_snapshot_backend, TechnicalSnapshotRecord,
};
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
        computed_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub fn load_or_compute_snapshots(
    conn: &Connection,
    symbols: &[String],
    timeframe: &str,
) -> HashMap<String, TechnicalSnapshotRecord> {
    let mut out = HashMap::new();
    let mut missing = HashSet::new();

    for symbol in symbols {
        match get_latest_snapshot(conn, symbol, timeframe) {
            Ok(Some(row)) => {
                out.insert(symbol.clone(), row);
            }
            _ => {
                missing.insert(symbol.clone());
            }
        }
    }

    for symbol in missing {
        let history = match get_history(conn, &symbol, 370) {
            Ok(rows) if !rows.is_empty() => rows,
            _ => continue,
        };
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
    let mut out = HashMap::new();
    let mut missing = HashSet::new();

    for symbol in symbols {
        match get_latest_snapshot_backend(backend, symbol, timeframe) {
            Ok(Some(row)) => {
                out.insert(symbol.clone(), row);
            }
            _ => {
                missing.insert(symbol.clone());
            }
        }
    }

    for symbol in missing {
        let history = match get_history_backend(backend, &symbol, 370) {
            Ok(rows) if !rows.is_empty() => rows,
            _ => continue,
        };
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
    fn compute_snapshot_requires_enough_history() {
        let history = build_history(10);
        assert!(compute_snapshot("AAPL", DEFAULT_TIMEFRAME, &history).is_none());
    }
}
