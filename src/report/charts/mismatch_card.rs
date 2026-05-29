//! JSON schema for `pftui report chart mismatch-card --from-json`:
//! `{ "asset": "BTC", "skylar_view": "...", "analyst_summary": "convergent-bull", "analyst_avg_conviction": 1.75, "skylar_date": "May 26, 2026" }`.
//!
//! This helper is HTML-native in the Python source, so use `--format html`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 580;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MismatchCardInput {
    pub asset: String,
    #[serde(alias = "user_view")]
    pub skylar_view: String,
    pub analyst_summary: String,
    #[serde(alias = "avg_conviction", alias = "analyst_avg")]
    pub analyst_avg_conviction: f64,
    #[serde(default)]
    pub skylar_date: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
}

impl MismatchCardInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }
}

pub fn render_html(input: &MismatchCardInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let analyst_color = summary_color(&input.analyst_summary);
    let summary_text = input.analyst_summary.replace('-', " ").to_ascii_uppercase();
    let skylar_date = input.skylar_date.as_deref().unwrap_or("recent");

    format!(
        r#"
<table style="width:100%;max-width:{}px;border-collapse:collapse;border:1px solid {}99;border-radius:4px;background:{};margin:10px 0;font-family:{}">
<tr>
  <td style="padding:10px 14px 6px 14px">
    <span style="color:{};font-size:12pt;font-weight:700;font-family:{}">{}</span>
  </td>
  <td style="padding:10px 14px 6px 14px;text-align:right">
    <span style="color:{};font-size:9.5pt;font-weight:700;letter-spacing:0.5px">⚠ VIEW MISMATCH</span>
  </td>
</tr>
<tr>
  <td style="width:50%;padding:0 14px 12px 14px;vertical-align:top;border-right:1px solid {}66">
    <div style="color:{};font-size:8.5pt;font-weight:700;letter-spacing:0.5px;margin-bottom:4px">SKYLAR ({})</div>
    <div style="color:{};font-size:9.5pt;font-style:italic;line-height:1.5">{}</div>
  </td>
  <td style="width:50%;padding:0 14px 12px 14px;vertical-align:top">
    <div style="color:{};font-size:8.5pt;font-weight:700;letter-spacing:0.5px;margin-bottom:4px">ANALYST CONVERGENCE</div>
    <div style="color:{};font-size:11pt;font-weight:700">{} <span style="color:{};font-weight:500;font-family:{};font-size:10pt">avg {:+.2}</span></div>
  </td>
</tr>
</table>"#,
        width,
        palette::DARK.amber,
        palette::DARK.bg_alt,
        FONT_SANS,
        palette::DARK.text,
        FONT_MONO,
        escape_text(&input.asset),
        palette::DARK.amber,
        palette::DARK.border,
        palette::DARK.muted,
        escape_text(skylar_date),
        palette::DARK.text,
        escape_text(&input.skylar_view),
        palette::DARK.muted,
        analyst_color,
        escape_text(&summary_text),
        palette::DARK.text,
        FONT_MONO,
        input.analyst_avg_conviction
    )
}

pub fn render_ascii(input: &MismatchCardInput) -> String {
    let skylar_date = input.skylar_date.as_deref().unwrap_or("recent");
    let summary_text = input.analyst_summary.replace('-', " ").to_ascii_uppercase();
    format!(
        "{} view mismatch\nSkylar ({}): {}\nAnalysts: {} avg {:+.2}",
        input.asset, skylar_date, input.skylar_view, summary_text, input.analyst_avg_conviction
    )
}

fn summary_color(summary: &str) -> &'static str {
    match summary.trim().to_ascii_lowercase().as_str() {
        "strong-convergent-bull" | "convergent-bull" => palette::DARK.bull,
        "convergent-neutral" => palette::DARK.neutral,
        "convergent-bear" | "strong-convergent-bear" => palette::DARK.bear,
        "divergent" | "neutral-with-divergence" => palette::DARK.amber,
        "insufficient-views" => palette::DARK.muted,
        _ => palette::DARK.neutral,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatch_card_matches_python_snapshot() {
        let rendered = render_html(&MismatchCardInput {
            asset: "BTC".to_string(),
            skylar_view: "Expects standard bear-market low around October 2026 at $45-60K; will buy heavily there. No adds at current $75-79K — current 1.1267 holding is the working accumulation, deliberate over-target.".to_string(),
            analyst_summary: "convergent-bull".to_string(),
            analyst_avg_conviction: 1.75,
            skylar_date: Some("May 26, 2026".to_string()),
            width: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/mismatch_card.html"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn mismatch_card_accepts_aliases_and_defaults_date() {
        let input = MismatchCardInput::from_value(serde_json::json!({
            "asset": "Gold",
            "user_view": "Wait for pullback",
            "analyst_summary": "divergent",
            "avg_conviction": -0.25
        }))
        .unwrap();

        assert_eq!(input.skylar_view, "Wait for pullback");
        assert_eq!(input.analyst_avg_conviction, -0.25);
        assert_eq!(input.skylar_date, None);
        assert!(render_html(&input).contains("SKYLAR (recent)"));
    }
}
