//! `pftui analytics narrative-divergence` — compare headline pressure with market pricing.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde::Serialize;

use crate::commands::news_sentiment;
use crate::db::agent_messages;
use crate::db::backend::BackendConnection;
use crate::db::news_cache::{self, NewsEntry, NewsSourceIndependence};
use crate::db::news_topic_markets;
use crate::db::predictions_history::{self, PredictionHistoryRecord};
use crate::db::scenario_contract_mappings::{self, EnrichedMapping};
use crate::db::scenarios::{self, Scenario};

const MAX_NEWS_ITEMS: usize = 2_000;

#[derive(Debug, Clone, Serialize)]
pub struct NarrativeDivergenceReport {
    pub generated_at: String,
    pub window_hours: i64,
    pub alert_threshold_z: f64,
    pub total_scenarios: usize,
    pub messages_emitted: usize,
    pub entries: Vec<NarrativeDivergenceEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NarrativeDivergenceEntry {
    pub scenario_id: i64,
    pub scenario_name: String,
    pub scenario_probability: f64,
    pub topic: String,
    pub article_count: usize,
    pub news_volume: f64,
    pub news_sentiment: f64,
    pub news_impact_z: f64,
    pub market_price: Option<f64>,
    pub market_delta_24h: Option<f64>,
    pub market_impact_z: f64,
    pub divergence_score: f64,
    pub label: String,
    pub bound_contracts: Vec<NarrativeMarketContract>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NarrativeMarketContract {
    pub contract_id: String,
    pub question: String,
    pub current_probability: Option<f64>,
    pub previous_probability: Option<f64>,
    pub delta_24h: Option<f64>,
}

#[derive(Debug, Clone)]
struct RawObservation {
    scenario: Scenario,
    topic: String,
    article_count: usize,
    news_volume: f64,
    news_sentiment: f64,
    news_signal: f64,
    market_price: Option<f64>,
    market_delta_24h: Option<f64>,
    market_signal: f64,
    bound_contracts: Vec<NarrativeMarketContract>,
}

pub fn run(
    backend: &BackendConnection,
    hours: i64,
    threshold_z: f64,
    json_output: bool,
) -> Result<()> {
    let mut report = build_report_backend(backend, hours, threshold_z)?;
    report.messages_emitted = emit_synthesis_messages(backend, &report)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }
    Ok(())
}

// R3 cull note: this command previously appended every computed row to
// `narrative_money_history` (here, from `data refresh`, and via a `rebuild`
// backfill subcommand). 107 rows accumulated and nothing ever read them —
// the live report below computes directly from news_cache + scenario
// contract mappings + predictions_history. The write path and the table
// were removed; see docs/DATA-ARCHITECTURE.md.

pub fn build_report_backend(
    backend: &BackendConnection,
    hours: i64,
    threshold_z: f64,
) -> Result<NarrativeDivergenceReport> {
    let scenarios = scenarios::list_scenarios_backend(backend, Some("active"))?;
    let news = news_cache::get_latest_news_backend(
        backend,
        MAX_NEWS_ITEMS,
        None,
        None,
        None,
        Some(hours),
    )?;
    let mappings = scenario_contract_mappings::list_enriched_backend(backend)?;
    let histories = load_prediction_histories_backend(backend, &mappings)?;
    build_report_from_parts(scenarios, news, mappings, histories, hours, threshold_z)
}

pub fn build_report_sqlite(
    conn: &Connection,
    hours: i64,
    threshold_z: f64,
) -> Result<NarrativeDivergenceReport> {
    let scenarios = scenarios::list_scenarios(conn, Some("active"))?;
    let news = news_cache::get_latest_news(conn, MAX_NEWS_ITEMS, None, None, None, Some(hours))?;
    let mappings = scenario_contract_mappings::list_enriched(conn)?;
    let histories = load_prediction_histories_sqlite(conn, &mappings)?;
    build_report_from_parts(scenarios, news, mappings, histories, hours, threshold_z)
}

fn build_report_from_parts(
    scenarios: Vec<Scenario>,
    news: Vec<NewsEntry>,
    mappings: Vec<EnrichedMapping>,
    histories: HashMap<String, Vec<PredictionHistoryRecord>>,
    hours: i64,
    threshold_z: f64,
) -> Result<NarrativeDivergenceReport> {
    let today = Utc::now().date_naive().to_string();
    let mut mappings_by_scenario: HashMap<i64, Vec<EnrichedMapping>> = HashMap::new();
    for mapping in mappings {
        mappings_by_scenario
            .entry(mapping.scenario_id)
            .or_default()
            .push(mapping);
    }

    let raw = scenarios
        .into_iter()
        .map(|scenario| {
            let topic = scenario_topic(&scenario);
            let topic_news = news
                .iter()
                .filter(|entry| entry.topic == topic)
                .collect::<Vec<_>>();
            let (article_count, news_volume, news_sentiment, news_signal) =
                score_topic_news(&topic_news);
            let (market_price, market_delta_24h, market_signal, bound_contracts) =
                scenario_market_signal(
                    mappings_by_scenario
                        .get(&scenario.id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    &histories,
                    &today,
                );
            RawObservation {
                scenario,
                topic,
                article_count,
                news_volume,
                news_sentiment,
                news_signal,
                market_price,
                market_delta_24h,
                market_signal,
                bound_contracts,
            }
        })
        .collect::<Vec<_>>();

    let news_z = z_scores(&raw.iter().map(|row| row.news_signal).collect::<Vec<_>>());
    let market_z = z_scores(&raw.iter().map(|row| row.market_signal).collect::<Vec<_>>());

    let mut entries = raw
        .into_iter()
        .enumerate()
        .map(|(idx, row)| {
            let divergence_score = news_z[idx] - market_z[idx];
            NarrativeDivergenceEntry {
                scenario_id: row.scenario.id,
                scenario_name: row.scenario.name,
                scenario_probability: row.scenario.probability,
                topic: row.topic,
                article_count: row.article_count,
                news_volume: round2(row.news_volume),
                news_sentiment: round2(row.news_sentiment),
                news_impact_z: round2(news_z[idx]),
                market_price: row.market_price.map(round2),
                market_delta_24h: row.market_delta_24h.map(round2),
                market_impact_z: round2(market_z[idx]),
                divergence_score: round2(divergence_score),
                label: divergence_label(divergence_score),
                bound_contracts: row.bound_contracts,
            }
        })
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| {
        b.divergence_score
            .abs()
            .partial_cmp(&a.divergence_score.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(NarrativeDivergenceReport {
        generated_at: Utc::now().to_rfc3339(),
        window_hours: hours,
        alert_threshold_z: threshold_z,
        total_scenarios: entries.len(),
        messages_emitted: 0,
        entries,
    })
}

fn scenario_topic(scenario: &Scenario) -> String {
    let mut snippets = Vec::new();
    if let Some(value) = &scenario.triggers {
        snippets.push(value.clone());
    }
    if let Some(value) = &scenario.historical_precedent {
        snippets.push(value.clone());
    }
    if let Some(value) = &scenario.asset_impact {
        snippets.push(value.clone());
    }
    news_topic_markets::classify_news_topic(
        &scenario.name,
        "macro",
        scenario.description.as_deref(),
        &snippets,
    )
}

fn score_topic_news(entries: &[&NewsEntry]) -> (usize, f64, f64, f64) {
    let mut weighted_count = 0.0;
    let mut weighted_sentiment = 0.0;
    let mut narrative_signal = 0.0;
    for entry in entries {
        let weight = source_weight(entry);
        let scored = news_sentiment::score_news(entry);
        weighted_count += weight;
        weighted_sentiment += weight * scored.score as f64;
        narrative_signal += weight * (1.0 + (scored.score as f64).abs() / 100.0);
    }
    let avg_sentiment = if weighted_count > 0.0 {
        weighted_sentiment / weighted_count
    } else {
        0.0
    };
    (
        entries.len(),
        weighted_count,
        avg_sentiment,
        narrative_signal,
    )
}

fn source_weight(entry: &NewsEntry) -> f64 {
    let tier_weight = match entry.source_tier {
        1 => 1.0,
        2 => 0.7,
        3 => 0.4,
        4 => 0.2,
        _ => 0.3,
    };
    let independence_weight = match entry.source_independence {
        NewsSourceIndependence::Independent => 1.0,
        NewsSourceIndependence::Wire => 0.9,
        NewsSourceIndependence::Unknown => 0.5,
        NewsSourceIndependence::Restatement => 0.35,
        NewsSourceIndependence::Rumor => 0.2,
    };
    tier_weight * independence_weight
}

fn scenario_market_signal(
    mappings: &[EnrichedMapping],
    histories: &HashMap<String, Vec<PredictionHistoryRecord>>,
    today: &str,
) -> (Option<f64>, Option<f64>, f64, Vec<NarrativeMarketContract>) {
    let mut contracts = Vec::new();
    let mut current_values = Vec::new();
    let mut delta_values = Vec::new();

    for mapping in mappings {
        let current_probability = if mapping.contract_question == "(contract not found)"
            && mapping.contract_probability == 0.0
        {
            None
        } else {
            Some(probability_pct(mapping.contract_probability))
        };
        let previous_probability = histories
            .get(&mapping.contract_id)
            .and_then(|rows| previous_probability(rows, today));
        let delta_24h = match (current_probability, previous_probability) {
            (Some(current), Some(previous)) => Some(current - previous),
            _ => None,
        };
        if let Some(current) = current_probability {
            current_values.push(current);
        }
        if let Some(delta) = delta_24h {
            delta_values.push(delta);
        }
        contracts.push(NarrativeMarketContract {
            contract_id: mapping.contract_id.clone(),
            question: mapping.contract_question.clone(),
            current_probability: current_probability.map(round2),
            previous_probability: previous_probability.map(round2),
            delta_24h: delta_24h.map(round2),
        });
    }

    let market_price = average(&current_values);
    let market_delta = average(&delta_values);
    let market_signal = market_delta.unwrap_or(0.0).abs();
    (market_price, market_delta, market_signal, contracts)
}

fn previous_probability(rows: &[PredictionHistoryRecord], today: &str) -> Option<f64> {
    rows.iter()
        .find(|row| row.date.as_str() < today)
        .or_else(|| rows.first())
        .map(|row| probability_pct(row.probability))
}

fn probability_pct(value: f64) -> f64 {
    if value.abs() <= 1.0 {
        value * 100.0
    } else {
        value
    }
}

fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn load_prediction_histories_backend(
    backend: &BackendConnection,
    mappings: &[EnrichedMapping],
) -> Result<HashMap<String, Vec<PredictionHistoryRecord>>> {
    let mut out = HashMap::new();
    let ids = mappings
        .iter()
        .map(|mapping| mapping.contract_id.clone())
        .collect::<HashSet<_>>();
    for id in ids {
        out.insert(
            id.clone(),
            predictions_history::get_history_backend(backend, &id, 8)?,
        );
    }
    Ok(out)
}

fn load_prediction_histories_sqlite(
    conn: &Connection,
    mappings: &[EnrichedMapping],
) -> Result<HashMap<String, Vec<PredictionHistoryRecord>>> {
    let mut out = HashMap::new();
    let ids = mappings
        .iter()
        .map(|mapping| mapping.contract_id.clone())
        .collect::<HashSet<_>>();
    for id in ids {
        out.insert(id.clone(), predictions_history::get_history(conn, &id, 8)?);
    }
    Ok(out)
}

fn z_scores(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| {
            let diff = value - mean;
            diff * diff
        })
        .sum::<f64>()
        / values.len() as f64;
    let std_dev = variance.sqrt();
    if std_dev < f64::EPSILON {
        return vec![0.0; values.len()];
    }
    values
        .iter()
        .map(|value| (value - mean) / std_dev)
        .collect()
}

pub fn divergence_label(score: f64) -> String {
    if score.abs() < 1.0 {
        "→ aligned".to_string()
    } else if score > 0.0 {
        format!("↑ narrative leading by {:.1}σ", score.abs())
    } else {
        format!("↓ money leading by {:.1}σ", score.abs())
    }
}

fn emit_synthesis_messages(
    backend: &BackendConnection,
    report: &NarrativeDivergenceReport,
) -> Result<usize> {
    let today = Utc::now().date_naive().to_string();
    let mut emitted = 0usize;
    for entry in &report.entries {
        if entry.divergence_score.abs() < report.alert_threshold_z {
            continue;
        }
        let package_id = format!("narrative-money:{}:{today}", entry.scenario_id);
        let existing = agent_messages::list_messages_backend(
            backend,
            Some("pftui"),
            Some("synthesis"),
            None,
            false,
            Some(&today),
            Some(&package_id),
            Some(1),
        )?;
        if !existing.is_empty() {
            continue;
        }
        let content = format!(
            "Narrative-vs-money divergence crossed {threshold:.1}σ for {scenario}: {label}. \
             Topic {topic}; weighted news volume {news_volume:.2}, sentiment {sentiment:.1}; \
             market price {market_price}, 24h market delta {market_delta}. Review before the next synthesis.",
            threshold = report.alert_threshold_z,
            scenario = entry.scenario_name,
            label = entry.label,
            topic = entry.topic,
            news_volume = entry.news_volume,
            sentiment = entry.news_sentiment,
            market_price = entry
                .market_price
                .map(|value| format!("{value:.1}%"))
                .unwrap_or_else(|| "unavailable".to_string()),
            market_delta = entry
                .market_delta_24h
                .map(|value| format!("{value:+.1}pp"))
                .unwrap_or_else(|| "unavailable".to_string()),
        );
        agent_messages::send_message_backend(
            backend,
            "pftui",
            Some("synthesis"),
            Some("high"),
            &content,
            Some("narrative_money_divergence"),
            Some("synthesis"),
            Some(&package_id),
            Some("Narrative vs money divergence"),
        )?;
        emitted += 1;
    }
    Ok(emitted)
}

fn print_text(report: &NarrativeDivergenceReport) {
    println!("Narrative vs Money Divergence");
    println!("════════════════════════════════════════════════════════════════");
    println!(
        "{} active scenario(s) • {}h news window • alert threshold ±{:.1}σ",
        report.total_scenarios, report.window_hours, report.alert_threshold_z
    );
    if report.entries.is_empty() {
        println!("No active scenarios found.");
        return;
    }
    println!();
    println!(
        "{:<28} {:<15} {:>8} {:>8} {:>8}  Label",
        "Scenario", "Topic", "NewsZ", "MktZ", "DivZ"
    );
    println!("{}", "-".repeat(92));
    for entry in &report.entries {
        println!(
            "{:<28} {:<15} {:>8.2} {:>8.2} {:>8.2}  {}",
            truncate(&entry.scenario_name, 28),
            truncate(&entry.topic, 15),
            entry.news_impact_z,
            entry.market_impact_z,
            entry.divergence_score,
            entry.label
        );
    }
    if report.messages_emitted > 0 {
        println!();
        println!(
            "Emitted {} synthesis message(s) for threshold crossings.",
            report.messages_emitted
        );
    }
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        value.to_string()
    } else {
        value
            .chars()
            .take(max_len.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::prediction_contracts::{self, PredictionContract};
    use crate::db::schema::run_migrations;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn add_active_scenario(conn: &Connection, name: &str, probability: f64) -> i64 {
        let id = scenarios::add_scenario(conn, name, probability, None, None, None, None).unwrap();
        scenarios::update_scenario(conn, id, None, None, None, Some("active")).unwrap();
        id
    }

    fn add_contract(conn: &Connection, id: &str, question: &str, price: f64) {
        prediction_contracts::upsert_contracts(
            conn,
            &[PredictionContract {
                contract_id: id.to_string(),
                exchange: "polymarket".to_string(),
                event_id: format!("event-{id}"),
                event_title: question.to_string(),
                question: question.to_string(),
                category: "macro".to_string(),
                last_price: price,
                volume_24h: 1_000.0,
                liquidity: 5_000.0,
                end_date: None,
                updated_at: 1_711_670_000,
            }],
        )
        .unwrap();
    }

    fn add_news(conn: &Connection, title: &str, url: &str, category: &str, published_at: i64) {
        news_cache::insert_news_with_source_type(
            conn,
            title,
            url,
            "Reuters",
            "rss",
            None,
            category,
            published_at,
            Some("Escalation risk"),
            &[],
        )
        .unwrap();
    }

    #[test]
    fn divergence_formula_separates_narrative_and_money_leaders() {
        let conn = setup();
        let now = Utc::now().timestamp();
        let today = Utc::now().date_naive();
        let yesterday = (today - chrono::Duration::days(1)).to_string();
        let iran = add_active_scenario(&conn, "Iran Hormuz escalation", 25.0);
        let fed = add_active_scenario(&conn, "Fed policy repricing", 40.0);
        let growth = add_active_scenario(&conn, "US recession growth scare", 35.0);

        add_contract(&conn, "iran-contract", "Iran ceasefire?", 0.10);
        add_contract(&conn, "fed-contract", "Fed cuts rates?", 0.70);
        add_contract(&conn, "growth-contract", "US recession?", 0.35);
        scenario_contract_mappings::add_mapping(&conn, iran, "iran-contract").unwrap();
        scenario_contract_mappings::add_mapping(&conn, fed, "fed-contract").unwrap();
        scenario_contract_mappings::add_mapping(&conn, growth, "growth-contract").unwrap();
        predictions_history::insert_history(&conn, "iran-contract", &yesterday, 0.10).unwrap();
        predictions_history::insert_history(&conn, "fed-contract", &yesterday, 0.30).unwrap();
        predictions_history::insert_history(&conn, "growth-contract", &yesterday, 0.35).unwrap();

        add_news(
            &conn,
            "Oil jumps as Iran threatens Hormuz shipping escalation",
            "https://reuters.com/iran-hormuz-1",
            "geopolitics",
            now,
        );
        add_news(
            &conn,
            "War risk rises after Iran strike pressure",
            "https://reuters.com/iran-hormuz-2",
            "geopolitics",
            now,
        );

        let report = build_report_sqlite(&conn, 24, 2.0).unwrap();
        let iran_row = report
            .entries
            .iter()
            .find(|entry| entry.scenario_id == iran)
            .unwrap();
        let fed_row = report
            .entries
            .iter()
            .find(|entry| entry.scenario_id == fed)
            .unwrap();
        assert!(iran_row.divergence_score > 0.0, "{iran_row:?}");
        assert!(fed_row.divergence_score < 0.0, "{fed_row:?}");
        assert!(iran_row.label.contains("narrative leading"));
        assert!(fed_row.label.contains("money leading"));
    }

    #[test]
    fn threshold_crossing_emits_deduped_agent_message() {
        let conn = setup();
        let now = Utc::now().timestamp();
        let scenario_id = add_active_scenario(&conn, "Iran Hormuz escalation", 25.0);
        let quiet_id = add_active_scenario(&conn, "US recession growth scare", 35.0);
        let policy_id = add_active_scenario(&conn, "Fed policy repricing", 40.0);
        add_contract(&conn, "iran-contract", "Iran ceasefire?", 0.10);
        add_contract(&conn, "quiet-contract", "US recession?", 0.10);
        add_contract(&conn, "policy-contract", "Fed cuts rates?", 0.40);
        scenario_contract_mappings::add_mapping(&conn, scenario_id, "iran-contract").unwrap();
        scenario_contract_mappings::add_mapping(&conn, quiet_id, "quiet-contract").unwrap();
        scenario_contract_mappings::add_mapping(&conn, policy_id, "policy-contract").unwrap();
        add_news(
            &conn,
            "Oil jumps as Iran threatens Hormuz shipping escalation",
            "https://reuters.com/iran-hormuz-message",
            "geopolitics",
            now,
        );

        let backend = BackendConnection::Sqlite { conn };
        let report = build_report_backend(&backend, 24, 1.0).unwrap();
        assert_eq!(emit_synthesis_messages(&backend, &report).unwrap(), 1);
        assert_eq!(emit_synthesis_messages(&backend, &report).unwrap(), 0);
        let messages = agent_messages::list_messages(
            backend.sqlite(),
            Some("pftui"),
            Some("synthesis"),
            Some("synthesis"),
            false,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].category.as_deref(),
            Some("narrative_money_divergence")
        );
    }
}
