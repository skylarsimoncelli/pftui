#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_cycle_watch(ctx: &BuildContext) -> Result<String> {
    let Some(summary) = &ctx.private_cycle_watch else {
        return Ok(super::suppressed("Bitcoin cycle watch unavailable"));
    };

    let mut out = String::from("## Bitcoin Cycle Watch\n\n");
    if !summary.panels.is_empty() {
        for (idx, panel) in summary.panels.iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(&format!(
                "### {}\n\nAs of {}: **{}/{}** criteria met — {}.\n\n",
                clean(&panel.title),
                clean(&panel.as_of),
                panel.met_count,
                panel.total,
                clean(&panel.verdict)
            ));
            render_table(&mut out, &panel.items);
            if let Some(headline) = &panel.backtest_headline {
                out.push_str(&format!("\nBacktest: {}\n", clean(headline)));
            }
            if let Some(caveat) = &panel.caveat {
                out.push_str(&format!("\nCaveat: {}\n", clean(caveat)));
            }
        }
        return Ok(out.trim_end().to_string());
    }

    out.push_str(&format!(
        "As of {}: **{}/{}** criteria met — {}.\n\n",
        clean(&summary.as_of),
        summary.met_count,
        summary.total,
        clean(&summary.verdict)
    ));
    render_table(&mut out, &summary.items);
    if let Some(headline) = &summary.backtest_headline {
        out.push_str(&format!("\nBacktest: {}\n", clean(headline)));
    }
    if let Some(caveat) = &summary.caveat {
        out.push_str(&format!("\nCaveat: {}", clean(caveat)));
    }
    Ok(out.trim_end().to_string())
}

fn render_table(out: &mut String, items: &[crate::report::build::daily::PrivateCycleWatchItem]) {
    out.push_str("| Check | Met | Progress | Detail | Distance |\n");
    out.push_str("|---|---:|---:|---|---|\n");
    for item in items {
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
    use crate::report::build::daily::{
        PrivateCycleWatchItem, PrivateCycleWatchPanel, PrivateCycleWatchSummary,
    };

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
                panels: vec![
                    PrivateCycleWatchPanel {
                        title: "Cycle-low accumulation".to_string(),
                        as_of: "2026-06-24".to_string(),
                        verdict: "monthly low suite: 1/7".to_string(),
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
                        backtest_headline: Some("low backtest".to_string()),
                        caveat: None,
                    },
                    PrivateCycleWatchPanel {
                        title: "Cycle-high exhaustion".to_string(),
                        as_of: "2026-06-24".to_string(),
                        verdict: "monthly high suite: 3/7".to_string(),
                        met_count: 3,
                        total: 7,
                        items: vec![PrivateCycleWatchItem {
                            label: "Roofing filter confirming down".to_string(),
                            met: false,
                            met_components: 1,
                            total_components: 2,
                            detail: "monthly value +1".to_string(),
                            distance_notes: vec!["filter change -2.00".to_string()],
                        }],
                        backtest_headline: Some("high backtest".to_string()),
                        caveat: Some("small sample".to_string()),
                    },
                ],
            }),
            ..BuildContext::default()
        };
        let out = render_private_cycle_watch(&ctx).unwrap();
        assert!(out.contains("## Bitcoin Cycle Watch"));
        assert!(out.contains("### Cycle-low accumulation"));
        assert!(out.contains("### Cycle-high exhaustion"));
        assert!(out.contains("Roofing filter confirming up"));
        assert!(out.contains("Roofing filter confirming down"));
    }
}
