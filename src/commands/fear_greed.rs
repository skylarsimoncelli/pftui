use anyhow::Result;
use serde::Serialize;

use crate::data::sentiment::{fetch_crypto_fng, fetch_traditional_fng};
use crate::db::backend::BackendConnection;
use crate::db::sentiment_cache::{self, SentimentReading};

const CRYPTO_KEYS: [&str; 2] = ["crypto_fng", "crypto"];
const TRADITIONAL_KEYS: [&str; 2] = ["traditional_fng", "traditional"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FearGreedReadingView {
    pub key: String,
    pub label: String,
    pub source: String,
    pub value: u8,
    pub classification: String,
    pub timestamp: i64,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FearGreedHistoryPoint {
    pub date: String,
    pub value: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct FearGreedSection {
    latest: Vec<FearGreedReadingView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    history: Option<Vec<FearGreedHistoryView>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct FearGreedHistoryView {
    key: String,
    label: String,
    source: String,
    points: Vec<FearGreedHistoryPoint>,
}

pub fn run(backend: &BackendConnection, history_days: Option<u32>, json_output: bool) -> Result<()> {
    refresh_if_needed(backend)?;
    let latest = load_latest_with_fallback(backend)?;
    let history = history_days
        .map(|days| load_history_with_fallback(backend, days))
        .transpose()?;

    if json_output {
        let payload = FearGreedSection {
            latest,
            history,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_terminal(&latest, history.as_deref());
    }

    Ok(())
}

pub fn load_latest_with_fallback(
    backend: &BackendConnection,
) -> Result<Vec<FearGreedReadingView>> {
    let mut rows = Vec::new();
    if let Some(reading) = get_latest_any_backend(backend, &CRYPTO_KEYS)? {
        rows.push(to_view("crypto", &reading));
    }
    if let Some(reading) = get_latest_any_backend(backend, &TRADITIONAL_KEYS)? {
        rows.push(to_view("traditional", &reading));
    }
    Ok(rows)
}

fn load_history_with_fallback(
    backend: &BackendConnection,
    days: u32,
) -> Result<Vec<FearGreedHistoryView>> {
    let mut rows = Vec::new();
    let crypto_points = get_history_any_backend(backend, &CRYPTO_KEYS, days)?;
    if !crypto_points.is_empty() {
        rows.push(FearGreedHistoryView {
            key: "crypto".to_string(),
            label: "Crypto Fear & Greed".to_string(),
            source: "Alternative.me".to_string(),
            points: crypto_points
                .into_iter()
                .map(|(date, value)| FearGreedHistoryPoint { date, value })
                .collect(),
        });
    }
    let traditional_points = get_history_any_backend(backend, &TRADITIONAL_KEYS, days)?;
    if !traditional_points.is_empty() {
        rows.push(FearGreedHistoryView {
            key: "traditional".to_string(),
            label: "Traditional Fear & Greed".to_string(),
            source: "derived".to_string(),
            points: traditional_points
                .into_iter()
                .map(|(date, value)| FearGreedHistoryPoint { date, value })
                .collect(),
        });
    }
    Ok(rows)
}

fn refresh_if_needed(backend: &BackendConnection) -> Result<()> {
    let crypto = get_latest_any_backend(backend, &CRYPTO_KEYS)?;
    let traditional = get_latest_any_backend(backend, &TRADITIONAL_KEYS)?;
    if crypto.is_some() && traditional.is_some() {
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339();
    if crypto.is_none() {
        let crypto = fetch_crypto_fng()?;
        sentiment_cache::upsert_reading_backend(
            backend,
            &SentimentReading {
                index_type: "crypto_fng".to_string(),
                value: crypto.value,
                classification: crypto.classification,
                timestamp: crypto.timestamp,
                fetched_at: now.clone(),
            },
        )?;
    }
    if traditional.is_none() {
        let traditional = fetch_traditional_fng()?;
        sentiment_cache::upsert_reading_backend(
            backend,
            &SentimentReading {
                index_type: "traditional_fng".to_string(),
                value: traditional.value,
                classification: traditional.classification,
                timestamp: traditional.timestamp,
                fetched_at: now,
            },
        )?;
    }
    Ok(())
}

fn get_latest_any_backend(
    backend: &BackendConnection,
    keys: &[&str],
) -> Result<Option<SentimentReading>> {
    for key in keys {
        if let Some(reading) = sentiment_cache::get_latest_backend(backend, key)? {
            return Ok(Some(reading));
        }
    }
    Ok(None)
}

fn get_history_any_backend(
    backend: &BackendConnection,
    keys: &[&str],
    days: u32,
) -> Result<Vec<(String, u8)>> {
    for key in keys {
        let rows = sentiment_cache::get_history_backend(backend, key, days)?;
        if !rows.is_empty() {
            return Ok(rows);
        }
    }
    Ok(Vec::new())
}

fn to_view(key: &str, reading: &SentimentReading) -> FearGreedReadingView {
    FearGreedReadingView {
        key: key.to_string(),
        label: label_for_key(key).to_string(),
        source: source_for_key(key).to_string(),
        value: reading.value,
        classification: reading.classification.clone(),
        timestamp: reading.timestamp,
        fetched_at: reading.fetched_at.clone(),
    }
}

fn label_for_key(key: &str) -> &'static str {
    match key {
        "crypto" => "Crypto Fear & Greed",
        "traditional" => "Traditional Fear & Greed",
        _ => "Fear & Greed",
    }
}

fn source_for_key(key: &str) -> &'static str {
    match key {
        "crypto" => "Alternative.me",
        "traditional" => "derived",
        _ => "unknown",
    }
}

fn print_terminal(latest: &[FearGreedReadingView], history: Option<&[FearGreedHistoryView]>) {
    println!("Fear & Greed\n");

    if latest.is_empty() {
        println!("No fresh Fear & Greed data cached. Run `pftui data refresh --only sentiment`.");
        return;
    }

    for row in latest {
        println!(
            "{}: {:>3}/100  {}  [{}]",
            row.label, row.value, row.classification, row.source
        );
    }

    if let Some(history) = history {
        println!();
        for series in history {
            println!("{} history:", series.label);
            for point in &series.points {
                println!("  {}  {}", point.date, point.value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn seed_reading(
        backend: &BackendConnection,
        index_type: &str,
        value: u8,
        classification: &str,
        timestamp: i64,
        fetched_at: &str,
    ) {
        sentiment_cache::upsert_reading_backend(
            backend,
            &SentimentReading {
                index_type: index_type.to_string(),
                value,
                classification: classification.to_string(),
                timestamp,
                fetched_at: fetched_at.to_string(),
            },
        )
        .unwrap();
    }

    #[test]
    fn load_latest_with_fallback_prefers_canonical_keys() {
        let backend = BackendConnection::Sqlite {
            conn: open_in_memory(),
        };
        seed_reading(
            &backend,
            "crypto",
            31,
            "Fear",
            1_700_000_000,
            "2026-04-22T00:00:00Z",
        );
        seed_reading(
            &backend,
            "crypto_fng",
            65,
            "Greed",
            1_700_000_100,
            "2026-04-22T01:00:00Z",
        );
        seed_reading(
            &backend,
            "traditional_fng",
            50,
            "Neutral",
            1_700_000_200,
            "2026-04-22T01:00:00Z",
        );

        let rows = load_latest_with_fallback(&backend).unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "crypto");
        assert_eq!(rows[0].value, 65);
        assert_eq!(rows[0].source, "Alternative.me");
        assert_eq!(rows[1].key, "traditional");
    }

    #[test]
    fn load_history_with_fallback_uses_legacy_rows_when_needed() {
        let backend = BackendConnection::Sqlite {
            conn: open_in_memory(),
        };
        seed_reading(
            &backend,
            "crypto",
            20,
            "Extreme Fear",
            1_700_000_000,
            "2026-04-21T00:00:00Z",
        );
        seed_reading(
            &backend,
            "traditional",
            55,
            "Neutral",
            1_700_010_000,
            "2026-04-22T00:00:00Z",
        );

        let history = load_history_with_fallback(&backend, 7).unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].key, "crypto");
        assert_eq!(history[0].points[0].value, 20);
        assert_eq!(history[1].key, "traditional");
        assert_eq!(history[1].points[0].value, 55);
    }
}
