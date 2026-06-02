use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use serde_json::Value;

use crate::cli::{ReportBuildMode, ReportChartFormat};
use crate::report::build::daily::{
    self as build_daily, assemble, plan_assembly, render_dry_run, resolve_report_date, BuildContext,
    BuildMode,
};
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations_backend;
use crate::db::analyst_views;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::scenarios;
use crate::db::transactions::list_transactions_backend;
use crate::db::user_predictions;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::report::charts::analyst_convergence_card::{
    AnalystConvergenceCardInput, AnalystConvergenceView,
};
use crate::report::charts::calibration_reliability::{
    CalibrationReliabilityBin, CalibrationReliabilityInput, CalibrationReliabilityLayer,
};
use crate::report::charts::conviction_grid::{ConvictionGridInput, ConvictionGridRow};
use crate::report::charts::conviction_trajectory::{
    ConvictionLayerSeries, ConvictionTrajectoryInput, ConvictionTrajectoryPoint,
};
use crate::report::charts::drift_bar::DriftBarInput;
use crate::report::charts::open_predictions_table::{OpenPredictionRow, OpenPredictionsTableInput};
use crate::report::charts::outlook_arrows::{OutlookArrowsInput, OutlookPoint};
use crate::report::charts::prob_bar::ProbBarInput;
use crate::report::charts::stacked_bar::{StackedBarInput, StackedBarSegment};
use crate::report::palette;
use crate::report::registry::{self, ChartInput, ChartKind, ChartOutputFormat};

#[derive(Debug, Serialize)]
struct ReportChartOutput {
    chart: &'static str,
    format: &'static str,
    content_type: &'static str,
    output: Option<String>,
    bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_base64: Option<String>,
}

pub struct ReportChartOptions<'a> {
    pub chart_name: &'a str,
    pub from_db: Option<&'a str>,
    pub from_json: Option<&'a Path>,
    pub out: Option<&'a Path>,
    pub format: ReportChartFormat,
    pub json_output: bool,
}

pub struct BuildDailyOptions<'a> {
    pub mode: ReportBuildMode,
    pub date: Option<&'a str>,
    pub out_dir: Option<&'a Path>,
    pub dry_run: bool,
    pub json: bool,
}

fn report_build_mode_from_cli(mode: ReportBuildMode) -> BuildMode {
    match mode {
        ReportBuildMode::Public => BuildMode::Public,
        ReportBuildMode::Private => BuildMode::Private,
        ReportBuildMode::Both => BuildMode::Both,
    }
}

#[derive(serde::Serialize)]
struct BuildDailyDryRunJson<'a> {
    mode: &'a str,
    date: &'a str,
    section_plan: Vec<BuildDailyPlanRowJson>,
    data_availability: Vec<BuildDailyDataRowJson>,
    output_paths: Vec<String>,
    privacy_audit_status: &'a str,
    dry_run: bool,
}

#[derive(serde::Serialize)]
struct BuildDailyPlanRowJson {
    name: &'static str,
    visibility: &'static str,
}

#[derive(serde::Serialize)]
struct BuildDailyDataRowJson {
    field: &'static str,
    populated: bool,
}

#[derive(serde::Serialize)]
struct BuildDailyOutcomeJson<'a> {
    mode: &'a str,
    date: &'a str,
    public_written: Option<String>,
    private_written: Option<String>,
    bytes_written: usize,
    dry_run: bool,
}

/// Dispatcher for `pftui report build daily`.
pub fn run_build_daily(
    backend: &BackendConnection,
    options: BuildDailyOptions<'_>,
) -> Result<()> {
    let mode = report_build_mode_from_cli(options.mode);
    let date = resolve_report_date(options.date);
    let ctx = BuildContext::load(backend, &date)?;

    let (public_out_dir, private_out_dir) = match options.out_dir {
        Some(dir) => (Some(dir), Some(dir)),
        None => (None, None),
    };

    if options.dry_run {
        let summary = render_dry_run(&ctx, mode, &date, public_out_dir, private_out_dir);
        if options.json {
            let plan_rows: Vec<BuildDailyPlanRowJson> = summary
                .plan
                .iter()
                .map(|spec| BuildDailyPlanRowJson {
                    name: spec.name,
                    visibility: match spec.visibility {
                        build_daily::SectionVisibility::Public => "public",
                        build_daily::SectionVisibility::Private => "private",
                    },
                })
                .collect();
            let data_rows: Vec<BuildDailyDataRowJson> = summary
                .data_availability
                .iter()
                .map(|row| BuildDailyDataRowJson {
                    field: row.field,
                    populated: row.populated,
                })
                .collect();
            let output_paths: Vec<String> = summary
                .output_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            let payload = BuildDailyDryRunJson {
                mode: mode.as_str(),
                date: &date,
                section_plan: plan_rows,
                data_availability: data_rows,
                output_paths,
                privacy_audit_status: summary.privacy_audit_status.as_str(),
                dry_run: true,
            };
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            print!("{}", summary.render_text());
        }
        return Ok(());
    }

    let outcome = assemble(&ctx, mode, &date, public_out_dir, private_out_dir)?;

    if options.json {
        let payload = BuildDailyOutcomeJson {
            mode: mode.as_str(),
            date: &date,
            public_written: outcome
                .public_written
                .as_ref()
                .map(|p| p.display().to_string()),
            private_written: outcome
                .private_written
                .as_ref()
                .map(|p| p.display().to_string()),
            bytes_written: outcome.bytes_written,
            dry_run: false,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        let plan = plan_assembly(mode, &date, public_out_dir, private_out_dir);
        println!(
            "pftui report build daily --mode {} --date {}",
            mode.as_str(),
            date
        );
        println!("  sections rendered: {}", plan.sections.len());
        if let Some(path) = outcome.public_written.as_ref() {
            println!("  wrote public: {}", path.display());
        }
        if let Some(path) = outcome.private_written.as_ref() {
            println!("  wrote private: {}", path.display());
        }
        println!("  bytes written: {}", outcome.bytes_written);
    }
    Ok(())
}

pub fn run_chart(
    backend: &BackendConnection,
    config: &Config,
    options: ReportChartOptions<'_>,
) -> Result<()> {
    let kind = registry::kind_from_name(options.chart_name)?;
    let input = load_chart_input(backend, config, kind, options.from_db, options.from_json)?;
    emit_chart(input, options.out, options.format, options.json_output)
}

pub fn run_chart_without_db(options: ReportChartOptions<'_>) -> Result<()> {
    let kind = registry::kind_from_name(options.chart_name)?;
    let path = options
        .from_json
        .context("report chart requires --from-json when --from-db is absent")?;
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse JSON from {}", path.display()))?;
    let input = registry::parse_input(kind, value)?;
    emit_chart(input, options.out, options.format, options.json_output)
}

fn emit_chart(
    input: ChartInput,
    out: Option<&Path>,
    format: ReportChartFormat,
    json_output: bool,
) -> Result<()> {
    let output_format = match format {
        ReportChartFormat::Svg => ChartOutputFormat::Svg,
        ReportChartFormat::Png => ChartOutputFormat::Png,
        ReportChartFormat::Ascii => ChartOutputFormat::Ascii,
        ReportChartFormat::Html => ChartOutputFormat::Html,
    };
    let (bytes, text_content) = render_chart(&input, output_format)?;

    if let Some(path) = out {
        fs::write(path, &bytes).with_context(|| format!("failed to write {}", path.display()))?;
    } else if !json_output {
        io::stdout().write_all(&bytes)?;
    }

    if json_output {
        let text = match output_format {
            ChartOutputFormat::Svg | ChartOutputFormat::Ascii | ChartOutputFormat::Html
                if out.is_none() =>
            {
                text_content
            }
            _ => None,
        };
        let content_base64 = if output_format == ChartOutputFormat::Png && out.is_none() {
            Some(general_purpose::STANDARD.encode(&bytes))
        } else {
            None
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&ReportChartOutput {
                chart: registry::chart_name(&input),
                format: report_format_name(output_format),
                content_type: registry::content_type(output_format),
                output: out.map(|path| path.display().to_string()),
                bytes: bytes.len(),
                content: text,
                content_base64,
            })?
        );
    } else if let Some(path) = out {
        println!(
            "Wrote {} chart to {}",
            report_format_name(output_format),
            path.display()
        );
    }

    Ok(())
}

