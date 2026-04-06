use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::power_flows;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::get_history_backend;
use crate::db::regime_snapshots;

// ── Asset Definitions ────────────────────────────────────────────────

struct ConflictAsset {
    symbol: &'static str,
    label: &'static str,
    group: &'static str,
}

const DEFENSE_ASSETS: &[ConflictAsset] = &[
    ConflictAsset { symbol: "ITA", label: "iShares U.S. Aerospace & Defense", group: "defense" },
    ConflictAsset { symbol: "XAR", label: "SPDR S&P Aerospace & Defense", group: "defense" },
    ConflictAsset { symbol: "PPA", label: "Invesco Aerospace & Defense", group: "defense" },
    ConflictAsset { symbol: "LMT", label: "Lockheed Martin", group: "defense" },
    ConflictAsset { symbol: "RTX", label: "RTX Corporation", group: "defense" },
];

const ENERGY_ASSETS: &[ConflictAsset] = &[
    ConflictAsset { symbol: "XLE", label: "Energy Select Sector SPDR", group: "energy" },
    ConflictAsset { symbol: "CL=F", label: "WTI Crude Oil", group: "energy" },
    ConflictAsset { symbol: "BZ=F", label: "Brent Crude Oil", group: "energy" },
];

const CONTEXT_ASSETS: &[ConflictAsset] = &[
    ConflictAsset { symbol: "^VIX", label: "CBOE Volatility Index", group: "volatility" },
    ConflictAsset { symbol: "GC=F", label: "Gold", group: "safe_haven" },
    ConflictAsset { symbol: "DX-Y.NYB", label: "US Dollar Index", group: "dollar" },
    ConflictAsset { symbol: "^GSPC", label: "S&P 500", group: "equities" },
];

// ── Output Structs ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ConflictsOutput {
    pub regime: RegimeContext,
    pub defense: SectorSnapshot,
    pub energy: SectorSnapshot,
    pub context_assets: Vec<AssetMetric>,
    pub defense_energy_ratio: Option<RatioMetric>,
    pub conflict_indicators: ConflictIndicators,
    pub power_flow_context: PowerFlowContext,
    pub assessment: ConflictAssessment,
}

#[derive(Debug, Serialize)]
pub struct RegimeContext {
    pub current_regime: String,
    pub confidence: Option<f64>,
    pub vix: Option<f64>,
    pub is_crisis: bool,
}

#[derive(Debug, Serialize)]
pub struct SectorSnapshot {
    pub group: String,
    pub assets: Vec<AssetMetric>,
    pub avg_change_5d_pct: Option<f64>,
    pub direction: String,
}

#[derive(Debug, Serialize)]
pub struct AssetMetric {
    pub symbol: String,
    pub label: String,
    pub group: String,
    pub price: Option<f64>,
    pub change_5d_pct: Option<f64>,
    pub change_20d_pct: Option<f64>,
    pub direction: String,
}

#[derive(Debug, Serialize)]
pub struct RatioMetric {
    pub name: String,
    pub numerator: String,
    pub denominator: String,
    pub current_value: Option<f64>,
    pub change_5d_pct: Option<f64>,
    pub interpretation: String,
}

#[derive(Debug, Serialize)]
pub struct ConflictIndicators {
    /// "elevated", "active", "cooling", "dormant"
    pub stress_level: String,
    /// 0-100 composite score
    pub composite_score: u32,
    pub signals: Vec<ConflictSignal>,
}

