use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

use super::charts::prob_bar::{self, ProbBarInput};
use super::charts::stacked_bar::{self, StackedBarInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ChartKind {
    StackedBar,
    ProbBar,
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
    StackedBar(StackedBarInput),
    #[serde(rename = "prob-bar")]
    ProbBar(ProbBarInput),
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
];

pub fn kind_from_name(name: &str) -> Result<ChartKind> {
    match name.trim().to_ascii_lowercase().as_str() {
        "stacked-bar" | "stacked_bar" | "stacked" | "allocation" => Ok(ChartKind::StackedBar),
        "prob-bar" | "prob_bar" | "probability" | "scenario-probability" => Ok(ChartKind::ProbBar),
        other => bail!(
            "unknown report chart '{}'. Available charts: {}",
            other,
            CHARTS.iter().map(|c| c.name).collect::<Vec<_>>().join(", ")
        ),
    }
}

pub fn parse_input(kind: ChartKind, value: Value) -> Result<ChartInput> {
    Ok(match kind {
        ChartKind::StackedBar => ChartInput::StackedBar(StackedBarInput::from_value(value)?),
        ChartKind::ProbBar => ChartInput::ProbBar(ProbBarInput::from_value(value)?),
    })
}

pub fn render_svg(input: &ChartInput) -> String {
    match input {
        ChartInput::StackedBar(input) => stacked_bar::render_svg(input),
        ChartInput::ProbBar(input) => prob_bar::render_svg(input),
    }
}

pub fn render_ascii(input: &ChartInput) -> String {
    match input {
        ChartInput::StackedBar(input) => stacked_bar::render_ascii(input),
        ChartInput::ProbBar(input) => prob_bar::render_ascii(input),
    }
}

pub fn chart_name(input: &ChartInput) -> &'static str {
    match input {
        ChartInput::StackedBar(_) => "stacked-bar",
        ChartInput::ProbBar(_) => "prob-bar",
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
        assert_eq!(
            kind_from_name("stacked-bar").unwrap(),
            ChartKind::StackedBar
        );
        assert_eq!(kind_from_name("probability").unwrap(), ChartKind::ProbBar);
    }
}