fn load_chart_input(
    backend: &BackendConnection,
    config: &Config,
    kind: ChartKind,
    from_db: Option<&str>,
    from_json: Option<&Path>,
) -> Result<ChartInput> {
    match (from_db, from_json) {
        (Some(_), Some(_)) => bail!("use either --from-db or --from-json, not both"),
        (None, None) => bail!("report chart requires --from-db or --from-json input"),
        (None, Some(path)) => {
            let raw = fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let value: Value = serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse JSON from {}", path.display()))?;
            registry::parse_input(kind, value)
        }
        (Some(query), None) => chart_input_from_db(backend, config, kind, query),
    }
}

fn chart_input_from_db(
    backend: &BackendConnection,
    config: &Config,
    kind: ChartKind,
    query: &str,
) -> Result<ChartInput> {
    match kind {
        ChartKind::Stacked => {
            let normalized = query.trim().to_ascii_lowercase();
            if !matches!(
                normalized.as_str(),
                "portfolio" | "portfolio-status" | "status" | "allocation" | "allocations"
            ) {
                bail!(
                    "stacked-bar --from-db expects portfolio/status/allocation, got '{}'",
                    query
                );
            }
            Ok(ChartInput::Stacked(stacked_bar_from_portfolio_backend(
                backend, config,
            )?))
        }
        ChartKind::Probability => Ok(ChartInput::Probability(prob_bar_from_scenario_backend(
            backend, query,
        )?)),
        ChartKind::Drift => Ok(ChartInput::Drift(drift_bar_from_portfolio_backend(
            backend, config, query,
        )?)),
        ChartKind::WhatChanged => {
            bail!("what-changed-strip does not have a canonical --from-db source; use --from-json")
        }
        ChartKind::OpenPredictions => Ok(ChartInput::OpenPredictions(
            open_predictions_table_from_backend(backend, query)?,
        )),
        ChartKind::OutlookArrows => Ok(ChartInput::OutlookArrows(
            outlook_arrows_from_analyst_views_backend(backend, query)?,
        )),
        ChartKind::FactorExposure => {
            bail!("factor-exposure does not have a canonical --from-db source; use --from-json")
        }
        ChartKind::ConvictionGrid => Ok(ChartInput::ConvictionGrid(
            conviction_grid_from_analyst_views_backend(backend, query)?,
        )),
        ChartKind::MismatchCard => {
            bail!("mismatch-card does not have a canonical --from-db source; use --from-json")
        }
        ChartKind::DecisionCard => {
            bail!("decision-card does not have a canonical --from-db source; use --from-json")
        }
        ChartKind::RegimeQuadrant => {
            bail!("regime-quadrant does not have a canonical --from-db source; use --from-json")
        }
        ChartKind::ConvictionTrajectory => Ok(ChartInput::ConvictionTrajectory(
            conviction_trajectory_from_analyst_view_history_backend(backend, query)?,
        )),
        ChartKind::AnalystConvergenceCard => Ok(ChartInput::AnalystConvergenceCard(
            analyst_convergence_card_from_backend(backend, query)?,
        )),
        ChartKind::CalibrationReliability => Ok(ChartInput::CalibrationReliability(
            calibration_reliability_from_predictions_backend(backend, query)?,
        )),
    }
}

