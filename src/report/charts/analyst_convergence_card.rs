//! JSON schema for `pftui report chart analyst-convergence-card --from-json`:
//! `{ "asset": "Gold", "views": [{ "analyst": "analyst-low", "conviction": 3, "reasoning_summary": "..." }], "summary": "strong-convergent-bull" }`.
//!
//! Optional allocation footer fields are `current_alloc`, `user_target`, and
//! `analyst_range` as a two-item array. This helper is HTML-native in the
//! Python source, so use `--format html`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 580;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalystConvergenceView {
    pub analyst: String,
    #[serde(default)]
    pub conviction: i64,
    #[serde(default, alias = "reasoning")]
    pub reasoning_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalystConvergenceCardInput {
    pub asset: String,
    #[serde(default)]
    pub views: Vec<AnalystConvergenceView>,
    #[serde(default)]
    pub user_target: Option<f64>,
    #[serde(default)]
    pub current_alloc: Option<f64>,
    #[serde(default)]
    pub analyst_range: Option<[f64; 2]>,
    #[serde(default = "default_summary")]
    pub summary: String,
    #[serde(default)]
    pub width: Option<u32>,
}

impl AnalystConvergenceCardInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }
}

pub fn render_html(input: &AnalystConvergenceCardInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let (badge_color, badge_text) = summary_badge(&input.summary);
    let rows = input
        .views
        .iter()
        .map(render_view_row)
        .collect::<Vec<_>>()
        .join("");
    let footer = render_footer(
        input.current_alloc,
        input.user_target,
        input.analyst_range,
        badge_color,
    );

    format!(
        r#"
<table style="width:100%;max-width:{}px;border-collapse:collapse;border:1px solid {};border-radius:4px;background:{};margin:10px 0;font-family:{}">
<tr>
  <td colspan="3" style="padding:12px 14px 8px 14px">
    <span style="color:{};font-size:13pt;font-weight:700;font-family:{}">{}</span>
  </td>
  <td style="padding:12px 14px 8px 14px;text-align:right">
    <span style="background:{}26;color:{};padding:3px 12px;border-radius:10px;font-size:9.5pt;font-weight:700;letter-spacing:0.5px">{}</span>
  </td>
</tr>
<tr><td colspan="4" style="padding:0"><div style="border-top:1px solid {};margin:0 14px"></div></td></tr>
{}
{}
</table>"#,
        width,
        palette::DARK.border,
        palette::DARK.bg_alt,
        FONT_SANS,
        palette::DARK.text,
        FONT_MONO,
        escape_text(&input.asset),
        badge_color,
        badge_color,
        escape_text(&badge_text),
        palette::DARK.border,
        rows,
        footer
    )
}

