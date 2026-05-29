//! JSON schema for `pftui report chart calibration-reliability --from-json`:
//! `{ "window_days": 90, "rows": [{ "layer": "low", "strict_hit_rate_pct": 50, "n": 12, "sigma_pp": 14.4, "low_sample": false, "bin_breakdown": [{ "band": "high", "strict_hit_rate_pct": 66.7, "n": 3, "sigma_pp": 27.2, "low_sample": true }] }] }`.
//!
//! The full JSON object from `pftui analytics calibration --by-layer --json` is
//! also accepted; the renderer reads its nested `by_layer` field.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 560;
const DEFAULT_HEIGHT: u32 = 360;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationReliabilityInput {
    #[serde(default)]
    pub window_days: Option<i64>,
    #[serde(default)]
    pub rows: Vec<CalibrationReliabilityLayer>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationReliabilityLayer {
    pub layer: String,
    #[serde(default)]
    pub strict_hit_rate: Option<f64>,
    #[serde(default)]
    pub strict_hit_rate_pct: Option<f64>,
    pub n: usize,
    #[serde(default)]
    pub sigma: Option<f64>,
    #[serde(default)]
    pub sigma_pp: Option<f64>,
    #[serde(default)]
    pub low_sample: bool,
    #[serde(default)]
    pub correct: usize,
    #[serde(default)]
    pub partial: usize,
    #[serde(default)]
    pub wrong: usize,
    #[serde(default)]
    pub bin_breakdown: Vec<CalibrationReliabilityBin>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationReliabilityBin {
    pub band: String,
    #[serde(default)]
    pub strict_hit_rate: Option<f64>,
    #[serde(default)]
    pub strict_hit_rate_pct: Option<f64>,
    pub n: usize,
    #[serde(default)]
    pub sigma: Option<f64>,
    #[serde(default)]
    pub sigma_pp: Option<f64>,
    #[serde(default)]
    pub low_sample: bool,
    #[serde(default)]
    pub correct: usize,
    #[serde(default)]
    pub partial: usize,
    #[serde(default)]
    pub wrong: usize,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CalibrationReliabilityPayload {
    Rows(Vec<CalibrationReliabilityLayer>),
    Analytics {
        by_layer: CalibrationReliabilityInput,
    },
    Direct(CalibrationReliabilityInput),
}

impl CalibrationReliabilityInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        match serde_json::from_value(value)? {
            CalibrationReliabilityPayload::Rows(rows) => Ok(Self {
                window_days: None,
                rows,
                width: None,
                height: None,
            }),
            CalibrationReliabilityPayload::Direct(input) => Ok(input),
            CalibrationReliabilityPayload::Analytics { by_layer } => Ok(by_layer),
        }
    }
}

pub fn render_svg(input: &CalibrationReliabilityInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let height = input.height.unwrap_or(DEFAULT_HEIGHT);
    let pad_l = 58_u32;
    let pad_r = 136_u32;
    let pad_t = 42_u32;
    let pad_b = 46_u32;
    let plot_w = width.saturating_sub(pad_l + pad_r);
    let plot_h = height.saturating_sub(pad_t + pad_b);

    let mut parts = vec![format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
        width, height, FONT_SANS
    )];
    parts.push(format!(
        r#"<rect width="{}" height="{}" fill="{}"/>"#,
        width,
        height,
        palette::DARK.bg
    ));
    parts.push(format!(
        r#"<text x="{}" y="20" fill="{}" font-size="13" font-weight="700">Calibration reliability</text>"#,
        pad_l, palette::DARK.text
    ));
    if let Some(window_days) = input.window_days {
        parts.push(format!(
            r#"<text x="{}" y="20" text-anchor="end" fill="{}" font-size="10">{}d window</text>"#,
            width - 12,
            palette::DARK.muted,
            window_days
        ));
    }
    parts.push(format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" stroke="{}" stroke-width="1"/>"#,
        pad_l,
        pad_t,
        plot_w,
        plot_h,
        palette::DARK.panel,
        palette::DARK.border
    ));

    for pct in [0.0, 25.0, 50.0, 75.0, 100.0] {
        let x = point_x(pct, pad_l, plot_w);
        let y = point_y(pct, pad_t, plot_h);
        parts.push(format!(
            r#"<line x1="{:.1}" y1="{}" x2="{:.1}" y2="{}" stroke="{}" stroke-width="0.5" opacity="0.45"/>"#,
            x,
            pad_t,
            x,
            pad_t + plot_h,
            palette::DARK.border
        ));
        parts.push(format!(
            r#"<line x1="{}" y1="{:.1}" x2="{}" y2="{:.1}" stroke="{}" stroke-width="0.5" opacity="0.45"/>"#,
            pad_l,
            y,
            pad_l + plot_w,
            y,
            palette::DARK.border
        ));
        parts.push(format!(
            r#"<text x="{:.1}" y="{}" text-anchor="middle" fill="{}" font-size="8" font-family="{}">{:.0}</text>"#,
            x,
            pad_t + plot_h + 14,
            palette::DARK.muted2,
            FONT_MONO,
            pct
        ));
        parts.push(format!(
            r#"<text x="{}" y="{:.1}" text-anchor="end" fill="{}" font-size="8" font-family="{}">{:.0}</text>"#,
            pad_l - 8,
            y + 3.0,
            palette::DARK.muted2,
            FONT_MONO,
            pct
        ));
    }

    parts.push(format!(
        r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1.4" stroke-dasharray="4 4"/>"#,
        pad_l,
        pad_t + plot_h,
        pad_l + plot_w,
        pad_t,
        palette::DARK.muted
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" text-anchor="middle" fill="{}" font-size="9" font-weight="600">expected confidence (%)</text>"#,
        pad_l + plot_w / 2,
        height - 12,
        palette::DARK.muted
    ));
    parts.push(format!(
        r#"<text x="16" y="{}" transform="rotate(-90 16 {})" text-anchor="middle" fill="{}" font-size="9" font-weight="600">strict hit rate (%)</text>"#,
        pad_t + plot_h / 2,
        pad_t + plot_h / 2,
        palette::DARK.muted
    ));

    for layer in &input.rows {
        let layer_color = layer_color(&layer.layer);
        if let Some(point) = layer_point(layer) {
            draw_point(
                &mut parts,
                point,
                PlotArea {
                    pad_l,
                    pad_t,
                    plot_w,
                    plot_h,
                },
                layer_color,
                true,
            );
        }
        for bin in &layer.bin_breakdown {
            let point = ReliabilityPoint {
                label: format!(
                    "{} {}",
                    layer_label(&layer.layer),
                    bin.band.to_ascii_uppercase()
                ),
                expected_pct: band_expected_pct(&bin.band),
                actual_pct: hit_rate_pct(bin.strict_hit_rate_pct, bin.strict_hit_rate),
                n: bin.n,
                sigma_pp: sigma_pp(bin.sigma_pp, bin.sigma),
                low_sample: bin.low_sample,
            };
            draw_point(
                &mut parts,
                point,
                PlotArea {
                    pad_l,
                    pad_t,
                    plot_w,
                    plot_h,
                },
                layer_color,
                false,
            );
        }
    }

    let legend_x = pad_l + plot_w + 18;
    let mut legend_y = pad_t + 12;
    for layer in ["low", "medium", "high", "macro"] {
        parts.push(format!(
            r#"<circle cx="{}" cy="{}" r="4" fill="{}"/>"#,
            legend_x,
            legend_y,
            layer_color(layer)
        ));
        parts.push(format!(
            r#"<text x="{}" y="{}" fill="{}" font-size="9" font-weight="600">{}</text>"#,
            legend_x + 10,
            legend_y + 3,
            palette::DARK.text,
            layer_label(layer)
        ));
        legend_y += 16;
    }
    parts.push(format!(
        r#"<circle cx="{}" cy="{}" r="5" fill="none" stroke="{}" stroke-width="1.4" stroke-dasharray="2 2"/>"#,
        legend_x,
        legend_y + 5,
        palette::DARK.muted
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" fill="{}" font-size="8.5">low sample</text>"#,
        legend_x + 10,
        legend_y + 8,
        palette::DARK.muted
    ));

    parts.push("</svg>".to_string());
    parts.join("")
}

