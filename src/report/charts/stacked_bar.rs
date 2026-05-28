use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{DEFAULT_CHART_WIDTH, FONT_MONO, FONT_SANS, STACKED_BAR_HEIGHT};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StackedBarSegment {
    pub label: String,
    pub value: f64,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StackedBarInput {
    pub segments: Vec<StackedBarSegment>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StackedBarPayload {
    Segments(Vec<StackedBarSegment>),
    Object(StackedBarInput),
}

impl StackedBarInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        match serde_json::from_value(value)? {
            StackedBarPayload::Segments(segments) => Ok(Self {
                segments,
                width: None,
                height: None,
            }),
            StackedBarPayload::Object(input) => Ok(input),
        }
    }
}

pub fn render_svg(input: &StackedBarInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_CHART_WIDTH);
    let height = input.height.unwrap_or(STACKED_BAR_HEIGHT);
    let total = input.segments.iter().map(|s| s.value).sum::<f64>();
    if total <= 0.0 {
        return String::new();
    }

    let label_h = 16_u32;
    let bar_h = height - label_h - 4;
    let bar_y = label_h + 4;
    let bar_w = f64::from(width - 2);
    let mut parts = Vec::new();

    parts.push(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
        width, height, FONT_SANS
    ));
    parts.push(format!(
        r#"<rect x="0" y="{}" width="{}" height="{}" rx="3" ry="3" fill="{}" stroke="{}" stroke-width="0.5"/>"#,
        bar_y,
        width - 2,
        bar_h,
        palette::DARK.panel,
        palette::DARK.border
    ));

    let mut x = 0.0;
    for seg in &input.segments {
        let seg_w = (seg.value / total) * bar_w;
        parts.push(format!(
            r#"<rect x="{:.1}" y="{}" width="{:.1}" height="{}" fill="{}" opacity="0.85"/>"#,
            x, bar_y, seg_w, bar_h, seg.color
        ));

        let label_x = x + seg_w / 2.0;
        if seg_w >= 50.0 {
            parts.push(format!(
                r#"<text x="{:.1}" y="12" text-anchor="middle" fill="{}" font-size="9.5" font-weight="600">{}</text>"#,
                label_x,
                palette::DARK.text,
                escape_text(&seg.label)
            ));
            parts.push(format!(
                r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" fill="{}" font-size="10" font-family="{}" font-weight="700">{:.1}%</text>"#,
                label_x,
                f64::from(bar_y) + f64::from(bar_h) / 2.0 + 4.0,
                palette::DARK.bg,
                FONT_MONO,
                seg.value
            ));
        }
        x += seg_w;
    }

    parts.push("</svg>".to_string());
    parts.join("")
}

pub fn render_ascii(input: &StackedBarInput) -> String {
    let total = input.segments.iter().map(|s| s.value).sum::<f64>();
    if total <= 0.0 {
        return "No allocation data".to_string();
    }
    input
        .segments
        .iter()
        .map(|segment| {
            let cells = ((segment.value / total) * 32.0).round().max(1.0) as usize;
            format!(
                "{:<12} {:>6.1}% {}",
                segment.label,
                segment.value,
                "#".repeat(cells)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> StackedBarInput {
        StackedBarInput {
            segments: vec![
                StackedBarSegment {
                    label: "USD".to_string(),
                    value: 48.91,
                    color: palette::DARK.cash.to_string(),
                },
                StackedBarSegment {
                    label: "BTC".to_string(),
                    value: 23.55,
                    color: palette::DARK.crypto.to_string(),
                },
                StackedBarSegment {
                    label: "Gold".to_string(),
                    value: 21.91,
                    color: palette::DARK.gold.to_string(),
                },
                StackedBarSegment {
                    label: "Silver".to_string(),
                    value: 5.62,
                    color: palette::DARK.silver.to_string(),
                },
            ],
            width: None,
            height: None,
        }
    }

    #[test]
    fn stacked_bar_matches_python_snapshot() {
        let rendered = render_svg(&sample_input());
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/stacked_bar.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn stacked_bar_escapes_labels() {
        let input = StackedBarInput {
            segments: vec![StackedBarSegment {
                label: "A & B <C>".to_string(),
                value: 100.0,
                color: "#fff".to_string(),
            }],
            width: None,
            height: None,
        };
        assert!(render_svg(&input).contains("A &amp; B &lt;C&gt;"));
    }
}
