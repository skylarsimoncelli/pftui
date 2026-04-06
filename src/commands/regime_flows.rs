use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::get_history_backend;
use crate::db::regime_snapshots;

/// Regime-Asset Flow Correlation Tracker.
///
/// Cross-references the current market regime with asset class flows to detect
/// power structure patterns automatically. Monitors key ratios (gold/oil,
/// copper/gold, defense/SPX), safe-haven vs risk flows, and energy complex
/// signals to systematize the pattern recognition that agents currently do
/// manually.

// ── Structs ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RegimeFlowsOutput {
    pub regime: RegimeContext,
    pub ratios: Vec<AssetRatio>,
    pub flow_signals: Vec<FlowSignal>,
    pub patterns: Vec<DetectedPattern>,
    pub summary: FlowSummary,
}

#[derive(Debug, Serialize)]
pub struct RegimeContext {
    pub current_regime: String,
    pub confidence: Option<f64>,
    pub vix: Option<f64>,
    pub dxy: Option<f64>,
    pub yield_10y: Option<f64>,
    pub oil: Option<f64>,
    pub gold: Option<f64>,
    pub btc: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AssetRatio {
    pub name: String,
    pub numerator: String,
    pub denominator: String,
    pub current_value: Option<f64>,
    pub change_5d: Option<f64>,
    pub direction: String,
    pub interpretation: String,
}

#[derive(Debug, Serialize)]
pub struct FlowSignal {
    pub asset_class: String,
    pub symbol: String,
    pub price: Option<f64>,
    pub change_5d_pct: Option<f64>,
    pub flow_direction: String,
    pub regime_alignment: String,
}

#[derive(Debug, Serialize)]
pub struct DetectedPattern {
    pub pattern_name: String,
    pub confidence: String,
    pub description: String,
    pub supporting_signals: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct FlowSummary {
    pub dominant_flow: String,
    pub safe_haven_bid: String,
    pub risk_appetite: String,
    pub energy_stress: String,
    pub pattern_count: usize,
    pub regime_consistent: bool,
}

// ── Ratio Definitions ────────────────────────────────────────────────

struct RatioDef {
    name: &'static str,
    numerator: &'static str,
    denominator: &'static str,
    rising_interpretation: &'static str,
    falling_interpretation: &'static str,
}

const RATIOS: &[RatioDef] = &[
    RatioDef {
        name: "Gold/Oil",
        numerator: "GC=F",
        denominator: "CL=F",
        rising_interpretation: "Safe-haven preference over energy; deflationary or geopolitical stress",
        falling_interpretation: "Energy demand outpacing safe-haven; reflationary or growth signal",
    },
    RatioDef {
        name: "Copper/Gold",
        numerator: "HG=F",
        denominator: "GC=F",
        rising_interpretation: "Industrial growth outpacing safety; risk-on macro signal",
        falling_interpretation: "Safety outpacing growth; recessionary or risk-off signal",
    },
    RatioDef {
        name: "Gold/SPX",
        numerator: "GC=F",
        denominator: "^GSPC",
        rising_interpretation: "Safety outperforming equities; risk-off rotation",
        falling_interpretation: "Equities outperforming gold; risk-on confidence",
    },
    RatioDef {
        name: "Silver/Gold",
        numerator: "SI=F",
        denominator: "GC=F",
        rising_interpretation: "Industrial-monetary hybrid bid; reflationary or speculative signal",
        falling_interpretation: "Pure safe-haven preference; deflationary signal",
    },
    RatioDef {
        name: "Oil/DXY",
        numerator: "CL=F",
        denominator: "DX-Y.NYB",
        rising_interpretation: "Commodity strength vs dollar; inflationary pressure",
        falling_interpretation: "Dollar strength crushing commodities; disinflationary",
    },
    RatioDef {
        name: "BTC/Gold",
        numerator: "BTC-USD",
        denominator: "GC=F",
        rising_interpretation: "Risk-on digital asset preference over traditional safety",
        falling_interpretation: "Traditional safe-haven preference over digital assets",
    },
];

// ── Flow signal assets ───────────────────────────────────────────────

struct FlowAsset {
    class: &'static str,
    symbol: &'static str,
    safe_haven: bool,
}

const FLOW_ASSETS: &[FlowAsset] = &[
    FlowAsset { class: "Safe Haven", symbol: "GC=F", safe_haven: true },
    FlowAsset { class: "Safe Haven", symbol: "SI=F", safe_haven: true },
    FlowAsset { class: "Safe Haven", symbol: "BTC-USD", safe_haven: false },
    FlowAsset { class: "Energy", symbol: "CL=F", safe_haven: false },
    FlowAsset { class: "Energy", symbol: "URA", safe_haven: false },
    FlowAsset { class: "Equities", symbol: "^GSPC", safe_haven: false },
    FlowAsset { class: "Equities", symbol: "^IXIC", safe_haven: false },
    FlowAsset { class: "Defense", symbol: "ITA", safe_haven: false },
    FlowAsset { class: "Volatility", symbol: "^VIX", safe_haven: true },
    FlowAsset { class: "Dollar", symbol: "DX-Y.NYB", safe_haven: true },
    FlowAsset { class: "Bonds", symbol: "^TNX", safe_haven: false },
    FlowAsset { class: "Industrial", symbol: "HG=F", safe_haven: false },
];

// ── Helpers ──────────────────────────────────────────────────────────

fn price_f64(prices: &HashMap<String, Decimal>, symbol: &str) -> Option<f64> {
    prices
        .get(symbol)
        .and_then(|d| d.to_string().parse::<f64>().ok())
}

fn change_5d(backend: &BackendConnection, symbol: &str, current: f64) -> Option<f64> {
    let rows = get_history_backend(backend, symbol, 10).ok()?;
    if rows.len() < 6 {
        return None;
    }
    let prev = rows[rows.len().saturating_sub(6)]
        .close
        .to_string()
        .parse::<f64>()
        .ok()?;
    if prev.abs() < 1e-10 {
        return None;
    }
    Some(((current - prev) / prev) * 100.0)
}

fn direction_label(change: Option<f64>) -> String {
    match change {
        Some(c) if c > 1.0 => "rising".to_string(),
        Some(c) if c < -1.0 => "falling".to_string(),
        Some(_) => "flat".to_string(),
        None => "unknown".to_string(),
    }
}

// ── Core Logic ───────────────────────────────────────────────────────

fn compute_ratios(
    backend: &BackendConnection,
    prices: &HashMap<String, Decimal>,
) -> Vec<AssetRatio> {
    RATIOS
        .iter()
        .filter_map(|def| {
            let num = price_f64(prices, def.numerator)?;
            let den = price_f64(prices, def.denominator)?;
            if den.abs() < 1e-10 {
                return None;
            }
            let current_value = num / den;

            // Compute 5-day change of the ratio from history
            let num_rows = get_history_backend(backend, def.numerator, 10).ok()?;
            let den_rows = get_history_backend(backend, def.denominator, 10).ok()?;
            let min_len = num_rows.len().min(den_rows.len());
            let change_5d = if min_len >= 6 {
                let prev_num = num_rows[num_rows.len().saturating_sub(6)]
                    .close
                    .to_string()
                    .parse::<f64>()
                    .ok()?;
                let prev_den = den_rows[den_rows.len().saturating_sub(6)]
                    .close
                    .to_string()
                    .parse::<f64>()
                    .ok()?;
                if prev_den.abs() > 1e-10 {
                    let prev_ratio = prev_num / prev_den;
                    if prev_ratio.abs() > 1e-10 {
                        Some(((current_value - prev_ratio) / prev_ratio) * 100.0)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let direction = direction_label(change_5d);
            let interpretation = if direction == "rising" {
                def.rising_interpretation.to_string()
            } else if direction == "falling" {
                def.falling_interpretation.to_string()
            } else {
                "Neutral — no strong directional signal".to_string()
            };

            Some(AssetRatio {
                name: def.name.to_string(),
                numerator: def.numerator.to_string(),
                denominator: def.denominator.to_string(),
                current_value: Some((current_value * 1000.0).round() / 1000.0),
                change_5d: change_5d.map(|c| (c * 100.0).round() / 100.0),
                direction,
                interpretation,
            })
        })
        .collect()
}

fn compute_flow_signals(
    backend: &BackendConnection,
    prices: &HashMap<String, Decimal>,
    regime: &str,
) -> Vec<FlowSignal> {
    FLOW_ASSETS
        .iter()
        .filter_map(|asset| {
            let price = price_f64(prices, asset.symbol)?;
            let change = change_5d(backend, asset.symbol, price);
            // VIX uses wider thresholds (2%) since it's naturally more volatile
            let threshold = if asset.symbol == "^VIX" { 2.0 } else { 1.0 };
            let flow_dir = match change {
                Some(c) if c > threshold => "inflow",
                Some(c) if c < -threshold => "outflow",
                _ => "flat",
            };

            let regime_align = if asset.safe_haven {
                // Safe havens rising in risk-off = aligned; rising in risk-on = divergent
                match (regime, flow_dir) {
                    ("risk-off" | "crisis" | "stagflation", "inflow") => "aligned",
                    ("risk-on", "inflow") => "divergent",
                    ("risk-off" | "crisis" | "stagflation", "outflow") => "divergent",
                    ("risk-on", "outflow") => "aligned",
                    _ => "neutral",
                }
            } else {
                // Risk assets rising in risk-on = aligned
                match (regime, flow_dir) {
                    ("risk-on", "inflow") => "aligned",
                    ("risk-off" | "crisis" | "stagflation", "outflow") => "aligned",
                    ("risk-on", "outflow") => "divergent",
                    ("risk-off" | "crisis" | "stagflation", "inflow") => "divergent",
                    _ => "neutral",
                }
            };

            Some(FlowSignal {
                asset_class: asset.class.to_string(),
                symbol: asset.symbol.to_string(),
                price: Some((price * 100.0).round() / 100.0),
                change_5d_pct: change.map(|c| (c * 100.0).round() / 100.0),
                flow_direction: flow_dir.to_string(),
                regime_alignment: regime_align.to_string(),
            })
        })
        .collect()
}

fn detect_patterns(
    _ratios: &[AssetRatio],
    flows: &[FlowSignal],
    regime: &str,
) -> Vec<DetectedPattern> {
    let mut patterns = Vec::new();

    let flow_map: HashMap<&str, &FlowSignal> =
        flows.iter().map(|f| (f.symbol.as_str(), f)).collect();

    // Pattern 1: Safe-haven rotation (gold + silver + VIX rising, equities falling)
    let gold_up = flow_map
        .get("GC=F")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);
    let silver_up = flow_map
        .get("SI=F")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);
    let vix_up = flow_map
        .get("^VIX")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);
    let equities_down = flow_map
        .get("^GSPC")
        .map(|f| f.flow_direction == "outflow")
        .unwrap_or(false);

    if gold_up && (silver_up || vix_up) && equities_down {
        let mut signals = vec!["Gold rising".to_string()];
        if silver_up {
            signals.push("Silver rising".to_string());
        }
        if vix_up {
            signals.push("VIX rising".to_string());
        }
        signals.push("Equities falling".to_string());
        patterns.push(DetectedPattern {
            pattern_name: "Safe-Haven Rotation".to_string(),
            confidence: if gold_up && silver_up && vix_up && equities_down {
                "high".to_string()
            } else {
                "medium".to_string()
            },
            description: "Capital rotating from risk assets into safe havens. Classic risk-off flow pattern.".to_string(),
            supporting_signals: signals,
        });
    }

    // Pattern 2: Geopolitical stress (oil up + gold up + defense up)
    let oil_up = flow_map
        .get("CL=F")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);
    let defense_up = flow_map
        .get("ITA")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);

