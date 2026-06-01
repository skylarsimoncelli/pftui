#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, PrivateConvictionTrajectoryPoint, PrivateConvictionTrajectoryRow,
    PrivatePositionSnapshotRow,
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
    let layer_args = LAYER_ORDER
        .iter()
        .map(|layer| {
            let points = rows
                .iter()
                .find(|row| {
                    row.symbol.eq_ignore_ascii_case(symbol)
                        && normalize_layer(&row.layer).eq_ignore_ascii_case(layer)
                })
                .map(|row| row.points.as_slice())
                .unwrap_or_default();
            format!("{}:{}", layer_arg(layer), format_points(points))
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "{{conviction_trajectory({}, layer_series=[{}])}}",
        clean_arg(symbol),
        layer_args
    )
}

fn format_points(points: &[PrivateConvictionTrajectoryPoint]) -> String {
    if points.is_empty() {
        return "[]".to_string();
    }

    let mut sorted = points.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    let joined = sorted
        .into_iter()
        .map(|point| {
            format!(
                "({}, {:+})",
                clean_arg(&point.date),
                point.conviction.clamp(-5, 5)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{joined}]")
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
        assert!(rendered.contains(
            "{conviction_trajectory(GLD, layer_series=[LOW:[(2026-05-01, -2)], MED:[], HIGH:[], MACRO:[]])}"
        ));
    }

    #[test]
    fn private_conviction_trajectory_layers_stay_ordered() {
        let rendered = render_private_conviction_trajectory(&fixture_context()).unwrap();
        let btc = rendered
            .lines()
            .find(|line| line.contains("conviction_trajectory(BTC"))
            .unwrap();

        assert!(btc.find("LOW:[").unwrap() < btc.find("MED:[").unwrap());
        assert!(btc.find("MED:[").unwrap() < btc.find("HIGH:[").unwrap());
        assert!(btc.find("HIGH:[").unwrap() < btc.find("MACRO:[").unwrap());
    }

    #[test]
    fn private_conviction_trajectory_includes_every_qualifying_held_asset() {
        let rendered = render_private_conviction_trajectory(&fixture_context()).unwrap();

        assert_eq!(rendered.matches("{conviction_trajectory(").count(), 2);
        assert!(rendered.contains("conviction_trajectory(BTC"));
        assert!(rendered.contains("conviction_trajectory(GLD"));
        assert!(!rendered.contains("conviction_trajectory(DOGE"));
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
