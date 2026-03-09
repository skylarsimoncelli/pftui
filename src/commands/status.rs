use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use sqlx::PgPool;

use crate::config::load_config;
use crate::db::backend::BackendConnection;
use crate::db::{bls_cache, calendar_cache, comex_cache, cot_cache, news_cache};
use crate::db::{onchain_cache, predictions_cache, price_cache, sentiment_cache, worldbank_cache};

/// Freshness thresholds in seconds
const PRICE_FRESHNESS_SECS: i64 = 15 * 60; // 15 minutes
const NEWS_FRESHNESS_SECS: i64 = 10 * 60; // 10 minutes
const PREDICTIONS_FRESHNESS_SECS: i64 = 60 * 60; // 1 hour
const SENTIMENT_FRESHNESS_SECS: i64 = 60 * 60; // 1 hour
const CALENDAR_FRESHNESS_SECS: i64 = 24 * 60 * 60; // 24 hours
const COT_FRESHNESS_SECS: i64 = 7 * 24 * 60 * 60; // 1 week
const COMEX_FRESHNESS_SECS: i64 = 24 * 60 * 60; // 24 hours
const BLS_FRESHNESS_DAYS: i64 = 30; // 1 month

#[derive(Debug)]
struct DataSourceStatus {
    name: &'static str,
    last_fetch: Option<String>,
    records: usize,
    status: SourceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SourceStatus {
    Fresh,
    Stale,
    Empty,
}

impl SourceStatus {
    fn symbol(&self) -> &'static str {
        match self {
            SourceStatus::Fresh => "✓",
            SourceStatus::Stale => "⚠",
            SourceStatus::Empty => "✗",
        }
    }

    fn name(&self) -> &'static str {
        match self {
            SourceStatus::Fresh => "Fresh",
            SourceStatus::Stale => "Stale",
            SourceStatus::Empty => "Empty",
        }
    }
}

fn format_time_ago(rfc3339_str: &str) -> String {
    let now = chrono::Utc::now();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc3339_str) {
        let age = now.signed_duration_since(dt.with_timezone(&chrono::Utc));
        let secs = age.num_seconds();
        
        if secs < 60 {
            return format!("{}s ago", secs);
        } else if secs < 3600 {
            return format!("{}m ago", secs / 60);
        } else if secs < 86400 {
            return format!("{}h ago", secs / 3600);
        } else {
            return format!("{}d ago", secs / 86400);
        }
    }
    "unknown".to_string()
}

