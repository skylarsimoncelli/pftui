use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use sqlx::PgPool;

use crate::commands::daemon;
use crate::config::load_config;
use crate::data::cot;
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

fn refresh_source_name(status_name: &str) -> Option<&'static str> {
    match status_name {
        "Prices" => Some("prices"),
        "Predictions" => Some("predictions"),
        "News" => Some("news"),
        "COT" => Some("cot"),
        "Sentiment" => Some("sentiment"),
        "Calendar" => Some("calendar"),
        "BLS" => Some("bls"),
        "WorldBank" => Some("worldbank"),
        "COMEX" => Some("comex"),
        "Onchain" => Some("onchain"),
        _ => None,
    }
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

type UtcDateTime = chrono::DateTime<chrono::Utc>;

fn parse_timestamp_utc(raw: &str) -> Option<UtcDateTime> {
    // RFC3339: "2026-04-04T00:10:41+00:00" or "2026-04-04T00:10:41.656Z"
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // Unix timestamp (integer seconds)
    if let Ok(ts) = raw.parse::<i64>() {
        if let Some(dt) = chrono::DateTime::from_timestamp(ts, 0) {
            return Some(dt.with_timezone(&chrono::Utc));
        }
    }
    // Postgres-style timestamps with timezone offset:
    //   "2026-04-04 00:10:41.656262+00"   (space sep, fractional secs, short tz)
    //   "2026-04-04 00:10:41+00"           (space sep, no fractional, short tz)
    //   "2026-04-04T00:10:41.656262+00"    (T sep, short tz)
    //   "2026-04-04 00:10:41.656262+00:00" (space sep, full tz)
    // The %#z specifier handles both short (+00) and full (+00:00) offsets.
    // The %.f specifier handles optional fractional seconds.
    chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%#z")
        .or_else(|_| chrono::DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f%#z"))
        .or_else(|_| chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%#z"))
        .or_else(|_| chrono::DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%#z"))
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|| {
            // Fallback: naive datetime without timezone (assume UTC)
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, chrono::Utc))
        })
}

