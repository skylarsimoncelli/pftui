//! JSON schema for `pftui report chart conviction-trajectory --from-json`:
//! `{ "symbol": "Gold", "layer_series": { "LOW": [["d1", 4], ["d2", 3]], "MED": [["d1", 2]] } }`.
//!
//! `layer_series` may also be an ordered array:
//! `{ "symbol": "Gold", "layer_series": [{ "layer": "LOW", "series": [["d1", 4]] }] }`.
//! Conviction values use the analyst-view -5 to +5 scale.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 560;
const DEFAULT_HEIGHT: u32 = 84;
const CANONICAL_LAYERS: [&str; 4] = ["LOW", "MED", "HIGH", "MACRO"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConvictionTrajectoryPoint(pub String, pub i64);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConvictionLayerSeries {
    pub layer: String,
    #[serde(default, alias = "points")]
    pub series: Vec<ConvictionTrajectoryPoint>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConvictionTrajectoryInput {
    pub symbol: String,
    pub layer_series: Vec<ConvictionLayerSeries>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RawConvictionTrajectoryInput {
    symbol: String,
    #[serde(default, alias = "series", alias = "layers")]
    layer_series: Value,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

impl ConvictionTrajectoryInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        let raw: RawConvictionTrajectoryInput = serde_json::from_value(value)?;
        Ok(Self {
            symbol: raw.symbol,
            layer_series: parse_layer_series(raw.layer_series)?,
            width: raw.width,
            height: raw.height,
        })
    }
}

pub fn render_svg(input: &ConvictionTrajectoryInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let height = input.height.unwrap_or(DEFAULT_HEIGHT);
    let pad_l = 64_u32;
    let pad_r = 70_u32;
    let pad_t = 18_u32;
    let pad_b = 18_u32;
    let plot_w = width.saturating_sub(pad_l + pad_r);
    let plot_h = height.saturating_sub(pad_t + pad_b);
    let mid_y = f64::from(pad_t) + f64::from(plot_h) / 2.0;
    let legend_y = pad_t - 2;
    let legend_x = pad_l + plot_w + 6;

    let mut parts = vec![format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
        width, height, FONT_SANS
    )];
    parts.push(format!(
        r#"<text x="0" y="{:.1}" fill="{}" font-size="11.5" font-weight="600" font-family="{}">{}</text>"#,
        mid_y + 4.0,
        palette::DARK.text,
        FONT_MONO,
        escape_text(&input.symbol)
    ));
    parts.push(format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" stroke="{}" stroke-width="0.5"/>"#,
        pad_l,
        pad_t,
        plot_w,
        plot_h,
        palette::DARK.panel,
        palette::DARK.border
    ));
    parts.push(format!(
        r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="0.5" stroke-dasharray="2 2"/>"#,
        pad_l,
        py_float(mid_y),
        pad_l + plot_w,
        py_float(mid_y),
        palette::DARK.muted2
    ));
    parts.push(format!(
        r#"<line x1="{}" y1="{:.1}" x2="{}" y2="{:.1}" stroke="{}" stroke-width="0.3" opacity="0.5"/>"#,
        pad_l,
        f64::from(pad_t) + f64::from(plot_h) * 0.2,
        pad_l + plot_w,
        f64::from(pad_t) + f64::from(plot_h) * 0.2,
        palette::DARK.muted2
    ));
    parts.push(format!(
        r#"<line x1="{}" y1="{:.1}" x2="{}" y2="{:.1}" stroke="{}" stroke-width="0.3" opacity="0.5"/>"#,
        pad_l,
        f64::from(pad_t) + f64::from(plot_h) * 0.8,
        pad_l + plot_w,
        f64::from(pad_t) + f64::from(plot_h) * 0.8,
        palette::DARK.muted2
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" text-anchor="end" fill="{}" font-size="7.5">+5</text>"#,
        pad_l - 6,
        pad_t + 4,
        palette::DARK.muted2
    ));
    parts.push(format!(
        r#"<text x="{}" y="{:.1}" text-anchor="end" fill="{}" font-size="7.5">0</text>"#,
        pad_l - 6,
        mid_y + 3.0,
        palette::DARK.muted2
    ));
    parts.push(format!(
        r#"<text x="{}" y="{}" text-anchor="end" fill="{}" font-size="7.5">-5</text>"#,
        pad_l - 6,
        pad_t + plot_h + 2,
        palette::DARK.muted2
    ));

    for (i, layer_series) in input.layer_series.iter().enumerate() {
        if layer_series.series.is_empty() {
            continue;
        }
        let color = layer_color(&layer_series.layer);
        let n = layer_series.series.len();
        if n == 1 {
            let conviction = layer_series.series[0].1;
            let x = f64::from(pad_l) + f64::from(plot_w) * 0.5;
            let y = conviction_y(conviction, pad_t, plot_h);
            parts.push(format!(
                r#"<circle cx="{:.1}" cy="{:.1}" r="2" fill="{}"/>"#,
                x, y, color
            ));
        } else {
            let points = layer_series
                .series
                .iter()
                .enumerate()
                .map(|(j, point)| {
                    let x = f64::from(pad_l) + (f64::from(plot_w) * j as f64 / (n - 1) as f64);
                    let y = conviction_y(point.1, pad_t, plot_h);
                    format!("{x:.1},{y:.1}")
                })
                .collect::<Vec<_>>()
                .join(" ");
            parts.push(format!(
                r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="1.4" stroke-linejoin="round"/>"#,
                points, color
            ));
            let last_y = conviction_y(layer_series.series[n - 1].1, pad_t, plot_h);
            parts.push(format!(
                r#"<circle cx="{:.1}" cy="{:.1}" r="2.5" fill="{}" stroke="{}" stroke-width="1"/>"#,
                f64::from(pad_l + plot_w),
                last_y,
                color,
                palette::DARK.bg
            ));
        }

        let ly = legend_y + i as u32 * 13;
        parts.push(format!(
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1.5"/>"#,
            legend_x,
            ly + 4,
            legend_x + 14,
            ly + 4,
            color
        ));
        let last_score = layer_series.series.last().map(|point| point.1);
        if let Some(last_score) = last_score {
            parts.push(format!(
                r#"<text x="{}" y="{}" fill="{}" font-size="8.5" font-family="{}">{}: {:+}</text>"#,
                legend_x + 18,
                ly + 7,
                palette::DARK.text,
                FONT_MONO,
                escape_text(&layer_series.layer),
                last_score
            ));
        } else {
            parts.push(format!(
                r#"<text x="{}" y="{}" fill="{}" font-size="8.5" font-family="{}">{}: —</text>"#,
                legend_x + 18,
                ly + 7,
                palette::DARK.muted,
                FONT_MONO,
                escape_text(&layer_series.layer)
            ));
        }
    }

    parts.push("</svg>".to_string());
    parts.join("")
}