pub fn render_ascii(input: &AnalystConvergenceCardInput) -> String {
    let (_, badge_text) = summary_badge(&input.summary);
    let rows = input
        .views
        .iter()
        .map(|view| {
            format!(
                "{} {:+}: {}",
                display_analyst(&view.analyst),
                view.conviction,
                view.reasoning_summary
            )
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return format!("{} analyst convergence: {}", input.asset, badge_text);
    }
    format!(
        "{} analyst convergence: {}\n{}",
        input.asset,
        badge_text,
        rows.join("\n")
    )
}

fn default_summary() -> String {
    "convergent-neutral".to_string()
}

fn render_view_row(view: &AnalystConvergenceView) -> String {
    let analyst = display_analyst(&view.analyst);
    let lcolor = layer_color(&analyst);
    let cconv = conviction_color(view.conviction);
    format!(
        r#"
<tr>
  <td style="padding:6px 8px;width:62px;color:{};font-weight:700;font-size:9.5pt;letter-spacing:0.5px;vertical-align:top">{}</td>
  <td style="padding:6px 4px;width:38px;color:{};font-weight:700;font-size:11pt;font-family:{};vertical-align:top">{:+}</td>
  <td style="padding:6px 8px;width:96px;vertical-align:middle">{}</td>
  <td style="padding:6px 12px 6px 8px;color:{};font-size:9.5pt;font-style:italic;line-height:1.4;vertical-align:top">{}</td>
</tr>"#,
        lcolor,
        escape_text(&analyst),
        cconv,
        FONT_MONO,
        view.conviction,
        mini_conviction_bar(view.conviction),
        palette::DARK.text,
        escape_text(&view.reasoning_summary)
    )
}

fn render_footer(
    current_alloc: Option<f64>,
    user_target: Option<f64>,
    analyst_range: Option<[f64; 2]>,
    badge_color: &'static str,
) -> String {
    if current_alloc.is_none() && user_target.is_none() && analyst_range.is_none() {
        return String::new();
    }

    let verdict_html = match (current_alloc, analyst_range) {
        (Some(current), Some(range)) if current < range[0] => {
            let gap = range[0] - current;
            format!(
                r#"<span style="color:{};font-weight:700;font-size:13pt;font-family:{}">ADD &nbsp;+{:.2}pp</span>"#,
                palette::DARK.bull,
                FONT_MONO,
                gap
            )
        }
        (Some(current), Some(range)) if current > range[1] => {
            let gap = current - range[1];
            format!(
                r#"<span style="color:{};font-weight:700;font-size:13pt;font-family:{}">TRIM &nbsp;-{:.2}pp</span>"#,
                palette::DARK.bear,
                FONT_MONO,
                gap
            )
        }
        (Some(_), Some(_)) => format!(
            r#"<span style="color:{};font-weight:700;font-size:13pt;font-family:{}">HOLD</span>"#,
            palette::DARK.neutral,
            FONT_MONO
        ),
        _ => String::new(),
    };

    let current = current_alloc
        .map(|value| format!("{value:.2}%"))
        .unwrap_or_else(|| "—".to_string());
    let target = user_target
        .map(|value| format!("{value:.1}%"))
        .unwrap_or_else(|| "—".to_string());
    let range = analyst_range
        .map(|range| format!("{:.1}–{:.1}%", range[0], range[1]))
        .unwrap_or_else(|| "—".to_string());

    format!(
        r#"
<tr><td colspan="4" style="padding:0"><div style="border-top:1px solid {};margin:0 14px"></div></td></tr>
<tr><td colspan="4" style="padding:0">
  <table style="width:100%;border-collapse:collapse">
    <tr>
      {}
      {}
      {}
      <td style="padding:10px 14px;vertical-align:middle;text-align:right">{}</td>
    </tr>
  </table>
</td></tr>"#,
        palette::DARK.border,
        footer_cell("CURRENT", &current, palette::DARK.text),
        footer_cell("YOUR TARGET", &target, palette::DARK.blue),
        footer_cell("ANALYST RANGE", &range, badge_color),
        verdict_html
    )
}

fn footer_cell(label: &str, value: &str, color: &str) -> String {
    format!(
        r#"
<td style="padding:10px 14px;vertical-align:top">
  <div style="color:{};font-size:8.5pt;letter-spacing:0.5px;font-weight:600;margin-bottom:3px">{}</div>
  <div style="color:{};font-size:12pt;font-weight:700;font-family:{}">{}</div>
</td>"#,
        palette::DARK.muted,
        label,
        color,
        FONT_MONO,
        value
    )
}

fn mini_conviction_bar(conviction: i64) -> String {
    let width = 80_u32;
    let height = 10_u32;
    let mid = f64::from(width) / 2.0;
    let color = conviction_color(conviction);
    let fill_mag = conviction.abs() as f64 / 5.0 * (f64::from(width) / 2.0);
    let mut parts = vec![format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" style="display:inline-block;vertical-align:middle;width:{}px;height:{}px">"#,
        width, height, width, height
    )];
    parts.push(format!(
        r#"<rect x="0" y="0" width="{}" height="{}" rx="1" ry="1" fill="{}" stroke="{}" stroke-width="0.5"/>"#,
        width,
        height,
        palette::DARK.panel,
        palette::DARK.border
    ));
    if conviction > 0 {
        parts.push(format!(
            r#"<rect x="{}" y="0" width="{:.1}" height="{}" fill="{}" opacity="0.7"/>"#,
            py_float(mid),
            fill_mag,
            height,
            color
        ));
    } else if conviction < 0 {
        parts.push(format!(
            r#"<rect x="{:.1}" y="0" width="{:.1}" height="{}" fill="{}" opacity="0.7"/>"#,
            mid - fill_mag,
            fill_mag,
            height,
            color
        ));
    }
    parts.push(format!(
        r#"<line x1="{}" y1="0" x2="{}" y2="{}" stroke="{}" stroke-width="0.5"/>"#,
        py_float(mid),
        py_float(mid),
        height,
        palette::DARK.border
    ));
    parts.push("</svg>".to_string());
    parts.join("")
}

