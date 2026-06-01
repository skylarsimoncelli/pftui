#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, PrivateOpenPredictionRow, PrivateOpenPredictionsCalibration,
};

const PENDING_WINDOW_DAYS: i64 = 7;

pub fn render_private_open_predictions(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Open Predictions Resolving in Next 7 Days\n\n");

    let pending = pending_window_rows(&ctx.private_open_predictions);

    if pending.is_empty() {
        output.push_str("No pending predictions resolve in the next 7 days.");
        return Ok(output);
    }

    output.push_str(&render_table_call(&pending));
    output.push_str("\n\n");
    output.push_str(&render_interpretation(&pending, ctx.private_open_predictions_calibration.as_ref()));

    Ok(output.trim_end().to_string())
}

fn pending_window_rows(rows: &[PrivateOpenPredictionRow]) -> Vec<&PrivateOpenPredictionRow> {
    let mut filtered = rows
        .iter()
        .filter(|row| row.days_remaining >= 0 && row.days_remaining <= PENDING_WINDOW_DAYS)
        .collect::<Vec<_>>();
    // Stable ordering: by target_date asc, then by id asc (with None last), then by symbol.
    filtered.sort_by(|a, b| {
        a.target_date
            .cmp(&b.target_date)
            .then_with(|| a.days_remaining.cmp(&b.days_remaining))
            .then_with(|| match (a.id, b.id) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    filtered
}

fn render_table_call(rows: &[&PrivateOpenPredictionRow]) -> String {
    let entries = rows
        .iter()
        .map(|row| {
            let id_part = row
                .id
                .map(|id| format!("id={id}, "))
                .unwrap_or_default();
            let direction = row
                .direction
                .as_deref()
                .map(clean_arg)
                .unwrap_or_else(|| "neutral".to_string());
            format!(
                "({id_part}asset={asset}, claim={claim}, days_remaining={days}, confidence={confidence}, direction={direction})",
                asset = clean_arg(&row.symbol),
                claim = clean_arg(&row.claim),
                days = row.days_remaining,
                confidence = format_confidence(row.confidence),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{open_predictions_table(predictions_from_db=[{entries}])}}")
}

fn render_interpretation(
    rows: &[&PrivateOpenPredictionRow],
    calibration: Option<&PrivateOpenPredictionsCalibration>,
) -> String {
    let count = rows.len();
    let plural = if count == 1 { "prediction" } else { "predictions" };
    let avg_conf = average_confidence(rows);
    let calibration_clause = match calibration {
        Some(cal) if cal.sample_size > 0 => {
            let layer = cal
                .layer
                .as_deref()
                .map(clean_text)
                .unwrap_or_else(|| "overall".to_string());
            match (cal.predicted_pct, cal.observed_pct) {
                (Some(predicted), Some(observed)) => format!(
                    "; {} calibration over the trailing window shows {:.0}% predicted vs {:.0}% observed (n={})",
                    layer, predicted, observed, cal.sample_size
                ),
                _ => format!(
                    "; {} calibration sample size is {}",
                    layer, cal.sample_size
                ),
            }
        }
        _ => "; no calibration context is attached".to_string(),
    };
    let conf_clause = match avg_conf {
        Some(value) => format!(" at an average stated confidence of {:.0}%", value * 100.0),
        None => String::new(),
    };
    let earliest = rows
        .first()
        .map(|row| clean_text(&row.target_date))
        .unwrap_or_else(|| "n/a".to_string());
    sentence(&format!(
        "{count} pending {plural} resolve by {earliest}{conf_clause}{calibration_clause}"
    ))
}

fn average_confidence(rows: &[&PrivateOpenPredictionRow]) -> Option<f64> {
    let values: Vec<f64> = rows.iter().filter_map(|row| row.confidence).collect();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn format_confidence(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.2}"),
        None => "none".to_string(),
    }
}

fn clean_text(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

fn clean_arg(value: &str) -> String {
    clean_text(value)
        .replace(['[', ']', '{', '}', '(', ')'], " ")
        .replace(',', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn sentence(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_open_predictions_filters_to_pending_window() {
        let rendered = render_private_open_predictions(&fixture_context()).unwrap();

        assert!(rendered.starts_with("## Open Predictions Resolving in Next 7 Days\n\n"));
        // In-window predictions present.
        assert!(rendered.contains("id=101"));
        assert!(rendered.contains("id=102"));
        assert!(rendered.contains("id=103"));
        // Out-of-window predictions excluded.
        assert!(!rendered.contains("id=200"), "30d-out prediction must be excluded: {rendered}");
        assert!(!rendered.contains("id=201"), "expired prediction must be excluded: {rendered}");
        assert!(!rendered.contains("id=202"), "8d-out prediction must be excluded: {rendered}");
    }

    #[test]
    fn private_open_predictions_date_ordering_is_stable() {
        let ctx = fixture_context();
        let rendered_a = render_private_open_predictions(&ctx).unwrap();
        let rendered_b = render_private_open_predictions(&ctx).unwrap();
        assert_eq!(rendered_a, rendered_b);

        // Order: id=101 (target 2026-06-02), id=102 (2026-06-04), id=103 (2026-06-07).
        let p101 = rendered_a.find("id=101").unwrap();
        let p102 = rendered_a.find("id=102").unwrap();
        let p103 = rendered_a.find("id=103").unwrap();
        assert!(p101 < p102, "{rendered_a}");
        assert!(p102 < p103, "{rendered_a}");
    }

    #[test]
    fn private_open_predictions_empty_fixture_renders_explicit_empty_state() {
        let ctx = BuildContext::default();
        let rendered = render_private_open_predictions(&ctx).unwrap();

        assert!(rendered.starts_with("## Open Predictions Resolving in Next 7 Days\n\n"));
        assert!(rendered.contains("No pending predictions resolve in the next 7 days."));
        assert!(!rendered.contains("{open_predictions_table"));
    }

    #[test]
    fn private_open_predictions_emits_native_chart_call() {
        let rendered = render_private_open_predictions(&fixture_context()).unwrap();
        assert!(
            rendered.contains("{open_predictions_table(predictions_from_db=["),
            "expected native chart helper call in: {rendered}"
        );
    }

    #[test]
    fn private_open_predictions_interpretation_mentions_calibration_when_present() {
        let rendered = render_private_open_predictions(&fixture_context()).unwrap();
        assert!(
            rendered.contains("calibration"),
            "expected calibration mention in: {rendered}"
        );
        assert!(rendered.contains("3 pending predictions"));
    }

    #[test]
    fn private_open_predictions_handles_missing_calibration_gracefully() {
        let mut ctx = fixture_context();
        ctx.private_open_predictions_calibration = None;
        let rendered = render_private_open_predictions(&ctx).unwrap();
        assert!(rendered.contains("no calibration context is attached"));
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            private_open_predictions: vec![
                // In-window — out of order on input, must sort by target_date.
                prediction(103, "GLD", "Gold range 4400-4700 holds through PCE", "2026-06-07", 6, Some(0.58), Some("neutral")),
                prediction(101, "SPY", "SPY trades below $745 on PCE day", "2026-06-02", 1, Some(0.40), Some("bear")),
                prediction(102, "BTC", "BTC closes above 100k", "2026-06-04", 3, Some(0.62), Some("bull")),
                // Out of window — past, on-boundary-out, and far future.
                prediction(201, "DXY", "DXY breaks 105", "2026-05-30", -2, Some(0.55), Some("bear")),
                prediction(202, "WTI", "WTI closes above $90", "2026-06-09", 8, Some(0.45), Some("bull")),
                prediction(200, "TLT", "TLT trades over 95", "2026-07-01", 30, Some(0.70), Some("bull")),
            ],
            private_open_predictions_calibration: Some(PrivateOpenPredictionsCalibration {
                layer: Some("low".to_string()),
                sample_size: 24,
                predicted_pct: Some(62.0),
                observed_pct: Some(58.0),
            }),
            ..BuildContext::default()
        }
    }

    fn prediction(
        id: i64,
        symbol: &str,
        claim: &str,
        target_date: &str,
        days_remaining: i64,
        confidence: Option<f64>,
        direction: Option<&str>,
    ) -> PrivateOpenPredictionRow {
        PrivateOpenPredictionRow {
            id: Some(id),
            symbol: symbol.to_string(),
            claim: claim.to_string(),
            target_date: target_date.to_string(),
            days_remaining,
            confidence,
            conviction: None,
            direction: direction.map(|d| d.to_string()),
        }
    }
}
