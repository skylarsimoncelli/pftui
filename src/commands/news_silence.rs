//! `pftui analytics news-silence` compares current topic volume with weekday baselines.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{anyhow, Result};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::db::agent_messages;
use crate::db::backend::BackendConnection;
use crate::db::news_silence::{
    self, NewsSilenceBaseline, NewsSilenceBaselineUpsert, NewsSilenceSample, NewsTopicDailyCount,
};

const MIN_BASELINE_SAMPLES: usize = 3;

#[derive(Debug, Clone, Serialize)]
pub struct NewsSilenceReport {
    pub generated_at: String,
    pub date: String,
    pub window_days: i64,
    pub min_baseline_samples: usize,
    pub total_topics: usize,
    pub recorded_rows: usize,
    pub messages_emitted: usize,
    pub entries: Vec<NewsSilenceEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NewsSilenceEntry {
    pub topic: String,
    pub day_of_week: i64,
    pub status: String,
    pub previous_status: Option<String>,
    pub status_changed: bool,
    pub observed_count: i64,
    pub median_count: f64,
    pub p30_count: f64,
    pub p80_count: f64,
    pub sample_days: usize,
    pub delta_vs_median: f64,
    pub label: String,
    #[serde(skip)]
    samples_for_storage: Vec<NewsSilenceSample>,
    #[serde(skip)]
    changed_at: Option<String>,
}

pub fn run(backend: &BackendConnection, window_days: i64, json_output: bool) -> Result<()> {
    let mut report = build_report_backend(backend, window_days)?;
    let rows = baseline_rows(&report.entries);
    report.recorded_rows = news_silence::upsert_baselines_backend(backend, &rows)?;
    report.messages_emitted = emit_synthesis_messages(backend, &report)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }
    Ok(())
}

pub fn build_report_backend(
    backend: &BackendConnection,
    window_days: i64,
) -> Result<NewsSilenceReport> {
    news_silence::ensure_table_backend(backend)?;
    let today = Utc::now().date_naive();
    build_report_for_date_backend(backend, today, window_days)
}

fn build_report_for_date_backend(
    backend: &BackendConnection,
    today: NaiveDate,
    window_days: i64,
) -> Result<NewsSilenceReport> {
    let window_days = window_days.max(1);
    let start_date = today - Duration::days(window_days);
    let today_start = day_start_ts(today)?;
    let tomorrow_start = day_start_ts(today + Duration::days(1))?;
    let window_start = day_start_ts(start_date)?;
    let baselines = news_silence::list_baselines_backend(backend)?;
    let historical_counts =
        news_silence::tier12_daily_counts_backend(backend, window_start, today_start)?;
    let today_counts =
        news_silence::tier12_daily_counts_backend(backend, today_start, tomorrow_start)?;
    build_report_from_parts(
        baselines,
        historical_counts,
        today_counts,
        today,
        window_days,
    )
}

