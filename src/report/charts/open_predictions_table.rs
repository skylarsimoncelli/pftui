//! JSON schema for `pftui report chart open-predictions-table --from-json`:
//! `{ "predictions": [{ "id": 835, "asset": "SPY", "claim": "...", "days_remaining": 1, "confidence": 0.40, "direction": "bear" }] }`.
//!
//! A bare prediction array is also accepted. `asset` also accepts `symbol`.
//! This helper is HTML-native in the Python source, so use `--format html`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::report::palette;
use crate::report::svg::escape_text;
use crate::report::theme::{FONT_MONO, FONT_SANS};

const DEFAULT_WIDTH: u32 = 580;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenPredictionRow {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub claim: String,
    #[serde(default = "default_asset", alias = "symbol")]
    pub asset: String,
    #[serde(default, alias = "due_in_days", alias = "days_until_due")]
    pub days_remaining: i64,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenPredictionsTableInput {
    pub predictions: Vec<OpenPredictionRow>,
    #[serde(default)]
    pub width: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpenPredictionsPayload {
    Predictions(Vec<OpenPredictionRow>),
    Object(OpenPredictionsTableInput),
}

fn default_asset() -> String {
    "\u{2014}".to_string()
}

impl OpenPredictionsTableInput {
    pub fn from_value(value: Value) -> serde_json::Result<Self> {
        match serde_json::from_value(value)? {
            OpenPredictionsPayload::Predictions(predictions) => Ok(Self {
                predictions,
                width: None,
            }),
            OpenPredictionsPayload::Object(input) => Ok(input),
        }
    }
}

pub fn render_html(input: &OpenPredictionsTableInput) -> String {
    if input.predictions.is_empty() {
        return String::new();
    }

    let width = input.width.unwrap_or(DEFAULT_WIDTH);
    let rows = input
        .predictions
        .iter()
        .map(|prediction| {
            let days = prediction.days_remaining;
            let urgency_color = urgency(days);
            let label = due_label(days);
            let confidence = prediction.confidence.unwrap_or(0.0);
            let confidence_color = if confidence >= 0.6 {
                palette::DARK.bull
            } else if confidence >= 0.4 {
                palette::DARK.amber
            } else {
                palette::DARK.muted
            };
            format!(
                r#"
<tr>
  <td style="padding:7px 0;vertical-align:top">
    <span style="display:inline-block;min-width:64px;text-align:center;background:{}26;color:{};padding:3px 10px;border-radius:11px;font-size:9.5pt;font-weight:700;font-family:{}">{}</span>
  </td>
  <td style="padding:7px 12px 7px 14px;color:{};font-size:10pt;font-weight:600;font-family:{};vertical-align:top;white-space:nowrap">{}</td>
  <td style="padding:7px 8px;color:{};font-size:10pt;vertical-align:top;line-height:1.4">{}</td>
  <td style="padding:7px 4px 7px 12px;color:{};font-size:10pt;font-weight:600;font-family:{};text-align:right;vertical-align:top;white-space:nowrap">{:.2}</td>
</tr>"#,
                urgency_color,
                urgency_color,
                FONT_MONO,
                label,
                palette::DARK.text,
                FONT_MONO,
                escape_text(&prediction.asset),
                palette::DARK.text,
                escape_text(&prediction.claim),
                confidence_color,
                FONT_MONO,
                confidence
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"
<table style="width:100%;max-width:{}px;border-collapse:collapse;font-family:{};margin:8px 0">
<thead>
<tr style="border-bottom:1px solid {}">
  <th style="text-align:left;padding:6px 0;color:{};font-size:8.5pt;font-weight:700;letter-spacing:0.8px;width:80px">DUE IN</th>
  <th style="text-align:left;padding:6px 14px;color:{};font-size:8.5pt;font-weight:700;letter-spacing:0.8px;width:80px">ASSET</th>
  <th style="text-align:left;padding:6px 8px;color:{};font-size:8.5pt;font-weight:700;letter-spacing:0.8px">PREDICTION</th>
  <th style="text-align:right;padding:6px 4px 6px 12px;color:{};font-size:8.5pt;font-weight:700;letter-spacing:0.8px;width:50px">CONF</th>
</tr>
</thead>
<tbody>{}</tbody>
</table>"#,
        width,
        FONT_SANS,
        palette::DARK.border,
        palette::DARK.muted,
        palette::DARK.muted,
        palette::DARK.muted,
        palette::DARK.muted,
        rows
    )
}

pub fn render_ascii(input: &OpenPredictionsTableInput) -> String {
    if input.predictions.is_empty() {
        return "No open predictions".to_string();
    }
    input
        .predictions
        .iter()
        .map(|prediction| {
            let id = prediction
                .id
                .map(|value| format!("#{value} "))
                .unwrap_or_default();
            let confidence = prediction.confidence.unwrap_or(0.0);
            format!(
                "{}{} {} conf {:.2}: {}",
                id,
                due_label(prediction.days_remaining),
                prediction.asset,
                confidence,
                prediction.claim
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn due_label(days: i64) -> String {
    if days <= 0 {
        "today".to_string()
    } else if days == 1 {
        "tomorrow".to_string()
    } else {
        format!("{days}d")
    }
}

fn urgency(days: i64) -> &'static str {
    if days <= 0 {
        palette::DARK.bear
    } else if days <= 1 {
        palette::DARK.amber
    } else if days <= 3 {
        palette::DARK.yellow
    } else {
        palette::DARK.neutral
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_predictions_table_matches_python_snapshot() {
        let rendered = render_html(&OpenPredictionsTableInput {
            predictions: vec![
                OpenPredictionRow {
                    id: Some(835),
                    asset: "SPY".to_string(),
                    claim: "SPY trades below $745 on PCE day".to_string(),
                    days_remaining: 1,
                    confidence: Some(0.40),
                    direction: Some("bear".to_string()),
                },
                OpenPredictionRow {
                    id: Some(829),
                    asset: "Gold".to_string(),
                    claim: "Gold range 4400-4700 holds through PCE".to_string(),
                    days_remaining: 1,
                    confidence: Some(0.58),
                    direction: Some("neutral".to_string()),
                },
                OpenPredictionRow {
                    id: Some(834),
                    asset: "CL=F".to_string(),
                    claim: "WTI does not close >$100 May 26-27".to_string(),
                    days_remaining: 0,
                    confidence: Some(0.62),
                    direction: Some("bear".to_string()),
                },
                OpenPredictionRow {
                    id: Some(827),
                    asset: "PCE".to_string(),
                    claim: "Core PCE in-line at 2.8-3.0%".to_string(),
                    days_remaining: 1,
                    confidence: Some(0.45),
                    direction: Some("neutral".to_string()),
                },
                OpenPredictionRow {
                    id: Some(822),
                    asset: "Gold".to_string(),
                    claim: "Gold monthly close \u{2265}$4,000 through Q1 2027".to_string(),
                    days_remaining: 220,
                    confidence: Some(0.70),
                    direction: Some("bull".to_string()),
                },
            ],
            width: None,
        });
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/open_predictions_table.html"
        ));
        assert_eq!(rendered, expected.trim_end());
    }

    #[test]
    fn open_predictions_table_accepts_bare_array_and_symbol_alias() {
        let input = OpenPredictionsTableInput::from_value(serde_json::json!([
            {"symbol": "BTC", "claim": "BTC holds support", "days_remaining": 2}
        ]))
        .unwrap();

        assert_eq!(input.predictions.len(), 1);
        assert_eq!(input.predictions[0].asset, "BTC");
        assert_eq!(input.predictions[0].confidence, None);
    }
}
