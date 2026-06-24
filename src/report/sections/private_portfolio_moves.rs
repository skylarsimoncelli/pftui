#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_portfolio_moves(ctx: &BuildContext) -> Result<String> {
    if ctx.private_portfolio_moves.is_empty() {
        return Ok(super::suppressed("no portfolio move rows"));
    }

    let mut out = String::from("## Portfolio Moves\n\n");
    out.push_str("| Asset | 1h | 24h | 7d | 30d |\n");
    out.push_str("|---|---:|---:|---:|---:|\n");
    for row in &ctx.private_portfolio_moves {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            clean(&row.symbol),
            fmt_pct(row.move_1h_pct),
            fmt_pct(row.move_24h_pct),
            fmt_pct(row.move_7d_pct),
            fmt_pct(row.move_30d_pct),
        ));
    }
    out.push_str("\n1h is `n/a` until pftui stores intraday price history; 24h/7d/30d use cached daily market series.");
    Ok(out.trim_end().to_string())
}

fn fmt_pct(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:+.2}%"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn clean(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::PrivatePortfolioMoveRow;

    #[test]
    fn renders_move_table_with_intraday_caveat() {
        let ctx = BuildContext {
            private_portfolio_moves: vec![PrivatePortfolioMoveRow {
                symbol: "TOTAL".to_string(),
                move_1h_pct: None,
                move_24h_pct: Some(1.25),
                move_7d_pct: Some(-2.0),
                move_30d_pct: None,
            }],
            ..BuildContext::default()
        };
        let out = render_private_portfolio_moves(&ctx).unwrap();
        assert!(out.contains("## Portfolio Moves"));
        assert!(out.contains("+1.25%"));
        assert!(out.contains("intraday price history"));
    }
}