fn build_report_from_parts(
    baselines: Vec<NewsSilenceBaseline>,
    historical_counts: Vec<NewsTopicDailyCount>,
    today_counts: Vec<NewsTopicDailyCount>,
    today: NaiveDate,
    window_days: i64,
) -> Result<NewsSilenceReport> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let today_dow = i64::from(today.weekday().number_from_monday());
    let start_date = today - Duration::days(window_days);

    let baseline_by_key = baselines
        .into_iter()
        .map(|row| ((row.topic.clone(), row.day_of_week), row))
        .collect::<HashMap<_, _>>();

    let mut samples_by_key: HashMap<(String, i64), BTreeMap<String, i64>> = HashMap::new();
    for ((topic, day_of_week), baseline) in &baseline_by_key {
        let samples = samples_by_key
            .entry((topic.clone(), *day_of_week))
            .or_default();
        for sample in &baseline.samples {
            if sample_in_window(&sample.date, start_date, today) {
                samples.insert(sample.date.clone(), sample.count);
            }
        }
    }
    for count in historical_counts {
        if count.day_of_week == today_dow && sample_in_window(&count.date, start_date, today) {
            samples_by_key
                .entry((count.topic, count.day_of_week))
                .or_default()
                .insert(count.date, count.count);
        }
    }

    let mut observed_by_topic = HashMap::new();
    for count in today_counts {
        if count.day_of_week == today_dow {
            observed_by_topic.insert(count.topic, count.count);
        }
    }

    let mut topics = BTreeSet::new();
    topics.extend(observed_by_topic.keys().cloned());
    topics.extend(
        samples_by_key
            .keys()
            .filter(|(_, dow)| *dow == today_dow)
            .map(|(topic, _)| topic.clone()),
    );
    topics.extend(
        baseline_by_key
            .keys()
            .filter(|(_, dow)| *dow == today_dow)
            .map(|(topic, _)| topic.clone()),
    );

    let mut entries = Vec::new();
    for topic in topics {
        let key = (topic.clone(), today_dow);
        let historical_samples = samples_by_key.remove(&key).unwrap_or_default();
        let sample_counts = historical_samples.values().copied().collect::<Vec<_>>();
        let observed_count = observed_by_topic.get(&topic).copied().unwrap_or(0);
        let (median_count, p30_count, p80_count) = distribution_stats(&sample_counts);
        let status = classify_status(observed_count, p30_count, p80_count, sample_counts.len());
        let previous_status = baseline_by_key
            .get(&key)
            .map(|baseline| baseline.status.clone());
        let status_changed = previous_status
            .as_deref()
            .is_some_and(|previous| previous != status && status != "insufficient");
        let changed_at = if status_changed {
            Some(today_str.clone())
        } else {
            baseline_by_key
                .get(&key)
                .and_then(|baseline| baseline.changed_at.clone())
        };
        let delta_vs_median = observed_count as f64 - median_count;
        let samples_for_storage =
            samples_with_today(historical_samples, &today_str, observed_count);
        entries.push(NewsSilenceEntry {
            topic,
            day_of_week: today_dow,
            status: status.to_string(),
            previous_status,
            status_changed,
            observed_count,
            median_count: round2(median_count),
            p30_count: round2(p30_count),
            p80_count: round2(p80_count),
            sample_days: sample_counts.len(),
            delta_vs_median: round2(delta_vs_median),
            label: status_label(status, observed_count, median_count, p30_count, p80_count),
            samples_for_storage,
            changed_at,
        });
    }

    entries.sort_by(|a, b| {
        status_rank(&b.status)
            .cmp(&status_rank(&a.status))
            .then_with(|| {
                b.delta_vs_median
                    .abs()
                    .partial_cmp(&a.delta_vs_median.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.topic.cmp(&b.topic))
    });

    Ok(NewsSilenceReport {
        generated_at: Utc::now().to_rfc3339(),
        date: today_str,
        window_days,
        min_baseline_samples: MIN_BASELINE_SAMPLES,
        total_topics: entries.len(),
        recorded_rows: 0,
        messages_emitted: 0,
        entries,
    })
}

fn baseline_rows(entries: &[NewsSilenceEntry]) -> Vec<NewsSilenceBaselineUpsert> {
    entries
        .iter()
        .map(|entry| NewsSilenceBaselineUpsert {
            topic: entry.topic.clone(),
            day_of_week: entry.day_of_week,
            samples: entry.samples_for_storage.clone(),
            median_count: entry.median_count,
            p30_count: entry.p30_count,
            p80_count: entry.p80_count,
            observed_count: entry.observed_count,
            status: entry.status.clone(),
            previous_status: entry.previous_status.clone(),
            changed_at: entry.changed_at.clone(),
        })
        .collect()
}

fn emit_synthesis_messages(
    backend: &BackendConnection,
    report: &NewsSilenceReport,
) -> Result<usize> {
    let mut emitted = 0usize;
    for entry in &report.entries {
        if !entry.status_changed || entry.status == "insufficient" {
            continue;
        }
        let package_id = format!("news-silence:{}:{}", entry.topic, report.date);
        let existing = agent_messages::list_messages_backend(
            backend,
            Some("pftui"),
            Some("synthesis"),
            None,
            false,
            Some(&report.date),
            Some(&package_id),
            Some(1),
        )?;
        if !existing.is_empty() {
            continue;
        }
        let previous = entry.previous_status.as_deref().unwrap_or("unknown");
        let content = format!(
            "News volume regime changed for {topic} on {date}: {previous} -> {status}. \
             Observed {observed} tier-1/2 article(s) vs weekday median {median:.1}; \
             p30 {p30:.1}, p80 {p80:.1}. Review whether negative-space or saturation changes the synthesis.",
            topic = entry.topic,
            date = report.date,
            previous = previous,
            status = entry.status,
            observed = entry.observed_count,
            median = entry.median_count,
            p30 = entry.p30_count,
            p80 = entry.p80_count,
        );
        agent_messages::send_message_backend(
            backend,
            "pftui",
            Some("synthesis"),
            Some("normal"),
            &content,
            Some("news_silence"),
            Some("synthesis"),
            Some(&package_id),
            Some("News volume regime change"),
        )?;
        emitted += 1;
    }
    Ok(emitted)
}

fn print_text(report: &NewsSilenceReport) {
    println!("News Volume vs Baseline");
    println!("════════════════════════════════════════════════════════════════");
    println!(
        "{} topic(s) • {}d rolling weekday baseline • min {} samples",
        report.total_topics, report.window_days, report.min_baseline_samples
    );
    if report.entries.is_empty() {
        println!("No tier-1/2 topic history found.");
        return;
    }
    println!();
    println!(
        "{:<18} {:<12} {:>8} {:>8} {:>8}  Label",
        "Topic", "Status", "Today", "Median", "Samples"
    );
    println!("{}", "-".repeat(86));
    for entry in &report.entries {
        println!(
            "{:<18} {:<12} {:>8} {:>8.1} {:>8}  {}",
            truncate(&entry.topic, 18),
            entry.status,
            entry.observed_count,
            entry.median_count,
            entry.sample_days,
            entry.label
        );
    }
    if report.messages_emitted > 0 {
        println!();
        println!(
            "Emitted {} synthesis message(s) for news-volume regime changes.",
            report.messages_emitted
        );
    }
}

fn classify_status(
    observed_count: i64,
    p30_count: f64,
    p80_count: f64,
    sample_days: usize,
) -> &'static str {
    if sample_days < MIN_BASELINE_SAMPLES {
        "insufficient"
    } else if (observed_count as f64) < p30_count {
        "silent"
    } else if (observed_count as f64) > p80_count {
        "saturated"
    } else {
        "normal"
    }
}