fn render_chart(
    input: &ChartInput,
    format: ChartOutputFormat,
) -> Result<(Vec<u8>, Option<String>)> {
    if !registry::supported_formats(input).contains(&report_format_name(format)) {
        bail!(
            "{} does not support {} output; supported formats: {}",
            registry::chart_name(input),
            report_format_name(format),
            registry::supported_formats(input).join(", ")
        );
    }

    match format {
        ChartOutputFormat::Svg => {
            let svg = registry::render_svg(input)?;
            Ok((svg.as_bytes().to_vec(), Some(svg)))
        }
        ChartOutputFormat::Ascii => {
            let ascii = registry::render_ascii(input);
            Ok((ascii.as_bytes().to_vec(), Some(ascii)))
        }
        ChartOutputFormat::Html => {
            let html = registry::render_html(input)?;
            Ok((html.as_bytes().to_vec(), Some(html)))
        }
        ChartOutputFormat::Png => {
            let svg = registry::render_svg(input)?;
            Ok((svg_to_png_bytes(&svg)?, None))
        }
    }
}

fn svg_to_png_bytes(svg: &str) -> Result<Vec<u8>> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &options)
        .context("failed to parse generated SVG for PNG rendering")?;
    let pixmap_size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .context("failed to allocate PNG pixmap")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.encode_png()?)
}

fn report_format_name(format: ChartOutputFormat) -> &'static str {
    match format {
        ChartOutputFormat::Svg => "svg",
        ChartOutputFormat::Png => "png",
        ChartOutputFormat::Ascii => "ascii",
        ChartOutputFormat::Html => "html",
    }
}

fn open_predictions_table_from_backend(
    backend: &BackendConnection,
    query: &str,
) -> Result<OpenPredictionsTableInput> {
    let limit = open_predictions_limit(query)?;
    let today = Utc::now().date_naive();
    let mut predictions =
        user_predictions::list_predictions_backend(backend, Some("pending"), None, None, None)?
            .into_iter()
            .filter_map(|prediction| {
                let target_date = prediction
                    .target_date
                    .as_deref()
                    .and_then(|raw| NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok())?;
                Some(OpenPredictionRow {
                    id: Some(prediction.id),
                    claim: prediction.claim,
                    asset: prediction.symbol.unwrap_or_else(|| "\u{2014}".to_string()),
                    days_remaining: (target_date - today).num_days(),
                    confidence: prediction.confidence,
                    direction: None,
                })
            })
            .collect::<Vec<_>>();

    predictions.sort_by(|a, b| {
        a.days_remaining
            .cmp(&b.days_remaining)
            .then_with(|| a.id.cmp(&b.id))
    });
    predictions.truncate(limit);

    if predictions.is_empty() {
        bail!("no pending predictions with parseable target_date available for open-predictions-table")
    }

    Ok(OpenPredictionsTableInput {
        predictions,
        width: None,
    })
}

fn open_predictions_limit(query: &str) -> Result<usize> {
    let normalized = query.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || matches!(
            normalized.as_str(),
            "pending" | "open" | "user-predictions" | "predictions"
        )
    {
        return Ok(10);
    }
    if let Some(raw) = normalized.strip_prefix("limit=") {
        return parse_open_predictions_limit(raw);
    }
    parse_open_predictions_limit(&normalized)
}

fn parse_open_predictions_limit(raw: &str) -> Result<usize> {
    let limit = raw.parse::<usize>().with_context(|| {
        format!("open-predictions-table --from-db expects pending/open or a limit, got '{raw}'")
    })?;
    if limit == 0 {
        bail!("open-predictions-table limit must be greater than zero");
    }
    Ok(limit)
}

fn calibration_reliability_from_predictions_backend(
    backend: &BackendConnection,
    query: &str,
) -> Result<CalibrationReliabilityInput> {
    let window_days = parse_calibration_window_days(query)?;
    let predictions = user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    let input = calibration_reliability_from_predictions(&predictions, window_days);
    if input.rows.is_empty() {
        bail!("no scored layer predictions available for calibration-reliability")
    }
    Ok(input)
}

fn parse_calibration_window_days(query: &str) -> Result<i64> {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        return Ok(90);
    }
    let normalized = trimmed
        .trim_start_matches("--window-days")
        .trim_start_matches("window-days")
        .trim_start_matches("--window")
        .trim_start_matches("window")
        .trim_start_matches('=')
        .trim();
    let days = normalized
        .trim_end_matches("days")
        .trim_end_matches('d')
        .trim()
        .parse::<i64>()
        .with_context(|| {
            format!("calibration-reliability --from-db expects a window like 90d, got '{query}'")
        })?;
    Ok(days.max(1))
}

fn calibration_reliability_from_predictions(
    predictions: &[user_predictions::UserPrediction],
    window_days: i64,
) -> CalibrationReliabilityInput {
    let cutoff = (Utc::now().date_naive() - Duration::days(window_days))
        .format("%Y-%m-%d")
        .to_string();
    let mut buckets: BTreeMap<String, Vec<&user_predictions::UserPrediction>> = BTreeMap::new();

    for prediction in predictions {
        if !is_scored_prediction(&prediction.outcome) || !prediction_in_window(prediction, &cutoff)
        {
            continue;
        }
        let Some(layer) = prediction_layer(prediction) else {
            continue;
        };
        buckets
            .entry(layer.to_string())
            .or_default()
            .push(prediction);
    }

    let mut rows = buckets
        .into_iter()
        .map(|(layer, bucket)| build_calibration_layer(layer, &bucket))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| layer_sort_key(&row.layer));

    CalibrationReliabilityInput {
        window_days: Some(window_days),
        rows,
        width: None,
        height: None,
    }
}

fn build_calibration_layer(
    layer: String,
    bucket: &[&user_predictions::UserPrediction],
) -> CalibrationReliabilityLayer {
    let stats = calibration_stats(bucket.iter().copied());
    let mut bin_buckets: BTreeMap<String, Vec<&user_predictions::UserPrediction>> = BTreeMap::new();
    for prediction in bucket {
        if let Some(band) = prediction_band(&prediction.conviction) {
            bin_buckets
                .entry(band.to_string())
                .or_default()
                .push(*prediction);
        }
    }
    let mut bin_breakdown = bin_buckets
        .into_iter()
        .map(|(band, bucket)| {
            let stats = calibration_stats(bucket.iter().copied());
            CalibrationReliabilityBin {
                band,
                strict_hit_rate: Some(stats.strict_hit_rate),
                strict_hit_rate_pct: Some(stats.strict_hit_rate_pct),
                n: stats.n,
                sigma: Some(stats.sigma),
                sigma_pp: Some(stats.sigma_pp),
                low_sample: stats.low_sample,
                correct: stats.correct,
                partial: stats.partial,
                wrong: stats.wrong,
            }
        })
        .collect::<Vec<_>>();
    bin_breakdown.sort_by_key(|bin| band_sort_key(&bin.band));

    CalibrationReliabilityLayer {
        layer,
        strict_hit_rate: Some(stats.strict_hit_rate),
        strict_hit_rate_pct: Some(stats.strict_hit_rate_pct),
        n: stats.n,
        sigma: Some(stats.sigma),
        sigma_pp: Some(stats.sigma_pp),
        low_sample: stats.low_sample,
        correct: stats.correct,
        partial: stats.partial,
        wrong: stats.wrong,
        bin_breakdown,
    }
}

