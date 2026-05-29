use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

use super::charts::conviction_grid::{self, ConvictionGridInput};
use super::charts::decision_card::{self, DecisionCardInput};
use super::charts::drift_bar::{self, DriftBarInput};
use super::charts::factor_exposure::{self, FactorExposureInput};
use super::charts::mismatch_card::{self, MismatchCardInput};
use super::charts::open_predictions_table::{self, OpenPredictionsTableInput};
use super::charts::outlook_arrows::{self, OutlookArrowsInput};
use super::charts::prob_bar::{self, ProbBarInput};
use super::charts::regime_quadrant::{self, RegimeQuadrantInput};
use super::charts::stacked_bar::{self, StackedBarInput};
use super::charts::what_changed_strip::{self, WhatChangedStripInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ChartKind {
    Stacked,
    Probability,
    Drift,
    WhatChanged,
    OpenPredictions,
    OutlookArrows,
    FactorExposure,
    ConvictionGrid,
    MismatchCard,
    DecisionCard,
    RegimeQuadrant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ChartOutputFormat {
    Svg,
    Png,
    Ascii,
    Html,
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
    #[serde(rename = "open-predictions-table")]
    OpenPredictions(OpenPredictionsTableInput),
    #[serde(rename = "outlook-arrows")]
    OutlookArrows(OutlookArrowsInput),
    #[serde(rename = "factor-exposure")]
    FactorExposure(FactorExposureInput),
    #[serde(rename = "conviction-grid")]
    ConvictionGrid(ConvictionGridInput),
    #[serde(rename = "mismatch-card")]
    MismatchCard(MismatchCardInput),
    #[serde(rename = "decision-card")]
    DecisionCard(DecisionCardInput),
    #[serde(rename = "regime-quadrant")]
    RegimeQuadrant(RegimeQuadrantInput),
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
    ChartDefinition {
        name: "open-predictions-table",
        description: "Open prediction due-date table",
        formats: &["html", "ascii"],
    },
    ChartDefinition {
        name: "outlook-arrows",
        description: "Days/weeks/months direction and conviction arrows",
        formats: &["svg", "png", "ascii"],
    },
    ChartDefinition {
        name: "factor-exposure",
        description: "Portfolio exposure bars by scenario factor",
        formats: &["svg", "png", "ascii"],
    },
    ChartDefinition {
        name: "conviction-grid",
        description: "Multi-timeframe analyst conviction grid",
        formats: &["svg", "png", "ascii"],
    },
    ChartDefinition {
        name: "mismatch-card",
        description: "Skylar-vs-analyst view mismatch card",
        formats: &["html", "ascii"],
    },
    ChartDefinition {
        name: "decision-card",
        description: "Operator decision question card",
        formats: &["html", "ascii"],
    },
    ChartDefinition {
        name: "regime-quadrant",
        description: "Growth-vs-inflation macro regime quadrant",
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
        "open-predictions-table"
        | "open_predictions_table"
        | "open-predictions"
        | "predictions-table"
        | "predictions" => Ok(ChartKind::OpenPredictions),
        "outlook-arrows" | "outlook_arrows" | "outlook" | "arrows" => Ok(ChartKind::OutlookArrows),
        "factor-exposure" | "factor_exposure" | "factor" | "exposure" | "factors" => {
            Ok(ChartKind::FactorExposure)
        }
        "conviction-grid" | "conviction_grid" | "convictions" | "conviction" | "grid" => {
            Ok(ChartKind::ConvictionGrid)
        }
        "mismatch-card" | "mismatch_card" | "mismatch" | "view-mismatch" => {
            Ok(ChartKind::MismatchCard)
        }
        "decision-card" | "decision_card" | "decision" | "question-card" | "question" => {
            Ok(ChartKind::DecisionCard)
        }
        "regime-quadrant" | "regime_quadrant" | "regime" | "macro-regime" | "quadrant" => {
            Ok(ChartKind::RegimeQuadrant)
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
        ChartKind::OpenPredictions => {
            ChartInput::OpenPredictions(OpenPredictionsTableInput::from_value(value)?)
        }
        ChartKind::OutlookArrows => {
            ChartInput::OutlookArrows(OutlookArrowsInput::from_value(value)?)
        }
        ChartKind::FactorExposure => {
            ChartInput::FactorExposure(FactorExposureInput::from_value(value)?)
        }
        ChartKind::ConvictionGrid => {
            ChartInput::ConvictionGrid(ConvictionGridInput::from_value(value)?)
        }
        ChartKind::MismatchCard => ChartInput::MismatchCard(MismatchCardInput::from_value(value)?),
        ChartKind::DecisionCard => ChartInput::DecisionCard(DecisionCardInput::from_value(value)?),
        ChartKind::RegimeQuadrant => {
            ChartInput::RegimeQuadrant(RegimeQuadrantInput::from_value(value)?)
        }
    })
}

pub fn render_svg(input: &ChartInput) -> Result<String> {
    match input {
        ChartInput::Stacked(input) => Ok(stacked_bar::render_svg(input)),
        ChartInput::Probability(input) => Ok(prob_bar::render_svg(input)),
        ChartInput::Drift(input) => Ok(drift_bar::render_svg(input)),
        ChartInput::WhatChanged(input) => Ok(what_changed_strip::render_svg(input)),
        ChartInput::OpenPredictions(_) => {
            bail!("open-predictions-table is HTML-native; use --format html or --format ascii")
        }
        ChartInput::OutlookArrows(input) => Ok(outlook_arrows::render_svg(input)),
        ChartInput::FactorExposure(input) => Ok(factor_exposure::render_svg(input)),
        ChartInput::ConvictionGrid(input) => Ok(conviction_grid::render_svg(input)),
        ChartInput::MismatchCard(_) => {
            bail!("mismatch-card is HTML-native; use --format html or --format ascii")
        }
        ChartInput::DecisionCard(_) => {
            bail!("decision-card is HTML-native; use --format html or --format ascii")
        }
        ChartInput::RegimeQuadrant(input) => Ok(regime_quadrant::render_svg(input)),
    }
}

pub fn render_html(input: &ChartInput) -> Result<String> {
    match input {
        ChartInput::OpenPredictions(input) => Ok(open_predictions_table::render_html(input)),
        ChartInput::MismatchCard(input) => Ok(mismatch_card::render_html(input)),
        ChartInput::DecisionCard(input) => Ok(decision_card::render_html(input)),
        _ => bail!(
            "{} does not support HTML output; supported formats: {}",
            chart_name(input),
            supported_formats(input).join(", ")
        ),
    }
}

pub fn render_ascii(input: &ChartInput) -> String {
    match input {
        ChartInput::Stacked(input) => stacked_bar::render_ascii(input),
        ChartInput::Probability(input) => prob_bar::render_ascii(input),
        ChartInput::Drift(input) => drift_bar::render_ascii(input),
        ChartInput::WhatChanged(input) => what_changed_strip::render_ascii(input),
        ChartInput::OpenPredictions(input) => open_predictions_table::render_ascii(input),
        ChartInput::OutlookArrows(input) => outlook_arrows::render_ascii(input),
        ChartInput::FactorExposure(input) => factor_exposure::render_ascii(input),
        ChartInput::ConvictionGrid(input) => conviction_grid::render_ascii(input),
        ChartInput::MismatchCard(input) => mismatch_card::render_ascii(input),
        ChartInput::DecisionCard(input) => decision_card::render_ascii(input),
        ChartInput::RegimeQuadrant(input) => regime_quadrant::render_ascii(input),
    }
}

pub fn chart_name(input: &ChartInput) -> &'static str {
    match input {
        ChartInput::Stacked(_) => "stacked-bar",
        ChartInput::Probability(_) => "prob-bar",
        ChartInput::Drift(_) => "drift-bar",
        ChartInput::WhatChanged(_) => "what-changed-strip",
        ChartInput::OpenPredictions(_) => "open-predictions-table",
        ChartInput::OutlookArrows(_) => "outlook-arrows",
        ChartInput::FactorExposure(_) => "factor-exposure",
        ChartInput::ConvictionGrid(_) => "conviction-grid",
        ChartInput::MismatchCard(_) => "mismatch-card",
        ChartInput::DecisionCard(_) => "decision-card",
        ChartInput::RegimeQuadrant(_) => "regime-quadrant",
    }
}

pub fn supported_formats(input: &ChartInput) -> &'static [&'static str] {
    match input {
        ChartInput::Stacked(_) => &["svg", "png", "ascii"],
        ChartInput::Probability(_) => &["svg", "png", "ascii"],
        ChartInput::Drift(_) => &["svg", "png", "ascii"],
        ChartInput::WhatChanged(_) => &["svg", "png", "ascii"],
        ChartInput::OpenPredictions(_) => &["html", "ascii"],
        ChartInput::OutlookArrows(_) => &["svg", "png", "ascii"],
        ChartInput::FactorExposure(_) => &["svg", "png", "ascii"],
        ChartInput::ConvictionGrid(_) => &["svg", "png", "ascii"],
        ChartInput::MismatchCard(_) => &["html", "ascii"],
        ChartInput::DecisionCard(_) => &["html", "ascii"],
        ChartInput::RegimeQuadrant(_) => &["svg", "png", "ascii"],
    }
}

pub fn content_type(format: ChartOutputFormat) -> &'static str {
    match format {
        ChartOutputFormat::Svg => "image/svg+xml",
        ChartOutputFormat::Png => "image/png",
        ChartOutputFormat::Ascii => "text/plain",
        ChartOutputFormat::Html => "text/html; charset=utf-8",
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
        assert_eq!(
            kind_from_name("open_predictions_table").unwrap(),
            ChartKind::OpenPredictions
        );
        assert_eq!(
            kind_from_name("outlook_arrows").unwrap(),
            ChartKind::OutlookArrows
        );
        assert_eq!(
            kind_from_name("factor_exposure").unwrap(),
            ChartKind::FactorExposure
        );
        assert_eq!(
            kind_from_name("conviction_grid").unwrap(),
            ChartKind::ConvictionGrid
        );
        assert_eq!(
            kind_from_name("mismatch_card").unwrap(),
            ChartKind::MismatchCard
        );
        assert_eq!(
            kind_from_name("decision_card").unwrap(),
            ChartKind::DecisionCard
        );
        assert_eq!(
            kind_from_name("regime_quadrant").unwrap(),
            ChartKind::RegimeQuadrant
        );
    }
}
