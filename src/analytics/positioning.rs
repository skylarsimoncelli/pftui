//! Positioning synthesizer — the capstone that conditions a stance on the
//! measured analog forward-return distribution, the regime quad, and the cycle
//! clock, then reports it with its honesty stats foregrounded
//! (`docs/ENVIRONMENT-ENGINE.md` §3.5).
//!
//! Transparent by construction: every driver contributes a score in [-1, 1]
//! with a stated weight and a human reason, so the stance is fully auditable —
//! no opaque blend. The **humility default** applies: when the analog evidence
//! is thin or its CI straddles zero, the card says "insufficient edge" rather
//! than manufacturing confidence.
//!
//! All values `f64`.

use serde::Serialize;

use super::analog::AnalogReport;
use super::regime_quad::Quad;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Stance {
    Bull,
    Bear,
    Neutral,
}

impl Stance {
    pub fn label(&self) -> &'static str {
        match self {
            Stance::Bull => "BULLISH",
            Stance::Bear => "BEARISH",
            Stance::Neutral => "NEUTRAL",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Driver {
    pub name: String,
    /// Directional score in [-1, 1].
    pub score: f64,
    /// Weight in the blend.
    pub weight: f64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PositioningCard {
    pub asset: String,
    pub as_of: String,
    pub stance: Stance,
    /// 0-100 confidence in the stance.
    pub confidence_pct: f64,
    /// Weighted blend score in [-1, 1].
    pub blend_score: f64,
    pub regime: String,
    pub drivers: Vec<Driver>,
    /// The measured analog distribution that anchors the call.
    pub analog_median_forward_pct: Option<f64>,
    pub analog_ci_pct: Option<(f64, f64)>,
    pub analog_n: usize,
    pub honesty_note: String,
}

/// Asset bucket for the (transparent) regime lean table.
fn bucket(asset: &str) -> &'static str {
    let a = asset.to_uppercase();
    if a.contains("BTC") || a.contains("ETH") || a.contains("SOL") {
        "crypto"
    } else if a.contains("GC=F") || a.contains("GOLD") || a.contains("SI=F") || a.contains("SILVER") {
        "hard_money"
    } else if a.contains("SPY") || a.contains("QQQ") || a.contains("GSPC") || a.contains("NDX") {
        "equity"
    } else if matches!(a.as_str(), "TLT" | "IEF" | "SHY" | "BIL" | "ZB=F" | "ZN=F" | "ZF=F" | "ZT=F")
        || a.contains("TREASURY")
    {
        "bonds"
    } else {
        "other"
    }
}

/// Transparent per-bucket regime lean (the GIP playbook, simplified):
/// score in [-1, 1] for how this asset class tends to do in each quad.
fn regime_lean(asset: &str, quad: Quad) -> (f64, String) {
    let b = bucket(asset);
    let s = match (b, quad) {
        ("crypto", Quad::Goldilocks) => 0.5,
        ("crypto", Quad::Reflation) => 0.6,
        ("crypto", Quad::Inflation) => -0.2,
        ("crypto", Quad::Deflation) => -0.7,
        ("hard_money", Quad::Goldilocks) => 0.0,
        ("hard_money", Quad::Reflation) => 0.4,
        ("hard_money", Quad::Inflation) => 0.7,
        ("hard_money", Quad::Deflation) => -0.2,
        ("equity", Quad::Goldilocks) => 0.6,
        ("equity", Quad::Reflation) => 0.3,
        ("equity", Quad::Inflation) => -0.3,
        ("equity", Quad::Deflation) => -0.6,
        // Duration: the textbook deflation/disinflation winner, inflation loser.
        ("bonds", Quad::Goldilocks) => 0.2,
        ("bonds", Quad::Reflation) => -0.3,
        ("bonds", Quad::Inflation) => -0.6,
        ("bonds", Quad::Deflation) => 0.6,
        _ => 0.0,
    };
    let detail = format!(
        "{b} in {} regime historically leans {}",
        quad.short(),
        if s > 0.1 {
            "supportive"
        } else if s < -0.1 {
            "headwind"
        } else {
            "neutral"
        }
    );
    (s, detail)
}

/// Convert the analog forward-return distribution into a directional score,
/// discounted for thin samples and CIs that straddle zero.
fn analog_score(a: &AnalogReport) -> (f64, String, f64) {
    let Some(median) = a.median_forward_pct else {
        return (0.0, "no resolvable analog forward returns".to_string(), 0.0);
    };
    // Base direction from the median, saturating at ±10%.
    let mut score = (median / 10.0).clamp(-1.0, 1.0);
    // Blend with the up-rate tilt.
    if let Some(up) = a.up_rate_pct {
        score = 0.6 * score + 0.4 * ((up - 50.0) / 50.0);
    }
    // Quality discount: thin sample and/or CI straddling zero shrink it.
    let mut quality = 1.0;
    if a.n_with_forward < 10 {
        quality *= a.n_with_forward as f64 / 10.0;
    }
    if let Some((lo, hi)) = a.mean_forward_ci_pct {
        if lo < 0.0 && hi > 0.0 {
            quality *= 0.5; // CI includes zero — direction is uncertain
        }
    }
    score *= quality;
    let detail = format!(
        "{} analogs: median {:+.1}% / up-rate {} over {}d (quality {:.0}%)",
        a.n_with_forward,
        median,
        a.up_rate_pct.map(|u| format!("{u:.0}%")).unwrap_or_else(|| "—".into()),
        a.horizon_days,
        quality * 100.0,
    );
    (score.clamp(-1.0, 1.0), detail, quality)
}

/// Synthesize a positioning card. `cycle` is an optional (score, detail) pair
/// the caller derives from the cycle clock (e.g. accumulation-zone lean).
pub fn synthesize(
    asset: &str,
    as_of: &str,
    analog: &AnalogReport,
    quad: Quad,
    cycle: Option<(f64, String)>,
) -> PositioningCard {
    let (a_score, a_detail, a_quality) = analog_score(analog);
    let (r_score, r_detail) = regime_lean(asset, quad);

    let mut drivers = vec![
        Driver {
            name: "analog forward returns".to_string(),
            score: round2(a_score),
            weight: 0.5,
            detail: a_detail,
        },
        Driver {
            name: "regime quad".to_string(),
            score: round2(r_score),
            weight: 0.3,
            detail: r_detail,
        },
    ];
    if let Some((c_score, c_detail)) = &cycle {
        drivers.push(Driver {
            name: "cycle clock".to_string(),
            score: round2(*c_score),
            weight: 0.2,
            detail: c_detail.clone(),
        });
    }

    let total_w: f64 = drivers.iter().map(|d| d.weight).sum();
    let blend: f64 = drivers.iter().map(|d| d.score * d.weight).sum::<f64>() / total_w.max(1e-9);

    let stance = if blend > 0.25 {
        Stance::Bull
    } else if blend < -0.25 {
        Stance::Bear
    } else {
        Stance::Neutral
    };

    // Confidence: magnitude of the blend, scaled by analog quality and driver
    // agreement; capped low when the analog evidence is weak.
    let agreement = driver_agreement(&drivers);
    let confidence = (blend.abs() * 100.0 * (0.4 + 0.6 * a_quality) * (0.5 + 0.5 * agreement))
        .clamp(0.0, 100.0);

    let honesty_note = if analog.n_with_forward < 10 {
        format!(
            "LOW CONFIDENCE — only {} resolvable analogs; treat as a lean, not a call. \
             The analog backtest is single-regime and silent on a true regime break.",
            analog.n_with_forward
        )
    } else if analog
        .mean_forward_ci_pct
        .map(|(lo, hi)| lo < 0.0 && hi > 0.0)
        .unwrap_or(true)
    {
        "MODERATE — the analog mean's 90% CI straddles zero, so direction is uncertain; \
         size accordingly. Backtest is single-regime."
            .to_string()
    } else {
        "Analog CI is one-sided and the sample is adequate — the measured edge is directional. \
         Still single-regime: a genuine regime break voids it."
            .to_string()
    };

    PositioningCard {
        asset: asset.to_string(),
        as_of: as_of.to_string(),
        stance,
        confidence_pct: round1(confidence),
        blend_score: round2(blend),
        regime: quad.short().to_string(),
        drivers,
        analog_median_forward_pct: analog.median_forward_pct,
        analog_ci_pct: analog.mean_forward_ci_pct,
        analog_n: analog.n_with_forward,
        honesty_note,
    }
}

/// Agreement in [0,1]: 1 when ≥2 non-trivial drivers share a sign, 0 when split.
/// A LONE firing driver scores 0.5 (neither corroborated nor contradicted) and
/// no firing driver scores 0 — an uncorroborated single signal must not earn
/// the full "everyone agrees" multiplier.
fn driver_agreement(drivers: &[Driver]) -> f64 {
    let signs: Vec<f64> = drivers
        .iter()
        .filter(|d| d.score.abs() > 0.05)
        .map(|d| d.score.signum())
        .collect();
    match signs.len() {
        0 => 0.0,
        1 => 0.5, // uncorroborated — moderate, not max
        _ => {
            let pos = signs.iter().filter(|s| **s > 0.0).count() as f64;
            let frac = pos / signs.len() as f64;
            (frac - 0.5).abs() * 2.0
        }
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::analog::AnalogReport;

    fn analog(median: f64, up: f64, n: usize, ci: Option<(f64, f64)>) -> AnalogReport {
        AnalogReport {
            query_date: "2026-06-18".to_string(),
            query_regime: "reflation".to_string(),
            target_asset: "BTC-USD".to_string(),
            horizon_days: 90,
            k: 25,
            k_effective: n,
            n_distinct_episodes: 25,
            n_with_forward: n,
            analogs: vec![],
            mean_distance: 3.0,
            median_forward_pct: Some(median),
            mean_forward_pct: Some(median),
            p25_forward_pct: Some(median - 5.0),
            p75_forward_pct: Some(median + 5.0),
            up_rate_pct: Some(up),
            mean_forward_ci_pct: ci,
            note: String::new(),
        }
    }

    #[test]
    fn strong_positive_analog_in_supportive_regime_is_bullish() {
        let a = analog(15.0, 75.0, 20, Some((3.0, 25.0)));
        let card = synthesize("BTC", "2026-06-18", &a, Quad::Reflation, Some((0.2, "accumulate".into())));
        assert_eq!(card.stance, Stance::Bull);
        assert!(card.confidence_pct > 0.0);
    }

    #[test]
    fn thin_analog_forces_low_confidence_and_humility_note() {
        let a = analog(15.0, 80.0, 4, None);
        let card = synthesize("BTC", "2026-06-18", &a, Quad::Goldilocks, None);
        assert!(card.honesty_note.contains("LOW CONFIDENCE"));
        // Quality discount should pull confidence down vs the robust case.
        assert!(card.confidence_pct < 60.0);
    }

    #[test]
    fn negative_analog_and_deflation_regime_is_bearish_for_crypto() {
        let a = analog(-12.0, 25.0, 20, Some((-20.0, -3.0)));
        let card = synthesize("BTC", "2026-06-18", &a, Quad::Deflation, None);
        assert_eq!(card.stance, Stance::Bear);
    }

    #[test]
    fn ci_straddling_zero_yields_moderate_note() {
        let a = analog(2.0, 55.0, 20, Some((-8.0, 12.0)));
        let card = synthesize("GC=F", "2026-06-18", &a, Quad::Inflation, None);
        assert!(card.honesty_note.contains("MODERATE"));
    }
}
