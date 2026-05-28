//! JSON schema for `pftui report chart outlook-arrows --from-json`:
//! `{ "days": ["flat", "medium"], "weeks": ["up", "medium"], "months": ["up_strong", "high"] }`.
//!
//! Each horizon also accepts object form:
//! `{ "direction": "bull", "conviction": "high" }`. `direction` accepts
//! `up`, `down`, `flat`, `bull`, `bear`, `neutral`, and slight/strong variants.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 380;
const DEFAULT_HEIGHT: u32 = 44;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OutlookPoint {
    pub direction: String,
    pub conviction: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OutlookPointPayload {
    Object {
        direction: String,
        conviction: String,
    },
    Pair(String, String),
}

impl<'de> Deserialize<'de> for OutlookPoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match OutlookPointPayload::deserialize(deserializer)? {
            OutlookPointPayload::Object {
                direction,
                conviction,
            } => Ok(Self {
                direction,
                conviction,
            }),
            OutlookPointPayload::Pair(direction, conviction) => Ok(Self {
                direction,
                conviction,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutlookArrowsInput {
    pub days: OutlookPoint,
    pub weeks: OutlookPoint,
    pub months: OutlookPoint,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl OutlookArrowsInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }
}

pub fn render_svg(input: &OutlookArrowsInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let height = input.height.unwrap_or(DEFAULT_HEIGHT);
    let horizons = [
        ("Days", &input.days),
        ("Weeks", &input.weeks),
        ("Months", &input.months),
    ];
    let col_w = f64::from(width) / 3.0;
    let mut parts = vec![format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
        width, height, FONT_SANS
    )];

    for (i, (label, value)) in horizons.iter().enumerate() {
        let cx = col_w * (i as f64 + 0.5);
        parts.push(format!(
            r#"<text x="{:.1}" y="11" text-anchor="middle" fill="{}" font-size="9" font-weight="600" letter-spacing="0.5">{}</text>"#,
            cx,
            palette::DARK.muted,
            label.to_ascii_uppercase()
        ));

        let (arrow, color) = direction_arrow_and_color(&value.direction);
        let (font_size, opacity) = conviction_style(&value.conviction);
        parts.push(format!(
            r#"<text x="{:.1}" y="{}" text-anchor="middle" fill="{}" opacity="{}" font-size="{}" font-weight="700" font-family="{}">{}</text>"#,
            cx,
            height - 6,
            color,
            opacity,
            font_size,
            FONT_MONO,
            arrow
        ));
        parts.push(format!(
            r#"<text x="{:.1}" y="{}" text-anchor="middle" fill="{}" font-size="7.5">{}</text>"#,
            cx,
            height - 1,
            palette::DARK.muted,
            escape_text(&value.conviction)
        ));
    }

    parts.push("</svg>".to_string());
    parts.join("")
}

pub fn render_ascii(input: &OutlookArrowsInput) -> String {
    [
        ("days", &input.days),
        ("weeks", &input.weeks),
        ("months", &input.months),
    ]
    .into_iter()
    .map(|(label, value)| format!("{label}: {} {}", value.direction, value.conviction))
    .collect::<Vec<_>>()
    .join("\n")
}

fn direction_arrow_and_color(direction: &str) -> (&'static str, &'static str) {
    match direction.trim().to_ascii_lowercase().as_str() {
        "up" | "bull" => ("↑", palette::DARK.bull),
        "down" | "bear" => ("↓", palette::DARK.bear),
        "up_strong" | "bull_strong" => ("⇈", palette::DARK.bull),
        "down_strong" | "bear_strong" => ("⇊", palette::DARK.bear),
        "up_slight" | "up_mild" => ("↗", palette::DARK.bull),
        "down_slight" | "down_mild" => ("↘", palette::DARK.bear),
        _ => ("→", palette::DARK.neutral),
    }
}

fn conviction_style(conviction: &str) -> (u32, &'static str) {
    match conviction.trim().to_ascii_lowercase().as_str() {
        "high" => (28, "1.0"),
        "medium" => (22, "0.85"),
        _ => (17, "0.65"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlook_arrows_matches_python_snapshot() {
        let rendered = render_svg(&OutlookArrowsInput {
            days: OutlookPoint {
                direction: "flat".to_string(),
                conviction: "medium".to_string(),
            },
            weeks: OutlookPoint {
                direction: "up".to_string(),
                conviction: "medium".to_string(),
            },
            months: OutlookPoint {
                direction: "up_strong".to_string(),
                conviction: "high".to_string(),
            },
            width: None,
            height: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/outlook_arrows.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn outlook_arrows_accepts_tuple_and_object_horizons() {
        let input = OutlookArrowsInput::from_value(serde_json::json!({
            "days": ["bear", "low"],
            "weeks": {"direction": "neutral", "conviction": "medium"},
            "months": ["bull_strong", "high"]
        }))
        .unwrap();

        assert_eq!(input.days.direction, "bear");
        assert_eq!(input.weeks.conviction, "medium");
        assert_eq!(input.months.direction, "bull_strong");
    }
}
