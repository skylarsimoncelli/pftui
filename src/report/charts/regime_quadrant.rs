//! JSON schema for `pftui report chart regime-quadrant --from-json`:
//! `{ "growth": -0.55, "inflation": 0.7, "trail": [[-0.2, 0.4], [-0.3, 0.5]] }`.
//!
//! `growth` and `inflation` use the Python helper's -1 to +1 axis scale.
//! `trail` is optional and should be ordered oldest first.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::theme::FONT_SANS;

const DEFAULT_WIDTH: u32 = 360;
const DEFAULT_HEIGHT: u32 = 300;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegimeTrailPoint(pub f64, pub f64);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegimeQuadrantInput {
    pub growth: f64,
    pub inflation: f64,
    #[serde(default)]
    pub trail: Vec<RegimeTrailPoint>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl RegimeQuadrantInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }
}

pub fn render_svg(input: &RegimeQuadrantInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let height = input.height.unwrap_or(DEFAULT_HEIGHT);
    let pad_l = 70_u32;
    let pad_r = 26_u32;
    let pad_t = 32_u32;
    let pad_b = 38_u32;
    let plot_w = width.saturating_sub(pad_l + pad_r);
    let plot_h = height.saturating_sub(pad_t + pad_b);
    let cx = f64::from(pad_l) + f64::from(plot_w) / 2.0;
    let cy = f64::from(pad_t) + f64::from(plot_h) / 2.0;

    let mut parts = vec![format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
        width, height, FONT_SANS
    )];
    parts.push(format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" stroke="{}" stroke-width="1"/>"#,
        pad_l,
        pad_t,
        plot_w,
        plot_h,
        palette::DARK.panel,
        palette::DARK.border
    ));
    parts.push(format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" opacity="0.08"/>"#,
        pad_l,
        pad_t,
        py_float(f64::from(plot_w) / 2.0),
        py_float(f64::from(plot_h) / 2.0),
        palette::DARK.bear
    ));
    parts.push(format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" opacity="0.08"/>"#,
        py_float(cx),
        pad_t,
        py_float(f64::from(plot_w) / 2.0),
        py_float(f64::from(plot_h) / 2.0),
        palette::DARK.amber
    ));
    parts.push(format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" opacity="0.08"/>"#,
        pad_l,
        py_float(cy),
        py_float(f64::from(plot_w) / 2.0),
        py_float(f64::from(plot_h) / 2.0),
        palette::DARK.mauve
    ));
    parts.push(format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" opacity="0.08"/>"#,
        py_float(cx),
        py_float(cy),
        py_float(f64::from(plot_w) / 2.0),
        py_float(f64::from(plot_h) / 2.0),
        palette::DARK.bull
    ));
    parts.push(format!(
        r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="0.6" stroke-dasharray="2 2"/>"#,
        py_float(cx),
        pad_t,
        py_float(cx),
        pad_t + plot_h,
        palette::DARK.border
    ));
    parts.push(format!(
        r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="0.6" stroke-dasharray="2 2"/>"#,
        pad_l,
        py_float(cy),
        pad_l + plot_w,
        py_float(cy),
        palette::DARK.border
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" fill="{}" font-size="9.5" font-weight="700" opacity="0.8">STAGFLATION</text>"#,
        pad_l + 8,
        pad_t + 14,
        palette::DARK.bear
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" text-anchor="end" fill="{}" font-size="9.5" font-weight="700" opacity="0.8">BOOM</text>"#,
        pad_l + plot_w - 8,
        pad_t + 14,
        palette::DARK.amber
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" fill="{}" font-size="9.5" font-weight="700" opacity="0.8">RECESSION</text>"#,
        pad_l + 8,
        pad_t + plot_h - 8,
        palette::DARK.mauve
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" text-anchor="end" fill="{}" font-size="9.5" font-weight="700" opacity="0.8">GOLDILOCKS</text>"#,
        pad_l + plot_w - 8,
        pad_t + plot_h - 8,
        palette::DARK.bull
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" text-anchor="middle" fill="{}" font-size="9" font-weight="600">GROWTH →</text>"#,
        py_float(cx),
        height - 18,
        palette::DARK.muted
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" text-anchor="middle" fill="{}" font-size="8">← contracting    expanding →</text>"#,
        py_float(cx),
        height - 6,
        palette::DARK.muted2
    ));
    parts.push(format!(
        r#"<text x="14" y="{}" transform="rotate(-90 14 {})" text-anchor="middle" fill="{}" font-size="9" font-weight="600">INFLATION →</text>"#,
        py_float(cy),
        py_float(cy),
        palette::DARK.muted
    ));
    parts.push(format!(
        r#"<text x="26" y="{}" transform="rotate(-90 26 {})" text-anchor="middle" fill="{}" font-size="8">← falling      rising →</text>"#,
        py_float(cy + 4.0),
        py_float(cy),
        palette::DARK.muted2
    ));

    let mut previous = None;
    let denom = input.trail.len().saturating_sub(1).max(1) as f64;
    for (idx, RegimeTrailPoint(growth, inflation)) in input.trail.iter().enumerate() {
        let tx = to_x(*growth, pad_l, plot_w);
        let ty = to_y(*inflation, pad_t, plot_h);
        let opacity = 0.15 + 0.5 * (idx as f64 / denom);
        if let Some((prev_x, prev_y)) = previous {
            parts.push(format!(
                r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="0.6" opacity="{:.2}"/>"#,
                prev_x,
                prev_y,
                tx,
                ty,
                palette::DARK.muted,
                opacity
            ));
        }
        parts.push(format!(
            r#"<circle cx="{:.1}" cy="{:.1}" r="2.5" fill="{}" opacity="{:.2}"/>"#,
            tx,
            ty,
            palette::DARK.muted,
            opacity
        ));
        previous = Some((tx, ty));
    }

    let px = to_x(input.growth, pad_l, plot_w);
    let py = to_y(input.inflation, pad_t, plot_h);
    parts.push(format!(
        r#"<circle cx="{:.1}" cy="{:.1}" r="11" fill="{}" opacity="0.18"/>"#,
        px,
        py,
        palette::DARK.cyan
    ));
    parts.push(format!(
        r#"<circle cx="{:.1}" cy="{:.1}" r="6" fill="{}" stroke="{}" stroke-width="1.5"/>"#,
        px,
        py,
        palette::DARK.cyan,
        palette::DARK.bg
    ));
    parts.push("</svg>".to_string());
    parts.join("")
}

