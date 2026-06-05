#![allow(dead_code)]

use anyhow::Result;

use crate::db::analyst_views::classify_convergence;
use crate::report::build::daily::{
    AssetIntelligenceBlob, BuildContext, PrivateAssetConvergenceRow, PrivateAssetConvergenceView,
    PrivatePositionSnapshotRow,
};
use crate::report::charts::analyst_convergence_card::{
    render_html as analyst_convergence_card_html, AnalystConvergenceCardInput,
    AnalystConvergenceView as ChartConvergenceView,
};

const HELD_ASSET_THRESHOLD_PCT: f64 = 1.0;

pub fn render_private_per_asset_convergence(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Per-Asset Convergence\n\n");
    let held = qualifying_positions(&ctx.private_positions);
    if held.is_empty() {
        output.push_str("No held assets above 1% are attached to this private build.");
        return Ok(output);
    }

    for position in held {
        let convergence = find_convergence(&ctx.private_asset_convergence, &position.symbol);
        output.push_str(&render_asset_card(position, convergence));
        if let Some(blob) = lookup_intelligence(ctx, &position.symbol) {
            output.push_str("\n\n");
            output.push_str(&render_deeper_analysis(blob));
        }
        output.push_str("\n\n");
    }

    Ok(output.trim_end().to_string())
}

fn lookup_intelligence<'a>(
    ctx: &'a BuildContext,
    symbol: &str,
) -> Option<&'a AssetIntelligenceBlob> {
    let upper = symbol.to_uppercase();
    ctx.private_asset_intelligence
        .get(&upper)
        .or_else(|| ctx.private_asset_intelligence.get(symbol))
}

