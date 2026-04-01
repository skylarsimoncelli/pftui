use anyhow::{anyhow, Result};
use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;
use std::collections::HashSet;

use crate::analytics::situation::{self, SituationPosition};
use crate::data::predictions::{MarketCategory, PredictionMarket};
use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::calendar_cache::CalendarEvent;
use crate::db::scenarios::Scenario;
use crate::db::watchlist::WatchlistEntry;

#[derive(Debug, Clone, Serialize)]
pub struct CatalystReport {
    pub window: String,
    pub label: String,
    pub generated_at: String,
    pub catalysts: Vec<CatalystEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalystEvent {
    pub title: String,
    pub time: String,
    pub source: String,
    pub category: String,
    pub significance: String,
    pub countdown_bucket: String,
    pub affected_assets: Vec<String>,
    pub linked_scenarios: Vec<LinkedScenario>,
    pub linked_predictions: Vec<String>,
    pub portfolio_relevance: i32,
    pub macro_significance: i32,
    pub score: i32,
    pub detail: String,
}

/// A scenario linked to a catalyst with direction and relevance context.
#[derive(Debug, Clone, Serialize)]
pub struct LinkedScenario {
    pub name: String,
    pub direction: String,
    pub relevance: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalystWindow {
    Today,
    Tomorrow,
    Week,
}

impl CatalystWindow {
    pub fn parse(raw: Option<&str>) -> Result<Self> {
        match raw.unwrap_or("week").trim().to_ascii_lowercase().as_str() {
            "today" => Ok(Self::Today),
            "tomorrow" => Ok(Self::Tomorrow),
            "week" | "7d" => Ok(Self::Week),
            other => Err(anyhow!(
                "invalid catalyst window '{}'. Use today, tomorrow, or week",
                other
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Tomorrow => "tomorrow",
            Self::Week => "week",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Tomorrow => "tomorrow",
            Self::Week => "this week",
        }
    }
}

pub fn build_report_backend(
    backend: &BackendConnection,
    window: CatalystWindow,
) -> Result<CatalystReport> {
    let today = Utc::now().date_naive();
    let events = db::calendar_cache::get_upcoming_events_backend(
        backend,
        &today.format("%Y-%m-%d").to_string(),
        64,
    )
    .unwrap_or_default();
    let inputs = situation::collect_inputs_backend(backend)?;
    let watchlist = db::watchlist::list_watchlist_backend(backend).unwrap_or_default();
    let scenarios =
        db::scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let predictions =
        db::predictions_cache::get_cached_predictions_backend(backend, 24).unwrap_or_default();

    let mut catalysts = events
        .into_iter()
        .filter_map(|event| {
            let event_date = parse_event_date(&event.date).ok()?;
            if !matches_window(event_date, today, window) {
                return None;
            }
            Some(build_catalyst(
                event,
                event_date,
                today,
                &inputs.positions,
                &watchlist,
                &scenarios,
                &predictions,
            ))
        })
        .collect::<Vec<_>>();

    catalysts.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.time.cmp(&right.time))
            .then_with(|| left.title.cmp(&right.title))
    });
    catalysts.truncate(12);

