#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{BuildContext, PrivateDriftRow, PrivatePositionSnapshotRow};

const DUST_THRESHOLD_PCT: f64 = 0.10;

pub fn render_private_portfolio_snapshot(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Portfolio Snapshot\n\n");
    output.push_str("<!-- stacked_bar - allocation overview -->\n");
    output.push_str("{stacked_bar(segments)}\n\n");
    output.push_str(&render_positions_table(&ctx.private_positions));
    output.push_str("\n\n");
    if let Some(dust) = render_dust_note(&ctx.private_positions) {
        output.push_str(&dust);
        output.push_str("\n\n");
    }
    output.push_str("### Drift vs Allocation Targets\n\n");
    output.push_str("Drift is computed against stated target bands. Analyst-recommended ranges appear separately in per-asset convergence cards.\n\n");
    output.push_str(&render_drift_bars(&ctx.private_drift_rows));

    Ok(output.trim_end().to_string())
}

fn render_positions_table(rows: &[PrivatePositionSnapshotRow]) -> String {
    let visible = visible_positions(rows);
    if visible.is_empty() {
        return "No held-position rows are attached to this build.".to_string();
    }

    let mut output = String::from(
        "| Symbol | Price | Day Chg | Allocation | Unrealized |\n|---|---:|---:|---:|---:|\n",
    );
    for row in visible {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            clean_cell(&row.symbol),
            clean_cell(row.price.as_deref().unwrap_or("n/a")),
            clean_cell(row.daily_change.as_deref().unwrap_or("n/a")),
            format_pct(row.allocation_pct),
            clean_cell(row.unrealized_pnl.as_deref().unwrap_or("n/a")),
        ));
    }
    output.trim_end().to_string()
}

fn visible_positions(rows: &[PrivatePositionSnapshotRow]) -> Vec<&PrivatePositionSnapshotRow> {
    let mut visible = rows
        .iter()
        .filter(|row| row.allocation_pct >= DUST_THRESHOLD_PCT)
        .collect::<Vec<_>>();
    visible.sort_by(|a, b| {
        b.allocation_pct
            .partial_cmp(&a.allocation_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    visible
}

fn render_dust_note(rows: &[PrivatePositionSnapshotRow]) -> Option<String> {
    let dust = rows
        .iter()
        .filter(|row| row.allocation_pct > 0.0 && row.allocation_pct < DUST_THRESHOLD_PCT)
        .map(|row| {
            format!(
                "{} ({})",
                clean_cell(&row.symbol),
                format_pct(row.allocation_pct)
            )
        })
        .collect::<Vec<_>>();
    if dust.is_empty() {
        None
    } else {
        Some(format!("Dust positions below 0.10%: {}.", dust.join(", ")))
    }
}

fn render_drift_bars(rows: &[PrivateDriftRow]) -> String {
    if rows.is_empty() {
        return "No allocation target drift rows are attached to this build.".to_string();
    }

    let mut sorted = rows.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    sorted
        .into_iter()
        .map(|row| {
            format!(
                "{{drift_bar({}, target={}, actual={}, band={})}}",
                clean_cell(&row.symbol),
                format_number(row.target_pct),
                format_number(row.actual_pct),
                format_number(row.band_pct),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_pct(value: f64) -> String {
    format!("{value:.2}%")
}

fn format_number(value: f64) -> String {
    format!("{value:.2}")
}

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_portfolio_snapshot_renders_synthetic_holdings_deterministically() {
        let rendered = render_private_portfolio_snapshot(&fixture_context()).unwrap();

        assert!(rendered.starts_with("## Portfolio Snapshot\n\n"));
        assert!(rendered.contains("{stacked_bar(segments)}"));
        assert!(rendered.contains("| BTC | 65000 | +2.10% | 42.00% | +12000 |"));
        assert!(rendered.contains("| USD | 1 | n/a | 35.00% | n/a |"));
        assert!(rendered.contains("| GLD | 225 | -0.50% | 22.95% | +3400 |"));
        assert!(rendered.find("| BTC |").unwrap() < rendered.find("| USD |").unwrap());
    }

    #[test]
    fn private_portfolio_snapshot_handles_dust_positions() {
        let rendered = render_private_portfolio_snapshot(&fixture_context()).unwrap();

        assert!(!rendered.contains("| DOGE |"));
        assert!(rendered.contains("Dust positions below 0.10%: DOGE (0.05%)."));
    }

    #[test]
    fn private_portfolio_snapshot_drift_bars_match_fixture_values() {
        let rendered = render_private_portfolio_snapshot(&fixture_context()).unwrap();

        assert!(rendered.contains("{drift_bar(BTC, target=40.00, actual=42.00, band=5.00)}"));
        assert!(rendered.contains("{drift_bar(GLD, target=25.00, actual=22.95, band=3.00)}"));
        assert!(rendered.contains("{drift_bar(USD, target=35.00, actual=35.00, band=5.00)}"));
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            private_positions: vec![
                position("USD", Some("1"), None, 35.0, None),
                position("BTC", Some("65000"), Some("+2.10%"), 42.0, Some("+12000")),
                position("GLD", Some("225"), Some("-0.50%"), 22.95, Some("+3400")),
                position("DOGE", Some("0.15"), Some("+4.00%"), 0.05, Some("-12")),
            ],
            private_drift_rows: vec![
                drift("BTC", 40.0, 42.0, 5.0),
                drift("USD", 35.0, 35.0, 5.0),
                drift("GLD", 25.0, 22.95, 3.0),
            ],
            ..BuildContext::default()
        }
    }

    fn position(
        symbol: &str,
        price: Option<&str>,
        daily_change: Option<&str>,
        allocation_pct: f64,
        unrealized_pnl: Option<&str>,
    ) -> PrivatePositionSnapshotRow {
        PrivatePositionSnapshotRow {
            symbol: symbol.to_string(),
            price: price.map(ToString::to_string),
            daily_change: daily_change.map(ToString::to_string),
            allocation_pct,
            unrealized_pnl: unrealized_pnl.map(ToString::to_string),
        }
    }

    fn drift(symbol: &str, target_pct: f64, actual_pct: f64, band_pct: f64) -> PrivateDriftRow {
        PrivateDriftRow {
            symbol: symbol.to_string(),
            target_pct,
            actual_pct,
            band_pct,
        }
    }
}