#[derive(Debug, Clone, Copy)]
struct CalibrationStats {
    n: usize,
    strict_hit_rate: f64,
    strict_hit_rate_pct: f64,
    sigma: f64,
    sigma_pp: f64,
    low_sample: bool,
    correct: usize,
    partial: usize,
    wrong: usize,
}

fn calibration_stats<'a>(
    predictions: impl Iterator<Item = &'a user_predictions::UserPrediction>,
) -> CalibrationStats {
    let bucket = predictions.collect::<Vec<_>>();
    let n = bucket.len();
    let correct = bucket.iter().filter(|p| p.outcome == "correct").count();
    let partial = bucket.iter().filter(|p| p.outcome == "partial").count();
    let wrong = bucket.iter().filter(|p| p.outcome == "wrong").count();
    let strict_hit_rate = if n == 0 {
        0.0
    } else {
        correct as f64 / n as f64
    };
    let sigma = if n == 0 {
        0.0
    } else {
        (strict_hit_rate * (1.0 - strict_hit_rate) / n as f64).sqrt()
    };
    CalibrationStats {
        n,
        strict_hit_rate: round4(strict_hit_rate),
        strict_hit_rate_pct: round2(strict_hit_rate * 100.0),
        sigma: round4(sigma),
        sigma_pp: round2(sigma * 100.0),
        low_sample: n < 10,
        correct,
        partial,
        wrong,
    }
}

fn is_scored_prediction(outcome: &str) -> bool {
    matches!(outcome, "correct" | "partial" | "wrong")
}

fn prediction_in_window(prediction: &user_predictions::UserPrediction, cutoff: &str) -> bool {
    let ts = prediction
        .scored_at
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(prediction.created_at.as_str());
    ts.get(..10).is_some_and(|date| date >= cutoff)
}

fn prediction_layer(prediction: &user_predictions::UserPrediction) -> Option<&'static str> {
    if let Some(timeframe) = prediction.timeframe.as_deref() {
        if let Some(layer) = normalize_prediction_layer(timeframe) {
            return Some(layer);
        }
    }
    prediction
        .source_agent
        .as_deref()
        .and_then(normalize_prediction_layer)
}

fn normalize_prediction_layer(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.contains("low") || normalized == "short" {
        Some("low")
    } else if normalized.contains("medium") || normalized == "med" {
        Some("medium")
    } else if normalized.contains("high") || normalized == "long" {
        Some("high")
    } else if normalized.contains("macro") {
        Some("macro")
    } else {
        None
    }
}

fn prediction_band(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        _ => None,
    }
}

fn layer_sort_key(layer: &str) -> u8 {
    match layer {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        "macro" => 3,
        _ => 4,
    }
}