    Ok(CatalystReport {
        window: window.as_str().to_string(),
        label: window.label().to_string(),
        generated_at: Utc::now().to_rfc3339(),
        catalysts,
    })
}

fn build_catalyst(
    event: CalendarEvent,
    event_date: NaiveDate,
    today: NaiveDate,
    positions: &[SituationPosition],
    watchlist: &[WatchlistEntry],
    scenarios: &[Scenario],
    predictions: &[PredictionMarket],
) -> CatalystEvent {
    let category = event_category(&event);
    let significance = normalize_significance(&event.impact).to_string();
    let countdown_bucket = countdown_bucket(event_date, today);
    let linked_scenarios = link_scenarios(&event, scenarios);
    let linked_predictions = link_predictions(&event, predictions);
    let affected_assets = infer_affected_assets(&event, positions, watchlist);
    let portfolio_relevance = portfolio_relevance(&event, &affected_assets, positions, watchlist);
    let macro_significance = macro_significance(
        &event,
        category.as_str(),
        countdown_bucket.as_str(),
        linked_scenarios.len(),
        linked_predictions.len(),
    );
    let score = portfolio_relevance * 2 + macro_significance;
    let detail = catalyst_detail(
        &event,
        category.as_str(),
        countdown_bucket.as_str(),
        &affected_assets,
        linked_scenarios.len(),
        linked_predictions.len(),
    );

    CatalystEvent {
        title: event.name,
        time: event.date,
        source: "calendar".to_string(),
        category,
        significance,
        countdown_bucket,
        affected_assets,
        linked_scenarios,
        linked_predictions,
        portfolio_relevance,
        macro_significance,
        score,
        detail,
    }
}

fn matches_window(date: NaiveDate, today: NaiveDate, window: CatalystWindow) -> bool {
    match window {
        CatalystWindow::Today => date == today,
        CatalystWindow::Tomorrow => date == today + Duration::days(1),
        CatalystWindow::Week => date >= today && date <= today + Duration::days(7),
    }
}

fn parse_event_date(raw: &str) -> Result<NaiveDate> {
    Ok(NaiveDate::parse_from_str(raw, "%Y-%m-%d")?)
}

fn countdown_bucket(date: NaiveDate, today: NaiveDate) -> String {
    match (date - today).num_days() {
        i64::MIN..=-1 => "past".to_string(),
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        2 | 3 => "next-3d".to_string(),
        _ => "this-week".to_string(),
    }
}

fn normalize_significance(raw: &str) -> &'static str {
    match raw.to_ascii_lowercase().as_str() {
        "high" => "high",
        "medium" => "medium",
        _ => "low",
    }
}

fn event_category(event: &CalendarEvent) -> String {
    if event.event_type.eq_ignore_ascii_case("earnings") {
        return "earnings".to_string();
    }
    if event.event_type.eq_ignore_ascii_case("geopolitical") {
        return "geopolitical".to_string();
    }

    let lower = event.name.to_ascii_lowercase();
    if contains_any(&lower, &["fomc", "fed", "rate", "central bank"]) {
        "policy".to_string()
    } else if contains_any(&lower, &["cpi", "inflation", "pce"]) {
        "inflation".to_string()
    } else if contains_any(&lower, &["payroll", "jobs", "unemployment", "labor"]) {
        "labor".to_string()
    } else if contains_any(&lower, &["gdp", "retail", "pmi", "manufacturing"]) {
        "growth".to_string()
    } else if contains_any(&lower, &["oil", "opec", "inventory", "crude"]) {
        "commodities".to_string()
    } else if contains_any(
        &lower,
        &[
            "iran", "hormuz", "brics", "sanctions", "war", "conflict", "tariff", "embargo",
            "summit", "treaty", "geopolitical", "nato", "taiwan", "annexation",
        ],
    ) {
        "geopolitical".to_string()
    } else {
        event.event_type.to_ascii_lowercase()
    }
}

fn infer_affected_assets(
    event: &CalendarEvent,
    positions: &[SituationPosition],
    watchlist: &[WatchlistEntry],
) -> Vec<String> {
    if let Some(symbol) = &event.symbol {
        return vec![symbol.to_uppercase()];
    }

    let proxies = proxy_assets_for_event(&event.name);
    let mut portfolio_symbols = positions
        .iter()
        .map(|row| row.symbol.to_uppercase())
        .collect::<HashSet<_>>();
    portfolio_symbols.extend(watchlist.iter().map(|row| row.symbol.to_uppercase()));

    let mut relevant = proxies
        .iter()
        .filter(|symbol| portfolio_symbols.contains(**symbol))
        .map(|symbol| (*symbol).to_string())
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        relevant = proxies
            .iter()
            .take(3)
            .map(|symbol| (*symbol).to_string())
            .collect();
    }
    relevant
}

fn proxy_assets_for_event(name: &str) -> &'static [&'static str] {
    let lower = name.to_ascii_lowercase();
    if contains_any(&lower, &["fomc", "fed", "rate"]) {
        &["SPY", "QQQ", "TLT", "DXY", "BTC-USD"]
    } else if contains_any(&lower, &["cpi", "inflation", "pce"]) {
        &["SPY", "TLT", "DXY", "GC=F", "BTC-USD"]
    } else if contains_any(&lower, &["payroll", "jobs", "unemployment", "labor"]) {
        &["SPY", "QQQ", "^TNX", "DXY", "BTC-USD"]
    } else if contains_any(&lower, &["gdp", "retail", "pmi", "manufacturing"]) {
        &["SPY", "QQQ", "CL=F", "HG=F", "IWM"]
    } else if contains_any(&lower, &["oil", "opec", "inventory", "crude"]) {
        &["CL=F", "XLE", "CADUSD=X"]
    } else if contains_any(
        &lower,
        &[
            "iran", "hormuz", "war", "conflict", "sanctions", "tariff", "embargo",
        ],
    ) {
        &["CL=F", "GC=F", "DXY", "XLE", "ITA", "BTC-USD"]
    } else if contains_any(&lower, &["brics", "summit", "treaty", "nato", "taiwan"]) {
        &["GC=F", "DXY", "SPY", "BTC-USD", "CL=F"]
    } else {
        &["SPY", "QQQ", "DXY"]
    }
}