    if oil_up && gold_up && defense_up {
        patterns.push(DetectedPattern {
            pattern_name: "Geopolitical Stress".to_string(),
            confidence: "high".to_string(),
            description: "Oil, gold, and defense all rising — classic geopolitical risk premium pattern.".to_string(),
            supporting_signals: vec![
                "Oil rising".to_string(),
                "Gold rising".to_string(),
                "Defense (ITA) rising".to_string(),
            ],
        });
    } else if (defense_up || gold_up) && oil_up || (gold_up && defense_up) {
        let mut signals = Vec::new();
        if oil_up {
            signals.push("Oil rising".to_string());
        }
        if gold_up {
            signals.push("Gold rising".to_string());
        }
        if defense_up {
            signals.push("Defense (ITA) rising".to_string());
        }
        patterns.push(DetectedPattern {
            pattern_name: "Geopolitical Stress (Partial)".to_string(),
            confidence: "medium".to_string(),
            description: "Two of three geopolitical stress signals active — monitor for escalation.".to_string(),
            supporting_signals: signals,
        });
    }

    // Pattern 3: Inflationary pulse (oil up + copper up + gold up + DXY down)
    let copper_up = flow_map
        .get("HG=F")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);
    let dxy_down = flow_map
        .get("DX-Y.NYB")
        .map(|f| f.flow_direction == "outflow")
        .unwrap_or(false);

    let inflation_signals: Vec<String> = [
        oil_up.then(|| "Oil rising".to_string()),
        copper_up.then(|| "Copper rising".to_string()),
        gold_up.then(|| "Gold rising".to_string()),
        dxy_down.then(|| "Dollar falling".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();

    if inflation_signals.len() >= 3 {
        patterns.push(DetectedPattern {
            pattern_name: "Inflationary Pulse".to_string(),
            confidence: if inflation_signals.len() >= 4 {
                "high".to_string()
            } else {
                "medium".to_string()
            },
            description: "Broad commodity strength with dollar weakness — inflationary pressure building.".to_string(),
            supporting_signals: inflation_signals,
        });
    }

    // Pattern 4: Risk-on breakout (equities up + VIX down + BTC up + copper up)
    let equities_up = flow_map
        .get("^GSPC")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);
    let vix_down = flow_map
        .get("^VIX")
        .map(|f| f.flow_direction == "outflow")
        .unwrap_or(false);
    let btc_up = flow_map
        .get("BTC-USD")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);

    let riskon_signals: Vec<String> = [
        equities_up.then(|| "Equities rising".to_string()),
        vix_down.then(|| "VIX falling".to_string()),
        btc_up.then(|| "Bitcoin rising".to_string()),
        copper_up.then(|| "Copper rising".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();

    if riskon_signals.len() >= 3 {
        patterns.push(DetectedPattern {
            pattern_name: "Risk-On Breakout".to_string(),
            confidence: if riskon_signals.len() >= 4 {
                "high".to_string()
            } else {
                "medium".to_string()
            },
            description: "Broad risk appetite with equities, BTC, and industrial metals all bid.".to_string(),
            supporting_signals: riskon_signals,
        });
    }

    // Pattern 5: Deflationary signal (oil down + copper down + yields down)
    let oil_down = flow_map
        .get("CL=F")
        .map(|f| f.flow_direction == "outflow")
        .unwrap_or(false);
    let copper_down = flow_map
        .get("HG=F")
        .map(|f| f.flow_direction == "outflow")
        .unwrap_or(false);
    let yields_down = flow_map
        .get("^TNX")
        .map(|f| f.flow_direction == "outflow")
        .unwrap_or(false);

    let deflation_signals: Vec<String> = [
        oil_down.then(|| "Oil falling".to_string()),
        copper_down.then(|| "Copper falling".to_string()),
        yields_down.then(|| "Yields falling".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();

    if deflation_signals.len() >= 2 {
        patterns.push(DetectedPattern {
            pattern_name: "Deflationary Signal".to_string(),
            confidence: if deflation_signals.len() >= 3 {
                "high".to_string()
            } else {
                "medium".to_string()
            },
            description: "Commodity and yield weakness — demand destruction or recessionary signal.".to_string(),
            supporting_signals: deflation_signals,
        });
    }

    // Pattern 6: Dollar wrecking ball (DXY up + commodities down + EM stress)
    let dxy_up = flow_map
        .get("DX-Y.NYB")
        .map(|f| f.flow_direction == "inflow")
        .unwrap_or(false);

    if dxy_up && (oil_down || copper_down) && !gold_up {
        let mut signals = vec!["Dollar rising".to_string()];
        if oil_down {
            signals.push("Oil falling".to_string());
        }
        if copper_down {
            signals.push("Copper falling".to_string());
        }
        signals.push("Gold not rising".to_string());
        patterns.push(DetectedPattern {
            pattern_name: "Dollar Wrecking Ball".to_string(),
            confidence: "medium".to_string(),
            description: "Strong dollar crushing commodities — tightening financial conditions globally.".to_string(),
            supporting_signals: signals,
        });
    }

    // Pattern 7: Energy crisis signal (oil spiking + VIX up + equities down)
    let oil_spike = flow_map
        .get("CL=F")
        .and_then(|f| f.change_5d_pct)
        .map(|c| c > 5.0)
        .unwrap_or(false);

    if oil_spike && vix_up && equities_down {
        patterns.push(DetectedPattern {
            pattern_name: "Energy Crisis Signal".to_string(),
            confidence: "high".to_string(),
            description: "Oil spiking with rising VIX and falling equities — energy supply shock pattern.".to_string(),
            supporting_signals: vec![
                format!(
                    "Oil +{:.1}% in 5 days",
                    flow_map
                        .get("CL=F")
                        .and_then(|f| f.change_5d_pct)
                        .unwrap_or(0.0)
                ),
                "VIX rising".to_string(),
                "Equities falling".to_string(),
            ],
        });
    }

    // Pattern 8: Regime divergence (flows contradict the current regime classification)
    let aligned_count = flows
        .iter()
        .filter(|f| f.regime_alignment == "aligned")
        .count();
    let divergent_count = flows
        .iter()
        .filter(|f| f.regime_alignment == "divergent")
        .count();
    let active_count = flows
        .iter()
        .filter(|f| f.flow_direction != "flat" && f.flow_direction != "unknown")
        .count();

    if active_count >= 4 && divergent_count > aligned_count {
        patterns.push(DetectedPattern {
            pattern_name: "Regime Divergence".to_string(),
            confidence: if divergent_count >= aligned_count + 3 {
                "high".to_string()
            } else {
                "medium".to_string()
            },
            description: format!(
                "Asset flows ({} divergent vs {} aligned) contradict the {} regime — potential regime transition.",
                divergent_count, aligned_count, regime
            ),
            supporting_signals: flows
                .iter()
                .filter(|f| f.regime_alignment == "divergent")
                .map(|f| {
                    format!(
                        "{} {} ({})",
                        f.symbol, f.flow_direction, f.asset_class
                    )
                })
                .collect(),
        });
    }

    patterns
}

fn build_summary(
    _ratios: &[AssetRatio],
    flows: &[FlowSignal],
    patterns: &[DetectedPattern],
    _regime: &str,
) -> FlowSummary {
    // Determine dominant flow direction
    let inflow_count = flows.iter().filter(|f| f.flow_direction == "inflow").count();
    let outflow_count = flows.iter().filter(|f| f.flow_direction == "outflow").count();
    let dominant = if inflow_count > outflow_count + 2 {
        "broad inflows (risk-on)"
    } else if outflow_count > inflow_count + 2 {
        "broad outflows (risk-off)"
    } else {
        "mixed / rotational"
    };

    // Safe-haven bid strength
    let haven_inflows = flows
        .iter()
        .filter(|f| {
            FLOW_ASSETS.iter().any(|a| a.symbol == f.symbol && a.safe_haven)
                && f.flow_direction == "inflow"
        })
        .count();
    let safe_haven = match haven_inflows {
        0 => "none",
        1 => "mild",
        2 => "moderate",
        _ => "strong",
    };

    // Risk appetite
    let risk_inflows = flows
        .iter()
        .filter(|f| {
            FLOW_ASSETS
                .iter()
                .any(|a| a.symbol == f.symbol && !a.safe_haven && a.class != "Volatility")
                && f.flow_direction == "inflow"
        })
        .count();
    let risk_appetite = match risk_inflows {
        0 => "none",
        1..=2 => "cautious",
        3..=4 => "moderate",
        _ => "strong",
    };

    // Energy stress
    let oil_change = flows
        .iter()
        .find(|f| f.symbol == "CL=F")
        .and_then(|f| f.change_5d_pct);
    let energy_stress = match oil_change {
        Some(c) if c > 5.0 => "elevated (oil spiking)",
        Some(c) if c > 2.0 => "mild (oil rising)",
        Some(c) if c < -5.0 => "demand concern (oil plunging)",
        Some(c) if c < -2.0 => "easing (oil falling)",
        _ => "neutral",
    };

    // Regime consistency: are flows consistent with the stated regime?
    let aligned = flows.iter().filter(|f| f.regime_alignment == "aligned").count();
    let divergent = flows.iter().filter(|f| f.regime_alignment == "divergent").count();
    let consistent = aligned >= divergent;

    FlowSummary {
        dominant_flow: dominant.to_string(),
        safe_haven_bid: safe_haven.to_string(),
        risk_appetite: risk_appetite.to_string(),
        energy_stress: energy_stress.to_string(),
        pattern_count: patterns.len(),
        regime_consistent: consistent,
    }
}

// ── Public Entry Point ───────────────────────────────────────────────

pub fn build_output(backend: &BackendConnection) -> Result<RegimeFlowsOutput> {
    // 1. Get current regime
    let regime_snap = regime_snapshots::get_current_backend(backend)?;
    let regime_str = regime_snap
        .as_ref()
        .map(|r| r.regime.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let regime_ctx = RegimeContext {
        current_regime: regime_str.clone(),
        confidence: regime_snap.as_ref().and_then(|r| r.confidence),
        vix: regime_snap.as_ref().and_then(|r| r.vix),
        dxy: regime_snap.as_ref().and_then(|r| r.dxy),
        yield_10y: regime_snap.as_ref().and_then(|r| r.yield_10y),
        oil: regime_snap.as_ref().and_then(|r| r.oil),
        gold: regime_snap.as_ref().and_then(|r| r.gold),
        btc: regime_snap.as_ref().and_then(|r| r.btc),
    };

    // 2. Build price lookup
    let all_prices = get_all_cached_prices_backend(backend)?;
    let prices: HashMap<String, Decimal> = all_prices
        .iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    // 3. Compute ratios
    let ratios = compute_ratios(backend, &prices);

    // 4. Compute flow signals
    let flows = compute_flow_signals(backend, &prices, &regime_str);

    // 5. Detect patterns
    let patterns = detect_patterns(&ratios, &flows, &regime_str);

    // 6. Build summary
    let summary = build_summary(&ratios, &flows, &patterns, &regime_str);

    Ok(RegimeFlowsOutput {
        regime: regime_ctx,
        ratios,
        flow_signals: flows,
        patterns,
        summary,
    })
}

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let output = build_output(backend)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_terminal(&output);
    }

    Ok(())
}

fn print_terminal(output: &RegimeFlowsOutput) {
    println!(
        "═══ Regime-Asset Flow Correlation ═══\n"
    );

    // Regime context
    println!(
        "Current Regime: {} (confidence: {:.0}%)",
        output.regime.current_regime.to_uppercase(),
        output.regime.confidence.unwrap_or(0.0) * 100.0
    );
    println!(
        "  VIX: {:.1}  DXY: {:.1}  10Y: {:.2}%  Oil: ${:.1}  Gold: ${:.0}  BTC: ${:.0}",
        output.regime.vix.unwrap_or(0.0),
        output.regime.dxy.unwrap_or(0.0),
        output.regime.yield_10y.unwrap_or(0.0),
        output.regime.oil.unwrap_or(0.0),
        output.regime.gold.unwrap_or(0.0),
        output.regime.btc.unwrap_or(0.0),
    );
    println!();

    // Key ratios
    if !output.ratios.is_empty() {
        println!("── Key Ratios (5d change) ──");
        for r in &output.ratios {
            let arrow = match r.direction.as_str() {
                "rising" => "↑",
                "falling" => "↓",
                _ => "→",
            };
            println!(
                "  {:<14} {:.3} {} ({:+.1}%)  {}",
                r.name,
                r.current_value.unwrap_or(0.0),
                arrow,
                r.change_5d.unwrap_or(0.0),
                r.interpretation
            );
        }
        println!();
    }

    // Flow signals
    if !output.flow_signals.is_empty() {
        println!("── Asset Flow Signals ──");
        for f in &output.flow_signals {
            let icon = match f.flow_direction.as_str() {
                "inflow" => "🟢",
                "outflow" => "🔴",
                _ => "⚪",
            };
            let align = match f.regime_alignment.as_str() {
                "aligned" => "✓",
                "divergent" => "✗",
                _ => "~",
            };
            println!(
                "  {} {:<10} {:<10} {:>9.2}  {:+.1}%  [{}]",
                icon,
                f.asset_class,
                f.symbol,
                f.price.unwrap_or(0.0),
                f.change_5d_pct.unwrap_or(0.0),
                align
            );
        }
        println!();
    }

    // Detected patterns
    if !output.patterns.is_empty() {
        println!("── Detected Patterns ──");
        for p in &output.patterns {
            println!(
                "  ◆ {} [{}]",
                p.pattern_name,
                p.confidence.to_uppercase()
            );
            println!("    {}", p.description);
            for s in &p.supporting_signals {
                println!("    • {}", s);
            }
        }
        println!();
    }

    // Summary
    println!("── Summary ──");
    println!("  Dominant flow:    {}", output.summary.dominant_flow);
    println!("  Safe-haven bid:   {}", output.summary.safe_haven_bid);
    println!("  Risk appetite:    {}", output.summary.risk_appetite);
    println!("  Energy stress:    {}", output.summary.energy_stress);
    println!("  Patterns found:   {}", output.summary.pattern_count);
    println!(
        "  Regime consistent: {}",
        if output.summary.regime_consistent {
            "YES ✓"
        } else {
            "NO ✗ (potential regime transition)"
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_label_rising() {
        assert_eq!(direction_label(Some(2.5)), "rising");
    }

    #[test]
    fn direction_label_falling() {
        assert_eq!(direction_label(Some(-3.0)), "falling");
    }

    #[test]
    fn direction_label_flat() {
        assert_eq!(direction_label(Some(0.5)), "flat");
    }

    #[test]
    fn direction_label_none() {
        assert_eq!(direction_label(None), "unknown");
    }

    #[test]
    fn detect_safe_haven_rotation() {
        let ratios = vec![];
        let flows = vec![
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "GC=F".into(),
                price: Some(3050.0),
                change_5d_pct: Some(2.5),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "SI=F".into(),
                price: Some(34.0),
                change_5d_pct: Some(3.1),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Volatility".into(),
                symbol: "^VIX".into(),
                price: Some(28.0),
                change_5d_pct: Some(15.0),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Equities".into(),
                symbol: "^GSPC".into(),
                price: Some(5200.0),
                change_5d_pct: Some(-3.0),
                flow_direction: "outflow".into(),
                regime_alignment: "aligned".into(),
            },
        ];
        let patterns = detect_patterns(&ratios, &flows, "risk-off");
        assert!(
            patterns.iter().any(|p| p.pattern_name == "Safe-Haven Rotation"),
            "should detect safe-haven rotation"
        );
    }

    #[test]
    fn detect_geopolitical_stress() {
        let ratios = vec![];
        let flows = vec![
            FlowSignal {
                asset_class: "Energy".into(),
                symbol: "CL=F".into(),
                price: Some(85.0),
                change_5d_pct: Some(4.0),
                flow_direction: "inflow".into(),
                regime_alignment: "divergent".into(),
            },
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "GC=F".into(),
                price: Some(3100.0),
                change_5d_pct: Some(2.0),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Defense".into(),
                symbol: "ITA".into(),
                price: Some(155.0),
                change_5d_pct: Some(3.5),
                flow_direction: "inflow".into(),
                regime_alignment: "divergent".into(),
            },
        ];
        let patterns = detect_patterns(&ratios, &flows, "risk-on");
        assert!(
            patterns.iter().any(|p| p.pattern_name == "Geopolitical Stress"),
            "should detect geopolitical stress"
        );
    }

    #[test]
    fn detect_inflationary_pulse() {
        let ratios = vec![];
        let flows = vec![
            FlowSignal {
                asset_class: "Energy".into(),
                symbol: "CL=F".into(),
                price: Some(85.0),
                change_5d_pct: Some(3.0),
                flow_direction: "inflow".into(),
                regime_alignment: "neutral".into(),
            },
            FlowSignal {
                asset_class: "Industrial".into(),
                symbol: "HG=F".into(),
                price: Some(4.5),
                change_5d_pct: Some(2.0),
                flow_direction: "inflow".into(),
                regime_alignment: "neutral".into(),
            },
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "GC=F".into(),
                price: Some(3100.0),
                change_5d_pct: Some(1.5),
                flow_direction: "inflow".into(),
                regime_alignment: "neutral".into(),
            },
            FlowSignal {
                asset_class: "Dollar".into(),
                symbol: "DX-Y.NYB".into(),
                price: Some(102.0),
                change_5d_pct: Some(-1.5),
                flow_direction: "outflow".into(),
                regime_alignment: "neutral".into(),
            },
        ];
        let patterns = detect_patterns(&ratios, &flows, "transition");
        assert!(
            patterns.iter().any(|p| p.pattern_name == "Inflationary Pulse"),
            "should detect inflationary pulse"
        );
    }

    #[test]
    fn detect_risk_on_breakout() {
        let flows = vec![
            FlowSignal {
                asset_class: "Equities".into(),
                symbol: "^GSPC".into(),
                price: Some(5800.0),
                change_5d_pct: Some(3.0),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Volatility".into(),
                symbol: "^VIX".into(),
                price: Some(14.0),
                change_5d_pct: Some(-10.0),
                flow_direction: "outflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "BTC-USD".into(),
                price: Some(95000.0),
                change_5d_pct: Some(5.0),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Industrial".into(),
                symbol: "HG=F".into(),
                price: Some(4.8),
                change_5d_pct: Some(2.0),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
        ];
        let patterns = detect_patterns(&[], &flows, "risk-on");
        assert!(
            patterns.iter().any(|p| p.pattern_name == "Risk-On Breakout"),
            "should detect risk-on breakout"
        );
    }

    #[test]
    fn detect_deflationary_signal() {
        let flows = vec![
            FlowSignal {
                asset_class: "Energy".into(),
                symbol: "CL=F".into(),
                price: Some(60.0),
                change_5d_pct: Some(-4.0),
                flow_direction: "outflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Industrial".into(),
                symbol: "HG=F".into(),
                price: Some(3.5),
                change_5d_pct: Some(-3.0),
                flow_direction: "outflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Bonds".into(),
                symbol: "^TNX".into(),
                price: Some(3.8),
                change_5d_pct: Some(-5.0),
                flow_direction: "outflow".into(),
                regime_alignment: "aligned".into(),
            },
        ];
        let patterns = detect_patterns(&[], &flows, "risk-off");
        assert!(
            patterns.iter().any(|p| p.pattern_name == "Deflationary Signal"),
            "should detect deflationary signal"
        );
    }

    #[test]
    fn detect_regime_divergence() {
        let flows = vec![
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "GC=F".into(),
                price: Some(3100.0),
                change_5d_pct: Some(3.0),
                flow_direction: "inflow".into(),
                regime_alignment: "divergent".into(),
            },
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "SI=F".into(),
                price: Some(35.0),
                change_5d_pct: Some(2.0),
                flow_direction: "inflow".into(),
                regime_alignment: "divergent".into(),
            },
            FlowSignal {
                asset_class: "Volatility".into(),
                symbol: "^VIX".into(),
                price: Some(26.0),
                change_5d_pct: Some(10.0),
                flow_direction: "inflow".into(),
                regime_alignment: "divergent".into(),
            },
            FlowSignal {
                asset_class: "Equities".into(),
                symbol: "^GSPC".into(),
                price: Some(5100.0),
                change_5d_pct: Some(-2.0),
                flow_direction: "outflow".into(),
                regime_alignment: "divergent".into(),
            },
            FlowSignal {
                asset_class: "Equities".into(),
                symbol: "^IXIC".into(),
                price: Some(16000.0),
                change_5d_pct: Some(-1.5),
                flow_direction: "outflow".into(),
                regime_alignment: "divergent".into(),
            },
        ];
        let patterns = detect_patterns(&[], &flows, "risk-on");
        assert!(
            patterns.iter().any(|p| p.pattern_name == "Regime Divergence"),
            "should detect regime divergence when flows contradict regime"
        );
    }

    #[test]
    fn detect_dollar_wrecking_ball() {
        let flows = vec![
            FlowSignal {
                asset_class: "Dollar".into(),
                symbol: "DX-Y.NYB".into(),
                price: Some(106.0),
                change_5d_pct: Some(2.0),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Energy".into(),
                symbol: "CL=F".into(),
                price: Some(65.0),
                change_5d_pct: Some(-3.0),
                flow_direction: "outflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "GC=F".into(),
                price: Some(2900.0),
                change_5d_pct: Some(-0.5),
                flow_direction: "flat".into(),
                regime_alignment: "neutral".into(),
            },
        ];
        let patterns = detect_patterns(&[], &flows, "risk-off");
        assert!(
            patterns.iter().any(|p| p.pattern_name == "Dollar Wrecking Ball"),
            "should detect dollar wrecking ball"
        );
    }

    #[test]
    fn summary_dominant_flow_risk_on() {
        let flows: Vec<FlowSignal> = (0..5)
            .map(|i| FlowSignal {
                asset_class: "Test".into(),
                symbol: format!("T{}", i),
                price: Some(100.0),
                change_5d_pct: Some(3.0),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            })
            .chain(std::iter::once(FlowSignal {
                asset_class: "Test".into(),
                symbol: "T5".into(),
                price: Some(100.0),
                change_5d_pct: Some(-2.0),
                flow_direction: "outflow".into(),
                regime_alignment: "divergent".into(),
            }))
            .collect();
        let summary = build_summary(&[], &flows, &[], "risk-on");
        assert!(summary.dominant_flow.contains("inflows"));
    }

    #[test]
    fn summary_regime_consistent_when_aligned() {
        let flows = vec![
            FlowSignal {
                asset_class: "Safe Haven".into(),
                symbol: "GC=F".into(),
                price: Some(3000.0),
                change_5d_pct: Some(2.0),
                flow_direction: "inflow".into(),
                regime_alignment: "aligned".into(),
            },
            FlowSignal {
                asset_class: "Equities".into(),
                symbol: "^GSPC".into(),
                price: Some(5500.0),
                change_5d_pct: Some(-1.0),
                flow_direction: "outflow".into(),
                regime_alignment: "aligned".into(),
            },
        ];
        let summary = build_summary(&[], &flows, &[], "risk-off");
        assert!(summary.regime_consistent);
    }

    #[test]
    fn energy_stress_spiking() {
        let flows = vec![FlowSignal {
            asset_class: "Energy".into(),
            symbol: "CL=F".into(),
            price: Some(95.0),
            change_5d_pct: Some(8.0),
            flow_direction: "inflow".into(),
            regime_alignment: "neutral".into(),
        }];
        let summary = build_summary(&[], &flows, &[], "transition");
        assert!(summary.energy_stress.contains("spiking"));
    }

    #[test]
    fn ratio_defs_have_valid_structure() {
        for r in RATIOS {
            assert!(!r.name.is_empty());
            assert!(!r.numerator.is_empty());
            assert!(!r.denominator.is_empty());
            assert!(!r.rising_interpretation.is_empty());
            assert!(!r.falling_interpretation.is_empty());
        }
    }

    #[test]
    fn flow_assets_have_valid_structure() {
        for a in FLOW_ASSETS {
            assert!(!a.class.is_empty());
            assert!(!a.symbol.is_empty());
        }
    }
}