pub fn render_ascii(input: &CalibrationReliabilityInput) -> String {
    if input.rows.is_empty() {
        return "Calibration reliability: no layer rows".to_string();
    }
    let mut lines = vec![format!(
        "Calibration reliability{}",
        input
            .window_days
            .map(|days| format!(" ({days}d)"))
            .unwrap_or_default()
    )];
    for layer in &input.rows {
        lines.push(format!(
            "{} aggregate: actual {:.1}% (n={}, sigma +/-{:.1}pp){}",
            layer_label(&layer.layer),
            hit_rate_pct(layer.strict_hit_rate_pct, layer.strict_hit_rate),
            layer.n,
            sigma_pp(layer.sigma_pp, layer.sigma),
            low_sample_suffix(layer.low_sample)
        ));
        for bin in &layer.bin_breakdown {
            lines.push(format!(
                "  {}: expected {:.0}%, actual {:.1}% (n={}, sigma +/-{:.1}pp){}",
                bin.band,
                band_expected_pct(&bin.band),
                hit_rate_pct(bin.strict_hit_rate_pct, bin.strict_hit_rate),
                bin.n,
                sigma_pp(bin.sigma_pp, bin.sigma),
                low_sample_suffix(bin.low_sample)
            ));
        }
    }
    lines.join("\n")
}