fn portfolio_relevance(
    event: &CalendarEvent,
    affected_assets: &[String],
    positions: &[SituationPosition],
    watchlist: &[WatchlistEntry],
) -> i32 {
    let held = positions
        .iter()
        .map(|row| row.symbol.to_uppercase())
        .collect::<HashSet<_>>();
    let watched = watchlist
        .iter()
        .map(|row| row.symbol.to_uppercase())
        .collect::<HashSet<_>>();

    let mut score = 0;
    for symbol in affected_assets {
        let upper = symbol.to_uppercase();
        if held.contains(&upper) {
            score += 6;
        } else if watched.contains(&upper) {
            score += 3;
        }
    }

    if let Some(symbol) = &event.symbol {
        let upper = symbol.to_uppercase();
        if held.contains(&upper) {
            score += 6;
        } else if watched.contains(&upper) {
            score += 4;
        }
    } else if normalize_significance(&event.impact) == "high"
        && (!held.is_empty() || !watched.is_empty())
    {
        score += 2;
    }

    score
}

fn macro_significance(
    event: &CalendarEvent,
    category: &str,
    countdown_bucket: &str,
    scenario_links: usize,
    prediction_links: usize,
) -> i32 {
    let mut score = match normalize_significance(&event.impact) {
        "high" => 9,
        "medium" => 6,
        _ => 3,
    };

    score += match category {
        "policy" | "inflation" | "labor" | "growth" | "geopolitical" => 4,
        "commodities" | "earnings" => 2,
        _ => 1,
    };
    score += match countdown_bucket {
        "today" => 4,
        "tomorrow" => 3,
        "next-3d" => 2,
        _ => 1,
    };
    score += scenario_links as i32;
    score += prediction_links as i32;
    if event.event_type.eq_ignore_ascii_case("earnings") {
        score += 1;
    }
    score
}

fn catalyst_detail(
    event: &CalendarEvent,
    category: &str,
    countdown_bucket: &str,
    affected_assets: &[String],
    scenario_links: usize,
    prediction_links: usize,
) -> String {
    let asset_text = if affected_assets.is_empty() {
        "broad market sensitivity".to_string()
    } else {
        affected_assets.join(", ")
    };
    format!(
        "{} {} catalyst in {}. Watch {}. Linked to {} scenario(s) and {} prediction market(s).",
        normalize_significance(&event.impact).to_uppercase(),
        category,
        countdown_bucket.replace('-', " "),
        asset_text,
        scenario_links,
        prediction_links
    )
}

fn link_scenarios(event: &CalendarEvent, scenarios: &[Scenario]) -> Vec<LinkedScenario> {
    let event_tokens = keyword_set(&format!(
        "{} {} {}",
        event.name,
        event.event_type,
        event.symbol.clone().unwrap_or_default()
    ));
    let category = event_category(event);

    let mut matches = scenarios
        .iter()
        .filter_map(|scenario| {
            let scenario_text = format!(
                "{} {} {} {}",
                scenario.name,
                scenario.description.as_deref().unwrap_or(""),
                scenario.asset_impact.as_deref().unwrap_or(""),
                scenario.triggers.as_deref().unwrap_or("")
            );
            let keyword_score = overlap_score(&event_tokens, &keyword_set(&scenario_text));
            let semantic_score =
                category_scenario_score(category.as_str(), &scenario.name, &scenario_text);
            let total = keyword_score + semantic_score;
            if total == 0 {
                return None;
            }
            let direction =
                infer_catalyst_direction(category.as_str(), &scenario.name, &scenario_text);
            let relevance = if keyword_score >= 2 {
                "direct"
            } else if keyword_score >= 1 || semantic_score >= 3 {
                "strong"
            } else {
                "thematic"
            };
            Some((
                total,
                LinkedScenario {
                    name: scenario.name.clone(),
                    direction: direction.to_string(),
                    relevance: relevance.to_string(),
                },
            ))
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.name.cmp(&right.1.name)));
    matches
        .into_iter()
        .take(3)
        .map(|(_, linked)| linked)
        .collect()
}

