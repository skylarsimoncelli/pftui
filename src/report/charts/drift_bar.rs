//! JSON schema for `pftui report chart drift-bar --from-json`:
//! `{ "symbol": "BTC", "target_pct": 25.0, "actual_pct": 31.5, "band_pct": 2.0 }`.
//!
//! `band_pct`, `max_pct`, `width`, and `height` are optional. `actual_pct`
//! also accepts `current_pct`; `band_pct` also accepts `drift_band` and
//! `drift_band_pct` for compatibility with portfolio drift payloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_DRIFT_WIDTH: u32 = 400;
const DEFAULT_DRIFT_HEIGHT: u32 = 22;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriftBarInput {
    pub symbol: String,
    #[serde(alias = "target")]
    pub target_pct: f64,
    #[serde(alias = "actual", alias = "current_pct")]
    pub actual_pct: f64,
    #[serde(
        default = "default_band_pct",
        alias = "drift_band",
        alias = "drift_band_pct"
    )]
    pub band_pct: f64,
    #[serde(default)]
    pub max_pct: Option<f64>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

fn default_band_pct() -> f64 {
    2.0
}

impl DriftBarInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }
}

pub fn render_svg(input: &DriftBarInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_DRIFT_WIDTH);
    let height = input.height.unwrap_or(DEFAULT_DRIFT_HEIGHT);
    let max_pct = input.max_pct.unwrap_or_else(|| {
        30.0_f64
            .max(input.target_pct * 1.6)
            .max(input.actual_pct * 1.2)
    });
    let label_w = 80_i32;
    let status_w = 110_i32;
    let bar_w =
        i32::try_from(width).unwrap_or(DEFAULT_DRIFT_WIDTH as i32) - label_w - status_w - 16;
    let bar_x = label_w + 8;
    let bar_h = 12_i32;
    let bar_y = (f64::from(height) - f64::from(bar_h)) / 2.0;

    let actual_w = (input.actual_pct / max_pct) * f64::from(bar_w);
    let target_x = f64::from(bar_x) + (input.target_pct / max_pct) * f64::from(bar_w);

    let drift = input.actual_pct - input.target_pct;
    let in_band = drift.abs() <= input.band_pct;
    let (fill, status, status_color) = if in_band {
        (
            palette::DARK.bull,
            format!("✓ in band ({:+.2}pp)", drift),
            palette::DARK.bull,
        )
    } else {
        let fill = if drift.abs() < input.band_pct * 2.0 {
            palette::DARK.amber
        } else {
            palette::DARK.bear
        };
        let direction = if drift > 0.0 { "over" } else { "under" };
        (fill, format!("⚠ {} {:.2}pp", direction, drift.abs()), fill)
    };

    [
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
            width, height, FONT_SANS
        ),
        format!(
            r#"<text x="0" y="{:.1}" fill="{}" font-size="11" font-weight="600" font-family="{}">{}</text>"#,
            f64::from(height) / 2.0 + 4.0,
            palette::DARK.text,
            FONT_MONO,
            escape_text(&input.symbol)
        ),
        format!(
            r#"<rect x="{}" y="{:.1}" width="{}" height="{}" rx="2" ry="2" fill="{}" stroke="{}" stroke-width="0.5"/>"#,
            bar_x,
            bar_y,
            bar_w,
            bar_h,
            palette::DARK.panel,
            palette::DARK.border
        ),
        format!(
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{}" fill="{}" opacity="0.1"/>"#,
            f64::from(bar_x) + ((input.target_pct - input.band_pct) / max_pct) * f64::from(bar_w),
            bar_y,
            (input.band_pct * 2.0 / max_pct) * f64::from(bar_w),
            bar_h,
            palette::DARK.bull
        ),
        format!(
            r#"<rect x="{}" y="{:.1}" width="{:.1}" height="{}" rx="2" ry="2" fill="{}" opacity="0.7"/>"#,
            bar_x, bar_y, actual_w, bar_h, fill
        ),
        format!(
            r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="1.5"/>"#,
            target_x,
            bar_y - 3.0,
            target_x,
            bar_y + f64::from(bar_h) + 3.0,
            palette::DARK.text
        ),
        format!(
            r#"<text x="{:.1}" y="{:.1}" fill="{}" font-size="9.5" font-family="{}">{:.1}%</text>"#,
            f64::from(bar_x) + actual_w + 4.0,
            f64::from(height) / 2.0 + 4.0,
            palette::DARK.text,
            FONT_MONO,
            input.actual_pct
        ),
        format!(
            r#"<text x="{}" y="{:.1}" text-anchor="end" fill="{}" font-size="9" font-family="{}" font-weight="600">{}</text>"#,
            width,
            f64::from(height) / 2.0 + 4.0,
            status_color,
            FONT_MONO,
            escape_text(&status)
        ),
        "</svg>".to_string(),
    ]
    .join("")
}

pub fn render_ascii(input: &DriftBarInput) -> String {
    let drift = input.actual_pct - input.target_pct;
    let status = if drift.abs() <= input.band_pct {
        "in band".to_string()
    } else if drift > 0.0 {
        format!("over by {:.2}pp", drift.abs())
    } else {
        format!("under by {:.2}pp", drift.abs())
    };
    format!(
        "{}: actual {:.1}% target {:.1}% +/- {:.1}pp ({})",
        input.symbol, input.actual_pct, input.target_pct, input.band_pct, status
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_bar_matches_python_snapshot() {
        let rendered = render_svg(&DriftBarInput {
            symbol: "BTC".to_string(),
            target_pct: 25.0,
            actual_pct: 31.5,
            band_pct: 2.0,
            max_pct: None,
            width: None,
            height: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/drift_bar.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn drift_bar_accepts_portfolio_drift_aliases() {
        let input = DriftBarInput::from_value(serde_json::json!({
            "symbol": "GC=F",
            "target_pct": 26.0,
            "current_pct": 30.0,
            "drift_band": 4.0
        }))
        .unwrap();

        assert_eq!(input.actual_pct, 30.0);
        assert_eq!(input.band_pct, 4.0);
    }
}
