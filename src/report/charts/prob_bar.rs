use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{DEFAULT_CHART_WIDTH, FONT_MONO, FONT_SANS, PROB_BAR_HEIGHT};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbBarInput {
    pub name: String,
    pub current: f64,
    pub prior_7d: f64,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub max_pct: Option<f64>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

fn default_color() -> String {
    "cyan".to_string()
}

impl ProbBarInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }
}

pub fn render_svg(input: &ProbBarInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_CHART_WIDTH);
    let height = input.height.unwrap_or(PROB_BAR_HEIGHT);
    let max_pct = input.max_pct.unwrap_or(100.0);
    let color_hex = palette::color_or_raw(&input.color);
    let label_w = 170_i32;
    let delta_w = 70_i32;
    let bar_w = i32::try_from(width).unwrap_or(580) - label_w - delta_w - 12;
    let bar_x = label_w + 6;
    let bar_h = 14_i32;
    let bar_y = (f64::from(height) - f64::from(bar_h)) / 2.0;

    let cur_w = (input.current / max_pct) * f64::from(bar_w);
    let prior_w = (input.prior_7d / max_pct) * f64::from(bar_w);
    let delta = input.current - input.prior_7d;
    let delta_color = if delta > 0.0 {
        palette::DARK.bull
    } else if delta < 0.0 {
        palette::DARK.bear
    } else {
        palette::DARK.muted
    };
    let arrow = if delta > 0.0 {
        "▲"
    } else if delta < 0.0 {
        "▼"
    } else {
        "—"
    };

    [
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="max-width:100%; font-family:{}">"#,
            width, height, FONT_SANS
        ),
        format!(
            r#"<text x="0" y="{:.1}" fill="{}" font-size="10.5" font-weight="500">{}</text>"#,
            f64::from(height) / 2.0 + 4.0,
            palette::DARK.text,
            escape_text(&input.name)
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
            r#"<rect x="{}" y="{:.1}" width="{:.1}" height="{}" rx="2" ry="2" fill="{}" opacity="0.22"/>"#,
            bar_x, bar_y, prior_w, bar_h, color_hex
        ),
        format!(
            r#"<rect x="{}" y="{:.1}" width="{:.1}" height="{}" rx="2" ry="2" fill="{}" opacity="0.85"/>"#,
            bar_x, bar_y, cur_w, bar_h, color_hex
        ),
        format!(
            r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="0.7" stroke-dasharray="2 1"/>"#,
            f64::from(bar_x) + prior_w,
            bar_y - 1.0,
            f64::from(bar_x) + prior_w,
            bar_y + f64::from(bar_h) + 1.0,
            palette::DARK.muted
        ),
        format!(
            r#"<text x="{:.1}" y="{:.1}" text-anchor="end" fill="{}" font-size="9" font-family="{}" font-weight="700">{:.0}%</text>"#,
            f64::from(bar_x) + cur_w - 4.0,
            bar_y + f64::from(bar_h) - 3.0,
            palette::DARK.bg,
            FONT_MONO,
            input.current
        ),
        format!(
            r#"<text x="{}" y="{:.1}" text-anchor="end" fill="{}" font-size="10.5" font-family="{}" font-weight="600">{} {:.0}pp</text>"#,
            width,
            f64::from(height) / 2.0 + 4.0,
            delta_color,
            FONT_MONO,
            arrow,
            delta.abs()
        ),
        "</svg>".to_string(),
    ]
    .join("")
}

pub fn render_ascii(input: &ProbBarInput) -> String {
    let delta = input.current - input.prior_7d;
    let arrow = if delta > 0.0 {
        "up"
    } else if delta < 0.0 {
        "down"
    } else {
        "flat"
    };
    format!(
        "{}: {:.0}% (7d {} {:+.0}pp)",
        input.name, input.current, arrow, delta
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prob_bar_matches_python_snapshot() {
        let rendered = render_svg(&ProbBarInput {
            name: "Inflation Spike".to_string(),
            current: 88.0,
            prior_7d: 80.0,
            color: "bear".to_string(),
            max_pct: None,
            width: None,
            height: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/prob_bar.svg"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn prob_bar_uses_raw_color_when_not_palette_token() {
        let rendered = render_svg(&ProbBarInput {
            name: "Risk".to_string(),
            current: 10.0,
            prior_7d: 12.0,
            color: "#123456".to_string(),
            max_pct: None,
            width: None,
            height: None,
        });
        assert!(rendered.contains(r##"fill="#123456" opacity="0.85""##));
    }
}
