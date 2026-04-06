use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::collections::HashSet;

use crate::analytics::catalysts::{self, CatalystWindow};
use crate::commands::news_sentiment::{score_all, ScoredNews, SentimentLabel};
use crate::db::backend::BackendConnection;
use crate::db::news_cache;
use crate::db::scenarios::{self, Scenario};

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioDetectionReport {
    pub generated_at: String,
    pub hours: i64,
    pub suggestions: Vec<ScenarioSuggestion>,
    pub summary: ScenarioDetectionSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioSuggestion {
    pub key: String,
    pub title: String,
    pub probability: f64,
    pub score: i32,
    pub description: String,
    pub triggers: String,
    pub asset_impact: String,
    pub precedent: Option<String>,
    pub duplicate_of: Option<String>,
    pub evidence: Vec<DetectionEvidence>,
    pub add_command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectionEvidence {
    pub kind: String,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioDetectionSummary {
    pub suggestions: usize,
    pub duplicates_suppressed: usize,
    pub scanned_news: usize,
    pub matching_news: usize,
    pub matching_catalysts: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SentimentBias {
    Bullish,
    Bearish,
}

#[derive(Debug, Clone)]
struct ThemeDefinition {
    key: &'static str,
    title: &'static str,
    probability: f64,
    description: &'static str,
    triggers: &'static str,
    asset_impact: &'static str,
    precedent: Option<&'static str>,
    keywords: &'static [&'static str],
    catalyst_categories: &'static [&'static str],
    catalyst_keywords: &'static [&'static str],
    sentiment_bias: SentimentBias,
}

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "for", "from", "in", "into", "is", "of", "on",
    "or", "that", "the", "to", "with",
];

const THEMES: &[ThemeDefinition] = &[
    ThemeDefinition {
        key: "trade-war-escalation",
        title: "Trade War Escalation",
        probability: 35.0,
        description: "Tariff and sanctions pressure intensifies, raising stagflation and supply-chain risk.",
        triggers: "New tariffs, retaliatory trade actions, sanctions escalation, shipping disruption.",
        asset_impact: "Bullish gold and oil exporters; bearish global cyclicals, semis, and import-dependent equities.",
        precedent: Some("2018-2019 US-China trade war"),
        keywords: &["tariff", "trade war", "sanction", "retaliat", "embargo", "export control"],
        catalyst_categories: &["geopolitical", "commodities", "policy"],
        catalyst_keywords: &["tariff", "sanction", "trade", "summit", "embargo"],
        sentiment_bias: SentimentBias::Bearish,
    },
    ThemeDefinition {
        key: "oil-shock-escalation",
        title: "Oil Shock From Geopolitical Escalation",
        probability: 30.0,
        description: "Conflict or transit disruption drives a sharp oil-risk premium and broad inflation spillover.",
        triggers: "Middle East escalation, Hormuz disruption, refinery outage, OPEC supply surprise.",
        asset_impact: "Bullish oil, defense, and gold; bearish transports, consumer discretionary, and duration.",
        precedent: Some("1973 oil embargo / 2022 energy shock"),
        keywords: &["oil", "hormuz", "opec", "refinery", "drone strike", "missile", "shipping lane"],
        catalyst_categories: &["geopolitical", "commodities"],
        catalyst_keywords: &["oil", "opec", "inventory", "hormuz", "iran", "war"],
        sentiment_bias: SentimentBias::Bearish,
    },
    ThemeDefinition {
        key: "inflation-reacceleration",
        title: "Inflation Re-acceleration",
        probability: 40.0,
        description: "Inflation stops cooling and re-accelerates, delaying cuts and pressuring duration-sensitive assets.",
        triggers: "Hot CPI/PCE prints, sticky services inflation, commodity spikes, wage pressure.",
        asset_impact: "Bullish commodities and value; bearish long-duration bonds, growth equities, and rate-cut trades.",
        precedent: Some("2021-2022 inflation repricing"),
        keywords: &["inflation", "cpi", "pce", "price pressure", "wage growth", "sticky"],
        catalyst_categories: &["inflation", "commodities", "labor"],
        catalyst_keywords: &["cpi", "pce", "inflation", "payroll", "wages"],
        sentiment_bias: SentimentBias::Bearish,
    },
    ThemeDefinition {
        key: "hawkish-policy-repricing",
        title: "Hawkish Policy Repricing",
        probability: 35.0,
        description: "Central banks lean more restrictive than markets expect, repricing rate-cut assumptions.",
        triggers: "Fed speakers turn hawkish, FOMC dot plot shifts up, stronger-than-expected macro data.",
        asset_impact: "Bullish USD and front-end yields; bearish high-beta equities, REITs, and speculative assets.",
        precedent: Some("2022 post-Jackson Hole repricing"),
        keywords: &["hawkish", "rate hike", "higher for longer", "fomc", "fed", "dot plot"],
        catalyst_categories: &["policy", "inflation", "labor"],
        catalyst_keywords: &["fomc", "fed", "rate", "central bank", "powell"],
        sentiment_bias: SentimentBias::Bearish,
    },
    ThemeDefinition {
        key: "growth-rollover",
        title: "Growth Rollover",
        probability: 30.0,
        description: "Growth data deteriorates quickly enough to revive recession fears and defensive rotation.",
        triggers: "Weak payrolls, PMI contraction, GDP misses, rising unemployment, soft retail sales.",
        asset_impact: "Bullish bonds and defensives; bearish cyclicals, small caps, and industrial commodities.",
        precedent: Some("2019 and 2020 growth scares"),
        keywords: &["recession", "slowdown", "contraction", "unemployment", "layoff", "gdp miss", "pmi"],
        catalyst_categories: &["growth", "labor"],
        catalyst_keywords: &["gdp", "retail", "pmi", "payroll", "jobs", "unemployment"],
        sentiment_bias: SentimentBias::Bearish,
    },
    ThemeDefinition {
        key: "liquidity-reflation",
        title: "Liquidity Reflation",
        probability: 30.0,
        description: "Policy easing or fiscal support restores liquidity and revives risk appetite faster than expected.",
        triggers: "Rate-cut signaling, stimulus package, liquidity injections, ceasefire or de-escalation.",
        asset_impact: "Bullish equities, crypto, and cyclicals; bearish USD and defensive havens.",
        precedent: Some("2019 mid-cycle easing / 2020 policy reflation"),
        keywords: &["rate cut", "easing", "stimulus", "liquidity", "ceasefire", "deal", "agreement"],
        catalyst_categories: &["policy", "growth", "geopolitical"],
        catalyst_keywords: &["rate", "fed", "stimulus", "ceasefire", "deal", "agreement"],
        sentiment_bias: SentimentBias::Bullish,
    },
];