pub fn render_ascii(input: &ConvictionTrajectoryInput) -> String {
    let scores = input
        .layer_series
        .iter()
        .filter_map(|layer| {
            layer
                .series
                .last()
                .map(|point| format!("{}={:+}", layer.layer, point.1))
        })
        .collect::<Vec<_>>();
    if scores.is_empty() {
        return format!("{} conviction trajectory: no series", input.symbol);
    }
    format!(
        "{} conviction trajectory: {}",
        input.symbol,
        scores.join(" ")
    )
}

fn parse_layer_series(value: Value) -> serde_json::Result<Vec<ConvictionLayerSeries>> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => serde_json::from_value(Value::Array(items)),
        Value::Object(map) => parse_layer_series_map(map),
        other => Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("layer_series must be an object or array, got {other}"),
        ))),
    }
}

fn parse_layer_series_map(
    map: Map<String, Value>,
) -> serde_json::Result<Vec<ConvictionLayerSeries>> {
    let mut used = HashSet::new();
    let mut layers = Vec::new();
    for canonical in CANONICAL_LAYERS {
        if let Some((key, value)) = map
            .iter()
            .find(|(key, _)| key.trim().eq_ignore_ascii_case(canonical))
        {
            layers.push(ConvictionLayerSeries {
                layer: canonical.to_string(),
                series: serde_json::from_value(value.clone())?,
            });
            used.insert(key.clone());
        }
    }

    for (key, value) in map {
        if used.contains(&key) {
            continue;
        }
        layers.push(ConvictionLayerSeries {
            layer: key,
            series: serde_json::from_value(value)?,
        });
    }
    Ok(layers)
}

fn conviction_y(conviction: i64, pad_t: u32, plot_h: u32) -> f64 {
    f64::from(pad_t) + f64::from(plot_h) * (1.0 - (conviction as f64 + 5.0) / 10.0)
}

fn layer_color(layer: &str) -> &'static str {
    match layer.trim().to_ascii_uppercase().as_str() {
        "LOW" => palette::DARK.blue,
        "MED" | "MEDIUM" => palette::DARK.cyan,
        "HIGH" => palette::DARK.mauve,
        "MACRO" => palette::DARK.yellow,
        _ => palette::DARK.neutral,
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
    fn conviction_trajectory_matches_python_snapshot() {
        let rendered = render_svg(&ConvictionTrajectoryInput {
            symbol: "Gold".to_string(),
            layer_series: vec![
                series("LOW", [4, 4, 4, 4, 3, 3, 3]),
                series("MED", [4, 4, 4, 3, 3, 3, 3]),
                series("HIGH", [4, 4, 4, 4, 4, 4, 4]),
                series("MACRO", [3, 3, 3, 3, 3, 3, 3]),
            ],
            width: None,
            height: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/conviction_trajectory.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn conviction_trajectory_accepts_layer_map_and_alias() {
        let input = ConvictionTrajectoryInput::from_value(serde_json::json!({
            "symbol": "BTC",
            "series": {
                "macro": [["d1", 1]],
                "LOW": [["d1", 2], ["d2", 3]]
            }
        }))
        .unwrap();

        assert_eq!(input.symbol, "BTC");
        assert_eq!(input.layer_series[0].layer, "LOW");
        assert_eq!(input.layer_series[0].series[1].1, 3);
        assert_eq!(input.layer_series[1].layer, "MACRO");
        assert_eq!(
            render_ascii(&input),
            "BTC conviction trajectory: LOW=+3 MACRO=+1"
        );
    }

    fn series<const N: usize>(layer: &str, values: [i64; N]) -> ConvictionLayerSeries {
        ConvictionLayerSeries {
            layer: layer.to_string(),
            series: values
                .into_iter()
                .enumerate()
                .map(|(idx, conviction)| {
                    ConvictionTrajectoryPoint(format!("d{}", idx + 1), conviction)
                })
                .collect(),
        }
    }
}
