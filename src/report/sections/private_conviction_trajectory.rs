#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, PrivateConvictionTrajectoryPoint, PrivateConvictionTrajectoryRow,
    PrivatePositionSnapshotRow,
};
use crate::report::charts::conviction_trajectory::{
    render_svg as conviction_trajectory_svg, ConvictionLayerSeries, ConvictionTrajectoryInput,
    ConvictionTrajectoryPoint as ChartTrajectoryPoint,
};

const HELD_ASSET_THRESHOLD_PCT: f64 = 1.0;
const LAYER_ORDER: [&str; 4] = ["LOW", "MEDIUM", "HIGH", "MACRO"];

pub fn render_private_conviction_trajectory(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Conviction Trajectory (30 days)\n\n");
    let held = qualifying_positions(&ctx.private_positions);
    if held.is_empty() {
        output.push_str("No held assets above 1% are attached to this private build.");
        return Ok(output);
    }

    for position in held {
        output.push_str(&render_asset_trajectory(
            &position.symbol,
            &ctx.private_conviction_trajectories,
        ));
        output.push_str("\n\n");
    }

    Ok(output.trim_end().to_string())
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

fn render_asset_trajectory(symbol: &str, rows: &[PrivateConvictionTrajectoryRow]) -> String {
    let layer_series: Vec<ConvictionLayerSeries> = LAYER_ORDER
        .iter()
        .map(|layer| {
            let series: Vec<ChartTrajectoryPoint> = rows
                .iter()
                .find(|row| {
                    row.symbol.eq_ignore_ascii_case(symbol)
                        && normalize_layer(&row.layer).eq_ignore_ascii_case(layer)
                })
                .map(|row| chart_points(&row.points))
                .unwrap_or_default();
            ConvictionLayerSeries {
                layer: layer_arg(layer).to_string(),
                series,
            }
        })
        .collect();

    conviction_trajectory_svg(&ConvictionTrajectoryInput {
        symbol: symbol.to_string(),
        layer_series,
        width: None,
        height: None,
    })
}

fn chart_points(points: &[PrivateConvictionTrajectoryPoint]) -> Vec<ChartTrajectoryPoint> {
    let mut sorted = points.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    sorted
        .into_iter()
        .map(|point| ChartTrajectoryPoint(point.date.clone(), point.conviction.clamp(-5, 5)))
        .collect()
}

fn normalize_layer(layer: &str) -> &'static str {
    match layer.to_ascii_uppercase().as_str() {
        "LOW" | "ANALYST-LOW" => "LOW",
        "MED" | "MEDIUM" | "ANALYST-MEDIUM" => "MEDIUM",
        "HIGH" | "ANALYST-HIGH" => "HIGH",
        "MACRO" | "ANALYST-MACRO" => "MACRO",
        _ => "UNKNOWN",
    }
}

fn layer_arg(layer: &str) -> &'static str {
    match layer {
        "LOW" => "LOW",
        "MEDIUM" => "MED",
        "HIGH" => "HIGH",
        "MACRO" => "MACRO",
        _ => "UNKNOWN",
    }
}

fn clean_arg(value: &str) -> String {
    value
        .replace(['|', ',', '[', ']', '{', '}'], " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_conviction_trajectory_sparse_series_render_without_panic() {
        let rendered = render_private_conviction_trajectory(&fixture_context()).unwrap();

        assert!(rendered.starts_with("## Conviction Trajectory (30 days)\n\n"));
        // SVG embedded inline; symbol labels appear in the SVG text nodes.
        assert!(rendered.contains(">GLD<"));
        assert!(
            !rendered.contains("{conviction_trajectory"),
            "must not leak token placeholder"
        );
    }

    #[test]
    fn private_conviction_trajectory_layers_stay_ordered() {
        let rendered = render_private_conviction_trajectory(&fixture_context()).unwrap();
        // BTC sparkline is rendered before GLD because BTC has higher allocation.
        let btc_pos = rendered.find(">BTC<").unwrap();
        let gld_pos = rendered.find(">GLD<").unwrap();
        assert!(btc_pos < gld_pos);
    }

    #[test]
    fn private_conviction_trajectory_includes_every_qualifying_held_asset() {
        let rendered = render_private_conviction_trajectory(&fixture_context()).unwrap();

        // Two SVG sparklines emitted: BTC and GLD; DOGE is dust and excluded.
        assert!(rendered.matches("<svg").count() >= 2);
        assert!(rendered.contains(">BTC<"));
        assert!(rendered.contains(">GLD<"));
        assert!(!rendered.contains(">DOGE<"));
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            private_positions: vec![
                position("BTC", 42.0),
                position("GLD", 22.95),
                position("DOGE", 0.05),
            ],
            private_conviction_trajectories: vec![
                trajectory(
                    "BTC",
                    "HIGH",
                    vec![point("2026-05-01", 2), point("2026-05-31", 4)],
                ),
                trajectory(
                    "BTC",
                    "LOW",
                    vec![point("2026-05-01", 1), point("2026-05-31", 3)],
                ),
                trajectory("BTC", "MEDIUM", vec![point("2026-05-31", 2)]),
                trajectory("BTC", "MACRO", vec![point("2026-05-31", 5)]),
                trajectory("GLD", "LOW", vec![point("2026-05-01", -2)]),
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

    fn trajectory(
        symbol: &str,
        layer: &str,
        points: Vec<PrivateConvictionTrajectoryPoint>,
    ) -> PrivateConvictionTrajectoryRow {
        PrivateConvictionTrajectoryRow {
            symbol: symbol.to_string(),
            layer: layer.to_string(),
            points,
        }
    }

    fn point(date: &str, conviction: i64) -> PrivateConvictionTrajectoryPoint {
        PrivateConvictionTrajectoryPoint {
            date: date.to_string(),
            conviction,
        }
    }
}
