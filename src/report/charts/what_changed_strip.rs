//! JSON schema for `pftui report chart what-changed-strip --from-json`:
//! `{ "deltas": [{ "label": "BTC", "delta_str": "+3.2%", "direction": "bull" }] }`.
//!
//! A bare delta array is also accepted. `direction` is optional and accepts
//! `bull`, `bear`, `neutral`, or `info`; unknown values render as neutral.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::FONT_SANS;

const DEFAULT_WIDTH: u32 = 580;
const DEFAULT_HEIGHT: u32 = 44;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WhatChangedDelta {
    pub label: String,
    #[serde(alias = "delta")]
    pub delta_str: String,
    #[serde(default = "default_direction")]
    pub direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WhatChangedStripInput {
    pub deltas: Vec<WhatChangedDelta>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WhatChangedPayload {
    Deltas(Vec<WhatChangedDelta>),
    Object(WhatChangedStripInput),
}

fn default_direction() -> String {
    "info".to_string()
}

impl WhatChangedStripInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        match serde_json::from_value(value)? {
            WhatChangedPayload::Deltas(deltas) => Ok(Self {
                deltas,
                width: None,
                height: None,
            }),
            WhatChangedPayload::Object(input) => Ok(input),
        }
    }
}

pub fn render_svg(input: &WhatChangedStripInput) -> String {
    if input.deltas.is_empty() {
        return String::new();
    }

    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let height = input.height.unwrap_or(DEFAULT_HEIGHT);
    let pill_h = 22_i32;
    let pill_pad = 10.0_f64;
    let py = 18.0_f64;
    let line_h = 22.0_f64;
    let mut line = 0.0_f64;
    let mut line_x = 0.0_f64;

    let mut parts = vec![
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
            width, height, FONT_SANS
        ),
        format!(
            r#"<text x="0" y="14" fill="{}" font-size="8.5" font-weight="700" letter-spacing="1">SINCE LAST REPORT</text>"#,
            palette::DARK.muted
        ),
    ];

    for delta in &input.deltas {
        let direction = delta.direction.trim().to_ascii_lowercase();
        let color = match direction.as_str() {
            "bull" => palette::DARK.bull,
            "bear" => palette::DARK.bear,
            "neutral" => palette::DARK.neutral,
            "info" => palette::DARK.cyan,
            _ => palette::DARK.neutral,
        };
        let arrow = match direction.as_str() {
            "bull" => "▲",
            "bear" => "▼",
            "neutral" => "→",
            "info" => "•",
            _ => "•",
        };
        let label_text = format!("{} {} {}", arrow, delta.label, delta.delta_str);
        let pill_w = (label_text.chars().count() as f64 * 6.5 + pill_pad * 2.0).max(60.0);

        if line_x + pill_w > f64::from(width) {
            line += 1.0;
            line_x = 0.0;
        }

        let cx = line_x;
        let cy = py + line * line_h;
        parts.push(format!(
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{}" rx="11" ry="11" fill="{}" opacity="0.13" stroke="{}" stroke-width="0.5" stroke-opacity="0.4"/>"#,
            cx, cy, pill_w, pill_h, color, color
        ));
        parts.push(format!(
            r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" fill="{}" font-size="9.5" font-weight="600">{}</text>"#,
            cx + pill_w / 2.0,
            cy + 14.0,
            color,
            escape_text(&label_text)
        ));
        line_x += pill_w + 6.0;
    }

    parts.push("</svg>".to_string());
    parts.join("")
}

pub fn render_ascii(input: &WhatChangedStripInput) -> String {
    if input.deltas.is_empty() {
        return "No changes".to_string();
    }
    input
        .deltas
        .iter()
        .map(|delta| {
            let direction = delta.direction.trim().to_ascii_lowercase();
            let arrow = match direction.as_str() {
                "bull" => "up",
                "bear" => "down",
                "neutral" => "flat",
                "info" => "info",
                _ => "info",
            };
            format!("{}: {} {}", arrow, delta.label, delta.delta_str)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_changed_strip_matches_python_snapshot() {
        let rendered = render_svg(&WhatChangedStripInput {
            deltas: vec![
                WhatChangedDelta {
                    label: "BTC".to_string(),
                    delta_str: "+3.2%".to_string(),
                    direction: "bull".to_string(),
                },
                WhatChangedDelta {
                    label: "VIX".to_string(),
                    delta_str: "+5.1".to_string(),
                    direction: "bear".to_string(),
                },
                WhatChangedDelta {
                    label: "DXY".to_string(),
                    delta_str: "flat".to_string(),
                    direction: "neutral".to_string(),
                },
                WhatChangedDelta {
                    label: "Fed".to_string(),
                    delta_str: "new dots".to_string(),
                    direction: "info".to_string(),
                },
            ],
            width: None,
            height: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/what_changed_strip.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn what_changed_strip_accepts_bare_delta_array() {
        let input = WhatChangedStripInput::from_value(serde_json::json!([
            {"label": "Gold", "delta": "+1.1%", "direction": "bull"}
        ]))
        .unwrap();

        assert_eq!(input.deltas.len(), 1);
        assert_eq!(input.deltas[0].delta_str, "+1.1%");
    }
}