fn band_sort_key(band: &str) -> u8 {
    match band {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        _ => 3,
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn round4(value: f64) -> f64 {
    (value * 10000.0).round() / 10000.0
}

fn outlook_arrows_from_analyst_views_backend(
    backend: &BackendConnection,
    asset: &str,
) -> Result<OutlookArrowsInput> {
    let asset = asset.trim();
    if asset.is_empty() {
        bail!("outlook-arrows --from-db expects an asset symbol");
    }

    let views = analyst_views::list_views_backend(backend, None, Some(asset), None)?;
    if views.is_empty() {
        bail!("no analyst views found for '{}'", asset);
    }

    Ok(OutlookArrowsInput {
        days: outlook_point_for_analyst(&views, "low", asset)?,
        weeks: outlook_point_for_analyst(&views, "medium", asset)?,
        months: outlook_point_for_analyst(&views, "high", asset)?,
        width: None,
        height: None,
    })
}

fn outlook_point_for_analyst(
    views: &[analyst_views::AnalystView],
    analyst: &str,
    asset: &str,
) -> Result<OutlookPoint> {
    let view = views
        .iter()
        .find(|view| view.analyst.eq_ignore_ascii_case(analyst))
        .with_context(|| format!("missing {} analyst view for '{}'", analyst, asset))?;
    Ok(outlook_point_from_view(view))
}

fn outlook_point_from_view(view: &analyst_views::AnalystView) -> OutlookPoint {
    let magnitude = view.conviction.abs();
    let conviction = if magnitude >= 4 {
        "high"
    } else if magnitude >= 2 {
        "medium"
    } else {
        "low"
    };
    let direction = match view.direction.trim().to_ascii_lowercase().as_str() {
        "bull" if magnitude >= 4 => "up_strong",
        "bull" => "up",
        "bear" if magnitude >= 4 => "down_strong",
        "bear" => "down",
        _ => "flat",
    };

    OutlookPoint {
        direction: direction.to_string(),
        conviction: conviction.to_string(),
    }
}

fn conviction_grid_from_analyst_views_backend(
    backend: &BackendConnection,
    query: &str,
) -> Result<ConvictionGridInput> {
    let normalized = query.trim().to_ascii_lowercase();
    let rows = if normalized.is_empty()
        || matches!(
            normalized.as_str(),
            "all" | "views" | "analyst-views" | "analyst_views" | "matrix"
        ) {
        analyst_views::get_view_matrix_backend(backend)?
    } else {
        let views = analyst_views::list_views_backend(backend, None, Some(query.trim()), None)?;
        if views.is_empty() {
            bail!("no analyst views found for '{}'", query.trim());
        }
        let asset = views
            .first()
            .map(|view| view.asset.clone())
            .unwrap_or_else(|| query.trim().to_ascii_uppercase());
        vec![analyst_views::AssetViewMatrix { asset, views }]
    };

    let rows = rows
        .into_iter()
        .map(|row| {
            let mut grid_row = ConvictionGridRow {
                symbol: row.asset,
                low: None,
                medium: None,
                high: None,
                macro_score: None,
                summary: None,
            };
            for view in row.views {
                match view.analyst.trim().to_ascii_lowercase().as_str() {
                    "low" => grid_row.low = Some(view.conviction),
                    "medium" => grid_row.medium = Some(view.conviction),
                    "high" => grid_row.high = Some(view.conviction),
                    "macro" => grid_row.macro_score = Some(view.conviction),
                    _ => {}
                }
            }
            grid_row
        })
        .collect::<Vec<_>>();

    if rows.is_empty() {
        bail!("no analyst views available for conviction-grid");
    }

    Ok(ConvictionGridInput { rows, width: None })
}

fn conviction_trajectory_from_analyst_view_history_backend(
    backend: &BackendConnection,
    query: &str,
) -> Result<ConvictionTrajectoryInput> {
    let parsed = parse_conviction_trajectory_db_query(query)?;
    let history = analyst_views::get_view_history_backend(backend, &parsed.asset, None, None)?;
    if history.is_empty() {
        bail!("no analyst view history found for '{}'", parsed.asset);
    }

    let cutoff = Utc::now().naive_utc() - Duration::days(parsed.window_days);
    let mut layers = ["low", "medium", "high", "macro"]
        .into_iter()
        .map(|analyst| (analyst, Vec::<ConvictionTrajectoryPoint>::new()))
        .collect::<Vec<_>>();

    for entry in history.iter().rev() {
        if parse_history_timestamp(&entry.recorded_at).is_some_and(|ts| ts < cutoff) {
            continue;
        }
        let Some((_, series)) = layers
            .iter_mut()
            .find(|(analyst, _)| entry.analyst.trim().eq_ignore_ascii_case(analyst))
        else {
            continue;
        };
        series.push(ConvictionTrajectoryPoint(
            trajectory_point_label(&entry.recorded_at),
            entry.conviction,
        ));
    }

    let layer_series = layers
        .into_iter()
        .filter_map(|(analyst, series)| {
            if series.is_empty() {
                None
            } else {
                Some(ConvictionLayerSeries {
                    layer: trajectory_layer_label(analyst).to_string(),
                    series,
                })
            }
        })
        .collect::<Vec<_>>();

    if layer_series.is_empty() {
        bail!(
            "no analyst view history found for '{}' in the last {}d",
            parsed.asset,
            parsed.window_days
        );
    }

    Ok(ConvictionTrajectoryInput {
        symbol: parsed.asset,
        layer_series,
        width: None,
        height: None,
    })
}

fn analyst_convergence_card_from_backend(
    backend: &BackendConnection,
    query: &str,
) -> Result<AnalystConvergenceCardInput> {
    let parsed = parse_analyst_convergence_card_db_query(query)?;
    let report =
        analyst_views::convergence_report_backend(backend, &parsed.asset, parsed.since.as_deref())?;
    if report.views.is_empty() {
        bail!("no analyst convergence views found for '{}'", parsed.asset);
    }

    let mut views = report
        .views
        .iter()
        .map(analyst_convergence_view_from_report)
        .collect::<Vec<_>>();
    views.sort_by(|a, b| {
        analyst_layer_order(&a.analyst)
            .cmp(&analyst_layer_order(&b.analyst))
            .then_with(|| a.analyst.cmp(&b.analyst))
    });

    Ok(AnalystConvergenceCardInput {
        asset: report.asset,
        views,
        user_target: None,
        current_alloc: None,
        analyst_range: None,
        summary: report.summary,
        width: None,
    })
}

fn analyst_convergence_view_from_report(
    view: &analyst_views::ConvergenceView,
) -> AnalystConvergenceView {
    let analyst = view.analyst.trim();
    let analyst = if analyst.to_ascii_lowercase().starts_with("analyst-") {
        analyst.to_string()
    } else {
        format!("analyst-{analyst}")
    };

    AnalystConvergenceView {
        analyst,
        conviction: view.conviction,
        reasoning_summary: view.reasoning_summary.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AnalystConvergenceCardDbQuery {
    asset: String,
    since: Option<String>,
}

fn parse_analyst_convergence_card_db_query(query: &str) -> Result<AnalystConvergenceCardDbQuery> {
    let mut parts = query.split_whitespace();
    let asset = parts
        .next()
        .map(str::trim)
        .filter(|asset| !asset.is_empty())
        .context("analyst-convergence-card --from-db expects an asset symbol")?
        .to_ascii_uppercase();
    let mut since_token = Some("30d".to_string());

    while let Some(part) = parts.next() {
        let part = part.trim();
        let normalized = part.to_ascii_lowercase();
        let value = if matches!(normalized.as_str(), "since" | "--since") {
            parts
                .next()
                .context("analyst-convergence-card --from-db since requires a value like 30d")?
        } else if let Some(value) = strip_case_insensitive_prefix(part, "since=") {
            value
        } else if let Some(value) = strip_case_insensitive_prefix(part, "--since=") {
            value
        } else {
            part
        };

        if value.eq_ignore_ascii_case("all") {
            since_token = None;
        } else {
            since_token = Some(value.to_string());
        }
    }

    let since = since_token
        .map(|value| {
            analyst_views::parse_since(&value).with_context(|| {
                format!(
                    "invalid analyst-convergence-card since '{}'; use 30d, 2w, YYYY-MM-DD, or all",
                    value
                )
            })
        })
        .transpose()?;

    Ok(AnalystConvergenceCardDbQuery { asset, since })
}

fn strip_case_insensitive_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .and_then(|_| value.get(prefix.len()..))
}

fn analyst_layer_order(analyst: &str) -> usize {
    match analyst
        .trim()
        .trim_start_matches("analyst-")
        .to_ascii_lowercase()
        .as_str()
    {
        "low" => 0,
        "medium" | "med" => 1,
        "high" => 2,
        "macro" => 3,
        _ => 4,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConvictionTrajectoryDbQuery {
    asset: String,
    window_days: i64,
}

fn parse_conviction_trajectory_db_query(query: &str) -> Result<ConvictionTrajectoryDbQuery> {
    let mut parts = query.split_whitespace();
    let asset = parts
        .next()
        .map(str::trim)
        .filter(|asset| !asset.is_empty())
        .context("conviction-trajectory --from-db expects an asset symbol")?
        .to_ascii_uppercase();
    let mut window_days = 30_i64;

    while let Some(part) = parts.next() {
        let part = part.trim();
        let value = if matches!(part, "window" | "--window") {
            parts
                .next()
                .context("conviction-trajectory --from-db window requires a value like 30d")?
        } else if let Some(value) = part.strip_prefix("window=") {
            value
        } else if let Some(value) = part.strip_prefix("--window=") {
            value
        } else {
            part
        };
        window_days = parse_window_days(value).with_context(|| {
            format!(
                "invalid conviction-trajectory window '{}'; use a value like 30d",
                value
            )
        })?;
    }

    Ok(ConvictionTrajectoryDbQuery { asset, window_days })
}

fn parse_window_days(value: &str) -> Result<i64> {
    let normalized = value.trim().to_ascii_lowercase();
    let days = normalized
        .strip_suffix("days")
        .or_else(|| normalized.strip_suffix("day"))
        .or_else(|| normalized.strip_suffix('d'))
        .unwrap_or(&normalized);
    let parsed = days.parse::<i64>()?;
    if parsed <= 0 {
        bail!("window must be positive");
    }
    Ok(parsed)
}

fn trajectory_layer_label(analyst: &str) -> &'static str {
    match analyst {
        "low" => "LOW",
        "medium" => "MED",
        "high" => "HIGH",
        "macro" => "MACRO",
        _ => "OTHER",
    }
}

fn trajectory_point_label(recorded_at: &str) -> String {
    parse_history_timestamp(recorded_at)
        .map(|ts| ts.date().format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| recorded_at.to_string())
}

fn portfolio_positions_backend(
    backend: &BackendConnection,
    config: &Config,
) -> Result<Vec<Position>> {
    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|quote| (quote.symbol, quote.price))
        .collect();
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();

    Ok(match config.portfolio_mode {
        PortfolioMode::Full => {
            let transactions = list_transactions_backend(backend)?;
            for tx in &transactions {
                if tx.category == crate::models::asset::AssetCategory::Cash {
                    prices.insert(tx.symbol.clone(), Decimal::ONE);
                }
            }
            compute_positions(&transactions, &prices, &fx_rates)
        }
        PortfolioMode::Percentage => {
            let allocations = list_allocations_backend(backend)?;
            compute_positions_from_allocations(&allocations, &prices, &fx_rates)
        }
    })
}

fn stacked_bar_from_portfolio_backend(
    backend: &BackendConnection,
    config: &Config,
) -> Result<StackedBarInput> {
    let mut segments = portfolio_positions_backend(backend, config)?
        .into_iter()
        .filter_map(|position| {
            let allocation = position.allocation_pct?;
            if allocation <= dec!(0) {
                return None;
            }
            Some(StackedBarSegment {
                label: position.symbol.clone(),
                value: decimal_to_f64_2(allocation),
                color: palette::asset_color(&position.symbol, position.category).to_string(),
            })
        })
        .collect::<Vec<_>>();

    segments.sort_by(|a, b| {
        b.value
            .partial_cmp(&a.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if segments.is_empty() {
        bail!("no portfolio allocation data available for stacked-bar chart");
    }

    Ok(StackedBarInput {
        segments,
        width: None,
        height: None,
    })
}

fn drift_bar_from_portfolio_backend(
    backend: &BackendConnection,
    config: &Config,
    symbol: &str,
) -> Result<DriftBarInput> {
    let symbol = symbol.trim();
    if symbol.is_empty() {
        bail!("drift-bar --from-db expects a portfolio symbol");
    }

    let target = crate::db::allocation_targets::list_targets_backend(backend)?
        .into_iter()
        .find(|target| target.symbol.eq_ignore_ascii_case(symbol))
        .with_context(|| format!("allocation target '{}' not found", symbol))?;
    let actual_pct = portfolio_positions_backend(backend, config)?
        .into_iter()
        .find(|position| position.symbol.eq_ignore_ascii_case(&target.symbol))
        .and_then(|position| position.allocation_pct)
        .unwrap_or(Decimal::ZERO);

    Ok(DriftBarInput {
        symbol: target.symbol,
        target_pct: decimal_to_f64_2(target.target_pct),
        actual_pct: decimal_to_f64_2(actual_pct),
        band_pct: decimal_to_f64_2(target.drift_band_pct),
        max_pct: None,
        width: None,
        height: None,
    })
}

fn prob_bar_from_scenario_backend(
    backend: &BackendConnection,
    scenario_name: &str,
) -> Result<ProbBarInput> {
    let scenario_name = scenario_name.trim();
    if scenario_name.is_empty() {
        bail!("prob-bar --from-db expects a scenario name");
    }
    let scenario = scenarios::get_scenario_by_name_backend(backend, scenario_name)?
        .with_context(|| format!("scenario '{}' not found", scenario_name))?;
    let history = scenarios::get_history_backend(backend, scenario.id, None)?;
    let prior_7d = prior_probability_7d(&history, scenario.probability);

    Ok(ProbBarInput {
        name: scenario.name,
        current: scenario.probability,
        prior_7d,
        color: scenario_color(scenario_name),
        max_pct: None,
        width: None,
        height: None,
    })
}

fn prior_probability_7d(history: &[scenarios::ScenarioHistoryEntry], current: f64) -> f64 {
    let cutoff = Utc::now().naive_utc() - Duration::days(7);
    history
        .iter()
        .find(|entry| parse_history_timestamp(&entry.recorded_at).is_some_and(|ts| ts <= cutoff))
        .map(|entry| entry.probability)
        .unwrap_or(current)
}

fn parse_history_timestamp(raw: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.naive_utc())
        .ok()
        .or_else(|| {
            DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%#z")
                .map(|dt| dt.naive_utc())
                .ok()
        })
        .or_else(|| {
            DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%#z")
                .map(|dt| dt.naive_utc())
                .ok()
        })
        .or_else(|| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S").ok())
        .or_else(|| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f").ok())
        .or_else(|| {
            NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
}

fn scenario_color(name: &str) -> String {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("recession") || normalized.contains("war") {
        "amber".to_string()
    } else if normalized.contains("inflation") {
        "bear".to_string()
    } else if normalized.contains("risk-on") || normalized.contains("growth") {
        "bull".to_string()
    } else {
        "cyan".to_string()
    }
}

fn decimal_to_f64_2(value: Decimal) -> f64 {
    value.round_dp(2).to_string().parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::allocation_targets::set_target_backend;
    use crate::db::backend::BackendConnection;
    use crate::db::price_cache::upsert_price;
    use crate::db::transactions::insert_transaction;
    use crate::models::asset::AssetCategory;
    use crate::models::price::PriceQuote;
    use crate::models::transaction::{NewTransaction, TxType};
    use rust_decimal_macros::dec;

    fn backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        }
    }

    #[test]
    fn report_portfolio_stacked_bar_uses_synthetic_db_allocations() {
        let backend = backend();
        let config = Config::default();
        let conn = backend.sqlite();

        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "USD".to_string(),
                category: AssetCategory::Cash,
                tx_type: TxType::Buy,
                quantity: dec!(50_000),
                price_per: dec!(1),
                currency: "USD".to_string(),
                date: "2026-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(50_000),
                currency: "USD".to_string(),
                date: "2026-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        upsert_price(
            conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: dec!(50_000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2026-01-01T00:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let input = stacked_bar_from_portfolio_backend(&backend, &config).unwrap();
        assert_eq!(input.segments.len(), 2);
        assert_eq!(input.segments[0].value, 50.0);
        assert_eq!(input.segments[1].value, 50.0);
        assert!(input
            .segments
            .iter()
            .any(|s| s.color == palette::DARK.crypto));
    }

    #[test]
    fn report_drift_bar_uses_synthetic_target_and_allocation() {
        let backend = backend();
        let config = Config::default();
        let conn = backend.sqlite();

        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "USD".to_string(),
                category: AssetCategory::Cash,
                tx_type: TxType::Buy,
                quantity: dec!(50_000),
                price_per: dec!(1),
                currency: "USD".to_string(),
                date: "2026-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(50_000),
                currency: "USD".to_string(),
                date: "2026-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        upsert_price(
            conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: dec!(50_000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2026-01-01T00:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
        set_target_backend(&backend, "BTC", dec!(40), dec!(5)).unwrap();

        let input = drift_bar_from_portfolio_backend(&backend, &config, "btc").unwrap();
        assert_eq!(input.symbol, "BTC");
        assert_eq!(input.target_pct, 40.0);
        assert_eq!(input.actual_pct, 50.0);
        assert_eq!(input.band_pct, 5.0);
    }

    #[test]
    fn report_open_predictions_table_uses_synthetic_pending_predictions() {
        let backend = backend();
        let target_date = (Utc::now().date_naive() + Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        user_predictions::add_prediction_backend(
            &backend,
            "BTC closes above support",
            Some("BTC"),
            Some("medium"),
            Some("low"),
            Some(0.65),
            Some("test-agent"),
            Some(&target_date),
            None,
            &[],
        )
        .unwrap();
        user_predictions::add_prediction_backend(
            &backend,
            "No target date should not render",
            Some("ETH"),
            Some("medium"),
            Some("low"),
            Some(0.45),
            Some("test-agent"),
            None,
            None,
            &[],
        )
        .unwrap();

        let input = open_predictions_table_from_backend(&backend, "pending").unwrap();

        assert_eq!(input.predictions.len(), 1);
        assert_eq!(input.predictions[0].asset, "BTC");
        assert_eq!(input.predictions[0].days_remaining, 2);
        assert_eq!(input.predictions[0].confidence, Some(0.65));
    }

    #[test]
    fn report_calibration_reliability_uses_synthetic_scored_predictions() {
        let backend = backend();
        seed_scored_prediction(&backend, "low", "high", "correct");
        seed_scored_prediction(&backend, "low", "high", "wrong");
        seed_scored_prediction(&backend, "macro", "low", "wrong");

        let input = calibration_reliability_from_predictions_backend(&backend, "90d").unwrap();

        assert_eq!(input.window_days, Some(90));
        assert_eq!(input.rows.len(), 2);
        let low = input.rows.iter().find(|row| row.layer == "low").unwrap();
        assert_eq!(low.n, 2);
        assert_eq!(low.strict_hit_rate_pct, Some(50.0));
        assert!(low.low_sample);
        assert_eq!(low.bin_breakdown[0].band, "high");
    }

    fn seed_scored_prediction(
        backend: &BackendConnection,
        layer: &str,
        conviction: &str,
        outcome: &str,
    ) {
        let id = user_predictions::add_prediction_backend_with_details(
            backend,
            &format!("{layer} {conviction} prediction"),
            Some("BTC"),
            Some(conviction),
            Some(layer),
            Some(0.6),
            Some(&format!("analyst-{layer}")),
            None,
            None,
            &[],
            Some("crypto"),
            None,
        )
        .unwrap();
        user_predictions::score_prediction_backend(backend, id, outcome, None, None).unwrap();
    }

    #[test]
    fn report_outlook_arrows_uses_synthetic_analyst_views() {
        let backend = backend();
        analyst_views::upsert_view_backend(
            &backend,
            "low",
            "BTC",
            "neutral",
            1,
            "Flat near term",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "medium",
            "BTC",
            "bull",
            3,
            "Trend improving",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "high",
            "BTC",
            "bull",
            5,
            "Structural bull case",
            None,
            None,
            None,
        )
        .unwrap();

        let input = outlook_arrows_from_analyst_views_backend(&backend, "btc").unwrap();

        assert_eq!(input.days.direction, "flat");
        assert_eq!(input.days.conviction, "low");
        assert_eq!(input.weeks.direction, "up");
        assert_eq!(input.weeks.conviction, "medium");
        assert_eq!(input.months.direction, "up_strong");
        assert_eq!(input.months.conviction, "high");
    }

    #[test]
    fn report_conviction_grid_uses_synthetic_analyst_views() {
        let backend = backend();
        analyst_views::upsert_view_backend(
            &backend,
            "low",
            "BTC",
            "bull",
            1,
            "Near-term support",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "medium",
            "BTC",
            "bull",
            2,
            "Trend improving",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "high",
            "BTC",
            "bull",
            4,
            "Structural case",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "macro",
            "BTC",
            "bull",
            3,
            "Macro tailwind",
            None,
            None,
            None,
        )
        .unwrap();

        let input = conviction_grid_from_analyst_views_backend(&backend, "btc").unwrap();

        assert_eq!(input.rows.len(), 1);
        assert_eq!(input.rows[0].symbol, "BTC");
        assert_eq!(input.rows[0].low, Some(1));
        assert_eq!(input.rows[0].medium, Some(2));
        assert_eq!(input.rows[0].high, Some(4));
        assert_eq!(input.rows[0].macro_score, Some(3));
    }

    #[test]
    fn report_conviction_trajectory_uses_synthetic_analyst_view_history() {
        let backend = backend();
        analyst_views::upsert_view_backend(
            &backend,
            "low",
            "BTC",
            "bull",
            1,
            "Initial near-term view",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "low",
            "BTC",
            "bull",
            3,
            "Near-term improving",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "medium",
            "BTC",
            "bear",
            -2,
            "Medium-term caution",
            None,
            None,
            None,
        )
        .unwrap();

        let input =
            conviction_trajectory_from_analyst_view_history_backend(&backend, "btc 30d").unwrap();

        assert_eq!(input.symbol, "BTC");
        assert_eq!(input.layer_series.len(), 2);
        assert_eq!(input.layer_series[0].layer, "LOW");
        assert_eq!(input.layer_series[0].series.len(), 2);
        assert_eq!(input.layer_series[0].series[0].1, 1);
        assert_eq!(input.layer_series[0].series[1].1, 3);
        assert_eq!(input.layer_series[1].layer, "MED");
        assert_eq!(input.layer_series[1].series[0].1, -2);
    }

    #[test]
    fn report_analyst_convergence_card_uses_synthetic_convergence_views() {
        let backend = backend();
        analyst_views::upsert_view_backend(
            &backend,
            "low",
            "BTC",
            "bull",
            3,
            "Near-term support",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "medium",
            "BTC",
            "bull",
            3,
            "Trend improving",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "high",
            "BTC",
            "bull",
            4,
            "Structural case",
            None,
            None,
            None,
        )
        .unwrap();
        analyst_views::upsert_view_backend(
            &backend,
            "macro",
            "BTC",
            "bull",
            3,
            "Macro tailwind",
            None,
            None,
            None,
        )
        .unwrap();

        let input = analyst_convergence_card_from_backend(&backend, "btc 30d").unwrap();

        assert_eq!(input.asset, "BTC");
        assert_eq!(input.summary, "strong-convergent-bull");
        assert_eq!(
            input
                .views
                .iter()
                .map(|view| view.analyst.as_str())
                .collect::<Vec<_>>(),
            vec![
                "analyst-low",
                "analyst-medium",
                "analyst-high",
                "analyst-macro"
            ]
        );
        assert_eq!(input.views[2].conviction, 4);
    }

    #[test]
    fn parse_conviction_trajectory_db_query_accepts_window_forms() {
        assert_eq!(
            parse_conviction_trajectory_db_query("btc").unwrap(),
            ConvictionTrajectoryDbQuery {
                asset: "BTC".to_string(),
                window_days: 30
            }
        );
        assert_eq!(
            parse_conviction_trajectory_db_query("Gold --window 14d").unwrap(),
            ConvictionTrajectoryDbQuery {
                asset: "GOLD".to_string(),
                window_days: 14
            }
        );
        assert_eq!(
            parse_conviction_trajectory_db_query("ETH window=7days").unwrap(),
            ConvictionTrajectoryDbQuery {
                asset: "ETH".to_string(),
                window_days: 7
            }
        );
    }

    #[test]
    fn parse_analyst_convergence_card_db_query_accepts_since_forms() {
        let default = parse_analyst_convergence_card_db_query("btc").unwrap();
        assert_eq!(default.asset, "BTC");
        assert!(default.since.is_some());

        let window = parse_analyst_convergence_card_db_query("Gold --since 14d").unwrap();
        assert_eq!(window.asset, "GOLD");
        assert!(window.since.is_some());

        let all = parse_analyst_convergence_card_db_query("ETH since=all").unwrap();
        assert_eq!(
            all,
            AnalystConvergenceCardDbQuery {
                asset: "ETH".to_string(),
                since: None
            }
        );
    }

    #[test]
    fn prior_probability_uses_latest_history_at_or_before_seven_day_cutoff() {
        let old = (Utc::now() - Duration::days(8)).to_rfc3339();
        let recent = (Utc::now() - Duration::days(2)).to_rfc3339();
        let history = vec![
            scenarios::ScenarioHistoryEntry {
                id: 2,
                scenario_id: 1,
                probability: 90.0,
                driver: None,
                recorded_at: recent,
            },
            scenarios::ScenarioHistoryEntry {
                id: 1,
                scenario_id: 1,
                probability: 72.0,
                driver: None,
                recorded_at: old,
            },
        ];
        assert_eq!(prior_probability_7d(&history, 88.0), 72.0);
    }

    #[test]
    fn svg_to_png_produces_png_bytes() {
        let png = svg_to_png_bytes(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/report/snapshots/prob_bar.svg"
        )))
        .unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
