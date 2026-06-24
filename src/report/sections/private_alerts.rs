#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_alerts(ctx: &BuildContext) -> Result<String> {
    if ctx.private_alerts.is_empty() {
        return Ok(super::suppressed("no armed or triggered alerts"));
    }

    let mut out = String::from("## Alerts\n\n");
    out.push_str("| ID | Status | Kind | Symbol | Condition | Label | Triggered |\n");
    out.push_str("|---:|---|---|---|---|---|---|\n");
    for row in &ctx.private_alerts {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            row.id,
            clean(&row.status),
            clean(&row.kind),
            clean(&row.symbol),
            clean(row.condition.as_deref().unwrap_or("n/a")),
            clean(&row.label),
            clean(row.triggered_at.as_deref().unwrap_or("n/a")),
        ));
    }
    Ok(out.trim_end().to_string())
}

fn clean(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::PrivateAlertRow;

    #[test]
    fn renders_alert_table() {
        let ctx = BuildContext {
            private_alerts: vec![PrivateAlertRow {
                id: 7,
                symbol: "BTC-USD".to_string(),
                kind: "technical".to_string(),
                status: "armed".to_string(),
                condition: Some("cycle_component_monthly_erf_turned_up".to_string()),
                label: "Bitcoin monthly filter ticked up".to_string(),
                triggered_at: None,
            }],
            ..BuildContext::default()
        };
        let out = render_private_alerts(&ctx).unwrap();
        assert!(out.contains("## Alerts"));
        assert!(out.contains("cycle_component_monthly_erf_turned_up"));
    }
}
