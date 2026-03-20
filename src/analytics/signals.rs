use anyhow::Result;

use crate::db::backend::BackendConnection;
use crate::db::technical_signals::{
    add_signal_backend, list_signals_backend, prune_signals_backend, NewSignal,
    TechnicalSignalRecord,
};
use crate::db::technical_snapshots::TechnicalSnapshotRecord;

/// Per-symbol signal definitions derived from a technical snapshot.
struct DetectedSignal {
    signal_type: &'static str,
    direction: &'static str,
    severity: &'static str,
    trigger_price: Option<f64>,
    description: String,
}

/// Generate technical signals from stored snapshots and persist them.
///
/// Called during `pftui data refresh` after technical snapshots are stored.
/// Prunes signals older than 72 hours to prevent unbounded growth.
pub fn generate_signals(backend: &BackendConnection) -> Result<usize> {
    let snapshots = crate::db::technical_snapshots::list_latest_snapshots_backend(
        backend, "1d", None,
    )?;
    if snapshots.is_empty() {
        return Ok(0);
    }

    // Prune old signals (keep last 72 hours)
    let _ = prune_signals_backend(backend, 72);

    // Get existing recent signals to deduplicate
    let existing = list_signals_backend(backend, None, None, Some(500)).unwrap_or_default();

    let mut count = 0;
    for snap in &snapshots {
        let signals = derive_signals(snap);
        for sig in signals {
            if is_duplicate(&existing, &snap.symbol, sig.signal_type) {
                continue;
            }
            let _ = add_signal_backend(
                backend,
                &NewSignal {
                    symbol: &snap.symbol,
                    signal_type: sig.signal_type,
                    direction: sig.direction,
                    severity: sig.severity,
                    trigger_price: sig.trigger_price,
                    description: &sig.description,
                    timeframe: &snap.timeframe,
                },
            );
            count += 1;
        }
    }

    Ok(count)
}

