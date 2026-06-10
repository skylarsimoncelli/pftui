//! Private "Epistemic Health" section (epistemics R4).
//!
//! Renders the `run_health` row recorded for the report date: a compact
//! metric table with threshold flags plus a one-line interpretation. This is
//! meta-content — how healthy the intelligence process itself was — so it is
//! the last section in the private report, after the closing.
//!
//! Auto-suppressed (returns an empty string) when no run_health row exists
//! for the report date.

use anyhow::Result;

use crate::db::run_health::{threshold_flags, RunHealth};
use crate::report::build::daily::BuildContext;

fn fmt_f64(v: Option<f64>, precision: usize) -> String {
    v.map(|x| format!("{:.*}", precision, x))
        .unwrap_or_else(|| "—".to_string())
}

fn fmt_i64(v: Option<i64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "—".to_string())
}

/// One-line interpretation of the run: name the worst breach, or declare the
/// process healthy.
fn interpretation(row: &RunHealth, flags: &[(&'static str, String)]) -> String {
    if flags.is_empty() {
        return "Process healthy: disagreement is alive, the blind read tracks the house view, \
                and scenario probabilities moved within discipline."
            .to_string();
    }
    let parts: Vec<&str> = flags
        .iter()
        .map(|(metric, _)| match *metric {
            "agreement_rate" => "voices agree with the operator too readily (echo risk)",
            "panel_dispersion" => "panel personas cluster too tightly (persona washing)",
            "blind_divergence" => "the house view sits far from the raw-data read",
            "conviction_price_corr" => {
                "conviction is tracking price — momentum dressed as structure (rule 15)"
            }
            _ => "threshold breach",
        })
        .collect();
    let mut line = format!("Caution: {}.", parts.join("; "));
    if let Some(delta) = row.scenario_delta_total {
        if delta > 0.0 {
            line.push_str(&format!(
                " Scenario probabilities moved {:.1}pp today.",
                delta
            ));
        }
    }
    line
}

/// The "Recommendation scoreboard" sub-block: one line per held symbol with
/// scored forward returns (action mix, 90d hit rate, ADD−WAIT window
/// quality), plus the conviction-price flag line. Empty string when the
/// ledger has no scored rows for held assets (sub-block auto-suppressed).
fn recommendation_scoreboard_block(ctx: &BuildContext) -> String {
    if ctx.recommendation_scoreboard.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n### Recommendation scoreboard — is our own advice making money?\n\n");
    out.push_str(
        "_Every recorded call, scored against 90d forward returns. Window quality = mean \
         90d return after ADD minus after WAIT: negative means the desk's ADD calls were \
         worse than its own WAIT calls._\n\n",
    );
    out.push_str("| Symbol | Action mix | 90d positive | Window quality (ADD−WAIT) |\n|---|---|---|---|\n");
    for line in &ctx.recommendation_scoreboard {
        let pct = line
            .pct_positive_90d
            .map(|p| format!("{:.0}% of {}", p, line.n_scored_90d))
            .unwrap_or_else(|| "—".to_string());
        let wq = match line.window_quality_delta_pct {
            Some(d) if d >= 0.0 => format!("{:+.1}pp", d),
            Some(d) => format!("{:+.1}pp ⚠", d),
            None => "accruing".to_string(),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            line.symbol, line.action_mix, pct, wq
        ));
    }
    if let Some(corr) = ctx
        .epistemic_health
        .as_ref()
        .and_then(|r| r.conviction_price_corr)
    {
        out.push('\n');
        if corr.abs() > crate::db::run_health::CONVICTION_PRICE_CORR_CEILING {
            out.push_str(&format!(
                "Conviction-price correlation: max |r| = {:.2} — ⚠ momentum dressed as \
                 structure (standing rule 15): conviction is tracking price, not evidence.",
                corr.abs()
            ));
        } else {
            out.push_str(&format!(
                "Conviction-price correlation: max |r| = {:.2} — clean (conviction is not \
                 just a lagged price chart).",
                corr.abs()
            ));
        }
    }
    out.trim_end().to_string()
}

pub fn render_private_epistemic_health(ctx: &BuildContext) -> Result<String> {
    // Render when either the run_health row or scored ledger lines exist;
    // suppress entirely only when both are absent.
    if ctx.epistemic_health.is_none() && ctx.recommendation_scoreboard.is_empty() {
        return Ok(String::new());
    }

    let mut output = String::from("## Epistemic Health — how the machine ran today\n\n");
    output.push_str(
        "_Meta-instrumentation: not what we believe, but whether today's process \
         deserved to be believed._",
    );

    if let Some(row) = ctx.epistemic_health.as_ref() {
        let flags: Vec<(&'static str, String)> = threshold_flags(row);
        let flag_for = |metric: &str| -> String {
            flags
                .iter()
                .find(|(m, _)| *m == metric)
                .map(|(_, w)| w.clone())
                .unwrap_or_else(|| "ok".to_string())
        };

        output.push_str("\n\n| Metric | Value | Flag |\n|---|---|---|\n");
        output.push_str(&format!(
            "| Agreement rate (voices vs operator) | {} | {} |\n",
            fmt_f64(row.agreement_rate, 2),
            flag_for("agreement_rate"),
        ));
        output.push_str(&format!(
            "| Blind divergence (house vs raw-data read) | {} | {} |\n",
            fmt_f64(row.blind_divergence, 2),
            flag_for("blind_divergence"),
        ));
        output.push_str(&format!(
            "| Panel dispersion (persona confidence stddev) | {} | {} |\n",
            fmt_f64(row.panel_dispersion, 1),
            flag_for("panel_dispersion"),
        ));
        output.push_str(&format!(
            "| Conviction-price correlation (max \\|r\\|, rule 15) | {} | {} |\n",
            fmt_f64(row.conviction_price_corr, 2),
            flag_for("conviction_price_corr"),
        ));
        output.push_str(&format!(
            "| Novelty rate (new vs repeated notes) | {} | — |\n",
            fmt_f64(row.novelty_rate, 2),
        ));
        output.push_str(&format!(
            "| Fallback warnings | {} | — |\n",
            fmt_i64(row.fallback_warnings),
        ));
        output.push_str(&format!(
            "| Scenario churn (sum pp moved today) | {} | — |\n",
            fmt_f64(row.scenario_delta_total, 1),
        ));
        output.push_str(&format!(
            "| Audit pass rate | {} | — |\n",
            fmt_f64(row.audit_pass_rate, 2),
        ));
        output.push_str(&format!(
            "| Agents spawned | {} | — |\n",
            fmt_i64(row.agents_spawned),
        ));

        output.push('\n');
        output.push_str(&interpretation(row, &flags));
    }

    output.push_str(&recommendation_scoreboard_block(ctx));

    Ok(output.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_health::RunHealth;

    #[test]
    fn suppressed_without_run_health_row() {
        let ctx = BuildContext::default();
        let out = render_private_epistemic_health(&ctx).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn renders_table_with_flags_firing() {
        let ctx = BuildContext {
            epistemic_health: Some(RunHealth {
                run_date: "2026-06-10".to_string(),
                agreement_rate: Some(0.92),
                blind_divergence: Some(2.6),
                panel_dispersion: Some(3.1),
                novelty_rate: Some(0.4),
                fallback_warnings: Some(2),
                scenario_delta_total: Some(7.5),
                audit_pass_rate: Some(0.9),
                agents_spawned: Some(14),
                ..RunHealth::default()
            }),
            ..BuildContext::default()
        };
        let out = render_private_epistemic_health(&ctx).unwrap();
        assert!(out.starts_with("## Epistemic Health"));
        assert!(out.contains("echo risk"), "agreement 0.92 must flag");
        assert!(out.contains("persona washing"), "dispersion 3.1 must flag");
        assert!(
            out.contains("house view far from raw-data read"),
            "blind divergence 2.6 must flag"
        );
        assert!(out.contains("Caution:"));
        assert!(out.contains("7.5pp"));
    }

    #[test]
    fn renders_healthy_interpretation_without_flags() {
        let ctx = BuildContext {
            epistemic_health: Some(RunHealth {
                run_date: "2026-06-10".to_string(),
                agreement_rate: Some(0.7),
                blind_divergence: Some(1.2),
                panel_dispersion: Some(6.5),
                ..RunHealth::default()
            }),
            ..BuildContext::default()
        };
        let out = render_private_epistemic_health(&ctx).unwrap();
        assert!(out.contains("Process healthy"));
        assert!(!out.contains("Caution:"));
        // Missing metrics render as em-dash placeholders, not zeros.
        assert!(out.contains("| Audit pass rate | — | — |"));
        // No scored ledger rows → the scoreboard sub-block is suppressed.
        assert!(!out.contains("Recommendation scoreboard"));
    }

    #[test]
    fn conviction_price_corr_renders_in_table_with_rule_15_flag() {
        let ctx = BuildContext {
            epistemic_health: Some(RunHealth {
                run_date: "2026-06-10".to_string(),
                agreement_rate: Some(0.7),
                panel_dispersion: Some(6.5),
                conviction_price_corr: Some(0.78),
                ..RunHealth::default()
            }),
            ..BuildContext::default()
        };
        let out = render_private_epistemic_health(&ctx).unwrap();
        assert!(out.contains("Conviction-price correlation"));
        assert!(out.contains("momentum dressed as structure"));
        assert!(out.contains("standing rule 15"));
        assert!(out.contains("Caution:"));
    }

    #[test]
    fn scoreboard_sub_block_renders_for_scored_held_assets() {
        use crate::report::build::daily::RecommendationScoreboardLine;
        let ctx = BuildContext {
            epistemic_health: Some(RunHealth {
                run_date: "2026-06-10".to_string(),
                agreement_rate: Some(0.7),
                panel_dispersion: Some(6.5),
                blind_divergence: Some(1.0),
                conviction_price_corr: Some(0.3),
                ..RunHealth::default()
            }),
            recommendation_scoreboard: vec![
                RecommendationScoreboardLine {
                    symbol: "GC=F".to_string(),
                    action_mix: "add×5 wait×2".to_string(),
                    n_scored_90d: 6,
                    pct_positive_90d: Some(33.3),
                    window_quality_delta_pct: Some(-4.2),
                },
                RecommendationScoreboardLine {
                    symbol: "BTC".to_string(),
                    action_mix: "hold×3".to_string(),
                    n_scored_90d: 3,
                    pct_positive_90d: Some(100.0),
                    window_quality_delta_pct: None,
                },
            ],
            ..BuildContext::default()
        };
        let out = render_private_epistemic_health(&ctx).unwrap();
        assert!(out.contains("### Recommendation scoreboard"));
        assert!(out.contains("| GC=F | add×5 wait×2 | 33% of 6 | -4.2pp ⚠ |"));
        assert!(out.contains("| BTC | hold×3 | 100% of 3 | accruing |"));
        // Conviction-price line below the table, clean at 0.3.
        assert!(out.contains("max |r| = 0.30 — clean"));
    }

    #[test]
    fn scoreboard_renders_even_without_run_health_row() {
        use crate::report::build::daily::RecommendationScoreboardLine;
        let ctx = BuildContext {
            recommendation_scoreboard: vec![RecommendationScoreboardLine {
                symbol: "SI=F".to_string(),
                action_mix: "wait×4".to_string(),
                n_scored_90d: 2,
                pct_positive_90d: Some(50.0),
                window_quality_delta_pct: None,
            }],
            ..BuildContext::default()
        };
        let out = render_private_epistemic_health(&ctx).unwrap();
        assert!(out.starts_with("## Epistemic Health"));
        assert!(out.contains("### Recommendation scoreboard"));
        // No metric table without a run_health row.
        assert!(!out.contains("| Agreement rate"));
    }
}