fn distribution_stats(values: &[i64]) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted = values.iter().map(|value| *value as f64).collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (
        percentile(&sorted, 0.50),
        percentile(&sorted, 0.30),
        percentile(&sorted, 0.80),
    )
}

fn percentile(sorted_values: &[f64], p: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }
    if sorted_values.len() == 1 {
        return sorted_values[0];
    }
    let rank = p.clamp(0.0, 1.0) * (sorted_values.len() as f64 - 1.0);
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        sorted_values[lower]
    } else {
        let weight = rank - lower as f64;
        sorted_values[lower] * (1.0 - weight) + sorted_values[upper] * weight
    }
}

fn status_label(
    status: &str,
    observed_count: i64,
    median_count: f64,
    p30_count: f64,
    p80_count: f64,
) -> String {
    match status {
        "silent" => format!(
            "silent: {observed_count} vs {:.1} median (< {:.1} p30)",
            median_count, p30_count
        ),
        "saturated" => format!(
            "saturated: {observed_count} vs {:.1} median (> {:.1} p80)",
            median_count, p80_count
        ),
        "normal" => format!("normal: {observed_count} vs {:.1} median", median_count),
        _ => "insufficient baseline".to_string(),
    }
}

fn samples_with_today(
    historical_samples: BTreeMap<String, i64>,
    today: &str,
    observed_count: i64,
) -> Vec<NewsSilenceSample> {
    let mut samples = historical_samples;
    samples.insert(today.to_string(), observed_count);
    samples
        .into_iter()
        .map(|(date, count)| NewsSilenceSample { date, count })
        .collect()
}

fn sample_in_window(date: &str, start_date: NaiveDate, today: NaiveDate) -> bool {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|value| value >= start_date && value < today)
        .unwrap_or(false)
}

fn day_start_ts(date: NaiveDate) -> Result<i64> {
    Ok(date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow!("invalid date for news silence baseline"))?
        .and_utc()
        .timestamp())
}