#[derive(Debug, Clone)]
struct ReliabilityPoint {
    label: String,
    expected_pct: f64,
    actual_pct: f64,
    n: usize,
    sigma_pp: f64,
    low_sample: bool,
}

#[derive(Debug, Clone, Copy)]
struct PlotArea {
    pad_l: u32,
    pad_t: u32,
    plot_w: u32,
    plot_h: u32,
}

fn draw_point(
    parts: &mut Vec<String>,
    point: ReliabilityPoint,
    plot: PlotArea,
    color: &'static str,
    aggregate: bool,
) {
    let x = point_x(point.expected_pct, plot.pad_l, plot.plot_w);
    let y = point_y(point.actual_pct, plot.pad_t, plot.plot_h);
    let radius = if aggregate { 6.0 } else { 4.2 };
    let fill = if point.low_sample { "none" } else { color };
    let opacity = if point.low_sample { "0.95" } else { "0.78" };
    let stroke_dash = if point.low_sample {
        r#" stroke-dasharray="2 2""#
    } else {
        ""
    };
    parts.push(format!(
        r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" stroke="{}" stroke-width="1.4" opacity="{}"{}><title>{}: expected {:.0}%, actual {:.1}%, n={}, sigma +/-{:.1}pp{}</title></circle>"#,
        x,
        y,
        radius,
        fill,
        color,
        opacity,
        stroke_dash,
        escape_text(&point.label),
        point.expected_pct,
        point.actual_pct,
        point.n,
        point.sigma_pp,
        if point.low_sample { ", low sample" } else { "" }
    ));
}

fn layer_point(layer: &CalibrationReliabilityLayer) -> Option<ReliabilityPoint> {
    if layer.n == 0 {
        return None;
    }
    let expected_pct = if layer.bin_breakdown.is_empty() {
        50.0
    } else {
        layer
            .bin_breakdown
            .iter()
            .map(|bin| band_expected_pct(&bin.band) * bin.n as f64)
            .sum::<f64>()
            / layer.bin_breakdown.iter().map(|bin| bin.n).sum::<usize>() as f64
    };
    Some(ReliabilityPoint {
        label: format!("{} aggregate", layer_label(&layer.layer)),
        expected_pct,
        actual_pct: hit_rate_pct(layer.strict_hit_rate_pct, layer.strict_hit_rate),
        n: layer.n,
        sigma_pp: sigma_pp(layer.sigma_pp, layer.sigma),
        low_sample: layer.low_sample,
    })
}

fn point_x(pct: f64, pad_l: u32, plot_w: u32) -> f64 {
    f64::from(pad_l) + pct.clamp(0.0, 100.0) / 100.0 * f64::from(plot_w)
}

fn point_y(pct: f64, pad_t: u32, plot_h: u32) -> f64 {
    f64::from(pad_t) + (1.0 - pct.clamp(0.0, 100.0) / 100.0) * f64::from(plot_h)
}

fn hit_rate_pct(strict_hit_rate_pct: Option<f64>, strict_hit_rate: Option<f64>) -> f64 {
    strict_hit_rate_pct.unwrap_or_else(|| strict_hit_rate.unwrap_or(0.0) * 100.0)
}

fn sigma_pp(sigma_pp: Option<f64>, sigma: Option<f64>) -> f64 {
    sigma_pp.unwrap_or_else(|| sigma.unwrap_or(0.0) * 100.0)
}

fn band_expected_pct(band: &str) -> f64 {
    match band.trim().to_ascii_lowercase().as_str() {
        "low" => 35.0,
        "medium" | "med" => 55.0,
        "high" => 75.0,
        _ => 50.0,
    }
}

fn layer_label(layer: &str) -> String {
    layer.trim().replace('-', " ").to_ascii_uppercase()
}

fn layer_color(layer: &str) -> &'static str {
    match layer.trim().to_ascii_lowercase().as_str() {
        "low" => palette::DARK.cyan,
        "medium" => palette::DARK.amber,
        "high" => palette::DARK.mauve,
        "macro" => palette::DARK.bull,
        _ => palette::DARK.muted,
    }
}

