#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_public_market_snapshot(ctx: &BuildContext) -> Result<String> {
    if ctx.market_snapshot.is_empty() {
        return Ok("## Market Snapshot\n\nMarket snapshot data is unavailable. Run a data refresh before relying on the daily report for cross-asset price context.".to_string());
    }

    let mut output = String::from(
        "## Market Snapshot\n\n| Asset | Price | Daily Chg | Weekly Chg | Signal |\n|---|---:|---:|---:|---|\n",
    );
    for row in &ctx.market_snapshot {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            clean_cell(&row.asset),
            clean_cell(row.price.as_deref().unwrap_or("n/a")),
            format_pct(row.daily_change_pct),
            format_pct(row.weekly_change_pct),
            clean_cell(row.signal.as_deref().unwrap_or("n/a")),
        ));
    }

    Ok(output.trim_end().to_string())
}

fn format_pct(value: Option<f64>) -> String {
    match value {
        Some(value) if value > 0.0 => format!("+{value:.1}%"),
        Some(value) => format!("{value:.1}%"),
        None => "n/a".to_string(),
    }
}

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::MarketSnapshotRow;

    #[test]
    fn public_market_snapshot_renders_required_rows() {
        let ctx = BuildContext {
            market_snapshot: vec![
                row("BTC", "$108,500", Some(1.2), Some(4.8), "risk bid"),
                row("GC=F", "$3,420", Some(-0.3), Some(2.1), "defensive bid"),
                row("SI=F", "$37.20", Some(0.4), None, "tracking gold"),
                row(
                    "CL=F",
                    "$78.10",
                    Some(2.0),
                    Some(-1.5),
                    "geopolitical premium",
                ),
                row("DXY", "99.4", Some(0.1), Some(0.6), "firmer dollar"),
                row("SPY", "$625.00", Some(-0.7), Some(-2.4), "breadth weak"),
                row("QQQ", "$540.00", Some(-1.1), Some(-3.2), "AI beta cooling"),
                row("VIX", "18.6", Some(5.5), Some(12.0), "volatility rising"),
                row("10Y yield", "4.12%", Some(0.0), Some(0.2), "rates sticky"),
            ],
            ..BuildContext::default()
        };

        let rendered = render_public_market_snapshot(&ctx).unwrap();

        assert!(rendered.starts_with("## Market Snapshot\n\n"));
        assert!(rendered.contains("| Asset | Price | Daily Chg | Weekly Chg | Signal |"));
        for asset in [
            "BTC",
            "GC=F",
            "SI=F",
            "CL=F",
            "DXY",
            "SPY",
            "QQQ",
            "VIX",
            "10Y yield",
        ] {
            assert!(rendered.contains(asset), "missing asset row: {asset}");
        }
        assert!(rendered.contains("| SI=F | $37.20 | +0.4% | n/a | tracking gold |"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_market_snapshot_handles_empty_data() {
        let rendered = render_public_market_snapshot(&BuildContext::default()).unwrap();

        assert!(rendered.contains("Market snapshot data is unavailable"));
        assert!(!rendered.contains("| Asset | Price |"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_market_snapshot_escapes_table_pipes() {
        let ctx = BuildContext {
            market_snapshot: vec![MarketSnapshotRow {
                asset: "BTC|USD".to_string(),
                price: Some("$100|000".to_string()),
                daily_change_pct: None,
                weekly_change_pct: None,
                signal: Some("risk|bid".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_market_snapshot(&ctx).unwrap();

        assert!(rendered.contains("BTC/USD"));
        assert!(rendered.contains("$100/000"));
        assert!(rendered.contains("risk/bid"));
    }

    fn row(
        asset: &str,
        price: &str,
        daily_change_pct: Option<f64>,
        weekly_change_pct: Option<f64>,
        signal: &str,
    ) -> MarketSnapshotRow {
        MarketSnapshotRow {
            asset: asset.to_string(),
            price: Some(price.to_string()),
            daily_change_pct,
            weekly_change_pct,
            signal: Some(signal.to_string()),
        }
    }

    fn assert_public_safe(markdown: &str) {
        let lowered = markdown.to_ascii_lowercase();
        for forbidden in [
            "i hold",
            "we own",
            "our position",
            "cost basis",
            "unrealized",
            "transaction",
            "allocation",
            "position size",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public snapshot leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