/// Score how strongly a catalyst category semantically relates to a scenario.
/// Returns 0 (no match) to 4 (strong thematic match).
fn category_scenario_score(category: &str, scenario_name: &str, scenario_text: &str) -> usize {
    let lower_name = scenario_name.to_ascii_lowercase();
    let lower_text = scenario_text.to_ascii_lowercase();

    match category {
        "inflation" => {
            let mut score = 0;
            if contains_any(
                &lower_text,
                &[
                    "inflation",
                    "cpi",
                    "pce",
                    "price",
                    "stagflation",
                    "disinflation",
                    "deflation",
                ],
            ) {
                score += 3;
            }
            if contains_any(&lower_text, &["fed", "rate", "monetary", "easing", "tightening"]) {
                score += 1;
            }
            if contains_any(&lower_name, &["inflation", "stagflation"]) {
                score += 1;
            }
            score
        }
        "policy" => {
            let mut score = 0;
            if contains_any(
                &lower_text,
                &[
                    "fed",
                    "fomc",
                    "rate",
                    "monetary",
                    "easing",
                    "tightening",
                    "hawkish",
                    "dovish",
                    "central bank",
                    "liquidity",
                ],
            ) {
                score += 3;
            }
            if contains_any(&lower_text, &["recession", "rally", "risk-on", "risk-off"]) {
                score += 1;
            }
            if contains_any(&lower_name, &["fed", "rate", "recession", "rally"]) {
                score += 1;
            }
            score
        }
        "labor" => {
            let mut score = 0;
            if contains_any(
                &lower_text,
                &[
                    "employment",
                    "labor",
                    "jobs",
                    "payroll",
                    "unemployment",
                    "nfp",
                    "workforce",
                ],
            ) {
                score += 3;
            }
            if contains_any(
                &lower_text,
                &["recession", "contraction", "consumer", "spending", "collapse"],
            ) {
                score += 2;
            }
            if contains_any(&lower_name, &["recession", "employment", "labor"]) {
                score += 1;
            }
            score
        }
        "growth" => {
            let mut score = 0;
            if contains_any(
                &lower_text,
                &[
                    "gdp",
                    "growth",
                    "pmi",
                    "manufacturing",
                    "retail",
                    "output",
                    "expansion",
                    "contraction",
                ],
            ) {
                score += 3;
            }
            if contains_any(&lower_text, &["recession", "rally", "equities", "risk-on"]) {
                score += 1;
            }
            if contains_any(&lower_name, &["recession", "rally", "growth"]) {
                score += 1;
            }
            score
        }
        "commodities" => {
            let mut score = 0;
            if contains_any(
                &lower_text,
                &[
                    "oil",
                    "crude",
                    "opec",
                    "commodity",
                    "gold",
                    "copper",
                    "hormuz",
                    "energy",
                ],
            ) {
                score += 3;
            }
            if contains_any(&lower_text, &["war", "geopolitical", "iran", "conflict"]) {
                score += 1;
            }
            if contains_any(&lower_name, &["war", "iran", "oil", "commodity"]) {
                score += 1;
            }
            score
        }
        "earnings" => {
            let mut score = 0;
            if contains_any(&lower_text, &["equities", "earnings", "stock", "rally", "crash"]) {
                score += 2;
            }
            if contains_any(&lower_name, &["rally", "recession", "crash"]) {
                score += 1;
            }
            score
        }
        "geopolitical" => {
            let mut score = 0;
            if contains_any(
                &lower_text,
                &[
                    "war",
                    "conflict",
                    "iran",
                    "hormuz",
                    "sanctions",
                    "tariff",
                    "geopolitical",
                    "brics",
                    "nato",
                    "taiwan",
                    "embargo",
                    "military",
                    "invasion",
                    "escalation",
                ],
            ) {
                score += 3;
            }
            if contains_any(
                &lower_text,
                &[
                    "oil",
                    "gold",
                    "commodity",
                    "energy",
                    "defense",
                    "risk-off",
                    "safe-haven",
                    "flight",
                ],
            ) {
                score += 2;
            }
            if contains_any(
                &lower_name,
                &[
                    "war", "iran", "conflict", "sanctions", "tariff", "taiwan", "brics",
                ],
            ) {
                score += 1;
            }
            score
        }
        _ => 0,
    }
}