/// Derive all applicable signals from a single technical snapshot.
fn derive_signals(snap: &TechnicalSnapshotRecord) -> Vec<DetectedSignal> {
    let mut signals = Vec::new();

    // RSI overbought/oversold
    if let Some(rsi) = snap.rsi_14 {
        if rsi >= 70.0 {
            let severity = if rsi >= 80.0 { "critical" } else { "notable" };
            signals.push(DetectedSignal {
                signal_type: "rsi_overbought",
                direction: "bearish",
                severity,
                trigger_price: None,
                description: format!("RSI 14 at {:.1} (overbought zone)", rsi),
            });
        } else if rsi <= 30.0 {
            let severity = if rsi <= 20.0 { "critical" } else { "notable" };
            signals.push(DetectedSignal {
                signal_type: "rsi_oversold",
                direction: "bullish",
                severity,
                trigger_price: None,
                description: format!("RSI 14 at {:.1} (oversold zone)", rsi),
            });
        }
    }

    // MACD cross (histogram sign as proxy)
    if let Some(hist) = snap.macd_histogram {
        if let Some(macd) = snap.macd {
            if hist > 0.0 && hist.abs() < macd.abs() * 0.15 {
                // Histogram just turned positive (bull cross vicinity)
                signals.push(DetectedSignal {
                    signal_type: "macd_bull_cross",
                    direction: "bullish",
                    severity: "notable",
                    trigger_price: None,
                    description: format!(
                        "MACD histogram turning positive ({:.4}), bullish cross signal",
                        hist
                    ),
                });
            } else if hist < 0.0 && hist.abs() < macd.abs() * 0.15 {
                signals.push(DetectedSignal {
                    signal_type: "macd_bear_cross",
                    direction: "bearish",
                    severity: "notable",
                    trigger_price: None,
                    description: format!(
                        "MACD histogram turning negative ({:.4}), bearish cross signal",
                        hist
                    ),
                });
            }
        }
    }

    // SMA 200 reclaim/break
    if let Some(above_200) = snap.above_sma_200 {
        if let Some(sma_200) = snap.sma_200 {
            if above_200 {
                // Price above 200 SMA — check proximity (within 2%)
                if let Some(rsi) = snap.rsi_14 {
                    // Only signal on reclaim (RSI not extreme = recently crossed)
                    if rsi > 40.0 && rsi < 65.0 {
                        signals.push(DetectedSignal {
                            signal_type: "sma200_reclaim",
                            direction: "bullish",
                            severity: "notable",
                            trigger_price: Some(sma_200),
                            description: format!(
                                "Price above SMA 200 ({:.2}) with neutral RSI — potential reclaim",
                                sma_200
                            ),
                        });
                    }
                }
            } else {
                // Price below 200 SMA
                if let Some(rsi) = snap.rsi_14 {
                    if rsi < 60.0 && rsi > 35.0 {
                        signals.push(DetectedSignal {
                            signal_type: "sma200_break",
                            direction: "bearish",
                            severity: "notable",
                            trigger_price: Some(sma_200),
                            description: format!(
                                "Price below SMA 200 ({:.2}) with neutral RSI — potential breakdown",
                                sma_200
                            ),
                        });
                    }
                }
            }
        }
    }

    // Bollinger Band squeeze (bandwidth < 5% of middle)
    if let Some(upper) = snap.bollinger_upper {
        if let Some(lower) = snap.bollinger_lower {
            if let Some(middle) = snap.bollinger_middle {
                if middle > 0.0 {
                    let bandwidth = (upper - lower) / middle;
                    if bandwidth < 0.05 {
                        signals.push(DetectedSignal {
                            signal_type: "bb_squeeze",
                            direction: "neutral",
                            severity: "notable",
                            trigger_price: Some(middle),
                            description: format!(
                                "Bollinger Band squeeze: bandwidth {:.1}% of price — volatility expansion imminent",
                                bandwidth * 100.0
                            ),
                        });
                    }
                }
            }
        }
    }

    // Volume expansion
    if let Some(ratio) = snap.volume_ratio_20 {
        if ratio >= 2.0 {
            let severity = if ratio >= 3.0 { "critical" } else { "notable" };
            signals.push(DetectedSignal {
                signal_type: "volume_expansion",
                direction: "neutral",
                severity,
                trigger_price: None,
                description: format!(
                    "Volume {:.1}x 20-day average — institutional activity",
                    ratio
                ),
            });
        }
    }

    // 52-week extremes
    if let Some(position) = snap.range_52w_position {
        if position >= 95.0 {
            signals.push(DetectedSignal {
                signal_type: "52w_high",
                direction: "bullish",
                severity: "notable",
                trigger_price: snap.range_52w_high,
                description: format!(
                    "Near 52-week high ({:.1}% of range)",
                    position
                ),
            });
        } else if position <= 5.0 {
            signals.push(DetectedSignal {
                signal_type: "52w_low",
                direction: "bearish",
                severity: "notable",
                trigger_price: snap.range_52w_low,
                description: format!(
                    "Near 52-week low ({:.1}% of range)",
                    position
                ),
            });
        }
    }

    // ATR-based range expansion (OHLCV-aware, F48)
    if let Some(true) = snap.range_expansion {
        if let Some(atr) = snap.atr_14 {
            let severity = if snap.atr_ratio.unwrap_or(0.0) > 5.0 {
                "critical"
            } else {
                "notable"
            };
            signals.push(DetectedSignal {
                signal_type: "range_expansion",
                direction: "neutral",
                severity,
                trigger_price: None,
                description: format!(
                    "ATR range expansion — current ATR {:.2} exceeds 1.5x its 20-period average (volatility breakout)",
                    atr
                ),
            });
        }
    }

    // Wide bar: day range > 2x ATR (potential breakout bar)
    if let Some(ratio) = snap.day_range_ratio {
        if ratio >= 2.0 {
            signals.push(DetectedSignal {
                signal_type: "wide_range_bar",
                direction: "neutral",
                severity: "notable",
                trigger_price: None,
                description: format!(
                    "Wide range bar: day range is {:.1}x ATR — potential breakout or reversal candle",
                    ratio
                ),
            });
        }
    }

    // Inside bar: day range < 0.5x ATR (compression, potential coil)
    if let Some(ratio) = snap.day_range_ratio {
        if ratio > 0.0 && ratio <= 0.5 {
            signals.push(DetectedSignal {
                signal_type: "inside_bar",
                direction: "neutral",
                severity: "informational",
                trigger_price: None,
                description: format!(
                    "Inside bar: day range is {:.1}x ATR — volatility compression, breakout setup forming",
                    ratio
                ),
            });
        }
    }

    signals
}