fn check_prices(conn: &Connection) -> Result<DataSourceStatus> {
    let prices = price_cache::get_all_cached_prices(conn)?;
    let count = prices.len();
    
    if count == 0 {
        return Ok(DataSourceStatus {
            name: "Prices",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    let now = chrono::Utc::now();
    let mut most_recent: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut is_stale = false;
    
    for price in &prices {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&price.fetched_at) {
            let dt_utc = dt.with_timezone(&chrono::Utc);
            if most_recent.is_none() || most_recent.unwrap() < dt_utc {
                most_recent = Some(dt_utc);
            }
            
            let age = now.signed_duration_since(dt_utc);
            if age.num_seconds() > PRICE_FRESHNESS_SECS {
                is_stale = true;
            }
        }
    }
    
    Ok(DataSourceStatus {
        name: "Prices",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_stale { SourceStatus::Stale } else { SourceStatus::Fresh },
    })
}

fn check_predictions(conn: &Connection) -> Result<DataSourceStatus> {
    let markets = predictions_cache::get_cached_predictions(conn, 100)?;
    let count = markets.len();
    
    if count == 0 {
        return Ok(DataSourceStatus {
            name: "Predictions",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    let last_update = predictions_cache::get_last_update(conn)?;
    let now = chrono::Utc::now().timestamp();
    
    let (last_fetch_str, is_stale) = match last_update {
        Some(ts) => {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .unwrap_or_else(chrono::Utc::now);
            let age = now - ts;
            (Some(dt.to_rfc3339()), age > PREDICTIONS_FRESHNESS_SECS)
        }
        None => (None, true),
    };
    
    Ok(DataSourceStatus {
        name: "Predictions",
        last_fetch: last_fetch_str,
        records: count,
        status: if is_stale { SourceStatus::Stale } else { SourceStatus::Fresh },
    })
}

fn check_news(conn: &Connection) -> Result<DataSourceStatus> {
    let news = news_cache::get_latest_news(conn, 1, None, None, None, None)?;
    
    if news.is_empty() {
        return Ok(DataSourceStatus {
            name: "News",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    let count = conn.query_row(
        "SELECT COUNT(*) FROM news_cache",
        [],
        |row| row.get::<_, i64>(0),
    ).unwrap_or(0) as usize;
    
    let now = chrono::Utc::now();
    let fetched_at = &news[0].fetched_at;
    let is_stale = if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(fetched_at) {
        let age = now.signed_duration_since(dt.with_timezone(&chrono::Utc));
        age.num_seconds() > NEWS_FRESHNESS_SECS
    } else {
        true
    };
    
    Ok(DataSourceStatus {
        name: "News",
        last_fetch: Some(fetched_at.clone()),
        records: count,
        status: if is_stale { SourceStatus::Stale } else { SourceStatus::Fresh },
    })
}

fn check_cot(conn: &Connection) -> Result<DataSourceStatus> {
    let reports = cot_cache::get_all_latest(conn)?;
    let count = reports.len();
    
    if count == 0 {
        return Ok(DataSourceStatus {
            name: "COT",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    let now = chrono::Utc::now();
    let mut most_recent: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut is_stale = false;
    
    for report in &reports {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&report.fetched_at) {
            let dt_utc = dt.with_timezone(&chrono::Utc);
            if most_recent.is_none() || most_recent.unwrap() < dt_utc {
                most_recent = Some(dt_utc);
            }
            
            let age = now.signed_duration_since(dt_utc);
            if age.num_seconds() > COT_FRESHNESS_SECS {
                is_stale = true;
            }
        }
    }
    
    Ok(DataSourceStatus {
        name: "COT",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_stale { SourceStatus::Stale } else { SourceStatus::Fresh },
    })
}

fn check_sentiment(conn: &Connection) -> Result<DataSourceStatus> {
    let crypto = sentiment_cache::get_latest(conn, "crypto_fng")?;
    let trad = sentiment_cache::get_latest(conn, "traditional_fng")?;
    
    let count = if crypto.is_some() { 1 } else { 0 } + if trad.is_some() { 1 } else { 0 };
    
    if count == 0 {
        return Ok(DataSourceStatus {
            name: "Sentiment",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    let now = chrono::Utc::now();
    let mut most_recent: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut is_stale = false;
    
    for reading in [crypto, trad].iter().filter_map(|r| r.as_ref()) {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&reading.fetched_at) {
            let dt_utc = dt.with_timezone(&chrono::Utc);
            if most_recent.is_none() || most_recent.unwrap() < dt_utc {
                most_recent = Some(dt_utc);
            }
            
            let age = now.signed_duration_since(dt_utc);
            if age.num_seconds() > SENTIMENT_FRESHNESS_SECS {
                is_stale = true;
            }
        }
    }
    
    Ok(DataSourceStatus {
        name: "Sentiment",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_stale { SourceStatus::Stale } else { SourceStatus::Fresh },
    })
}

fn check_calendar(conn: &Connection) -> Result<DataSourceStatus> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let events = calendar_cache::get_upcoming_events(conn, &today, 100)?;
    let count = events.len();
    
    if count == 0 {
        return Ok(DataSourceStatus {
            name: "Calendar",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    let now = chrono::Utc::now();
    let mut most_recent: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut is_stale = false;
    
    for event in &events {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&event.fetched_at) {
            let dt_utc = dt.with_timezone(&chrono::Utc);
            if most_recent.is_none() || most_recent.unwrap() < dt_utc {
                most_recent = Some(dt_utc);
            }
            
            let age = now.signed_duration_since(dt_utc);
            if age.num_seconds() > CALENDAR_FRESHNESS_SECS {
                is_stale = true;
            }
        }
    }
    
    Ok(DataSourceStatus {
        name: "Calendar",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_stale { SourceStatus::Stale } else { SourceStatus::Fresh },
    })
}

fn check_bls(conn: &Connection) -> Result<DataSourceStatus> {
    // Count all BLS records
    let count = conn.query_row(
        "SELECT COUNT(*) FROM bls_cache",
        [],
        |row| row.get::<_, i64>(0),
    ).unwrap_or(0) as usize;
    
    if count == 0 {
        return Ok(DataSourceStatus {
            name: "BLS",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    // Check freshness of a key series
    let is_fresh = bls_cache::is_cache_fresh(conn, "CUUR0000SA0", BLS_FRESHNESS_DAYS).unwrap_or(false);
    
    // Get last update timestamp from bls_cache table
    let last_fetch = conn.query_row(
        "SELECT MAX(fetched_at) FROM bls_cache",
        [],
        |row| row.get::<_, Option<i64>>(0),
    ).optional().ok().flatten().flatten().and_then(|ts| {
        chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())
    });
    
    Ok(DataSourceStatus {
        name: "BLS",
        last_fetch,
        records: count,
        status: if count == 0 {
            SourceStatus::Empty
        } else if is_fresh {
            SourceStatus::Fresh
        } else {
            SourceStatus::Stale
        },
    })
}

fn check_worldbank(conn: &Connection) -> Result<DataSourceStatus> {
    // Count all World Bank records
    let count = conn.query_row(
        "SELECT COUNT(*) FROM worldbank_cache",
        [],
        |row| row.get::<_, i64>(0),
    ).unwrap_or(0) as usize;
    
    if count == 0 {
        return Ok(DataSourceStatus {
            name: "World Bank",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    let needs_refresh = worldbank_cache::needs_refresh(conn).unwrap_or(true);
    
    // Get last update timestamp from worldbank_cache table
    let last_fetch = conn.query_row(
        "SELECT MAX(fetched_at) FROM worldbank_cache",
        [],
        |row| row.get::<_, Option<i64>>(0),
    ).optional().ok().flatten().flatten().and_then(|ts| {
        chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())
    });
    
    Ok(DataSourceStatus {
        name: "World Bank",
        last_fetch,
        records: count,
        status: if needs_refresh {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
    })
}

fn check_comex(conn: &Connection) -> Result<DataSourceStatus> {
    let mut count = 0;
    let mut most_recent: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut is_stale = false;
    let now = chrono::Utc::now();
    
    for symbol in &["GC=F", "SI=F"] {
        if let Ok(Some(inv)) = comex_cache::get_latest_inventory(conn, symbol) {
            count += 1;
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&inv.fetched_at) {
                let dt_utc = dt.with_timezone(&chrono::Utc);
                if most_recent.is_none() || most_recent.unwrap() < dt_utc {
                    most_recent = Some(dt_utc);
                }
                
                let age = now.signed_duration_since(dt_utc);
                if age.num_seconds() > COMEX_FRESHNESS_SECS {
                    is_stale = true;
                }
            }
        }
    }
    
    Ok(DataSourceStatus {
        name: "COMEX",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if count == 0 {
            SourceStatus::Empty
        } else if is_stale {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
    })
}

fn check_onchain(conn: &Connection) -> Result<DataSourceStatus> {
    let metrics = onchain_cache::get_metrics_by_type(conn, "network", 100)?;
    let count = metrics.len();
    
    if count == 0 {
        return Ok(DataSourceStatus {
            name: "On-chain",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }
    
    let now = chrono::Utc::now();
    let mut most_recent: Option<chrono::DateTime<chrono::Utc>> = None;
    
    for metric in &metrics {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&metric.fetched_at) {
            let dt_utc = dt.with_timezone(&chrono::Utc);
            if most_recent.is_none() || most_recent.unwrap() < dt_utc {
                most_recent = Some(dt_utc);
            }
        }
    }
    
    // On-chain data is "fresh" if fetched today
    let is_fresh = most_recent.map(|dt| {
        let age = now.signed_duration_since(dt);
        age.num_hours() < 24
    }).unwrap_or(false);
    
    Ok(DataSourceStatus {
        name: "On-chain",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_fresh {
            SourceStatus::Fresh
        } else {
            SourceStatus::Stale
        },
    })
}

pub fn run(conn: &Connection, json: bool) -> Result<()> {
    let config = load_config()?;

    let sources = vec![
        check_prices(conn)?,
        check_predictions(conn)?,
        check_news(conn)?,
        check_cot(conn)?,
        check_sentiment(conn)?,
        check_calendar(conn)?,
        check_bls(conn)?,
        check_worldbank(conn)?,
        check_comex(conn)?,
        check_onchain(conn)?,
    ];

    if json {
        print_json(&config, &sources)?;
    } else {
        print_table(&config, &sources);
    }
    
    Ok(())
}

pub fn run_backend(backend: &BackendConnection, json: bool) -> Result<()> {
    if let Some(conn) = backend.sqlite_native() {
        return run(conn, json);
    }
    if let Some(pool) = backend.postgres_pool() {
        return run_postgres(pool, json);
    }
    Err(anyhow::anyhow!("Unsupported database backend state"))
}

fn run_postgres(pool: &PgPool, json: bool) -> Result<()> {
    let config = load_config()?;

    let sources = vec![
        check_source_postgres(
            pool,
            "Prices",
            "price_cache",
            "MAX(fetched_at)",
            Some(PRICE_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "Predictions",
            "predictions_cache",
            "MAX(updated_at)::TEXT",
            Some(PREDICTIONS_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "News",
            "news_cache",
            "MAX(fetched_at)",
            Some(NEWS_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "COT",
            "cot_cache",
            "MAX(fetched_at)",
            Some(COT_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "Sentiment",
            "sentiment_cache",
            "MAX(fetched_at)",
            Some(SENTIMENT_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "Calendar",
            "calendar_events",
            "MAX(fetched_at)",
            Some(CALENDAR_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "BLS",
            "bls_cache",
            "MAX(fetched_at)::TEXT",
            Some(BLS_FRESHNESS_DAYS * 24 * 60 * 60),
        ),
        check_source_postgres(
            pool,
            "World Bank",
            "worldbank_cache",
            "TO_CHAR(MAX(updated_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')",
            Some(30 * 24 * 60 * 60),
        ),
        check_source_postgres(
            pool,
            "COMEX",
            "comex_cache",
            "MAX(fetched_at)",
            Some(COMEX_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "On-chain",
            "onchain_cache",
            "MAX(fetched_at)",
            Some(24 * 60 * 60),
        ),
    ];

    if json {
        print_json(&config, &sources)?;
    } else {
        print_table(&config, &sources);
    }

    Ok(())
}

fn check_source_postgres(
    pool: &PgPool,
    name: &'static str,
    table: &str,
    last_fetch_expr: &str,
    freshness_secs: Option<i64>,
) -> DataSourceStatus {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => {
            return DataSourceStatus {
                name,
                last_fetch: None,
                records: 0,
                status: SourceStatus::Empty,
            };
        }
    };

    let count_sql = format!("SELECT COUNT(*)::BIGINT FROM {}", table);
    let count: i64 = match runtime.block_on(async { sqlx::query_scalar::<_, i64>(&count_sql).fetch_one(pool).await }) {
        Ok(v) => v,
        Err(_) => {
            return DataSourceStatus {
                name,
                last_fetch: None,
                records: 0,
                status: SourceStatus::Empty,
            };
        }
    };

    if count == 0 {
        return DataSourceStatus {
            name,
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        };
    }

    let last_sql = format!("SELECT {} FROM {}", last_fetch_expr, table);
    let last_fetch: Option<String> =
        runtime.block_on(async { sqlx::query_scalar::<_, Option<String>>(&last_sql).fetch_one(pool).await }).ok().flatten();

    let status = match (freshness_secs, last_fetch.as_deref()) {
        (Some(max_age), Some(ts)) => {
            if is_stale_timestamp(ts, max_age) {
                SourceStatus::Stale
            } else {
                SourceStatus::Fresh
            }
        }
        (Some(_), None) => SourceStatus::Stale,
        (None, _) => SourceStatus::Fresh,
    };

    DataSourceStatus {
        name,
        last_fetch,
        records: count as usize,
        status,
    }
}

fn is_stale_timestamp(raw: &str, max_age_secs: i64) -> bool {
    let now = chrono::Utc::now();

    if let Ok(ts) = raw.parse::<i64>() {
        if let Some(dt) = chrono::DateTime::from_timestamp(ts, 0) {
            return now.signed_duration_since(dt).num_seconds() > max_age_secs;
        }
    }

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return now
            .signed_duration_since(dt.with_timezone(&chrono::Utc))
            .num_seconds()
            > max_age_secs;
    }

    true
}

fn print_json(config: &crate::config::Config, sources: &[DataSourceStatus]) -> Result<()> {
    use serde_json::json;
    
    let brave_configured = config.brave_api_key
        .as_ref()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);

    let sources_json: Vec<_> = sources
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "last_fetch": s.last_fetch,
                "records": s.records,
                "status": s.status.name().to_lowercase(),
            })
        })
        .collect();

    let output = json!({
        "brave_api_key_configured": brave_configured,
        "sources": sources_json,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_table(config: &crate::config::Config, sources: &[DataSourceStatus]) {
    let brave_key = config.brave_api_key.as_deref().unwrap_or("").trim();
    if brave_key.is_empty() {
        println!(
            "Brave Search: ✗ No key (add with `pftui config set brave_api_key <key>` — free tier at brave.com/search/api/)"
        );
    } else {
        println!("Brave Search: ✓ Configured");
        println!("Brave usage: query count / credits unavailable via current API response metadata");
    }
    println!();
    
    // Print header
    println!("{:<16} {:<18} {:<8} Status", "Source", "Last Fetch", "Records");
    println!("{}", "─".repeat(60));
    
    // Print each source
    for source in sources {
        let last_fetch_str = source.last_fetch
            .as_ref()
            .map(|s| format_time_ago(s))
            .unwrap_or_else(|| "never".to_string());
        
        println!(
            "{:<16} {:<18} {:<8} {} {}",
            source.name,
            last_fetch_str,
            source.records,
            source.status.symbol(),
            source.status.name()
        );
    }
}