fn render_deeper_analysis(blob: &AssetIntelligenceBlob) -> String {
    let mut bullets: Vec<String> = Vec::new();

    if blob.spot_price.is_some() || blob.daily_change_pct.is_some() {
        let mut parts = Vec::new();
        if let Some(p) = blob.spot_price.as_deref() {
            parts.push(format!("Spot {p}"));
        }
        if let Some(d) = blob.daily_change_pct {
            parts.push(format!("daily {d:+.2}%"));
        }
        if !parts.is_empty() {
            bullets.push(format!("Price action: {}", parts.join(" · ")));
        }
    }

    let mut levels = Vec::new();
    if let Some(s) = blob.nearest_support.as_deref() {
        levels.push(format!("support {s}"));
    }
    if let Some(r) = blob.nearest_resistance.as_deref() {
        levels.push(format!("resistance {r}"));
    }
    if !levels.is_empty() {
        bullets.push(format!("Key levels: {}", levels.join(" · ")));
    }

    if let Some(rsi) = blob.rsi_14 {
        let label = blob.rsi_signal.as_deref().unwrap_or("neutral");
        bullets.push(format!("RSI(14) {rsi:.1} ({label})"));
    }

    if let Some(t) = blob.trend.as_deref() {
        bullets.push(format!("Trend: {t}"));
    }

    if let Some(pos) = blob.range_52w_position {
        bullets.push(format!("52w range position: {:.1}%", pos * 100.0));
    }

    if blob.scenario_count > 0 || blob.open_predictions_count > 0 {
        bullets.push(format!(
            "Scenario alignment: {} active scenario(s) · {} open prediction(s)",
            blob.scenario_count, blob.open_predictions_count,
        ));
    }

    if let Some(ctx) = blob.structural_context.as_deref() {
        bullets.push(format!("Structural context: {ctx}"));
    }

    if bullets.is_empty() {
        return format!(
            "**Deeper Analysis — {}**\n\nNo synthesised intelligence is cached for this symbol yet.",
            clean_text(&blob.symbol)
        );
    }

    // Cap at five bullets per the spec.
    bullets.truncate(5);

    let mut out = format!("**Deeper Analysis — {}**\n\n", clean_text(&blob.symbol));
    for b in bullets {
        out.push_str("- ");
        out.push_str(&b);
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn qualifying_positions(rows: &[PrivatePositionSnapshotRow]) -> Vec<&PrivatePositionSnapshotRow> {
    let mut held = rows
        .iter()
        .filter(|row| row.allocation_pct >= HELD_ASSET_THRESHOLD_PCT)
        .collect::<Vec<_>>();
    held.sort_by(|a, b| {
        b.allocation_pct
            .partial_cmp(&a.allocation_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    held
}

fn find_convergence<'a>(
    rows: &'a [PrivateAssetConvergenceRow],
    symbol: &str,
) -> Option<&'a PrivateAssetConvergenceRow> {
    rows.iter()
        .find(|row| row.symbol.eq_ignore_ascii_case(symbol))
}

fn render_asset_card(
    position: &PrivatePositionSnapshotRow,
    convergence: Option<&PrivateAssetConvergenceRow>,
) -> String {
    let views = convergence
        .map(|row| row.views.as_slice())
        .unwrap_or_default();
    let summary = convergence_summary(views);
    let range = convergence
        .and_then(|row| row.target_pct)
        .map(|target| analyst_range(position.allocation_pct, target, average_conviction(views)));
    let target = convergence.and_then(|row| row.target_pct);
    let missing = missing_layers(views);

    let chart_views = layer_sorted_chart_views(views);
    let analyst_range = range.map(|(low, high)| [low, high]);
    let mut output = analyst_convergence_card_html(&AnalystConvergenceCardInput {
        asset: position.symbol.clone(),
        views: chart_views,
        user_target: target,
        current_alloc: Some(position.allocation_pct),
        analyst_range,
        summary: summary.to_string(),
        width: None,
    });
    if !missing.is_empty() {
        output.push_str(&format!(
            "\nMissing analyst layers for {}: {}. Summary remains insufficient-views until at least two layers are attached.",
            clean_text(&position.symbol),
            missing.join(", ")
        ));
    }
    output
}

fn layer_sorted_chart_views(views: &[PrivateAssetConvergenceView]) -> Vec<ChartConvergenceView> {
    let mut sorted = views.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        layer_order(&a.analyst)
            .cmp(&layer_order(&b.analyst))
            .then_with(|| a.analyst.cmp(&b.analyst))
    });
    sorted
        .into_iter()
        .map(|view| ChartConvergenceView {
            analyst: view.analyst.clone(),
            conviction: view.conviction,
            reasoning_summary: view.reasoning_summary.clone(),
        })
        .collect()
}

fn convergence_summary(views: &[PrivateAssetConvergenceView]) -> &'static str {
    let n_views = views.len();
    let avg = average_conviction(views);
    let max_divergence = conviction_divergence(views);
    classify_convergence(n_views, avg, max_divergence)
}

fn average_conviction(views: &[PrivateAssetConvergenceView]) -> f64 {
    if views.is_empty() {
        return 0.0;
    }
    views.iter().map(|view| view.conviction).sum::<i64>() as f64 / views.len() as f64
}

fn conviction_divergence(views: &[PrivateAssetConvergenceView]) -> i64 {
    let Some(min) = views.iter().map(|view| view.conviction).min() else {
        return 0;
    };
    let max = views
        .iter()
        .map(|view| view.conviction)
        .max()
        .unwrap_or(min);
    max - min
}

fn analyst_range(current_alloc_pct: f64, target_pct: f64, avg_conviction: f64) -> (f64, f64) {
    let conviction_tilt = avg_conviction * 1.25;
    let center = (target_pct + conviction_tilt).clamp(0.0, 100.0);
    let half_width = (3.0 + (current_alloc_pct - target_pct).abs() * 0.10).clamp(2.0, 6.0);
    (
        (center - half_width).max(0.0),
        (center + half_width).min(100.0),
    )
}