/// Infer the direction a catalyst outcome would push a scenario (bullish, bearish, or mixed).
/// Direction reflects: "if this catalyst prints hot/strong, does it push this scenario up or down?"
fn infer_catalyst_direction(
    category: &str,
    scenario_name: &str,
    scenario_text: &str,
) -> &'static str {
    let lower_name = scenario_name.to_ascii_lowercase();
    let lower_text = scenario_text.to_ascii_lowercase();

    match category {
        "inflation" => {
            if contains_any(&lower_name, &["inflation", "stagflation"]) {
                "confirming" // hot data confirms inflation scenarios
            } else if contains_any(&lower_text, &["easing", "rally", "risk-on", "liquidity"]) {
                "opposing" // hot data opposes easing/rally scenarios
            } else {
                "mixed"
            }
        }
        "labor" => {
            if contains_any(&lower_name, &["recession"]) {
                "confirming" // weak jobs confirm recession
            } else if contains_any(&lower_text, &["rally", "risk-on"]) {
                "opposing" // weak jobs oppose rally
            } else {
                "mixed"
            }
        }
        "policy" => {
            if contains_any(&lower_text, &["easing", "cut", "dovish", "liquidity"]) {
                "confirming" // policy events can confirm easing path
            } else if contains_any(&lower_text, &["tightening", "hawkish"]) {
                "opposing"
            } else {
                "mixed"
            }
        }
        "growth" => {
            if contains_any(&lower_name, &["recession"]) {
                "confirming" // weak growth confirms recession
            } else {
                "mixed" // growth data can go either way
            }
        }
        "commodities" => {
            if contains_any(
                &lower_text,
                &["war", "iran", "conflict", "hormuz", "inflation"],
            ) {
                "confirming" // commodity spikes confirm war and inflation scenarios
            } else {
                "mixed"
            }
        }
        "geopolitical" => {
            // Check peace/de-escalation first (more specific than escalation substring match)
            if contains_any(
                &lower_text,
                &["peace", "de-escalation", "diplomacy", "truce", "ceasefire"],
            ) {
                "opposing" // peace developments oppose conflict scenarios
            } else if contains_any(
                &lower_text,
                &[
                    "war", "conflict", "iran", "hormuz", "escalation", "invasion", "sanctions",
                ],
            ) {
                "confirming" // geopolitical escalation confirms conflict/war scenarios
            } else {
                "mixed"
            }
        }
        _ => "mixed",
    }
}