pub fn run(
    backend: &BackendConnection,
    hours: i64,
    limit: usize,
    json_output: bool,
) -> Result<()> {
    let report = detect_scenarios(backend, hours, limit)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    if report.suggestions.is_empty() {
        println!(
            "No new scenario suggestions detected from the last {}h of news/catalysts.",
            report.hours
        );
        if report.summary.duplicates_suppressed > 0 {
            println!(
                "{} candidate(s) matched existing active scenarios and were suppressed.",
                report.summary.duplicates_suppressed
            );
        }
        return Ok(());
    }

    println!(
        "Scenario Detection — {} suggestion(s) from {} news items\n",
        report.suggestions.len(),
        report.summary.scanned_news
    );

    for suggestion in &report.suggestions {
        println!(
            "{} (score {}, seed {:.0}%)",
            suggestion.title, suggestion.score, suggestion.probability
        );
        println!("  {}", suggestion.description);
        println!("  Triggers: {}", suggestion.triggers);
        println!("  Impact:   {}", suggestion.asset_impact);
        if let Some(precedent) = &suggestion.precedent {
            println!("  Precedent: {}", precedent);
        }
        println!("  Add: {}", suggestion.add_command);
        for evidence in suggestion.evidence.iter().take(3) {
            println!("    [{}] {} — {}", evidence.kind, evidence.title, evidence.detail);
        }
        println!();
    }

    if report.summary.duplicates_suppressed > 0 {
        println!(
            "Suppressed {} duplicate candidate(s) because similar active scenarios already exist.",
            report.summary.duplicates_suppressed
        );
    }

    Ok(())
}