#[derive(Debug, Serialize)]
pub struct ConflictSignal {
    pub name: String,
    pub active: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct PowerFlowContext {
    pub recent_events: Vec<RecentConflictEvent>,
    pub fic_mic_balance: FicMicBalance,
    pub conflict_events_30d: usize,
}

#[derive(Debug, Serialize)]
pub struct RecentConflictEvent {
    pub date: String,
    pub event: String,
    pub source_complex: String,
    pub direction: String,
    pub magnitude: i32,
}

#[derive(Debug, Serialize)]
pub struct FicMicBalance {
    pub fic_net: i64,
    pub mic_net: i64,
    pub dominant: String,
    pub interpretation: String,
}

#[derive(Debug, Serialize)]
pub struct ConflictAssessment {
    /// "high_alert", "elevated", "monitoring", "low"
    pub alert_level: String,
    pub summary: String,
    pub portfolio_implications: Vec<String>,
}

// ── Helpers ──────────────────────────────────────────────────────────

fn price_f64(prices: &HashMap<String, Decimal>, symbol: &str) -> Option<f64> {
    prices
        .get(symbol)
        .and_then(|d| d.to_string().parse::<f64>().ok())
}

fn change_nd(
    backend: &BackendConnection,
    symbol: &str,
    current: f64,
    lookback: usize,
) -> Option<f64> {
    let rows = get_history_backend(backend, symbol, (lookback + 5) as u32).ok()?;
    if rows.len() < lookback + 1 {
        return None;
    }
    let prev = rows[rows.len().saturating_sub(lookback + 1)]
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
        Some(c) if c > 2.0 => "rising_strong".to_string(),
        Some(c) if c > 0.5 => "rising".to_string(),
        Some(c) if c < -2.0 => "falling_strong".to_string(),
        Some(c) if c < -0.5 => "falling".to_string(),
        Some(_) => "flat".to_string(),
        None => "unknown".to_string(),
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ── Core Logic ───────────────────────────────────────────────────────

fn build_metrics(
    backend: &BackendConnection,
    prices: &HashMap<String, Decimal>,
    assets: &[ConflictAsset],
) -> Vec<AssetMetric> {
    assets
        .iter()
        .map(|a| {
            let price = price_f64(prices, a.symbol);
            let c5 = price.and_then(|p| change_nd(backend, a.symbol, p, 5));
            let c20 = price.and_then(|p| change_nd(backend, a.symbol, p, 20));
            let dir = direction_label(c5);
            AssetMetric {
                symbol: a.symbol.to_string(),
                label: a.label.to_string(),
                group: a.group.to_string(),
                price: price.map(round2),
                change_5d_pct: c5.map(round2),
                change_20d_pct: c20.map(round2),
                direction: dir,
            }
        })
        .collect()
}

fn sector_snapshot(
    backend: &BackendConnection,
    prices: &HashMap<String, Decimal>,
    assets: &[ConflictAsset],
    group_name: &str,
) -> SectorSnapshot {
    let metrics = build_metrics(backend, prices, assets);

    let changes: Vec<f64> = metrics.iter().filter_map(|m| m.change_5d_pct).collect();
    let avg = if changes.is_empty() {
        None
    } else {
        Some(round2(changes.iter().sum::<f64>() / changes.len() as f64))
    };
    let dir = direction_label(avg);

    SectorSnapshot {
        group: group_name.to_string(),
        assets: metrics,
        avg_change_5d_pct: avg,
        direction: dir,
    }
}

fn compute_defense_energy_ratio(
    backend: &BackendConnection,
    prices: &HashMap<String, Decimal>,
) -> Option<RatioMetric> {
    let ita = price_f64(prices, "ITA")?;
    let xle = price_f64(prices, "XLE")?;
    if xle.abs() < 1e-10 {
        return None;
    }
    let current = ita / xle;

    // 5-day change of ratio
    let ita_hist = get_history_backend(backend, "ITA", 10).ok()?;
    let xle_hist = get_history_backend(backend, "XLE", 10).ok()?;
    let min_len = ita_hist.len().min(xle_hist.len());
    let change_5d = if min_len >= 6 {
        let prev_ita = ita_hist[ita_hist.len().saturating_sub(6)]
            .close
            .to_string()
            .parse::<f64>()
            .ok()?;
        let prev_xle = xle_hist[xle_hist.len().saturating_sub(6)]
            .close
            .to_string()
            .parse::<f64>()
            .ok()?;
        if prev_xle.abs() > 1e-10 {
            let prev_ratio = prev_ita / prev_xle;
            if prev_ratio.abs() > 1e-10 {
                Some(round2(((current - prev_ratio) / prev_ratio) * 100.0))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let interp = match change_5d {
        Some(c) if c > 1.0 => {
            "Defense outpacing energy — capital rotating into conflict beneficiaries".to_string()
        }
        Some(c) if c < -1.0 => {
            "Energy outpacing defense — supply/demand dynamics dominant over conflict premium"
                .to_string()
        }
        _ => {
            "Defense and energy moving in tandem — no clear conflict rotation signal".to_string()
        }
    };

    Some(RatioMetric {
        name: "Defense/Energy (ITA/XLE)".to_string(),
        numerator: "ITA".to_string(),
        denominator: "XLE".to_string(),
        current_value: Some(round2(current)),
        change_5d_pct: change_5d,
        interpretation: interp,
    })
}

fn detect_conflict_signals(
    defense: &SectorSnapshot,
    energy: &SectorSnapshot,
    context: &[AssetMetric],
) -> ConflictIndicators {
    let mut signals = Vec::new();
    let mut score: u32 = 0;

    // 1. Defense sector strength
    let defense_strong = defense
        .avg_change_5d_pct
        .map(|c| c > 2.0)
        .unwrap_or(false);
    let defense_rising = defense
        .avg_change_5d_pct
        .map(|c| c > 0.5)
        .unwrap_or(false);
    signals.push(ConflictSignal {
        name: "Defense sector bid".to_string(),
        active: defense_rising,
        detail: format!(
            "Defense avg 5d: {:+.1}%{}",
            defense.avg_change_5d_pct.unwrap_or(0.0),
            if defense_strong { " (strong)" } else { "" }
        ),
    });
    if defense_strong {
        score += 25;
    } else if defense_rising {
        score += 10;
    }

    // 2. Oil/energy spike
    let oil_price = energy
        .assets
        .iter()
        .find(|a| a.symbol == "CL=F")
        .and_then(|a| a.change_5d_pct);
    let oil_spiking = oil_price.map(|c| c > 5.0).unwrap_or(false);
    let oil_rising = oil_price.map(|c| c > 1.0).unwrap_or(false);
    signals.push(ConflictSignal {
        name: "Oil supply-risk premium".to_string(),
        active: oil_rising,
        detail: format!(
            "WTI 5d: {:+.1}%{}",
            oil_price.unwrap_or(0.0),
            if oil_spiking { " (spiking — supply threat)" } else { "" }
        ),
    });
    if oil_spiking {
        score += 25;
    } else if oil_rising {
        score += 10;
    }

    // 3. VIX elevated
    let vix_val = context
        .iter()
        .find(|a| a.symbol == "^VIX")
        .and_then(|a| a.price);
    let vix_elevated = vix_val.map(|v| v >= 25.0).unwrap_or(false);
    let vix_rising = context
        .iter()
        .find(|a| a.symbol == "^VIX")
        .and_then(|a| a.change_5d_pct)
        .map(|c| c > 2.0)
        .unwrap_or(false);
    signals.push(ConflictSignal {
        name: "Fear elevated".to_string(),
        active: vix_elevated || vix_rising,
        detail: format!(
            "VIX: {:.1}{}",
            vix_val.unwrap_or(0.0),
            if vix_elevated { " (≥25 — fear regime)" } else { "" }
        ),
    });
    if vix_elevated {
        score += 20;
    } else if vix_rising {
        score += 8;
    }

    // 4. Gold safe-haven bid
    let gold_rising = context
        .iter()
        .find(|a| a.symbol == "GC=F")
        .and_then(|a| a.change_5d_pct)
        .map(|c| c > 1.0)
        .unwrap_or(false);
    signals.push(ConflictSignal {
        name: "Safe-haven gold bid".to_string(),
        active: gold_rising,
        detail: format!(
            "Gold 5d: {:+.1}%",
            context
                .iter()
                .find(|a| a.symbol == "GC=F")
                .and_then(|a| a.change_5d_pct)
                .unwrap_or(0.0)
        ),
    });
    if gold_rising {
        score += 15;
    }

    // 5. Equities under pressure (flight from risk)
    let spx_falling = context
        .iter()
        .find(|a| a.symbol == "^GSPC")
        .and_then(|a| a.change_5d_pct)
        .map(|c| c < -1.0)
        .unwrap_or(false);
    signals.push(ConflictSignal {
        name: "Equity risk-off".to_string(),
        active: spx_falling,
        detail: format!(
            "S&P 500 5d: {:+.1}%",
            context
                .iter()
                .find(|a| a.symbol == "^GSPC")
                .and_then(|a| a.change_5d_pct)
                .unwrap_or(0.0)
        ),
    });
    if spx_falling {
        score += 15;
    }

    // Clamp to 100
    score = score.min(100);

    let active_count = signals.iter().filter(|s| s.active).count();
    let stress_level = if score >= 70 {
        "elevated"
    } else if score >= 40 || active_count >= 3 {
        "active"
    } else if active_count >= 1 {
        "cooling"
    } else {
        "dormant"
    };

    ConflictIndicators {
        stress_level: stress_level.to_string(),
        composite_score: score,
        signals,
    }
}

fn build_power_flow_context(
    backend: &BackendConnection,
    days: usize,
) -> PowerFlowContext {
    // Get power flow events filtered to FIC and MIC (the conflict-relevant complexes)
    let all_entries = power_flows::list_power_flows_backend(backend, None, None, days)
        .unwrap_or_default();

    // Filter to conflict-relevant events (FIC or MIC involved)
    let conflict_entries: Vec<_> = all_entries
        .iter()
        .filter(|e| {
            e.source_complex == "FIC"
                || e.source_complex == "MIC"
                || e.target_complex.as_deref() == Some("FIC")
                || e.target_complex.as_deref() == Some("MIC")
        })
        .collect();

    let recent_events: Vec<RecentConflictEvent> = conflict_entries
        .iter()
        .rev()
        .take(5)
        .map(|e| RecentConflictEvent {
            date: e.date.clone(),
            event: e.event.clone(),
            source_complex: e.source_complex.clone(),
            direction: e.direction.clone(),
            magnitude: e.magnitude,
        })
        .collect();

    // Compute FIC vs MIC balance
    let balances =
        power_flows::compute_balance_backend(backend, days).unwrap_or_default();
    let fic_balance = balances.iter().find(|b| b.complex == "FIC");
    let mic_balance = balances.iter().find(|b| b.complex == "MIC");
    let fic_net = fic_balance
        .map(|b| b.gaining_magnitude - b.losing_magnitude)
        .unwrap_or(0);
    let mic_net = mic_balance
        .map(|b| b.gaining_magnitude - b.losing_magnitude)
        .unwrap_or(0);

    let dominant = if fic_net > mic_net + 2 {
        "FIC"
    } else if mic_net > fic_net + 2 {
        "MIC"
    } else {
        "contested"
    };

    let interpretation = match dominant {
        "MIC" => {
            "Military-Industrial Complex gaining — defense/security spending accelerating, conflict premium in markets".to_string()
        }
        "FIC" => {
            "Financial-Industrial Complex dominant — capital/financial flows driving markets over security concerns".to_string()
        }
        _ => {
            "FIC and MIC in balance — no clear power shift between financial and military-industrial complexes".to_string()
        }
    };

    PowerFlowContext {
        recent_events,
        fic_mic_balance: FicMicBalance {
            fic_net,
            mic_net,
            dominant: dominant.to_string(),
            interpretation,
        },
        conflict_events_30d: conflict_entries.len(),
    }
}

fn build_assessment(
    indicators: &ConflictIndicators,
    power_flow: &PowerFlowContext,
    regime: &RegimeContext,
    defense: &SectorSnapshot,
    energy: &SectorSnapshot,
) -> ConflictAssessment {
    let score = indicators.composite_score;
    let mic_dominant = power_flow.fic_mic_balance.dominant == "MIC";

    let alert_level = if score >= 70 && mic_dominant {
        "high_alert"
    } else if score >= 70 || (score >= 40 && mic_dominant) {
        "elevated"
    } else if score >= 25 || power_flow.conflict_events_30d >= 3 {
        "monitoring"
    } else {
        "low"
    };

    let active_signals: Vec<&str> = indicators
        .signals
        .iter()
        .filter(|s| s.active)
        .map(|s| s.name.as_str())
        .collect();

    let summary = if active_signals.is_empty() {
        "No active conflict signals. Defense and energy sectors showing normal behaviour. Geopolitical risk premium minimal.".to_string()
    } else {
        format!(
            "Conflict stress: {} (score: {}/100). Active signals: {}. {}{}",
            indicators.stress_level,
            score,
            active_signals.join(", "),
            if regime.is_crisis {
                "Market in crisis regime — conflict premium amplified. "
            } else {
                ""
            },
            if mic_dominant {
                "MIC gaining power — military-industrial complex strengthening."
            } else {
                ""
            }
        )
    };

    let mut implications = Vec::new();

    if score >= 40 {
        implications.push(
            "Defense sector (ITA, XAR, PPA) showing conflict premium — monitor for sustained breakout".to_string()
        );
    }
    if indicators
        .signals
        .iter()
        .any(|s| s.name == "Oil supply-risk premium" && s.active)
    {
        implications.push(
            "Oil supply-risk premium active — energy exposure benefits from conflict escalation".to_string()
        );
    }
    if regime.is_crisis {
        implications.push(
            "Crisis regime amplifies conflict premium — safe havens (gold, USD) outperforming".to_string()
        );
    }
    if mic_dominant {
        implications.push(
            "MIC gaining power suggests sustained defense spending cycle — structural tailwind for defense names".to_string()
        );
    }

    let defense_strong = defense
        .avg_change_5d_pct
        .map(|c| c > 2.0)
        .unwrap_or(false);
    let energy_strong = energy
        .avg_change_5d_pct
        .map(|c| c > 2.0)
        .unwrap_or(false);

    if defense_strong && energy_strong {
        implications.push(
            "Both defense and energy rising — classic geopolitical escalation pattern".to_string()
        );
    } else if defense_strong && !energy_strong {
        implications.push(
            "Defense outpacing energy — market pricing in defense spending over supply disruption".to_string()
        );
    } else if !defense_strong && energy_strong {
        implications.push(
            "Energy outpacing defense — supply disruption fear dominant over defense spending narrative".to_string()
        );
    }

    if implications.is_empty() {
        implications.push(
            "No significant conflict-driven portfolio implications at current levels".to_string()
        );
    }

    ConflictAssessment {
        alert_level: alert_level.to_string(),
        summary,
        portfolio_implications: implications,
    }
}

// ── Public Entry Point ───────────────────────────────────────────────

pub fn build_output(
    backend: &BackendConnection,
    days: usize,
) -> Result<ConflictsOutput> {
    // 1. Get current regime
    let regime_snap = regime_snapshots::get_current_backend(backend)?;
    let regime_str = regime_snap
        .as_ref()
        .map(|r| r.regime.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let vix_val = regime_snap.as_ref().and_then(|r| r.vix);
    let is_crisis = matches!(regime_str.as_str(), "crisis" | "stagflation")
        || vix_val.map(|v| v >= 30.0).unwrap_or(false);

    let regime_ctx = RegimeContext {
        current_regime: regime_str,
        confidence: regime_snap.as_ref().and_then(|r| r.confidence),
        vix: vix_val,
        is_crisis,
    };

    // 2. Build price lookup
    let all_prices = get_all_cached_prices_backend(backend)?;
    let prices: HashMap<String, Decimal> = all_prices
        .iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    // 3. Build sector snapshots
    let defense = sector_snapshot(backend, &prices, DEFENSE_ASSETS, "defense");
    let energy = sector_snapshot(backend, &prices, ENERGY_ASSETS, "energy");
    let context_assets = build_metrics(backend, &prices, CONTEXT_ASSETS);

    // 4. Defense/Energy ratio
    let ratio = compute_defense_energy_ratio(backend, &prices);

    // 5. Conflict indicators
    let indicators = detect_conflict_signals(&defense, &energy, &context_assets);

    // 6. Power flow context
    let power_flow = build_power_flow_context(backend, days);

    // 7. Assessment
    let assessment = build_assessment(&indicators, &power_flow, &regime_ctx, &defense, &energy);

    Ok(ConflictsOutput {
        regime: regime_ctx,
        defense,
        energy,
        context_assets,
        defense_energy_ratio: ratio,
        conflict_indicators: indicators,
        power_flow_context: power_flow,
        assessment,
    })
}

pub fn run(
    backend: &BackendConnection,
    days: usize,
    json_output: bool,
) -> Result<()> {
    let output = build_output(backend, days)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_terminal(&output);
    }

    Ok(())
}

fn print_terminal(output: &ConflictsOutput) {
    println!("═══ FIC/MIC Conflict Monitor ═══\n");

    // Regime
    println!(
        "Regime: {} | VIX: {:.1} | Crisis: {}",
        output.regime.current_regime.to_uppercase(),
        output.regime.vix.unwrap_or(0.0),
        if output.regime.is_crisis { "YES" } else { "no" }
    );
    println!();

    // Alert level + score
    let alert_icon = match output.assessment.alert_level.as_str() {
        "high_alert" => "🔴",
        "elevated" => "🟠",
        "monitoring" => "🟡",
        _ => "🟢",
    };
    println!(
        "{} Alert: {} (score: {}/100)",
        alert_icon,
        output.assessment.alert_level.to_uppercase(),
        output.conflict_indicators.composite_score
    );
    println!();

    // Defense sector
    println!(
        "── Defense Sector (avg 5d: {:+.1}%) ──",
        output.defense.avg_change_5d_pct.unwrap_or(0.0)
    );
    for a in &output.defense.assets {
        let arrow = match a.direction.as_str() {
            "rising_strong" => "↑↑",
            "rising" => "↑",
            "falling_strong" => "↓↓",
            "falling" => "↓",
            _ => "→",
        };
        println!(
            "  {:<6} ${:>9.2}  5d: {:+.1}%  20d: {:+.1}%  {}",
            a.symbol,
            a.price.unwrap_or(0.0),
            a.change_5d_pct.unwrap_or(0.0),
            a.change_20d_pct.unwrap_or(0.0),
            arrow
        );
    }
    println!();

    // Energy sector
    println!(
        "── Energy Sector (avg 5d: {:+.1}%) ──",
        output.energy.avg_change_5d_pct.unwrap_or(0.0)
    );
    for a in &output.energy.assets {
        let arrow = match a.direction.as_str() {
            "rising_strong" => "↑↑",
            "rising" => "↑",
            "falling_strong" => "↓↓",
            "falling" => "↓",
            _ => "→",
        };
        println!(
            "  {:<6} ${:>9.2}  5d: {:+.1}%  20d: {:+.1}%  {}",
            a.symbol,
            a.price.unwrap_or(0.0),
            a.change_5d_pct.unwrap_or(0.0),
            a.change_20d_pct.unwrap_or(0.0),
            arrow
        );
    }
    println!();

    // Defense/Energy ratio
    if let Some(ratio) = &output.defense_energy_ratio {
        println!(
            "── {} ──",
            ratio.name
        );
        println!(
            "  Value: {:.3}  5d: {:+.1}%",
            ratio.current_value.unwrap_or(0.0),
            ratio.change_5d_pct.unwrap_or(0.0)
        );
        println!("  {}", ratio.interpretation);
        println!();
    }

    // Context assets
    println!("── Context ──");
    for a in &output.context_assets {
        let arrow = match a.direction.as_str() {
            "rising_strong" => "↑↑",
            "rising" => "↑",
            "falling_strong" => "↓↓",
            "falling" => "↓",
            _ => "→",
        };
        println!(
            "  {:<10} {:>9.2}  5d: {:+.1}%  {}",
            a.symbol,
            a.price.unwrap_or(0.0),
            a.change_5d_pct.unwrap_or(0.0),
            arrow
        );
    }
    println!();

    // Conflict signals
    println!("── Conflict Signals ──");
    for s in &output.conflict_indicators.signals {
        let icon = if s.active { "🔥" } else { "⚪" };
        println!("  {} {}: {}", icon, s.name, s.detail);
    }
    println!();

    // Power flow context
    let pf = &output.power_flow_context;
    println!(
        "── Power Flow ({} conflict events in {}d) ──",
        pf.conflict_events_30d,
        30, // default days shown
    );
    println!(
        "  FIC net: {:+}  MIC net: {:+}  Dominant: {}",
        pf.fic_mic_balance.fic_net,
        pf.fic_mic_balance.mic_net,
        pf.fic_mic_balance.dominant
    );
    println!("  {}", pf.fic_mic_balance.interpretation);
    if !pf.recent_events.is_empty() {
        println!("  Recent:");
        for e in &pf.recent_events {
            println!(
                "    [{}] {} {} (mag {}) — {}",
                e.date, e.source_complex, e.direction, e.magnitude, e.event
            );
        }
    }
    println!();

    // Assessment
    println!("── Assessment ──");
    println!("{}", output.assessment.summary);
    if !output.assessment.portfolio_implications.is_empty() {
        println!("\nImplications:");
        for imp in &output.assessment.portfolio_implications {
            println!("  • {}", imp);
        }
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direction_label() {
        assert_eq!(direction_label(Some(3.0)), "rising_strong");
        assert_eq!(direction_label(Some(1.0)), "rising");
        assert_eq!(direction_label(Some(0.2)), "flat");
        assert_eq!(direction_label(Some(-0.8)), "falling");
        assert_eq!(direction_label(Some(-3.0)), "falling_strong");
        assert_eq!(direction_label(None), "unknown");
    }

    #[test]
    fn test_round2() {
        assert_eq!(round2(1.2345), 1.23);
        assert_eq!(round2(-0.999), -1.0);
        assert_eq!(round2(0.0), 0.0);
    }

    #[test]
    fn test_detect_conflict_signals_dormant() {
        let defense = SectorSnapshot {
            group: "defense".to_string(),
            assets: vec![],
            avg_change_5d_pct: Some(0.1),
            direction: "flat".to_string(),
        };
        let energy = SectorSnapshot {
            group: "energy".to_string(),
            assets: vec![AssetMetric {
                symbol: "CL=F".to_string(),
                label: "WTI".to_string(),
                group: "energy".to_string(),
                price: Some(70.0),
                change_5d_pct: Some(0.2),
                change_20d_pct: Some(-1.0),
                direction: "flat".to_string(),
            }],
            avg_change_5d_pct: Some(0.2),
            direction: "flat".to_string(),
        };
        let context = vec![
            AssetMetric {
                symbol: "^VIX".to_string(),
                label: "VIX".to_string(),
                group: "volatility".to_string(),
                price: Some(15.0),
                change_5d_pct: Some(-0.5),
                change_20d_pct: None,
                direction: "flat".to_string(),
            },
            AssetMetric {
                symbol: "GC=F".to_string(),
                label: "Gold".to_string(),
                group: "safe_haven".to_string(),
                price: Some(2000.0),
                change_5d_pct: Some(-0.3),
                change_20d_pct: None,
                direction: "flat".to_string(),
            },
            AssetMetric {
                symbol: "^GSPC".to_string(),
                label: "S&P 500".to_string(),
                group: "equities".to_string(),
                price: Some(5000.0),
                change_5d_pct: Some(0.5),
                change_20d_pct: None,
                direction: "flat".to_string(),
            },
        ];

        let indicators = detect_conflict_signals(&defense, &energy, &context);
        assert_eq!(indicators.stress_level, "dormant");
        assert_eq!(indicators.composite_score, 0);
        assert!(indicators.signals.iter().all(|s| !s.active));
    }

    #[test]
    fn test_detect_conflict_signals_elevated() {
        let defense = SectorSnapshot {
            group: "defense".to_string(),
            assets: vec![],
            avg_change_5d_pct: Some(4.5),
            direction: "rising_strong".to_string(),
        };
        let energy = SectorSnapshot {
            group: "energy".to_string(),
            assets: vec![AssetMetric {
                symbol: "CL=F".to_string(),
                label: "WTI".to_string(),
                group: "energy".to_string(),
                price: Some(85.0),
                change_5d_pct: Some(7.0),
                change_20d_pct: Some(10.0),
                direction: "rising_strong".to_string(),
            }],
            avg_change_5d_pct: Some(5.0),
            direction: "rising_strong".to_string(),
        };
        let context = vec![
            AssetMetric {
                symbol: "^VIX".to_string(),
                label: "VIX".to_string(),
                group: "volatility".to_string(),
                price: Some(28.0),
                change_5d_pct: Some(5.0),
                change_20d_pct: None,
                direction: "rising_strong".to_string(),
            },
            AssetMetric {
                symbol: "GC=F".to_string(),
                label: "Gold".to_string(),
                group: "safe_haven".to_string(),
                price: Some(2100.0),
                change_5d_pct: Some(3.0),
                change_20d_pct: None,
                direction: "rising_strong".to_string(),
            },
            AssetMetric {
                symbol: "^GSPC".to_string(),
                label: "S&P 500".to_string(),
                group: "equities".to_string(),
                price: Some(4800.0),
                change_5d_pct: Some(-2.5),
                change_20d_pct: None,
                direction: "falling_strong".to_string(),
            },
        ];

        let indicators = detect_conflict_signals(&defense, &energy, &context);
        assert_eq!(indicators.stress_level, "elevated");
        assert_eq!(indicators.composite_score, 100); // all signals active, clamped
        assert!(indicators.signals.iter().filter(|s| s.active).count() >= 4);
    }

    #[test]
    fn test_detect_conflict_signals_partial() {
        let defense = SectorSnapshot {
            group: "defense".to_string(),
            assets: vec![],
            avg_change_5d_pct: Some(3.0),
            direction: "rising_strong".to_string(),
        };
        let energy = SectorSnapshot {
            group: "energy".to_string(),
            assets: vec![AssetMetric {
                symbol: "CL=F".to_string(),
                label: "WTI".to_string(),
                group: "energy".to_string(),
                price: Some(72.0),
                change_5d_pct: Some(0.5),
                change_20d_pct: Some(1.0),
                direction: "flat".to_string(),
            }],
            avg_change_5d_pct: Some(0.5),
            direction: "flat".to_string(),
        };
        let context = vec![
            AssetMetric {
                symbol: "^VIX".to_string(),
                label: "VIX".to_string(),
                group: "volatility".to_string(),
                price: Some(18.0),
                change_5d_pct: Some(1.0),
                change_20d_pct: None,
                direction: "rising".to_string(),
            },
            AssetMetric {
                symbol: "GC=F".to_string(),
                label: "Gold".to_string(),
                group: "safe_haven".to_string(),
                price: Some(2050.0),
                change_5d_pct: Some(2.0),
                change_20d_pct: None,
                direction: "rising".to_string(),
            },
            AssetMetric {
                symbol: "^GSPC".to_string(),
                label: "S&P 500".to_string(),
                group: "equities".to_string(),
                price: Some(5100.0),
                change_5d_pct: Some(0.3),
                change_20d_pct: None,
                direction: "flat".to_string(),
            },
        ];

        let indicators = detect_conflict_signals(&defense, &energy, &context);
        // Defense strong (25) + gold rising (15) = 40
        assert_eq!(indicators.composite_score, 40);
        assert_eq!(indicators.stress_level, "active");
    }

    #[test]
    fn test_build_assessment_high_alert() {
        let indicators = ConflictIndicators {
            stress_level: "elevated".to_string(),
            composite_score: 75,
            signals: vec![
                ConflictSignal {
                    name: "Defense sector bid".to_string(),
                    active: true,
                    detail: "test".to_string(),
                },
                ConflictSignal {
                    name: "Oil supply-risk premium".to_string(),
                    active: true,
                    detail: "test".to_string(),
                },
            ],
        };
        let power_flow = PowerFlowContext {
            recent_events: vec![],
            fic_mic_balance: FicMicBalance {
                fic_net: -2,
                mic_net: 8,
                dominant: "MIC".to_string(),
                interpretation: "MIC dominant".to_string(),
            },
            conflict_events_30d: 5,
        };
        let regime = RegimeContext {
            current_regime: "crisis".to_string(),
            confidence: Some(0.85),
            vix: Some(32.0),
            is_crisis: true,
        };
        let defense = SectorSnapshot {
            group: "defense".to_string(),
            assets: vec![],
            avg_change_5d_pct: Some(4.0),
            direction: "rising_strong".to_string(),
        };
        let energy = SectorSnapshot {
            group: "energy".to_string(),
            assets: vec![],
            avg_change_5d_pct: Some(3.0),
            direction: "rising_strong".to_string(),
        };

        let assessment =
            build_assessment(&indicators, &power_flow, &regime, &defense, &energy);
        assert_eq!(assessment.alert_level, "high_alert");
        assert!(assessment.summary.contains("Conflict stress"));
        assert!(!assessment.portfolio_implications.is_empty());
    }

    #[test]
    fn test_build_assessment_low() {
        let indicators = ConflictIndicators {
            stress_level: "dormant".to_string(),
            composite_score: 0,
            signals: vec![],
        };
        let power_flow = PowerFlowContext {
            recent_events: vec![],
            fic_mic_balance: FicMicBalance {
                fic_net: 2,
                mic_net: 1,
                dominant: "contested".to_string(),
                interpretation: "balanced".to_string(),
            },
            conflict_events_30d: 0,
        };
        let regime = RegimeContext {
            current_regime: "risk-on".to_string(),
            confidence: Some(0.7),
            vix: Some(14.0),
            is_crisis: false,
        };
        let defense = SectorSnapshot {
            group: "defense".to_string(),
            assets: vec![],
            avg_change_5d_pct: Some(0.5),
            direction: "flat".to_string(),
        };
        let energy = SectorSnapshot {
            group: "energy".to_string(),
            assets: vec![],
            avg_change_5d_pct: Some(-0.2),
            direction: "flat".to_string(),
        };

        let assessment =
            build_assessment(&indicators, &power_flow, &regime, &defense, &energy);
        assert_eq!(assessment.alert_level, "low");
        assert!(assessment.summary.contains("No active conflict signals"));
    }
}