fn status_rank(status: &str) -> i32 {
    match status {
        "silent" | "saturated" => 3,
        "normal" => 2,
        "insufficient" => 1,
        _ => 0,
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
    use crate::db::agent_messages;
    use crate::db::backend::BackendConnection;
    use crate::db::news_cache;
    use crate::db::schema::run_migrations;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn weekday_samples(today: NaiveDate, counts: &[i64]) -> Vec<NewsSilenceSample> {
        counts
            .iter()
            .enumerate()
            .map(|(idx, count)| NewsSilenceSample {
                date: (today - Duration::days(7 * (idx as i64 + 1)))
                    .format("%Y-%m-%d")
                    .to_string(),
                count: *count,
            })
            .collect()
    }

    fn add_current_news(conn: &Connection, topic: &str, today: NaiveDate, count: usize) {
        let ts = day_start_ts(today).unwrap() + 3600;
        for idx in 0..count {
            let url = format!("https://reuters.com/{topic}-{idx}");
            news_cache::insert_news_with_source_type(
                conn,
                &format!("{topic} report {idx}"),
                &url,
                "Reuters",
                "rss",
                None,
                "macro",
                ts,
                Some("Tier one source"),
                &[],
            )
            .unwrap();
            conn.execute(
                "UPDATE news_cache SET topic = ?1, source_tier = 1 WHERE url = ?2",
                rusqlite::params![topic, url],
            )
            .unwrap();
        }
    }

    #[test]
    fn baseline_percentiles_are_computed_from_synthetic_histogram() {
        let values = vec![1, 3, 5, 7, 9];
        let (median, p30, p80) = distribution_stats(&values);
        assert!((median - 5.0).abs() < f64::EPSILON);
        assert!((p30 - 3.4).abs() < 1e-9);
        assert!((p80 - 7.4).abs() < 1e-9);
    }

    #[test]
    fn silence_and_saturation_thresholds_fire() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 28).unwrap();
        let day_of_week = i64::from(today.weekday().number_from_monday());
        let baselines = vec![
            NewsSilenceBaseline {
                topic: "fed-policy".to_string(),
                day_of_week,
                samples: weekday_samples(today, &[8, 9, 10, 11, 12]),
                status: "normal".to_string(),
                changed_at: None,
            },
            NewsSilenceBaseline {
                topic: "geopolitics".to_string(),
                day_of_week,
                samples: weekday_samples(today, &[1, 2, 2, 3, 3]),
                status: "normal".to_string(),
                changed_at: None,
            },
        ];
        let today_counts = vec![NewsTopicDailyCount {
            topic: "geopolitics".to_string(),
            date: "2026-05-28".to_string(),
            day_of_week,
            count: 6,
        }];

        let report = build_report_from_parts(baselines, vec![], today_counts, today, 90).unwrap();
        let fed = report
            .entries
            .iter()
            .find(|entry| entry.topic == "fed-policy")
            .unwrap();
        let geo = report
            .entries
            .iter()
            .find(|entry| entry.topic == "geopolitics")
            .unwrap();
        assert_eq!(fed.status, "silent");
        assert_eq!(fed.observed_count, 0);
        assert_eq!(geo.status, "saturated");
        assert_eq!(geo.observed_count, 6);
    }

    #[test]
    fn status_change_emits_deduped_synthesis_message() {
        let conn = setup();
        let today = Utc::now().date_naive();
        let day_of_week = i64::from(today.weekday().number_from_monday());
        news_silence::upsert_baselines(
            &conn,
            &[NewsSilenceBaselineUpsert {
                topic: "fed-policy".to_string(),
                day_of_week,
                samples: weekday_samples(today, &[8, 9, 10, 11, 12]),
                median_count: 10.0,
                p30_count: 9.2,
                p80_count: 11.2,
                observed_count: 10,
                status: "normal".to_string(),
                previous_status: None,
                changed_at: None,
            }],
        )
        .unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let mut report = build_report_backend(&backend, 90).unwrap();
        assert_eq!(report.entries[0].status, "silent");
        let rows = baseline_rows(&report.entries);
        news_silence::upsert_baselines_backend(&backend, &rows).unwrap();
        report.recorded_rows = rows.len();
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
        assert_eq!(messages[0].category.as_deref(), Some("news_silence"));
    }

    #[test]
    fn command_counts_current_tier_one_news() {
        let conn = setup();
        let today = Utc::now().date_naive();
        let day_of_week = i64::from(today.weekday().number_from_monday());
        news_silence::upsert_baselines(
            &conn,
            &[NewsSilenceBaselineUpsert {
                topic: "fed-policy".to_string(),
                day_of_week,
                samples: weekday_samples(today, &[1, 2, 2, 3, 3]),
                median_count: 2.0,
                p30_count: 2.0,
                p80_count: 3.0,
                observed_count: 2,
                status: "normal".to_string(),
                previous_status: None,
                changed_at: None,
            }],
        )
        .unwrap();
        add_current_news(&conn, "fed-policy", today, 5);

        let backend = BackendConnection::Sqlite { conn };
        let report = build_report_backend(&backend, 90).unwrap();
        let fed = report
            .entries
            .iter()
            .find(|entry| entry.topic == "fed-policy")
            .unwrap();
        assert_eq!(fed.observed_count, 5);
        assert_eq!(fed.status, "saturated");
    }
}
