//! JSON schema for `pftui report chart conviction-grid --from-json`:
//! `{ "rows": [{ "symbol": "BTC", "low": 1, "medium": 1, "high": 3, "macro": 2, "summary": "convergent" }] }`.
//!
//! A bare row array is also accepted. Convictions are integer scores from -5
//! to +5; missing layer fields render as no-view cells.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 580;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConvictionGridRow {
    pub symbol: String,
    #[serde(default)]
    pub low: Option<i64>,
    #[serde(default, alias = "med")]
    pub medium: Option<i64>,
    #[serde(default)]
    pub high: Option<i64>,
    #[serde(default, rename = "macro", alias = "macro_view")]
    pub macro_score: Option<i64>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConvictionGridInput {
    pub rows: Vec<ConvictionGridRow>,
    #[serde(default)]
    pub width: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ConvictionGridPayload {
    Rows(Vec<ConvictionGridRow>),
    Object(ConvictionGridInput),
}

impl ConvictionGridInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        match serde_json::from_value(value)? {
            ConvictionGridPayload::Rows(rows) => Ok(Self { rows, width: None }),
            ConvictionGridPayload::Object(input) => Ok(input),
        }
    }
}

pub fn render_svg(input: &ConvictionGridInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let layers = ["LOW", "MED", "HIGH", "MACRO"];
    let sym_w = 80_u32;
    let cell_w = 84_u32;
    let row_h = 28_u32;
    let header_h = 22_u32;
    let total_h = header_h + row_h * input.rows.len() as u32 + 4;
    let summary_x = sym_w + cell_w * layers.len() as u32 + 8;

    let mut parts = vec![format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
        width, total_h, FONT_SANS
    )];

    for (i, layer) in layers.iter().enumerate() {
        let cx = f64::from(sym_w) + f64::from(cell_w) * (i as f64 + 0.5);
        parts.push(format!(
            r#"<text x="{:.1}" y="14" text-anchor="middle" fill="{}" font-size="9.5" font-weight="700" letter-spacing="0.5">{}</text>"#,
            cx,
            palette::DARK.muted,
            layer
        ));
    }
    parts.push(format!(
        r#"<text x="{}" y="14" fill="{}" font-size="9.5" font-weight="700" letter-spacing="0.5">CONVERGENCE</text>"#,
        summary_x,
        palette::DARK.muted
    ));

    for (r, row) in input.rows.iter().enumerate() {
        let y = f64::from(header_h) + r as f64 * f64::from(row_h) + f64::from(row_h) / 2.0;
        parts.push(format!(
            r#"<text x="0" y="{:.1}" fill="{}" font-size="11" font-weight="600" font-family="{}">{}</text>"#,
            y + 4.0,
            palette::DARK.text,
            FONT_MONO,
            escape_text(&row.symbol)
        ));

        let scores = [row.low, row.medium, row.high, row.macro_score];
        let nonnull = scores.into_iter().flatten().collect::<Vec<_>>();
        for (i, score) in scores.iter().enumerate() {
            let cx = f64::from(sym_w) + f64::from(cell_w) * (i as f64 + 0.5);
            let cy = y - 8.0;
            let cell_h = 18_u32;
            let cell_x = sym_w + cell_w * i as u32 + 6;
            let inner_w = cell_w - 12;
            let mid_x = f64::from(cell_x) + f64::from(inner_w) / 2.0;

            parts.push(format!(
                r#"<rect x="{}" y="{:.1}" width="{}" height="{}" rx="2" ry="2" fill="{}" stroke="{}" stroke-width="0.5"/>"#,
                cell_x,
                cy,
                inner_w,
                cell_h,
                palette::DARK.panel,
                palette::DARK.border
            ));
            let Some(score) = score else {
                parts.push(format!(
                    r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" fill="{}" font-size="9" font-family="{}">—</text>"#,
                    cx,
                    y + 4.0,
                    palette::DARK.muted2,
                    FONT_MONO
                ));
                continue;
            };

            let magnitude = score.abs() as f64 / 5.0;
            let fill_w = magnitude * f64::from(inner_w) / 2.0;
            let color = if *score > 0 {
                palette::DARK.bull
            } else if *score < 0 {
                palette::DARK.bear
            } else {
                palette::DARK.neutral
            };

            if *score > 0 {
                parts.push(format!(
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{}" rx="0" ry="0" fill="{}" opacity="0.55"/>"#,
                    mid_x, cy, fill_w, cell_h, color
                ));
            } else if *score < 0 {
                parts.push(format!(
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{}" rx="0" ry="0" fill="{}" opacity="0.55"/>"#,
                    mid_x - fill_w,
                    cy,
                    fill_w,
                    cell_h,
                    color
                ));
            }
            parts.push(format!(
                r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="0.5"/>"#,
                mid_x,
                cy,
                mid_x,
                cy + f64::from(cell_h),
                palette::DARK.border
            ));
            parts.push(format!(
                r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" fill="{}" font-size="10" font-family="{}" font-weight="700">{:+}</text>"#,
                cx,
                y + 4.0,
                palette::DARK.text,
                FONT_MONO,
                score
            ));
        }

        let (summary, summary_color) = summary_and_color(row.summary.as_deref(), &nonnull);
        parts.push(format!(
            r#"<text x="{}" y="{:.1}" fill="{}" font-size="10" font-style="italic">{}</text>"#,
            summary_x,
            y + 4.0,
            summary_color,
            escape_text(&summary)
        ));
    }

    parts.push("</svg>".to_string());
    parts.join("")
}