fn low_sample_suffix(low_sample: bool) -> &'static str {
    if low_sample {
        " [low sample]"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_reliability_matches_snapshot() {
        let rendered = render_svg(&sample_input());
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/calibration_reliability.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn calibration_reliability_accepts_analytics_payload() {
        let value = serde_json::json!({
            "total_mappings": 0,
            "by_layer": {
                "window_days": 30,
                "rows": [{
                    "layer": "low",
                    "strict_hit_rate_pct": 50.0,
                    "n": 2,
                    "sigma_pp": 35.36,
                    "low_sample": true,
                    "bin_breakdown": [{
                        "band": "high",
                        "strict_hit_rate_pct": 50.0,
                        "n": 2,
                        "sigma_pp": 35.36,
                        "low_sample": true
                    }]
                }]
            }
        });
        let input = CalibrationReliabilityInput::from_value(value).unwrap();
        assert_eq!(input.window_days, Some(30));
        assert_eq!(input.rows[0].bin_breakdown[0].band, "high");
    }

    #[test]
    fn calibration_reliability_ascii_marks_low_sample_bins() {
        let rendered = render_ascii(&sample_input());
        assert!(rendered.contains("Calibration reliability (90d)"));
        assert!(rendered.contains("sigma +/-"));
        assert!(rendered.contains("[low sample]"));
    }

    fn sample_input() -> CalibrationReliabilityInput {
        CalibrationReliabilityInput {
            window_days: Some(90),
            rows: vec![
                CalibrationReliabilityLayer {
                    layer: "low".to_string(),
                    strict_hit_rate: Some(0.5),
                    strict_hit_rate_pct: Some(50.0),
                    n: 12,
                    sigma: Some(0.1443),
                    sigma_pp: Some(14.43),
                    low_sample: false,
                    correct: 6,
                    partial: 2,
                    wrong: 4,
                    bin_breakdown: vec![
                        CalibrationReliabilityBin {
                            band: "low".to_string(),
                            strict_hit_rate: Some(0.4),
                            strict_hit_rate_pct: Some(40.0),
                            n: 5,
                            sigma: Some(0.2191),
                            sigma_pp: Some(21.91),
                            low_sample: true,
                            correct: 2,
                            partial: 1,
                            wrong: 2,
                        },
                        CalibrationReliabilityBin {
                            band: "high".to_string(),
                            strict_hit_rate: Some(0.6667),
                            strict_hit_rate_pct: Some(66.67),
                            n: 3,
                            sigma: Some(0.2722),
                            sigma_pp: Some(27.22),
                            low_sample: true,
                            correct: 2,
                            partial: 0,
                            wrong: 1,
                        },
                    ],
                },
                CalibrationReliabilityLayer {
                    layer: "medium".to_string(),
                    strict_hit_rate: Some(0.6),
                    strict_hit_rate_pct: Some(60.0),
                    n: 10,
                    sigma: Some(0.1549),
                    sigma_pp: Some(15.49),
                    low_sample: false,
                    correct: 6,
                    partial: 1,
                    wrong: 3,
                    bin_breakdown: vec![CalibrationReliabilityBin {
                        band: "medium".to_string(),
                        strict_hit_rate: Some(0.6),
                        strict_hit_rate_pct: Some(60.0),
                        n: 10,
                        sigma: Some(0.1549),
                        sigma_pp: Some(15.49),
                        low_sample: false,
                        correct: 6,
                        partial: 1,
                        wrong: 3,
                    }],
                },
                CalibrationReliabilityLayer {
                    layer: "high".to_string(),
                    strict_hit_rate: Some(0.75),
                    strict_hit_rate_pct: Some(75.0),
                    n: 8,
                    sigma: Some(0.1531),
                    sigma_pp: Some(15.31),
                    low_sample: true,
                    correct: 6,
                    partial: 0,
                    wrong: 2,
                    bin_breakdown: vec![CalibrationReliabilityBin {
                        band: "high".to_string(),
                        strict_hit_rate: Some(0.75),
                        strict_hit_rate_pct: Some(75.0),
                        n: 8,
                        sigma: Some(0.1531),
                        sigma_pp: Some(15.31),
                        low_sample: true,
                        correct: 6,
                        partial: 0,
                        wrong: 2,
                    }],
                },
                CalibrationReliabilityLayer {
                    layer: "macro".to_string(),
                    strict_hit_rate: Some(0.25),
                    strict_hit_rate_pct: Some(25.0),
                    n: 4,
                    sigma: Some(0.2165),
                    sigma_pp: Some(21.65),
                    low_sample: true,
                    correct: 1,
                    partial: 1,
                    wrong: 2,
                    bin_breakdown: vec![CalibrationReliabilityBin {
                        band: "low".to_string(),
                        strict_hit_rate: Some(0.25),
                        strict_hit_rate_pct: Some(25.0),
                        n: 4,
                        sigma: Some(0.2165),
                        sigma_pp: Some(21.65),
                        low_sample: true,
                        correct: 1,
                        partial: 1,
                        wrong: 2,
                    }],
                },
            ],
            width: None,
            height: None,
        }
    }
}