fn link_predictions(event: &CalendarEvent, predictions: &[PredictionMarket]) -> Vec<String> {
    let event_tokens = keyword_set(&format!(
        "{} {} {}",
        event.name,
        event.event_type,
        event.symbol.clone().unwrap_or_default()
    ));
    let mut matches = predictions
        .iter()
        .filter_map(|prediction| {
            let mut score = overlap_score(&event_tokens, &keyword_set(&prediction.question));
            score += category_match_score(event, prediction.category);
            (score > 0).then_some((score, prediction.question.clone()))
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    matches
        .into_iter()
        .take(3)
        .map(|(_, question)| question)
        .collect()
}

fn category_match_score(event: &CalendarEvent, category: MarketCategory) -> usize {
    match (event_category(event).as_str(), category) {
        ("policy" | "inflation" | "labor" | "growth", MarketCategory::Economics) => 2,
        ("geopolitical", MarketCategory::Geopolitics) => 3,
        ("commodities", MarketCategory::Geopolitics) => 1,
        ("earnings", MarketCategory::AI) => 1,
        _ => 0,
    }
}

fn keyword_set(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter_map(|token| {
            let lower = token.trim().to_ascii_lowercase();
            if lower.len() < 3 || STOPWORDS.contains(&lower.as_str()) {
                None
            } else {
                Some(lower)
            }
        })
        .collect()
}

fn overlap_score(left: &HashSet<String>, right: &HashSet<String>) -> usize {
    left.intersection(right).count()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

static STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "this", "that", "will", "into", "rate", "data", "index",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::predictions::{MarketCategory, PredictionMarket};
    use crate::db::backend::BackendConnection;
    use crate::models::asset::AssetCategory;

    #[test]
    fn today_tomorrow_and_week_windows_filter_correctly() {
        let conn = crate::db::open_in_memory();
        let today = Utc::now().date_naive();
        let tomorrow = today + Duration::days(1);
        let next_week = today + Duration::days(5);

        db::calendar_cache::upsert_event(
            &conn,
            &today.format("%Y-%m-%d").to_string(),
            "FOMC Rate Decision",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        db::calendar_cache::upsert_event(
            &conn,
            &tomorrow.format("%Y-%m-%d").to_string(),
            "Consumer Price Index (CPI)",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        db::calendar_cache::upsert_event(
            &conn,
            &next_week.format("%Y-%m-%d").to_string(),
            "Retail Sales",
            "medium",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let today_report = build_report_backend(&backend, CatalystWindow::Today).unwrap();
        let tomorrow_report = build_report_backend(&backend, CatalystWindow::Tomorrow).unwrap();
        let week_report = build_report_backend(&backend, CatalystWindow::Week).unwrap();

        assert_eq!(today_report.catalysts.len(), 1);
        assert_eq!(tomorrow_report.catalysts.len(), 1);
        assert_eq!(week_report.catalysts.len(), 3);
    }

    #[test]
    fn ranking_and_linkage_are_portfolio_aware() {
        let conn = crate::db::open_in_memory();
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();

        db::watchlist::add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        db::calendar_cache::upsert_event(
            &conn,
            &today,
            "AAPL Earnings",
            "high",
            None,
            None,
            "earnings",
            Some("AAPL"),
        )
        .unwrap();
        db::calendar_cache::upsert_event(
            &conn,
            &today,
            "FOMC Rate Decision",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        let scenario_id = db::scenarios::add_scenario(
            &conn,
            "Fed Cuts",
            0.62,
            Some("Disinflation pushes the Fed toward easing"),
            Some("Bonds, growth equities"),
            Some("FOMC and CPI confirm easing pressure"),
            None,
        )
        .unwrap();
        db::scenarios::update_scenario(&conn, scenario_id, None, None, None, Some("active"))
            .unwrap();
        db::predictions_cache::upsert_predictions(
            &conn,
            &[PredictionMarket {
                id: "fed-cut".to_string(),
                question: "Will the Fed cut rates in June?".to_string(),
                probability: 0.58,
                volume_24h: 1_000_000.0,
                category: MarketCategory::Economics,
                updated_at: Utc::now().timestamp(),
            }],
        )
        .unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let report = build_report_backend(&backend, CatalystWindow::Today).unwrap();
        let aapl = report
            .catalysts
            .iter()
            .find(|row| row.title == "AAPL Earnings")
            .unwrap();
        let fomc = report
            .catalysts
            .iter()
            .find(|row| row.title == "FOMC Rate Decision")
            .unwrap();

        assert!(aapl.portfolio_relevance > fomc.portfolio_relevance);
        assert!(fomc
            .linked_scenarios
            .iter()
            .any(|row| row.name == "Fed Cuts"));
        assert!(!fomc.linked_predictions.is_empty());
    }

    #[test]
    fn category_semantic_matching_links_inflation_catalyst_to_inflation_scenario() {
        let conn = crate::db::open_in_memory();
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();

        db::calendar_cache::upsert_event(
            &conn,
            &today,
            "Core PCE Price Index",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        let scenario_id = db::scenarios::add_scenario(
            &conn,
            "Inflation Spike",
            0.70,
            Some("Oil-driven inflation acceleration. CPI heading to 4%+. Stagflation."),
            None,
            None,
            None,
        )
        .unwrap();
        db::scenarios::update_scenario(&conn, scenario_id, None, None, None, Some("active"))
            .unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let report = build_report_backend(&backend, CatalystWindow::Today).unwrap();
        assert_eq!(report.catalysts.len(), 1);

        let pce = &report.catalysts[0];
        assert!(
            !pce.linked_scenarios.is_empty(),
            "Core PCE should link to Inflation Spike via category semantics"
        );
        assert!(pce
            .linked_scenarios
            .iter()
            .any(|row| row.name == "Inflation Spike"));
    }

    #[test]
    fn labor_catalyst_links_to_recession_scenario() {
        let conn = crate::db::open_in_memory();
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();

        db::calendar_cache::upsert_event(
            &conn,
            &today,
            "Non-Farm Payrolls",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        let scenario_id = db::scenarios::add_scenario(
            &conn,
            "Hard Recession",
            0.55,
            Some("NFP collapse signals labor market breakdown. Consumer spending contraction."),
            None,
            None,
            None,
        )
        .unwrap();
        db::scenarios::update_scenario(&conn, scenario_id, None, None, None, Some("active"))
            .unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let report = build_report_backend(&backend, CatalystWindow::Today).unwrap();
        let nfp = &report.catalysts[0];
        assert!(
            nfp.linked_scenarios
                .iter()
                .any(|row| row.name == "Hard Recession"),
            "NFP should link to Hard Recession via labor-recession semantics"
        );
    }

    #[test]
    fn linked_scenario_has_direction_and_relevance() {
        let conn = crate::db::open_in_memory();
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();

        db::calendar_cache::upsert_event(
            &conn,
            &today,
            "Consumer Price Index (CPI)",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        let s1 = db::scenarios::add_scenario(
            &conn,
            "Inflation Spike",
            0.70,
            Some("CPI heading to 4%+. Stagflation scenario."),
            None,
            None,
            None,
        )
        .unwrap();
        db::scenarios::update_scenario(&conn, s1, None, None, None, Some("active")).unwrap();
        let s2 = db::scenarios::add_scenario(
            &conn,
            "Risk-On Rally",
            0.40,
            Some("Fed easing triggers liquidity flood. BTC/equities rally."),
            None,
            None,
            None,
        )
        .unwrap();
        db::scenarios::update_scenario(&conn, s2, None, None, None, Some("active")).unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let report = build_report_backend(&backend, CatalystWindow::Today).unwrap();
        let cpi = &report.catalysts[0];

        let inflation_link = cpi
            .linked_scenarios
            .iter()
            .find(|row| row.name == "Inflation Spike");
        assert!(inflation_link.is_some(), "CPI should link to Inflation Spike");
        let inflation_link = inflation_link.unwrap();
        assert_eq!(inflation_link.direction, "confirming");

        let rally_link = cpi
            .linked_scenarios
            .iter()
            .find(|row| row.name == "Risk-On Rally");
        assert!(rally_link.is_some(), "CPI should link to Risk-On Rally");
        let rally_link = rally_link.unwrap();
        assert_eq!(rally_link.direction, "opposing");
    }

    #[test]
    fn growth_catalyst_links_to_multiple_scenarios() {
        let conn = crate::db::open_in_memory();
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();

        db::calendar_cache::upsert_event(
            &conn,
            &today,
            "ISM Manufacturing PMI",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        let s1 = db::scenarios::add_scenario(
            &conn,
            "Hard Recession",
            0.55,
            Some("Manufacturing contraction deepens. GDP decline. Equities -20%."),
            None,
            None,
            None,
        )
        .unwrap();
        db::scenarios::update_scenario(&conn, s1, None, None, None, Some("active")).unwrap();
        let s2 = db::scenarios::add_scenario(
            &conn,
            "Risk-On Rally",
            0.40,
            Some("Fed cut triggers liquidity flood. Risk-on equities rally."),
            None,
            None,
            None,
        )
        .unwrap();
        db::scenarios::update_scenario(&conn, s2, None, None, None, Some("active")).unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let report = build_report_backend(&backend, CatalystWindow::Today).unwrap();
        let ism = &report.catalysts[0];
        assert!(
            ism.linked_scenarios.len() >= 2,
            "ISM PMI should link to both recession and rally scenarios: got {}",
            ism.linked_scenarios.len()
        );
    }

    #[test]
    fn category_scenario_score_returns_zero_for_unrelated() {
        assert_eq!(
            category_scenario_score("inflation", "Iran-US War", "military conflict in Middle East"),
            0
        );
        assert_eq!(
            category_scenario_score("commodities", "Risk-On Rally", "Fed easing triggers liquidity flood"),
            0
        );
    }

    #[test]
    fn linked_scenario_serializes_to_json() {
        let ls = LinkedScenario {
            name: "Inflation Spike".to_string(),
            direction: "confirming".to_string(),
            relevance: "strong".to_string(),
        };
        let json = serde_json::to_string(&ls).unwrap();
        assert!(json.contains("\"name\":\"Inflation Spike\""));
        assert!(json.contains("\"direction\":\"confirming\""));
        assert!(json.contains("\"relevance\":\"strong\""));
    }

    #[test]
    fn geopolitical_event_type_categorized_correctly() {
        let event = CalendarEvent {
            id: 1,
            date: "2026-04-06".to_string(),
            name: "Iran Hormuz Strait Deadline".to_string(),
            impact: "high".to_string(),
            previous: None,
            forecast: None,
            event_type: "geopolitical".to_string(),
            symbol: None,
            fetched_at: "2026-04-01".to_string(),
        };
        assert_eq!(event_category(&event), "geopolitical");
    }

    #[test]
    fn geopolitical_keywords_detected_in_economic_event() {
        let event = CalendarEvent {
            id: 2,
            date: "2026-05-01".to_string(),
            name: "BRICS Summit Opening".to_string(),
            impact: "medium".to_string(),
            previous: None,
            forecast: None,
            event_type: "economic".to_string(),
            symbol: None,
            fetched_at: "2026-04-01".to_string(),
        };
        assert_eq!(event_category(&event), "geopolitical");
    }

    #[test]
    fn geopolitical_proxy_assets_include_oil_and_gold() {
        let proxies = proxy_assets_for_event("Iran Hormuz Strait Deadline");
        assert!(
            proxies.contains(&"CL=F"),
            "Iran event should proxy to oil"
        );
        assert!(
            proxies.contains(&"GC=F"),
            "Iran event should proxy to gold"
        );
    }

    #[test]
    fn geopolitical_macro_significance_scores_high() {
        let event = CalendarEvent {
            id: 3,
            date: "2026-04-06".to_string(),
            name: "Iran Hormuz Strait Deadline".to_string(),
            impact: "high".to_string(),
            previous: None,
            forecast: None,
            event_type: "geopolitical".to_string(),
            symbol: None,
            fetched_at: "2026-04-01".to_string(),
        };
        let score = macro_significance(&event, "geopolitical", "this-week", 1, 0);
        // high impact (9) + geopolitical category (4) + this-week (1) + 1 scenario link + 0 predictions = 15
        assert_eq!(score, 15);
    }

    #[test]
    fn geopolitical_scenario_score_matches_war_scenarios() {
        let score = category_scenario_score(
            "geopolitical",
            "Iran-US Conflict Escalation",
            "military conflict in Hormuz Strait disrupts oil flows, gold safe-haven demand surges",
        );
        // Should match: "conflict" and "hormuz" keywords (3), "oil" and "gold" (2), "iran" and "conflict" in name (1) = 6
        assert!(
            score >= 5,
            "geopolitical should strongly link to war scenarios: got {}",
            score
        );
    }

    #[test]
    fn geopolitical_direction_confirms_war_scenarios() {
        let direction = infer_catalyst_direction(
            "geopolitical",
            "Iran Escalation",
            "war conflict iran hormuz military escalation",
        );
        assert_eq!(direction, "confirming");
    }

    #[test]
    fn geopolitical_direction_opposes_peace_scenarios() {
        let direction = infer_catalyst_direction(
            "geopolitical",
            "Diplomatic Resolution",
            "peace de-escalation diplomacy truce negotiations",
        );
        assert_eq!(direction, "opposing");
    }

    #[test]
    fn geopolitical_prediction_match_scores_high() {
        let event = CalendarEvent {
            id: 4,
            date: "2026-04-06".to_string(),
            name: "Iran Hormuz Deadline".to_string(),
            impact: "high".to_string(),
            previous: None,
            forecast: None,
            event_type: "geopolitical".to_string(),
            symbol: None,
            fetched_at: "2026-04-01".to_string(),
        };
        let score = category_match_score(&event, MarketCategory::Geopolitics);
        assert_eq!(score, 3, "geopolitical events should strongly match geopolitics predictions");
    }

    #[test]
    fn geopolitical_event_in_report() {
        let conn = crate::db::open_in_memory();
        let today = Utc::now().date_naive();

        db::calendar_cache::upsert_event(
            &conn,
            &today.format("%Y-%m-%d").to_string(),
            "Iran Hormuz Strait Deadline",
            "high",
            None,
            None,
            "geopolitical",
            None,
        )
        .unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let report = build_report_backend(&backend, CatalystWindow::Today).unwrap();
        assert_eq!(report.catalysts.len(), 1);
        assert_eq!(report.catalysts[0].category, "geopolitical");
        assert!(
            report.catalysts[0].score > 10,
            "geopolitical high-impact event should score >10: got {}",
            report.catalysts[0].score
        );
    }
}