/// Check if a signal of this type for this symbol already exists within the last 6 hours.
fn is_duplicate(
    existing: &[TechnicalSignalRecord],
    symbol: &str,
    signal_type: &str,
) -> bool {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(6);
    existing.iter().any(|s| {
        s.symbol == symbol && s.signal_type == signal_type && {
            chrono::DateTime::parse_from_rfc3339(&s.detected_at)
                .or_else(|_| {
                    // PostgreSQL format: "2026-03-19 02:06:28.050511+00"
                    chrono::DateTime::parse_from_str(&s.detected_at, "%Y-%m-%d %H:%M:%S%.f%#z")
                })
                .map(|d| d.with_timezone(&chrono::Utc) >= cutoff)
                .unwrap_or(false)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(symbol: &str) -> TechnicalSnapshotRecord {
        TechnicalSnapshotRecord {
            symbol: symbol.to_string(),
            timeframe: "1d".to_string(),
            rsi_14: Some(50.0),
            macd: Some(0.5),
            macd_signal: Some(0.3),
            macd_histogram: Some(0.2),
            sma_20: Some(100.0),
            sma_50: Some(98.0),
            sma_200: Some(95.0),
            bollinger_upper: Some(105.0),
            bollinger_middle: Some(100.0),
            bollinger_lower: Some(95.0),
            range_52w_low: Some(80.0),
            range_52w_high: Some(110.0),
            range_52w_position: Some(66.0),
            volume_avg_20: Some(1_000_000.0),
            volume_ratio_20: Some(1.0),
            volume_regime: Some("normal".to_string()),
            above_sma_20: Some(true),
            above_sma_50: Some(true),
            above_sma_200: Some(true),
            atr_14: Some(3.5),
            atr_ratio: Some(3.5),
            range_expansion: Some(false),
            day_range_ratio: Some(1.0),
            computed_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn detects_rsi_overbought() {
        let mut snap = make_snapshot("AAPL");
        snap.rsi_14 = Some(75.0);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "rsi_overbought"));
    }

    #[test]
    fn detects_rsi_oversold() {
        let mut snap = make_snapshot("AAPL");
        snap.rsi_14 = Some(18.0);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "rsi_oversold"));
        // RSI <= 20 → critical
        assert!(signals.iter().any(|s| s.severity == "critical"));
    }

    #[test]
    fn detects_volume_expansion() {
        let mut snap = make_snapshot("AAPL");
        snap.volume_ratio_20 = Some(2.5);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "volume_expansion"));
    }

    #[test]
    fn detects_bb_squeeze() {
        let mut snap = make_snapshot("AAPL");
        // Tight bands: 100 ± 2 = bandwidth 4% < 5% threshold
        snap.bollinger_upper = Some(102.0);
        snap.bollinger_lower = Some(98.0);
        snap.bollinger_middle = Some(100.0);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "bb_squeeze"));
    }

    #[test]
    fn detects_52w_high() {
        let mut snap = make_snapshot("AAPL");
        snap.range_52w_position = Some(97.0);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "52w_high"));
    }

    #[test]
    fn detects_52w_low() {
        let mut snap = make_snapshot("AAPL");
        snap.range_52w_position = Some(3.0);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "52w_low"));
    }

    #[test]
    fn neutral_snapshot_produces_no_signals() {
        let snap = make_snapshot("AAPL");
        let signals = derive_signals(&snap);
        // Neutral snapshot: RSI 50, normal volume, mid-range position, moderate BB
        // Should only produce sma200_reclaim since above_sma_200=true and RSI=50 (in 40-65 range)
        let non_sma = signals
            .iter()
            .filter(|s| s.signal_type != "sma200_reclaim")
            .count();
        assert_eq!(non_sma, 0, "unexpected signals from neutral snapshot");
    }

    #[test]
    fn dedup_prevents_repeated_signals() {
        let existing = vec![TechnicalSignalRecord {
            id: 1,
            symbol: "AAPL".to_string(),
            signal_type: "rsi_overbought".to_string(),
            direction: "bearish".to_string(),
            severity: "notable".to_string(),
            trigger_price: None,
            description: "test".to_string(),
            timeframe: "1d".to_string(),
            detected_at: chrono::Utc::now().to_rfc3339(),
        }];
        assert!(is_duplicate(&existing, "AAPL", "rsi_overbought"));
        assert!(!is_duplicate(&existing, "BTC", "rsi_overbought"));
        assert!(!is_duplicate(&existing, "AAPL", "macd_bull_cross"));
    }

    #[test]
    fn detects_range_expansion() {
        let mut snap = make_snapshot("AAPL");
        snap.range_expansion = Some(true);
        snap.atr_14 = Some(5.0);
        snap.atr_ratio = Some(5.0);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "range_expansion"));
    }

    #[test]
    fn no_range_expansion_when_false() {
        let mut snap = make_snapshot("AAPL");
        snap.range_expansion = Some(false);
        let signals = derive_signals(&snap);
        assert!(!signals.iter().any(|s| s.signal_type == "range_expansion"));
    }

    #[test]
    fn detects_wide_range_bar() {
        let mut snap = make_snapshot("AAPL");
        snap.day_range_ratio = Some(2.5);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "wide_range_bar"));
    }

    #[test]
    fn detects_inside_bar() {
        let mut snap = make_snapshot("AAPL");
        snap.day_range_ratio = Some(0.3);
        let signals = derive_signals(&snap);
        assert!(signals.iter().any(|s| s.signal_type == "inside_bar"));
    }

    #[test]
    fn no_bar_signals_at_normal_range() {
        let snap = make_snapshot("AAPL"); // day_range_ratio = 1.0
        let signals = derive_signals(&snap);
        assert!(!signals.iter().any(|s| s.signal_type == "wide_range_bar"));
        assert!(!signals.iter().any(|s| s.signal_type == "inside_bar"));
    }
}