pub fn render_ascii(input: &RegimeQuadrantInput) -> String {
    format!(
        "Regime quadrant: {} (growth {:+.2}, inflation {:+.2}, trail {} points)",
        quadrant_name(input.growth, input.inflation),
        input.growth,
        input.inflation,
        input.trail.len()
    )
}

fn to_x(growth: f64, pad_l: u32, plot_w: u32) -> f64 {
    f64::from(pad_l) + ((growth + 1.0) / 2.0) * f64::from(plot_w)
}

fn to_y(inflation: f64, pad_t: u32, plot_h: u32) -> f64 {
    f64::from(pad_t) + (1.0 - (inflation + 1.0) / 2.0) * f64::from(plot_h)
}

fn quadrant_name(growth: f64, inflation: f64) -> &'static str {
    match (growth >= 0.0, inflation >= 0.0) {
        (false, true) => "stagflation",
        (true, true) => "boom",
        (false, false) => "recession",
        (true, false) => "goldilocks",
    }
}

fn py_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regime_quadrant_matches_python_snapshot() {
        let rendered = render_svg(&RegimeQuadrantInput {
            growth: -0.55,
            inflation: 0.7,
            trail: vec![
                RegimeTrailPoint(-0.2, 0.4),
                RegimeTrailPoint(-0.3, 0.5),
                RegimeTrailPoint(-0.4, 0.55),
                RegimeTrailPoint(-0.5, 0.6),
                RegimeTrailPoint(-0.55, 0.7),
            ],
            width: None,
            height: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/regime_quadrant.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn regime_quadrant_accepts_minimal_json() {
        let input = RegimeQuadrantInput::from_value(serde_json::json!({
            "growth": 0.2,
            "inflation": -0.4
        }))
        .unwrap();

        assert_eq!(input.growth, 0.2);
        assert_eq!(input.inflation, -0.4);
        assert!(input.trail.is_empty());
        assert_eq!(
            render_ascii(&input),
            "Regime quadrant: goldilocks (growth +0.20, inflation -0.40, trail 0 points)"
        );
    }
}
