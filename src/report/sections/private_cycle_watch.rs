#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_cycle_watch(ctx: &BuildContext) -> Result<String> {
    let Some(summary) = &ctx.private_cycle_watch else {
        return Ok(super::suppressed("Bitcoin cycle watch unavailable"));
    };

    let mut out = String::from("## Bitcoin Cycle Watch\n\n");
    out.push_str(&format!(
        "As of {}: **{}/{}** criteria met — {}.\n\n",
        clean(&summary.as_of),
        summary.met_count,
        summary.total,
        clean(&summary.verdict)
    ));
    out.push_str("| Check | Met | Progress | Detail | Distance |\n");
    out.push_str("|---|---:|---:|---|---|\n");
    for item in &summary.items {
        out.push_str(&format!(
            "| {} | {} | {}/{} | {} | {} |\n",
            clean(&item.label),
            if item.met { "yes" } else { "no" },
            item.met_components,
            item.total_components,
            clean(&item.detail),
            clean(&item.distance_notes.join("; ")),
        ));
    }
    if let Some(headline) = &summary.backtest_headline {
        out.push_str(&format!("\nBacktest: {}\n", clean(headline)));
    }
    if let Some(caveat) = &summary.caveat {
        out.push_str(&format!("\nCaveat: {}", clean(caveat)));
    }
    Ok(out.trim_end().to_string())
}

fn clean(value: &str) -> String {
    value
        .replace('|', "/")
        .replace('\n', " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{PrivateCycleWatchItem, PrivateCycleWatchSummary};

    #[test]
    fn renders_cycle_watch_summary() {
        let ctx = BuildContext {
            private_cycle_watch: Some(PrivateCycleWatchSummary {
                as_of: "2026-06-24".to_string(),
                verdict: "monthly suite: 1/7".to_string(),
                met_count: 1,
                total: 7,
                items: vec![PrivateCycleWatchItem {
                    label: "Roofing filter confirming up".to_string(),
                    met: true,
                    met_components: 2,
                    total_components: 2,
                    detail: "monthly value -1".to_string(),
                    distance_notes: vec!["filter change +2.00".to_string()],
                }],
                backtest_headline: Some("small-N".to_string()),
                caveat: None,
            }),
            ..BuildContext::default()
        };
        let out = render_private_cycle_watch(&ctx).unwrap();
        assert!(out.contains("## Bitcoin Cycle Watch"));
        assert!(out.contains("Roofing filter confirming up"));
    }
}