fn display_analyst(analyst: &str) -> String {
    analyst.replace("analyst-", "").to_ascii_uppercase()
}

fn layer_color(layer: &str) -> &'static str {
    match layer {
        "LOW" => palette::DARK.blue,
        "MEDIUM" | "MED" => palette::DARK.cyan,
        "HIGH" => palette::DARK.mauve,
        "MACRO" => palette::DARK.yellow,
        _ => palette::DARK.text,
    }
}

fn conviction_color(conviction: i64) -> &'static str {
    if conviction > 0 {
        palette::DARK.bull
    } else if conviction < 0 {
        palette::DARK.bear
    } else {
        palette::DARK.neutral
    }
}

fn summary_badge(summary: &str) -> (&'static str, String) {
    match summary.trim().to_ascii_lowercase().as_str() {
        "strong-convergent-bull" => (palette::DARK.bull, "STRONG BULL".to_string()),
        "convergent-bull" => (palette::DARK.bull, "CONVERGENT BULL".to_string()),
        "convergent-neutral" => (palette::DARK.neutral, "NEUTRAL".to_string()),
        "convergent-bear" => (palette::DARK.bear, "CONVERGENT BEAR".to_string()),
        "strong-convergent-bear" => (palette::DARK.bear, "STRONG BEAR".to_string()),
        "divergent" => (palette::DARK.amber, "DIVERGENT".to_string()),
        "neutral-with-divergence" => (palette::DARK.amber, "NEUTRAL (DIVERGENT)".to_string()),
        "insufficient-views" => (palette::DARK.muted, "INSUFFICIENT VIEWS".to_string()),
        _ => (palette::DARK.neutral, summary.to_ascii_uppercase()),
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
    fn analyst_convergence_card_matches_python_snapshot() {
        let rendered = render_html(&AnalystConvergenceCardInput {
            asset: "Gold".to_string(),
            views: vec![
                view("analyst-low", 3, "COT 1.9 pctl extreme short = mechanical squeeze floor near $4,475; SMA50 breach tactical only"),
                view("analyst-medium", 3, "Structural bull intact (CB buying, COFER, COT); ceiling at DXY 99 until daily close <99"),
                view("analyst-high", 4, "Trends 3+5+7 converging on gold; CB structural baseline 750-850t/year; BTC-gold +0.66"),
                view("analyst-macro", 3, "Structural floor through 2027 via CB dual-flow pattern; 1942-44 reserve drawdown parallel"),
            ],
            user_target: Some(25.0),
            current_alloc: Some(21.91),
            analyst_range: Some([24.0, 28.0]),
            summary: "strong-convergent-bull".to_string(),
            width: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/analyst_convergence_card.html"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn analyst_convergence_card_accepts_defaults() {
        let input = AnalystConvergenceCardInput::from_value(serde_json::json!({
            "asset": "BTC",
            "views": [{"analyst": "low", "conviction": -2, "reasoning": "Risk off"}]
        }))
        .unwrap();

        assert_eq!(input.summary, "convergent-neutral");
        assert!(input.user_target.is_none());
        assert_eq!(
            render_ascii(&input),
            "BTC analyst convergence: NEUTRAL\nLOW -2: Risk off"
        );
    }

    fn view(analyst: &str, conviction: i64, reasoning_summary: &str) -> AnalystConvergenceView {
        AnalystConvergenceView {
            analyst: analyst.to_string(),
            conviction,
            reasoning_summary: reasoning_summary.to_string(),
        }
    }
}