#[allow(dead_code)]
fn render_view_args(views: &[PrivateAssetConvergenceView]) -> String {
    // Legacy formatter retained for any caller that still needs the
    // pre-substitution token-arg representation. The chart layer now
    // consumes the typed view list directly.
    if views.is_empty() {
        return "[]".to_string();
    }
    let mut sorted = views.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        layer_order(&a.analyst)
            .cmp(&layer_order(&b.analyst))
            .then_with(|| a.analyst.cmp(&b.analyst))
    });
    let joined = sorted
        .into_iter()
        .map(|view| {
            format!(
                "{}:{:+}:{}",
                clean_arg(&view.analyst),
                view.conviction,
                clean_arg(&view.reasoning_summary)
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("[{joined}]")
}

fn missing_layers(views: &[PrivateAssetConvergenceView]) -> Vec<&'static str> {
    ["LOW", "MEDIUM", "HIGH", "MACRO"]
        .into_iter()
        .filter(|layer| {
            !views
                .iter()
                .any(|view| view.analyst.eq_ignore_ascii_case(layer))
        })
        .collect()
}

fn layer_order(layer: &str) -> usize {
    match layer.to_ascii_uppercase().as_str() {
        "LOW" | "ANALYST-LOW" => 0,
        "MEDIUM" | "ANALYST-MEDIUM" => 1,
        "HIGH" | "ANALYST-HIGH" => 2,
        "MACRO" | "ANALYST-MACRO" => 3,
        _ => 4,
    }
}

fn format_number(value: f64) -> String {
    format!("{value:.2}")
}

fn clean_text(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

fn clean_arg(value: &str) -> String {
    clean_text(value).replace([',', '[', ']', '{', '}'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_per_asset_convergence_surfaces_missing_layers() {
        let rendered = render_private_per_asset_convergence(&fixture_context()).unwrap();

        assert!(rendered.starts_with("## Per-Asset Convergence\n\n"));
        // GLD card present as HTML (asset name appears in the card markup) with
        // the missing-layers annotation below.
        assert!(rendered.contains("GLD"));
        assert!(rendered.contains("insufficient-views"));
        assert!(rendered.contains("Missing analyst layers for GLD: MEDIUM, HIGH, MACRO"));
        assert!(
            !rendered.contains("{analyst_convergence_card"),
            "must not leak token placeholder"
        );
    }

    #[test]
    fn private_per_asset_convergence_derived_ranges_follow_formula() {
        let rendered = render_private_per_asset_convergence(&fixture_context()).unwrap();

        // Range bounds appear in the HTML card output. BTC range 40.86–47.26
        // renders as "40.9–47.3%" (one-decimal precision). GLD insufficient-views
        // case still shows the range but with INSUFFICIENT VIEWS badge.
        assert!(rendered.contains("STRONG BULL"));
        assert!(rendered.contains("INSUFFICIENT VIEWS"));
        assert!(rendered.contains("40.9"));
        assert!(rendered.contains("47.3"));
        assert!(rendered.contains("18.1") || rendered.contains("18.0"));
        assert!(rendered.contains("24.5") || rendered.contains("24.4"));
    }

    #[test]
    fn private_per_asset_convergence_card_count_matches_held_assets_above_threshold() {
        let rendered = render_private_per_asset_convergence(&fixture_context()).unwrap();

        // Two qualifying assets ⇒ two HTML cards rendered (DOGE dust is excluded).
        assert!(rendered.matches("<table").count() >= 2);
        assert!(rendered.contains("BTC"));
        assert!(rendered.contains("GLD"));
        assert!(!rendered.contains("DOGE"));
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            private_positions: vec![
                position("BTC", 42.0),
                position("GLD", 22.95),
                position("DOGE", 0.05),
            ],
            private_asset_convergence: vec![
                convergence(
                    "BTC",
                    Some(40.0),
                    vec![
                        view("LOW", 3, "spot momentum confirms risk bid"),
                        view("MEDIUM", 3, "ETF flow trend remains supportive"),
                        view("HIGH", 4, "halving supply pressure is constructive"),
                        view("MACRO", 3, "debasement thesis still supports allocation"),
                    ],
                ),
                convergence(
                    "GLD",
                    Some(25.0),
                    vec![view("LOW", -3, "real yields are a short-term headwind")],
                ),
            ],
            ..BuildContext::default()
        }
    }

    fn position(symbol: &str, allocation_pct: f64) -> PrivatePositionSnapshotRow {
        PrivatePositionSnapshotRow {
            symbol: symbol.to_string(),
            price: None,
            daily_change: None,
            allocation_pct,
            unrealized_pnl: None,
        }
    }

    fn convergence(
        symbol: &str,
        target_pct: Option<f64>,
        views: Vec<PrivateAssetConvergenceView>,
    ) -> PrivateAssetConvergenceRow {
        PrivateAssetConvergenceRow {
            symbol: symbol.to_string(),
            target_pct,
            views,
        }
    }

    fn view(
        analyst: &str,
        conviction: i64,
        reasoning_summary: &str,
    ) -> PrivateAssetConvergenceView {
        PrivateAssetConvergenceView {
            analyst: analyst.to_string(),
            conviction,
            reasoning_summary: reasoning_summary.to_string(),
        }
    }

    #[test]
    fn appends_deeper_analysis_block_when_intelligence_present() {
        let mut ctx = fixture_context();
        let mut blob = AssetIntelligenceBlob {
            symbol: "BTC".to_string(),
            spot_price: Some("$67,100.00".to_string()),
            daily_change_pct: Some(1.25),
            rsi_14: Some(52.3),
            rsi_signal: Some("neutral".to_string()),
            trend: Some("above 50/200 SMA (uptrend)".to_string()),
            nearest_support: Some("$65,000.00".to_string()),
            nearest_resistance: Some("$70,000.00".to_string()),
            range_52w_position: Some(0.78),
            scenario_count: 3,
            open_predictions_count: 4,
            structural_context: Some("Above both 50/200 SMA — structural uptrend".to_string()),
        };
        blob.symbol = "BTC".to_string();
        ctx.private_asset_intelligence
            .insert("BTC".to_string(), blob);

        let rendered = render_private_per_asset_convergence(&ctx).unwrap();

        assert!(rendered.contains("**Deeper Analysis — BTC**"));
        assert!(rendered.contains("Price action: Spot $67,100.00"));
        assert!(rendered.contains("daily +1.25%"));
        assert!(rendered.contains("Key levels: support $65,000.00"));
        assert!(rendered.contains("RSI(14) 52.3"));
        // Bullets cap at 5 per spec — the leading bullets win. Confirm the
        // first five are present.
        assert!(rendered.contains("Trend: above 50/200 SMA (uptrend)"));
        assert!(rendered.contains("52w range position: 78.0%"));
    }

    #[test]
    fn deeper_analysis_skipped_when_no_intelligence_for_symbol() {
        let rendered = render_private_per_asset_convergence(&fixture_context()).unwrap();
        // No intelligence cached → no Deeper Analysis sub-section emitted.
        assert!(!rendered.contains("**Deeper Analysis"));
    }

    #[test]
    fn deeper_analysis_caps_at_five_bullets() {
        let mut ctx = fixture_context();
        let blob = AssetIntelligenceBlob {
            symbol: "BTC".to_string(),
            spot_price: Some("$67k".to_string()),
            daily_change_pct: Some(2.0),
            rsi_14: Some(60.0),
            rsi_signal: Some("neutral".to_string()),
            trend: Some("above 50/200 SMA".to_string()),
            nearest_support: Some("$60k".to_string()),
            nearest_resistance: Some("$70k".to_string()),
            range_52w_position: Some(0.5),
            scenario_count: 2,
            open_predictions_count: 1,
            structural_context: Some("structural uptrend".to_string()),
        };
        ctx.private_asset_intelligence
            .insert("BTC".to_string(), blob);
        let rendered = render_private_per_asset_convergence(&ctx).unwrap();
        // Find the Deeper Analysis section and count its bullet lines.
        let start = rendered.find("**Deeper Analysis — BTC**").unwrap();
        let block = &rendered[start..];
        let bullet_count = block.lines().filter(|l| l.starts_with("- ")).count();
        assert!(bullet_count <= 5, "expected ≤5 bullets, got {bullet_count}");
    }
}
