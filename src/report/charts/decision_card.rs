//! JSON schema for `pftui report chart decision-card --from-json`:
//! `{ "question": "...", "context_lines": ["..."], "recommendation": "...", "response_format": ["yes", "no"], "reference": "...", "urgency": "high" }`.
//!
//! This helper is HTML-native in the Python source, so use `--format html`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 580;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionCardInput {
    pub question: String,
    #[serde(default, alias = "context")]
    pub context_lines: Vec<String>,
    #[serde(default)]
    pub recommendation: Option<String>,
    #[serde(default, alias = "reply_options", alias = "options")]
    pub response_format: Option<Vec<String>>,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default = "default_urgency")]
    pub urgency: String,
    #[serde(default)]
    pub width: Option<u32>,
}

impl DecisionCardInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }
}

pub fn render_html(input: &DecisionCardInput) -> String {
    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let accent = urgency_color(&input.urgency);
    let ctx_html = context_html(&input.context_lines);
    let rec_html = recommendation_html(input.recommendation.as_deref(), accent);
    let fmt_html = response_format_html(input.response_format.as_deref());
    let ref_html = input.reference.as_deref().map_or_else(String::new, |reference| {
        format!(
            r#"<div style="color:{};font-size:8.5pt;margin-top:6px;font-style:italic">↑ {}</div>"#,
            palette::DARK.muted,
            escape_text(reference)
        )
    });

    format!(
        r#"
<table style="width:100%;max-width:{}px;border-collapse:collapse;border:1px solid {};border-left:3px solid {};border-radius:4px;background:{};margin:12px 0;font-family:{}">
<tr>
  <td style="padding:12px 16px">
    <div style="color:{};font-size:8.5pt;font-weight:700;letter-spacing:0.8px;margin-bottom:5px">QUESTION</div>
    <div style="color:{};font-size:11pt;font-weight:600;line-height:1.45">{}</div>
    {}
    {}
    {}
    {}
  </td>
</tr>
</table>"#,
        width,
        palette::DARK.border,
        accent,
        palette::DARK.bg_alt,
        FONT_SANS,
        accent,
        palette::DARK.text,
        escape_text(&input.question),
        ctx_html,
        rec_html,
        fmt_html,
        ref_html
    )
}

pub fn render_ascii(input: &DecisionCardInput) -> String {
    let mut lines = vec![
        format!("Decision question: {}", input.question),
        format!("Urgency: {}", input.urgency),
    ];
    if !input.context_lines.is_empty() {
        lines.push("Context:".to_string());
        lines.extend(input.context_lines.iter().map(|line| format!("- {line}")));
    }
    if let Some(recommendation) = &input.recommendation {
        lines.push(format!("Recommendation: {recommendation}"));
    }
    if let Some(response_format) = &input.response_format {
        if !response_format.is_empty() {
            lines.push(format!("Reply format: {}", response_format.join(", ")));
        }
    }
    if let Some(reference) = &input.reference {
        lines.push(format!("Reference: {reference}"));
    }
    lines.join("\n")
}

fn context_html(context_lines: &[String]) -> String {
    if context_lines.is_empty() {
        return String::new();
    }

    let items = context_lines
        .iter()
        .map(|line| format!("<li style='margin:2px 0'>{}</li>", escape_text(line)))
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<ul style="margin:6px 0 8px 22px;padding:0;color:{};font-size:10pt;line-height:1.55">{items}</ul>"#,
        palette::DARK.text
    )
}

fn recommendation_html(recommendation: Option<&str>, accent: &str) -> String {
    recommendation.map_or_else(String::new, |recommendation| {
        format!(
            r#"
<div style="margin:8px 0;padding:8px 12px;background:{}1a;border-left:2px solid {};font-size:10pt;line-height:1.5">
  <span style="color:{};font-size:8.5pt;font-weight:700;letter-spacing:0.5px">RECOMMENDATION  </span>
  <span style="color:{};font-style:italic">{}</span>
</div>"#,
            accent,
            accent,
            accent,
            palette::DARK.text,
            escape_text(recommendation)
        )
    })
}

fn response_format_html(response_format: Option<&[String]>) -> String {
    let Some(response_format) = response_format else {
        return String::new();
    };
    if response_format.is_empty() {
        return String::new();
    }

    let chips = response_format
        .iter()
        .map(|option| {
            format!(
                r#"<span style="display:inline-block;background:{};color:{};padding:2px 9px;border-radius:10px;font-size:8.5pt;font-family:{};margin-right:5px">{}</span>"#,
                palette::DARK.panel,
                palette::DARK.muted,
                FONT_MONO,
                escape_text(option)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"
<div style="margin-top:8px;color:{};font-size:8.5pt">
  <span style="letter-spacing:0.5px;font-weight:700">REPLY FORMAT &nbsp;</span>{chips}
</div>"#,
        palette::DARK.muted
    )
}

fn urgency_color(urgency: &str) -> &'static str {
    match urgency.trim().to_ascii_lowercase().as_str() {
        "high" => palette::DARK.bear,
        "normal" => palette::DARK.cyan,
        "low" => palette::DARK.muted,
        _ => palette::DARK.cyan,
    }
}

fn default_urgency() -> String {
    "normal".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_card_matches_python_snapshot() {
        let rendered = render_html(&DecisionCardInput {
            question: "Add ~$19,663 of physical gold over the next week if PCE pulls price into the $4,475-$4,500 zone?".to_string(),
            context_lines: vec![
                "Analyst convergence on Gold: strong-bull (avg +3.25, divergence 1 across 4 layers)".to_string(),
                "Analyst-recommended allocation range: 27.2–29.2% (you're at 21.91%, target 25%)".to_string(),
                "Mechanical squeeze floor at $4,475 from COT 1.9 percentile extreme short".to_string(),
                "Vehicle: physical bullion only (per your sovereignty principle — no GLD/WGLD/futures)".to_string(),
            ],
            recommendation: Some("Yes if PCE prints in-line or cool. Wait if PCE prints hot (>3.1%) — that's a different trade.".to_string()),
            response_format: Some(vec![
                "yes".to_string(),
                "yes-if".to_string(),
                "no".to_string(),
                "wait".to_string(),
                "other".to_string(),
            ]),
            reference: Some("See Gold convergence card and PCE catalyst above.".to_string()),
            urgency: "high".to_string(),
            width: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/decision_card.html"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn decision_card_accepts_aliases_and_defaults() {
        let input = DecisionCardInput::from_value(serde_json::json!({
            "question": "Trim BTC?",
            "context": ["Above target"],
            "options": ["yes", "no"]
        }))
        .unwrap();

        assert_eq!(input.context_lines, vec!["Above target"]);
        assert_eq!(
            input.response_format,
            Some(vec!["yes".to_string(), "no".to_string()])
        );
        assert_eq!(input.urgency, "normal");
        assert!(render_html(&input).contains("border-left:3px solid #89dceb"));
    }
}
