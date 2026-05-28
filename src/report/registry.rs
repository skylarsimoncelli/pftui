use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

use super::charts::drift_bar::{self, DriftBarInput};
use super::charts::prob_bar::{self, ProbBarInput};
use super::charts::stacked_bar::{self, StackedBarInput};
use super::charts::what_changed_strip::{self, WhatChangedStripInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ChartKind {
    Stacked,
    Probability,
    Drift,
    WhatChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ChartOutputFormat {
    Svg,
    Png,
    Ascii,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChartDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub formats: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "chart", content = "input")]
pub enum ChartInput {
    #[serde(rename = "stacked-bar")]
    Stacked(StackedBarInput),
    #[serde(rename = "prob-bar")]
    Probability(ProbBarInput),
    #[serde(rename = "drift-bar")]
    Drift(DriftBarInput),
    #[serde(rename = "what-changed-strip")]
    WhatChanged(WhatChangedStripInput),
}

pub const CHARTS: &[ChartDefinition] = &[
    ChartDefinition {
        name: "stacked-bar",
        description: "Portfolio allocation stacked bar",
        formats: &["svg", "png", "ascii"],
    },
    ChartDefinition {
        name: "prob-bar",
        description: "Scenario probability bar with 7-day ghost and delta",
        formats: &["svg", "png", "ascii"],
    },
    ChartDefinition {
        name: "drift-bar",
        description: "Allocation drift bar with target tick and tolerance band",
        formats: &["svg", "png", "ascii"],
    },
    ChartDefinition {
        name: "what-changed-strip",
        description: "Since-last-report delta pill strip",
        formats: &["svg", "png", "ascii"],
    },
];

pub fn kind_from_name(name: &str) -> Result<ChartKind> {
    match name.trim().to_ascii_lowercase().as_str() {
        "stacked-bar" | "stacked_bar" | "stacked" | "allocation" => Ok(ChartKind::Stacked),
        "prob-bar" | "prob_bar" | "probability" | "scenario-probability" => {
            Ok(ChartKind::Probability)
        }
        "drift-bar" | "drift_bar" | "drift" | "allocation-drift" => Ok(ChartKind::Drift),
        "what-changed-strip" | "what_changed_strip" | "what-changed" | "changes" => {
            Ok(ChartKind::WhatChanged)
        }
        other => bail!(
            "unknown report chart '{}'. Available charts: {}",
            other,
            CHARTS.iter().map(|c| c.name).collect::<Vec<_>>().join(", ")
        ),
    }
}

pub fn parse_input(kind: ChartKind, value: Value) -> Result<ChartInput> {
    Ok(match kind {
        ChartKind::Stacked => ChartInput::Stacked(StackedBarInput::from_value(value)?),
        ChartKind::Probability => ChartInput::Probability(ProbBarInput::from_value(value)?),
        ChartKind::Drift => ChartInput::Drift(DriftBarInput::from_value(value)?),
        ChartKind::WhatChanged => {
            ChartInput::WhatChanged(WhatChangedStripInput::from_value(value)?)
        }
    })
}

pub fn render_svg(input: &ChartInput) -> String {
    match input {
        ChartInput::Stacked(input) => stacked_bar::render_svg(input),
        ChartInput::Probability(input) => prob_bar::render_svg(input),
        ChartInput::Drift(input) => drift_bar::render_svg(input),
        ChartInput::WhatChanged(input) => what_changed_strip::render_svg(input),
    }
}

pub fn render_ascii(input: &ChartInput) -> String {
    match input {
        ChartInput::Stacked(input) => stacked_bar::render_ascii(input),
        ChartInput::Probability(input) => prob_bar::render_ascii(input),
        ChartInput::Drift(input) => drift_bar::render_ascii(input),
        ChartInput::WhatChanged(input) => what_changed_strip::render_ascii(input),
    }
}

pub fn chart_name(input: &ChartInput) -> &'static str {
    match input {
        ChartInput::Stacked(_) => "stacked-bar",
        ChartInput::Probability(_) => "prob-bar",
        ChartInput::Drift(_) => "drift-bar",
        ChartInput::WhatChanged(_) => "what-changed-strip",
    }
}

pub fn content_type(format: ChartOutputFormat) -> &'static str {
    match format {
        ChartOutputFormat::Svg => "image/svg+xml",
        ChartOutputFormat::Png => "image/png",
        ChartOutputFormat::Ascii => "text/plain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_maps_chart_aliases() {
        assert_eq!(kind_from_name("stacked-bar").unwrap(), ChartKind::Stacked);
        assert_eq!(
            kind_from_name("probability").unwrap(),
            ChartKind::Probability
        );
        assert_eq!(
            kind_from_name("allocation-drift").unwrap(),
            ChartKind::Drift
        );
        assert_eq!(
            kind_from_name("what_changed_strip").unwrap(),
            ChartKind::WhatChanged
        );
    }
}