pub fn render_ascii(input: &ConvictionGridInput) -> String {
    if input.rows.is_empty() {
        return "No conviction rows".to_string();
    }
    input
        .rows
        .iter()
        .map(|row| {
            let scores = [row.low, row.medium, row.high, row.macro_score];
            let nonnull = scores.into_iter().flatten().collect::<Vec<_>>();
            let (summary, _) = summary_and_color(row.summary.as_deref(), &nonnull);
            format!(
                "{} low={} medium={} high={} macro={} {}",
                row.symbol,
                score_text(row.low),
                score_text(row.medium),
                score_text(row.high),
                score_text(row.macro_score),
                summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn score_text(score: Option<i64>) -> String {
    score.map_or_else(|| "-".to_string(), |score| format!("{score:+}"))
}

fn summary_and_color(override_summary: Option<&str>, scores: &[i64]) -> (String, &'static str) {
    let (summary, color) = if scores.is_empty() {
        ("no view".to_string(), palette::DARK.muted)
    } else {
        let avg = scores.iter().sum::<i64>() as f64 / scores.len() as f64;
        let min = scores.iter().min().copied().unwrap_or(0);
        let max = scores.iter().max().copied().unwrap_or(0);
        let spread = max - min;
        if spread <= 1 && avg.abs() >= 2.0 {
            (
                "strong convergence".to_string(),
                if avg > 0.0 {
                    palette::DARK.bull
                } else {
                    palette::DARK.bear
                },
            )
        } else if spread <= 2 {
            (
                "convergent".to_string(),
                if avg > 0.0 {
                    palette::DARK.bull
                } else if avg < 0.0 {
                    palette::DARK.bear
                } else {
                    palette::DARK.neutral
                },
            )
        } else {
            ("divergent".to_string(), palette::DARK.amber)
        }
    };
    (
        override_summary.map(str::to_string).unwrap_or(summary),
        color,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conviction_grid_matches_python_snapshot() {
        let rendered = render_svg(&ConvictionGridInput {
            rows: vec![
                ConvictionGridRow {
                    symbol: "BTC".to_string(),
                    low: Some(1),
                    medium: Some(1),
                    high: Some(3),
                    macro_score: Some(2),
                    summary: None,
                },
                ConvictionGridRow {
                    symbol: "Gold".to_string(),
                    low: Some(3),
                    medium: Some(3),
                    high: Some(4),
                    macro_score: Some(3),
                    summary: None,
                },
                ConvictionGridRow {
                    symbol: "Silver".to_string(),
                    low: None,
                    medium: Some(3),
                    high: Some(3),
                    macro_score: None,
                    summary: None,
                },
            ],
            width: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/conviction_grid.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn conviction_grid_accepts_bare_array_and_macro_alias() {
        let input = ConvictionGridInput::from_value(serde_json::json!([
            {"symbol": "BTC", "low": -2, "med": 1, "high": 4, "macro_view": 3}
        ]))
        .unwrap();

        assert_eq!(input.rows.len(), 1);
        assert_eq!(input.rows[0].medium, Some(1));
        assert_eq!(input.rows[0].macro_score, Some(3));
    }
}