fn format_time_ago(rfc3339_str: &str) -> String {
    let now = chrono::Utc::now();
    if let Some(dt) = parse_timestamp_utc(rfc3339_str) {
        let age = now.signed_duration_since(dt);
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

fn parse_rfc3339_utc(raw: &str) -> Option<UtcDateTime> {
    parse_timestamp_utc(raw)
}

fn update_most_recent(most_recent: &mut Option<UtcDateTime>, candidate: UtcDateTime) {
    match most_recent {
        Some(current) => {
            if candidate > *current {
                *current = candidate;
            }
        }
        None => *most_recent = Some(candidate),
    }
}

fn most_recent_and_stale_from_fetched<I>(
    fetched_values: I,
    now: UtcDateTime,
    freshness_secs: i64,
) -> (Option<UtcDateTime>, bool)
where
    I: IntoIterator<Item = String>,
{
    let mut most_recent: Option<UtcDateTime> = None;
    for fetched_at in fetched_values {
        if let Some(dt_utc) = parse_rfc3339_utc(&fetched_at) {
            update_most_recent(&mut most_recent, dt_utc);
        }
    }
    let is_stale = most_recent
        .map(|dt| now.signed_duration_since(dt).num_seconds() > freshness_secs)
        .unwrap_or(true);
    (most_recent, is_stale)
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
    let (most_recent, is_stale) = most_recent_and_stale_from_fetched(
        prices.iter().map(|price| price.fetched_at.clone()),
        now,
        PRICE_FRESHNESS_SECS,
    );

    Ok(DataSourceStatus {
        name: "Prices",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_stale {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
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
            let dt = chrono::DateTime::from_timestamp(ts, 0).unwrap_or_else(chrono::Utc::now);
            let age = now - ts;
            (Some(dt.to_rfc3339()), age > PREDICTIONS_FRESHNESS_SECS)
        }
        None => (None, true),
    };

    Ok(DataSourceStatus {
        name: "Predictions",
        last_fetch: last_fetch_str,
        records: count,
        status: if is_stale {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
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

    let count = conn
        .query_row("SELECT COUNT(*) FROM news_cache", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;

    let now = chrono::Utc::now();
    let fetched_at = &news[0].fetched_at;
    let is_stale = if let Some(dt) = parse_timestamp_utc(fetched_at) {
        let age = now.signed_duration_since(dt);
        age.num_seconds() > NEWS_FRESHNESS_SECS
    } else {
        true
    };

    Ok(DataSourceStatus {
        name: "News",
        last_fetch: parse_timestamp_utc(fetched_at).map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_stale {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
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

    let (most_recent_report_date, is_stale) = most_recent_cot_report_date(
        reports.iter().map(|report| report.report_date.clone()),
        chrono::Utc::now().date_naive(),
    );

    Ok(DataSourceStatus {
        name: "COT",
        last_fetch: most_recent_report_date.map(|date| format!("{date}T00:00:00Z")),
        records: count,
        status: if is_stale {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
    })
}

fn check_cot_postgres(pool: &PgPool) -> DataSourceStatus {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => {
            return DataSourceStatus {
                name: "COT",
                last_fetch: None,
                records: 0,
                status: SourceStatus::Empty,
            };
        }
    };

    let count = runtime
        .block_on(async {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*)::BIGINT FROM cot_cache")
                .fetch_one(pool)
                .await
        })
        .unwrap_or(0);
    if count == 0 {
        return DataSourceStatus {
            name: "COT",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        };
    }

    let report_dates = runtime
        .block_on(async {
            sqlx::query_scalar::<_, String>(
                "SELECT MAX(report_date)::TEXT FROM cot_cache GROUP BY cftc_code",
            )
            .fetch_all(pool)
            .await
        })
        .unwrap_or_default();
    let (most_recent_report_date, is_stale) =
        most_recent_cot_report_date(report_dates, chrono::Utc::now().date_naive());

    DataSourceStatus {
        name: "COT",
        last_fetch: most_recent_report_date.map(|date| format!("{date}T00:00:00Z")),
        records: count as usize,
        status: if is_stale {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
    }
}

fn most_recent_cot_report_date<I>(
    report_dates: I,
    today: chrono::NaiveDate,
) -> (Option<chrono::NaiveDate>, bool)
where
    I: IntoIterator<Item = String>,
{
    let most_recent = report_dates
        .into_iter()
        .filter_map(|raw| chrono::NaiveDate::parse_from_str(&raw, "%Y-%m-%d").ok())
        .max();
    let expected = cot::expected_latest_report_date(
        today
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc())
            .unwrap_or_else(chrono::Utc::now),
    );
    let is_stale = most_recent
        .map(|date| date < expected || (today - date).num_days() * 24 * 60 * 60 > COT_FRESHNESS_SECS)
        .unwrap_or(true);
    (most_recent, is_stale)
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
    let (most_recent, is_stale) = most_recent_and_stale_from_fetched(
        [crypto, trad]
            .iter()
            .filter_map(|r| r.as_ref())
            .map(|reading| reading.fetched_at.clone()),
        now,
        SENTIMENT_FRESHNESS_SECS,
    );

    Ok(DataSourceStatus {
        name: "Sentiment",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_stale {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
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
    let (most_recent, is_stale) = most_recent_and_stale_from_fetched(
        events.iter().map(|event| event.fetched_at.clone()),
        now,
        CALENDAR_FRESHNESS_SECS,
    );

    Ok(DataSourceStatus {
        name: "Calendar",
        last_fetch: most_recent.map(|dt| dt.to_rfc3339()),
        records: count,
        status: if is_stale {
            SourceStatus::Stale
        } else {
            SourceStatus::Fresh
        },
    })
}

fn check_bls(conn: &Connection) -> Result<DataSourceStatus> {
    // Count all BLS records
    let count = conn
        .query_row("SELECT COUNT(*) FROM bls_cache", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;

    if count == 0 {
        return Ok(DataSourceStatus {
            name: "BLS",
            last_fetch: None,
            records: 0,
            status: SourceStatus::Empty,
        });
    }

    // Check freshness of a key series
    let is_fresh =
        bls_cache::is_cache_fresh(conn, "CUUR0000SA0", BLS_FRESHNESS_DAYS).unwrap_or(false);

    // Get last update timestamp from bls_cache table
    let last_fetch = conn
        .query_row("SELECT MAX(updated_at) FROM bls_cache", [], |row| {
            row.get::<_, Option<String>>(0)
        })
        .optional()
        .ok()
        .flatten()
        .flatten()
        .and_then(|raw| parse_timestamp_utc(&raw).map(|dt| dt.to_rfc3339()));

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
    let count = conn
        .query_row("SELECT COUNT(*) FROM worldbank_cache", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;

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
    let last_fetch = conn
        .query_row("SELECT MAX(updated_at) FROM worldbank_cache", [], |row| {
            row.get::<_, Option<String>>(0)
        })
        .optional()
        .ok()
        .flatten()
        .flatten()
        .and_then(|raw| parse_timestamp_utc(&raw).map(|dt| dt.to_rfc3339()));

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
    let mut most_recent: Option<UtcDateTime> = None;
    let now = chrono::Utc::now();

    for symbol in &["GC=F", "SI=F"] {
        if let Ok(Some(inv)) = comex_cache::get_latest_inventory(conn, symbol) {
            count += 1;
            if let Some(dt_utc) = parse_rfc3339_utc(&inv.fetched_at) {
                update_most_recent(&mut most_recent, dt_utc);
            }
        }
    }
    let is_stale = most_recent
        .map(|dt| now.signed_duration_since(dt).num_seconds() > COMEX_FRESHNESS_SECS)
        .unwrap_or(true);

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
    let (most_recent, _) = most_recent_and_stale_from_fetched(
        metrics.iter().map(|metric| metric.fetched_at.clone()),
        now,
        i64::MAX,
    );

    // On-chain data is "fresh" if fetched today
    let is_fresh = most_recent
        .map(|dt| {
            let age = now.signed_duration_since(dt);
            age.num_hours() < 24
        })
        .unwrap_or(false);

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
    match backend {
        BackendConnection::Sqlite { conn } => run(conn, json),
        BackendConnection::Postgres { pool } => run_postgres(pool, json),
    }
}

pub fn stale_refresh_sources_backend(backend: &BackendConnection) -> Result<Vec<String>> {
    let sources = match backend {
        BackendConnection::Sqlite { conn } => vec![
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
        ],
        BackendConnection::Postgres { pool } => vec![
            check_source_postgres(
                pool,
                "Prices",
                "price_cache",
                "MAX(fetched_at)::TEXT",
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
                "MAX(fetched_at)::TEXT",
                Some(NEWS_FRESHNESS_SECS),
            ),
            check_cot_postgres(pool),
            check_source_postgres(
                pool,
                "Sentiment",
                "sentiment_cache",
                "MAX(fetched_at)::TEXT",
                Some(SENTIMENT_FRESHNESS_SECS),
            ),
            check_source_postgres(
                pool,
                "Calendar",
                "calendar_events",
                "MAX(fetched_at)::TEXT",
                Some(CALENDAR_FRESHNESS_SECS),
            ),
            check_source_postgres(pool, "BLS", "bls_cache", "MAX(date)", None),
            check_source_postgres(
                pool,
                "WorldBank",
                "worldbank_cache",
                "MAX(fetched_at)::TEXT",
                None,
            ),
            check_source_postgres(
                pool,
                "COMEX",
                "comex_cache",
                "MAX(fetched_at)::TEXT",
                Some(COMEX_FRESHNESS_SECS),
            ),
            check_source_postgres(
                pool,
                "Onchain",
                "onchain_cache",
                "MAX(fetched_at)::TEXT",
                Some(24 * 60 * 60),
            ),
        ],
    };

    Ok(sources
        .into_iter()
        .filter(|source| source.status != SourceStatus::Fresh)
        .filter_map(|source| refresh_source_name(source.name).map(str::to_string))
        .collect())
}

fn run_postgres(pool: &PgPool, json: bool) -> Result<()> {
    let config = load_config()?;

    let sources = vec![
        check_source_postgres(
            pool,
            "Prices",
            "price_cache",
            "MAX(fetched_at)::TEXT",
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
            "MAX(fetched_at)::TEXT",
            Some(NEWS_FRESHNESS_SECS),
        ),
        check_cot_postgres(pool),
        check_source_postgres(
            pool,
            "Sentiment",
            "sentiment_cache",
            "MAX(fetched_at)::TEXT",
            Some(SENTIMENT_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "Calendar",
            "calendar_events",
            "MAX(fetched_at)::TEXT",
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
            "MAX(updated_at)::TEXT",
            Some(30 * 24 * 60 * 60),
        ),
        check_source_postgres(
            pool,
            "COMEX",
            "comex_cache",
            "MAX(fetched_at)::TEXT",
            Some(COMEX_FRESHNESS_SECS),
        ),
        check_source_postgres(
            pool,
            "On-chain",
            "onchain_cache",
            "MAX(fetched_at)::TEXT",
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
    let count: i64 = match runtime.block_on(async {
        sqlx::query_scalar::<_, i64>(&count_sql)
            .fetch_one(pool)
            .await
    }) {
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
    let last_fetch: Option<String> = runtime
        .block_on(async {
            sqlx::query_scalar::<_, Option<String>>(&last_sql)
                .fetch_one(pool)
                .await
        })
        .ok()
        .flatten();

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
    if let Some(dt) = parse_timestamp_utc(raw) {
        return now.signed_duration_since(dt).num_seconds() > max_age_secs;
    }
    true
}

fn print_json(config: &crate::config::Config, sources: &[DataSourceStatus]) -> Result<()> {
    use serde_json::json;

    let brave_configured = config
        .brave_api_key
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

    let daemon_status = daemon::read_status()?;
    let output = json!({
        "brave_api_key_configured": brave_configured,
        "daemon": daemon_status,
        "sources": sources_json,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_table(config: &crate::config::Config, sources: &[DataSourceStatus]) {
    if let Ok(daemon_status) = daemon::read_status() {
        if daemon_status.running {
            let heartbeat = daemon_status
                .last_heartbeat
                .as_deref()
                .map(format_time_ago)
                .unwrap_or_else(|| "unknown".to_string());
            println!(
                "Daemon: ✓ {} (cycle {}, heartbeat {}, wake {}s)",
                daemon_status.status, daemon_status.cycle, heartbeat, daemon_status.interval_secs
            );
        } else if let Some(message) = daemon_status.message {
            println!("Daemon: ✗ {}", message);
        }
        println!();
    }

    let brave_key = config.brave_api_key.as_deref().unwrap_or("").trim();
    if brave_key.is_empty() {
        println!(
            "Brave Search: ✗ No key (add with `pftui config set brave_api_key <key>` — free tier at brave.com/search/api/)"
        );
    } else {
        println!("Brave Search: ✓ Configured");
        println!(
            "Brave usage: query count / credits unavailable via current API response metadata"
        );
    }
    println!();

    // Print header
    println!(
        "{:<16} {:<18} {:<8} Status",
        "Source", "Last Fetch", "Records"
    );
    println!("{}", "─".repeat(60));

    // Print each source
    for source in sources {
        let last_fetch_str = source
            .last_fetch
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Duration, Timelike};

    #[test]
    fn stale_check_uses_most_recent_timestamp() {
        let now = chrono::Utc::now();
        let fresh = (now - Duration::minutes(5)).to_rfc3339();
        let old = (now - Duration::hours(4)).to_rfc3339();
        let (_latest, is_stale) =
            most_recent_and_stale_from_fetched(vec![old, fresh], now, 15 * 60);
        assert!(!is_stale);
    }

    #[test]
    fn cot_staleness_uses_report_date_age() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 4, 6).unwrap();
        let (latest, is_stale) = most_recent_cot_report_date(
            vec!["2026-04-01".to_string(), "2026-03-25".to_string()],
            today,
        );
        assert_eq!(
            latest,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap())
        );
        assert!(!is_stale);

        let (_, stale) = most_recent_cot_report_date(vec!["2026-03-20".to_string()], today);
        assert!(stale);
    }

    #[test]
    fn cot_staleness_marks_missed_friday_release_as_stale() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();
        let (_, is_stale) = most_recent_cot_report_date(vec!["2026-03-31".to_string()], today);
        assert!(is_stale);
    }

    #[test]
    fn parse_rfc3339_timestamp() {
        let dt = parse_timestamp_utc("2026-04-04T00:10:41+00:00");
        assert!(dt.is_some());
        let dt = dt.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 4);
        assert_eq!(dt.day(), 4);
    }

    #[test]
    fn parse_postgres_timestamp_with_fractional_and_short_tz() {
        // This is the format Postgres returns for timestamptz columns
        let dt = parse_timestamp_utc("2026-04-04 00:10:41.656262+00");
        assert!(dt.is_some(), "failed to parse Postgres timestamp with fractional secs and short tz");
        let dt = dt.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 4);
        assert_eq!(dt.day(), 4);
        assert_eq!(dt.hour(), 0);
        assert_eq!(dt.minute(), 10);
        assert_eq!(dt.second(), 41);
    }

    #[test]
    fn parse_postgres_timestamp_without_fractional() {
        let dt = parse_timestamp_utc("2026-04-04 00:10:41+00");
        assert!(dt.is_some(), "failed to parse Postgres timestamp without fractional secs");
        let dt = dt.unwrap();
        assert_eq!(dt.year(), 2026);
    }

    #[test]
    fn parse_postgres_timestamp_full_tz_offset() {
        let dt = parse_timestamp_utc("2026-04-04 00:10:41.656262+00:00");
        assert!(dt.is_some(), "failed to parse Postgres timestamp with full tz offset");
    }

    #[test]
    fn parse_postgres_timestamp_negative_tz() {
        let dt = parse_timestamp_utc("2026-04-04 00:10:41.123456-05");
        assert!(dt.is_some(), "failed to parse Postgres timestamp with negative tz offset");
        let dt = dt.unwrap();
        // -05 offset means the UTC time is 5 hours ahead
        assert_eq!(dt.hour(), 5);
        assert_eq!(dt.minute(), 10);
    }

    #[test]
    fn parse_unix_timestamp() {
        let dt = parse_timestamp_utc("1775258763");
        assert!(dt.is_some(), "failed to parse unix timestamp");
    }

    #[test]
    fn parse_naive_datetime() {
        let dt = parse_timestamp_utc("2026-04-04 00:10:41");
        assert!(dt.is_some(), "failed to parse naive datetime");
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert!(parse_timestamp_utc("not-a-date").is_none());
        assert!(parse_timestamp_utc("").is_none());
    }

    #[test]
    fn stale_check_with_postgres_timestamps() {
        // Verify that most_recent_and_stale_from_fetched works with Postgres-format timestamps
        let now = chrono::Utc::now();
        let fresh_pg = format!(
            "{} {}+00",
            now.format("%Y-%m-%d"),
            now.format("%H:%M:%S%.6f")
        );
        let old_pg = format!(
            "{} {}+00",
            (now - Duration::hours(4)).format("%Y-%m-%d"),
            (now - Duration::hours(4)).format("%H:%M:%S%.6f")
        );
        let (latest, is_stale) =
            most_recent_and_stale_from_fetched(vec![old_pg, fresh_pg], now, 15 * 60);
        assert!(latest.is_some(), "should parse Postgres-format timestamps in staleness check");
        assert!(!is_stale, "fresh Postgres timestamp should not be stale");
    }

    #[test]
    fn refresh_source_name_maps_status_rows_to_refresh_plan_names() {
        assert_eq!(refresh_source_name("News"), Some("news"));
        assert_eq!(refresh_source_name("COMEX"), Some("comex"));
        assert_eq!(refresh_source_name("Unknown"), None);
    }
}