pub fn detect_scenarios(
    backend: &BackendConnection,
    hours: i64,
    limit: usize,
) -> Result<ScenarioDetectionReport> {
    let news = news_cache::get_latest_news_backend(backend, 200, None, None, None, Some(hours))?;
    let scored_news = score_all(&news);
    let catalysts = catalysts::build_report_backend(backend, CatalystWindow::Week)
        .map(|report| report.catalysts)
        .unwrap_or_default();
    let active_scenarios = scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();

    let mut suggestions = Vec::new();
    let mut duplicates_suppressed = 0usize;
    let mut matching_news = 0usize;
    let mut matching_catalysts = 0usize;

    for theme in THEMES {
        let news_matches = matching_news_items(theme, &scored_news);
        let catalyst_matches = matching_catalysts_for_theme(theme, &catalysts);

        if news_matches.is_empty() && catalyst_matches.is_empty() {
            continue;
        }

        matching_news += news_matches.len();
        matching_catalysts += catalyst_matches.len();

        let score = theme_score(theme, &news_matches, &catalyst_matches);
        if score < 10 {
            continue;
        }

        if find_similar_scenario(theme, &active_scenarios).is_some() {
            duplicates_suppressed += 1;
            continue;
        }

        let mut evidence = Vec::new();
        for item in news_matches.iter().take(3) {
            evidence.push(DetectionEvidence {
                kind: "news".to_string(),
                title: item.entry.title.clone(),
                detail: format!(
                    "{} sentiment (score {})",
                    item.label.as_str(),
                    item.score
                ),
                url: Some(item.entry.url.clone()),
            });
        }
        for catalyst in catalyst_matches.iter().take(2) {
            evidence.push(DetectionEvidence {
                kind: "catalyst".to_string(),
                title: catalyst.title.clone(),
                detail: format!(
                    "{} / {} / score {}",
                    catalyst.category, catalyst.countdown_bucket, catalyst.score
                ),
                url: None,
            });
        }

        suggestions.push(ScenarioSuggestion {
            key: theme.key.to_string(),
            title: theme.title.to_string(),
            probability: theme.probability,
            score,
            description: theme.description.to_string(),
            triggers: theme.triggers.to_string(),
            asset_impact: theme.asset_impact.to_string(),
            precedent: theme.precedent.map(str::to_string),
            duplicate_of: None,
            evidence,
            add_command: build_add_command(theme),
        });
    }

    suggestions.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.title.cmp(&right.title))
    });
    suggestions.truncate(limit);

    Ok(ScenarioDetectionReport {
        generated_at: Utc::now().to_rfc3339(),
        hours,
        summary: ScenarioDetectionSummary {
            suggestions: suggestions.len(),
            duplicates_suppressed,
            scanned_news: news.len(),
            matching_news,
            matching_catalysts,
        },
        suggestions,
    })
}

fn build_add_command(theme: &ThemeDefinition) -> String {
    format!(
        "pftui journal scenario add \"{}\" --probability {:.0} --description \"{}\" --impact \"{}\" --triggers \"{}\"",
        theme.title, theme.probability, theme.description, theme.asset_impact, theme.triggers
    )
}

fn matching_news_items<'a>(theme: &ThemeDefinition, items: &'a [ScoredNews]) -> Vec<&'a ScoredNews> {
    items.iter()
        .filter(|item| news_matches_theme(theme, item))
        .collect()
}

fn news_matches_theme(theme: &ThemeDefinition, item: &ScoredNews) -> bool {
    if !sentiment_matches(theme.sentiment_bias, item.label) {
        return false;
    }
    let haystack = combined_text(&item.entry.title, &item.entry.description, &item.entry.extra_snippets);
    theme.keywords.iter().any(|keyword| haystack.contains(&keyword.to_ascii_lowercase()))
}

fn matching_catalysts_for_theme<'a>(
    theme: &ThemeDefinition,
    catalysts: &'a [catalysts::CatalystEvent],
) -> Vec<&'a catalysts::CatalystEvent> {
    catalysts
        .iter()
        .filter(|event| catalyst_matches_theme(theme, event))
        .collect()
}

fn catalyst_matches_theme(theme: &ThemeDefinition, event: &catalysts::CatalystEvent) -> bool {
    if theme
        .catalyst_categories
        .iter()
        .any(|category| event.category.eq_ignore_ascii_case(category))
    {
        return true;
    }
    let title = event.title.to_ascii_lowercase();
    theme
        .catalyst_keywords
        .iter()
        .any(|keyword| title.contains(&keyword.to_ascii_lowercase()))
}

fn theme_score(
    theme: &ThemeDefinition,
    news_matches: &[&ScoredNews],
    catalyst_matches: &[&catalysts::CatalystEvent],
) -> i32 {
    let news_points = news_matches
        .iter()
        .map(|item| 4 + (item.score.abs() / 20))
        .sum::<i32>()
        .min(20);
    let catalyst_points = catalyst_matches
        .iter()
        .map(|event| {
            let impact_bonus = if event.significance.eq_ignore_ascii_case("high") { 5 } else { 2 };
            (impact_bonus + (event.score / 10)).max(2)
        })
        .sum::<i32>()
        .min(16);
    let mixed_bonus = if !news_matches.is_empty() && !catalyst_matches.is_empty() {
        6
    } else {
        0
    };

    let match_floor = if news_matches.len() >= 2 { 4 } else { 0 };
    let category_bonus = if theme.catalyst_categories.contains(&"geopolitical") && catalyst_matches.len() >= 2 {
        3
    } else {
        0
    };

    news_points + catalyst_points + mixed_bonus + match_floor + category_bonus
}

