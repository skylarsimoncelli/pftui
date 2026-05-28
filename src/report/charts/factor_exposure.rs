//! JSON schema for `pftui report chart factor-exposure --from-json`:
//! `{ "factors": [{ "name": "Inflation Spike", "exposure_pct": 51.0, "direction": "bull", "prob_pct": 88.0 }] }`.
//!
//! A bare factor array is also accepted. `direction` is optional and accepts
//! `bull`, `bear`, `neutral`, or `mixed`; unknown values render as mixed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 580;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactorExposure {
    pub name: String,
    pub exposure_pct: f64,
    #[serde(default = "default_direction")]
    pub direction: String,
    #[serde(default)]
    pub prob_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactorExposureInput {
    pub factors: Vec<FactorExposure>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FactorExposurePayload {
    Factors(Vec<FactorExposure>),
    Object(FactorExposureInput),
}

fn default_direction() -> String {
    "mixed".to_string()
}

impl FactorExposureInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        match serde_json::from_value(value)? {
            FactorExposurePayload::Factors(factors) => Ok(Self {
                factors,
                width: None,
                height: None,
            }),
            FactorExposurePayload::Object(input) => Ok(input),
        }
    }
}

pub fn render_svg(input: &FactorExposureInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let label_w = 200_u32;
    let bar_w = width.saturating_sub(label_w + 90);
    let bar_x = label_w + 8;
    let bar_h = 16_u32;
    let spacing = 8_u32;
    let height = input.factors.len() as u32 * (bar_h + spacing) + 12;
    let mut parts = vec![format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
        width, height, FONT_SANS
    )];

    for (i, factor) in input.factors.iter().enumerate() {
        let y = i as u32 * (bar_h + spacing) + 4;
        let color = direction_color(&factor.direction);
        parts.push(format!(
            r#"<text x="0" y="{:.1}" fill="{}" font-size="10.5" font-weight="500">{}</text>"#,
            f64::from(y + bar_h - 4),
            palette::DARK.text,
            escape_text(&factor.name)
        ));
        parts.push(format!(
            r#"<rect x="{}" y="{}" width="{}" height="{}" rx="2" ry="2" fill="{}" stroke="{}" stroke-width="0.5"/>"#,
            bar_x,
            y,
            bar_w,
            bar_h,
            palette::DARK.panel,
            palette::DARK.border
        ));
        let fill_w = factor.exposure_pct / 100.0 * f64::from(bar_w);
        parts.push(format!(
            r#"<rect x="{}" y="{}" width="{:.1}" height="{}" rx="2" ry="2" fill="{}" opacity="0.75"/>"#,
            bar_x, y, fill_w, bar_h, color
        ));
        if let Some(prob_pct) = factor.prob_pct {
            let tick_x = f64::from(bar_x) + prob_pct / 100.0 * f64::from(bar_w);
            parts.push(format!(
                r#"<line x1="{:.1}" y1="{}" x2="{:.1}" y2="{}" stroke="{}" stroke-width="1.2" stroke-dasharray="2 1"/>"#,
                tick_x,
                y - 2,
                tick_x,
                y + bar_h + 2,
                palette::DARK.text
            ));
        }
        parts.push(format!(
            r#"<text x="{}" y="{:.1}" text-anchor="end" fill="{}" font-size="10" font-family="{}" font-weight="600">{:.1}% exposed</text>"#,
            width,
            f64::from(y + bar_h - 4),
            color,
            FONT_MONO,
            factor.exposure_pct
        ));
    }

    parts.push("</svg>".to_string());
    parts.join("")
}

pub fn render_ascii(input: &FactorExposureInput) -> String {
    if input.factors.is_empty() {
        return "No factor exposure".to_string();
    }
    input
        .factors
        .iter()
        .map(|factor| {
            let direction = factor.direction.trim().to_ascii_lowercase();
            match factor.prob_pct {
                Some(prob_pct) => format!(
                    "{}: {:.1}% exposed ({}, prob {:.1}%)",
                    factor.name, factor.exposure_pct, direction, prob_pct
                ),
                None => format!(
                    "{}: {:.1}% exposed ({})",
                    factor.name, factor.exposure_pct, direction
                ),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn direction_color(direction: &str) -> &'static str {
    match direction.trim().to_ascii_lowercase().as_str() {
        "bull" => palette::DARK.bull,
        "bear" => palette::DARK.bear,
        "neutral" => palette::DARK.neutral,
        _ => palette::DARK.amber,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factor_exposure_matches_python_snapshot() {
        let rendered = render_svg(&FactorExposureInput {
            factors: vec![
                FactorExposure {
                    name: "Inflation Spike (bull)".to_string(),
                    exposure_pct: 51.0,
                    direction: "bull".to_string(),
                    prob_pct: Some(88.0),
                },
                FactorExposure {
                    name: "Hard Recession (bear)".to_string(),
                    exposure_pct: 27.0,
                    direction: "bear".to_string(),
                    prob_pct: Some(32.0),
                },
                FactorExposure {
                    name: "Iran-US War (bull/bear)".to_string(),
                    exposure_pct: 22.0,
                    direction: "mixed".to_string(),
                    prob_pct: Some(12.0),
                },
                FactorExposure {
                    name: "Risk-On Rally (mixed)".to_string(),
                    exposure_pct: 12.0,
                    direction: "neutral".to_string(),
                    prob_pct: Some(10.0),
                },
            ],
            width: None,
            height: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/factor_exposure.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn factor_exposure_accepts_bare_array_with_default_direction() {
        let input = FactorExposureInput::from_value(serde_json::json!([
            {"name": "Liquidity Shock", "exposure_pct": 34.5}
        ]))
        .unwrap();

        assert_eq!(input.factors.len(), 1);
        assert_eq!(input.factors[0].direction, "mixed");
    }
}