fn find_similar_scenario<'a>(
    theme: &ThemeDefinition,
    active_scenarios: &'a [Scenario],
) -> Option<&'a Scenario> {
    let theme_tokens = token_set(&format!("{} {}", theme.title, theme.description));
    active_scenarios.iter().find(|scenario| {
        let scenario_tokens = token_set(&format!(
            "{} {} {} {}",
            scenario.name,
            scenario.description.as_deref().unwrap_or(""),
            scenario.asset_impact.as_deref().unwrap_or(""),
            scenario.triggers.as_deref().unwrap_or("")
        ));
        overlap_ratio(&theme_tokens, &scenario_tokens) >= 0.45
    })
}

fn combined_text(title: &str, description: &str, snippets: &[String]) -> String {
    let mut combined = String::new();
    combined.push_str(&title.to_ascii_lowercase());
    combined.push(' ');
    combined.push_str(&description.to_ascii_lowercase());
    if !snippets.is_empty() {
        combined.push(' ');
        combined.push_str(
            &snippets
                .iter()
                .map(|item| item.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    combined
}

fn sentiment_matches(bias: SentimentBias, label: SentimentLabel) -> bool {
    match bias {
        SentimentBias::Bullish => label == SentimentLabel::Bullish,
        SentimentBias::Bearish => label == SentimentLabel::Bearish,
    }
}

fn token_set(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|token| {
            let lowered = token.trim().to_ascii_lowercase();
            if lowered.len() < 3 || STOPWORDS.contains(&lowered.as_str()) {
                None
            } else {
                Some(lowered)
            }
        })
        .collect()
}

fn overlap_ratio(left: &HashSet<String>, right: &HashSet<String>) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(right).count() as f64;
    let denominator = left.len().min(right.len()) as f64;
    intersection / denominator
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::news_cache::NewsEntry;

    #[test]
    fn overlap_ratio_detects_similar_scenarios() {
        let left = token_set("Trade War Escalation tariff sanctions supply chain");
        let right = token_set("Tariff escalation and sanctions pressure on supply chains");
        assert!(overlap_ratio(&left, &right) >= 0.45);
    }

    #[test]
    fn detect_scenarios_suggests_new_theme_from_news_and_catalyst() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        news_cache::insert_news_backend(
            &backend,
            "New tariff escalation sparks global sell-off fears",
            "https://example.com/tariff-1",
            "Reuters",
            "macro",
            Utc::now().timestamp(),
        )
        .unwrap();
        news_cache::insert_news_backend(
            &backend,
            "Sanctions and export controls deepen trade war pressure",
            "https://example.com/tariff-2",
            "Reuters",
            "macro",
            Utc::now().timestamp(),
        )
        .unwrap();
        crate::db::calendar_cache::upsert_event_backend(
            &backend,
            &Utc::now().date_naive().format("%Y-%m-%d").to_string(),
            "Tariff summit and sanctions review",
            "high",
            None,
            None,
            "geopolitical",
            None,
        )
        .unwrap();

        let report = detect_scenarios(&backend, 72, 5).unwrap();
        assert!(
            report
                .suggestions
                .iter()
                .any(|item| item.title == "Trade War Escalation")
        );
    }

    #[test]
    fn detect_scenarios_suppresses_duplicate_active_scenario() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        news_cache::insert_news_backend(
            &backend,
            "Tariff escalation drives trade war fear",
            "https://example.com/tariff-3",
            "Reuters",
            "macro",
            Utc::now().timestamp(),
        )
        .unwrap();
        crate::db::calendar_cache::upsert_event_backend(
            &backend,
            &Utc::now().date_naive().format("%Y-%m-%d").to_string(),
            "Trade sanctions review",
            "high",
            None,
            None,
            "geopolitical",
            None,
        )
        .unwrap();
        scenarios::add_scenario_backend(
            &backend,
            "Trade War Escalation",
            40.0,
            Some("Tariff and sanctions pressure rises."),
            Some("Bearish cyclicals."),
            Some("Tariff retaliation."),
            None,
        )
        .unwrap();

        let report = detect_scenarios(&backend, 72, 5).unwrap();
        assert!(report.suggestions.is_empty());
        assert_eq!(report.summary.duplicates_suppressed, 1);
    }

    #[test]
    fn bullish_theme_requires_bullish_news() {
        let entry = NewsEntry {
            id: 1,
            title: "Rate cut and stimulus deal lifts risk appetite".to_string(),
            url: "https://example.com/liquidity".to_string(),
            source: "Reuters".to_string(),
            source_type: "rss".to_string(),
            symbol_tag: None,
            description: "Markets rally on easing hopes.".to_string(),
            extra_snippets: Vec::new(),
            category: "macro".to_string(),
            published_at: Utc::now().timestamp(),
            fetched_at: Utc::now().to_rfc3339(),
        };
        let scored = crate::commands::news_sentiment::score_news(&entry);
        assert!(news_matches_theme(
            THEMES.iter().find(|theme| theme.key == "liquidity-reflation").unwrap(),
            &scored
        ));
    }
}
