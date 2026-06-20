use anyhow::{Context, Result};
use rusqlite::Connection;
use std::time::Duration;

use crate::config::{load_config, DatabaseBackend};
use crate::db::backend::BackendConnection;

/// Run diagnostic checks on DB connection, API endpoints, and cache freshness.
/// Reports what's working vs broken. Essential for diagnosing connectivity issues.
pub async fn run(json_output: bool) -> Result<()> {
    let config = load_config()?;
    let mut checks: Vec<DiagnosticCheck> = Vec::new();

    // 1. Database connection test
    let db_check =
        test_db_connection(&config.database_backend, config.database_url.as_deref()).await;
    checks.push(db_check);

    // 2. API endpoint tests (only if DB is reachable)
    if checks[0].passed {
        checks.push(test_yahoo_api().await);
        checks.push(test_coingecko_api().await);

        if let Some(ref brave_key) = config.brave_api_key {
            checks.push(test_brave_api(brave_key).await);
        }

        if let Some(ref fred_key) = config.fred_api_key {
            checks.push(test_fred_api(fred_key).await);
        }

        // Free API endpoints
        checks.push(test_polymarket_api().await);
        checks.push(test_cot_api().await);
        checks.push(test_bls_api().await);
    }

    // 3. Cache freshness check (only if DB is reachable)
    if checks[0].passed {
        let cache_check =
            test_cache_freshness(&config.database_backend, config.database_url.as_deref()).await;
        checks.push(cache_check);
    }

    // 4. BTC series divergence guard (SQLite only; the price_history dual
    // series problem is a local-cache concern).
    if checks[0].passed && matches!(config.database_backend, DatabaseBackend::Sqlite) {
        let db_path = crate::db::default_db_path();
        if let Ok(conn) =
            Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        {
            checks.push(btc_series_divergence_check(&conn));
            // 5. Registered-series freshness vs 2x SLA (driven by
            // series_registry — see `pftui data series status`).
            checks.push(series_registry_staleness_check(&conn));
            // 6. DB-wide false-value audit summary (read-only per-table
            // signature checks — full detail: `pftui data audit`).
            checks.push(data_audit_summary_check(&conn));
            // 7. Thesis evidence contract: re-run the verification SQL
            // embedded in curated thesis sections (read-only — full
            // detail: `pftui research verify-thesis`).
            checks.push(thesis_evidence_check(&conn));
        }
    }

    // Output results
    if json_output {
        output_json(&checks)?;
    } else {
        output_human(&checks);
    }

    // Return error if any critical check failed
    let failed_critical = checks.iter().any(|c| c.critical && !c.passed);
    if failed_critical {
        anyhow::bail!("Critical diagnostic checks failed. See output above for details.");
    }

    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct DiagnosticCheck {
    name: String,
    category: String,
    passed: bool,
    critical: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
}

async fn test_db_connection(
    backend: &DatabaseBackend,
    postgres_url: Option<&str>,
) -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let result = match backend {
        DatabaseBackend::Sqlite => test_sqlite_connection().await,
        DatabaseBackend::Postgres => test_postgres_connection(postgres_url).await,
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(msg) => DiagnosticCheck {
            name: "Database Connection".to_string(),
            category: "Infrastructure".to_string(),
            passed: true,
            critical: true,
            message: msg,
            duration_ms: Some(duration_ms),
        },
        Err(e) => DiagnosticCheck {
            name: "Database Connection".to_string(),
            category: "Infrastructure".to_string(),
            passed: false,
            critical: true,
            message: format!("Failed: {}", e),
            duration_ms: Some(duration_ms),
        },
    }
}

async fn test_sqlite_connection() -> Result<String> {
    let db_path = crate::db::default_db_path();
    let conn = Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("Failed to open SQLite database")?;

    // Test a simple query
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))?;

    Ok(format!("SQLite connection OK ({} tables)", count))
}

async fn test_postgres_connection(postgres_url: Option<&str>) -> Result<String> {
    let database_url = postgres_url.context("postgres_url not configured in config.toml")?;

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
        .context("Failed to connect to PostgreSQL")?;

    // Test a simple query
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .fetch_one(&pool)
    .await
    .context("Failed to query PostgreSQL")?;

    Ok(format!("PostgreSQL connection OK ({} tables)", row.0))
}

async fn test_yahoo_api() -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let result = client
        .get("https://query1.finance.yahoo.com/v8/finance/chart/SPY?range=1d&interval=1d")
        .send()
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => DiagnosticCheck {
            name: "Yahoo Finance API".to_string(),
            category: "Data Sources".to_string(),
            passed: true,
            critical: false,
            message: format!("OK ({}ms)", duration_ms),
            duration_ms: Some(duration_ms),
        },
        Ok(resp) => DiagnosticCheck {
            name: "Yahoo Finance API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("HTTP {}", resp.status()),
            duration_ms: Some(duration_ms),
        },
        Err(e) => DiagnosticCheck {
            name: "Yahoo Finance API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("Failed: {}", e),
            duration_ms: Some(duration_ms),
        },
    }
}

async fn test_coingecko_api() -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let result = client
        .get("https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd")
        .send()
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => DiagnosticCheck {
            name: "CoinGecko API".to_string(),
            category: "Data Sources".to_string(),
            passed: true,
            critical: false,
            message: format!("OK ({}ms)", duration_ms),
            duration_ms: Some(duration_ms),
        },
        Ok(resp) => DiagnosticCheck {
            name: "CoinGecko API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("HTTP {}", resp.status()),
            duration_ms: Some(duration_ms),
        },
        Err(e) => DiagnosticCheck {
            name: "CoinGecko API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("Failed: {}", e),
            duration_ms: Some(duration_ms),
        },
    }
}

async fn test_brave_api(api_key: &str) -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let result = client
        .get("https://api.search.brave.com/res/v1/web/search?q=test")
        .header("X-Subscription-Token", api_key)
        .send()
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => DiagnosticCheck {
            name: "Brave Search API".to_string(),
            category: "Data Sources".to_string(),
            passed: true,
            critical: false,
            message: format!("OK ({}ms)", duration_ms),
            duration_ms: Some(duration_ms),
        },
        Ok(resp) => DiagnosticCheck {
            name: "Brave Search API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("HTTP {} (check API key?)", resp.status()),
            duration_ms: Some(duration_ms),
        },
        Err(e) => DiagnosticCheck {
            name: "Brave Search API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("Failed: {}", e),
            duration_ms: Some(duration_ms),
        },
    }
}

async fn test_fred_api(api_key: &str) -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let result = client
        .get(format!(
            "https://api.stlouisfed.org/fred/series/observations?series_id=DGS10&api_key={}&file_type=json&limit=1",
            api_key
        ))
        .send()
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => DiagnosticCheck {
            name: "FRED API".to_string(),
            category: "Data Sources".to_string(),
            passed: true,
            critical: false,
            message: format!("OK ({}ms)", duration_ms),
            duration_ms: Some(duration_ms),
        },
        Ok(resp) => DiagnosticCheck {
            name: "FRED API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("HTTP {} (check API key?)", resp.status()),
            duration_ms: Some(duration_ms),
        },
        Err(e) => DiagnosticCheck {
            name: "FRED API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("Failed: {}", e),
            duration_ms: Some(duration_ms),
        },
    }
}

async fn test_polymarket_api() -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let result = client
        .get("https://gamma-api.polymarket.com/events?limit=1")
        .send()
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => DiagnosticCheck {
            name: "Polymarket API".to_string(),
            category: "Data Sources".to_string(),
            passed: true,
            critical: false,
            message: format!("OK ({}ms)", duration_ms),
            duration_ms: Some(duration_ms),
        },
        Ok(resp) => DiagnosticCheck {
            name: "Polymarket API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("HTTP {}", resp.status()),
            duration_ms: Some(duration_ms),
        },
        Err(e) => DiagnosticCheck {
            name: "Polymarket API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("Failed: {}", e),
            duration_ms: Some(duration_ms),
        },
    }
}

async fn test_cot_api() -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let result = client
        .get("https://publicreporting.cftc.gov/resource/72hh-3qpy.json?cftc_contract_market_code=088691&$limit=1")
        .send()
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => DiagnosticCheck {
            name: "CFTC COT API".to_string(),
            category: "Data Sources".to_string(),
            passed: true,
            critical: false,
            message: format!("OK ({}ms)", duration_ms),
            duration_ms: Some(duration_ms),
        },
        Ok(resp) => DiagnosticCheck {
            name: "CFTC COT API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("HTTP {}", resp.status()),
            duration_ms: Some(duration_ms),
        },
        Err(e) => DiagnosticCheck {
            name: "CFTC COT API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("Failed: {}", e),
            duration_ms: Some(duration_ms),
        },
    }
}

async fn test_bls_api() -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let result = client
        .get("https://api.bls.gov/publicAPI/v2/timeseries/data/CUUR0000SA0")
        .send()
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => DiagnosticCheck {
            name: "BLS API".to_string(),
            category: "Data Sources".to_string(),
            passed: true,
            critical: false,
            message: format!("OK ({}ms)", duration_ms),
            duration_ms: Some(duration_ms),
        },
        Ok(resp) => DiagnosticCheck {
            name: "BLS API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("HTTP {}", resp.status()),
            duration_ms: Some(duration_ms),
        },
        Err(e) => DiagnosticCheck {
            name: "BLS API".to_string(),
            category: "Data Sources".to_string(),
            passed: false,
            critical: false,
            message: format!("Failed: {}", e),
            duration_ms: Some(duration_ms),
        },
    }
}

/// Parse a timestamp string flexibly (RFC3339, Postgres-style, naive).
fn parse_timestamp_flexible(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%#z")
        .or_else(|_| chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%#z"))
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, chrono::Utc))
        })
}

async fn test_cache_freshness(
    backend: &DatabaseBackend,
    postgres_url: Option<&str>,
) -> DiagnosticCheck {
    let backend = match backend {
        DatabaseBackend::Sqlite => {
            let db_path = crate::db::default_db_path();

            let conn = match Connection::open(&db_path) {
                Ok(c) => c,
                Err(e) => {
                    return DiagnosticCheck {
                        name: "Cache Freshness".to_string(),
                        category: "Data Health".to_string(),
                        passed: false,
                        critical: false,
                        message: format!("Failed to open SQLite: {}", e),
                        duration_ms: None,
                    }
                }
            };

            BackendConnection::Sqlite { conn }
        }
        DatabaseBackend::Postgres => {
            let database_url = match postgres_url {
                Some(url) => url,
                None => {
                    return DiagnosticCheck {
                        name: "Cache Freshness".to_string(),
                        category: "Data Health".to_string(),
                        passed: false,
                        critical: false,
                        message: "postgres_url not configured in config.toml".to_string(),
                        duration_ms: None,
                    }
                }
            };

            let pool = match sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(5))
                .connect(database_url)
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    return DiagnosticCheck {
                        name: "Cache Freshness".to_string(),
                        category: "Data Health".to_string(),
                        passed: false,
                        critical: false,
                        message: format!("Failed to connect to PostgreSQL: {}", e),
                        duration_ms: None,
                    }
                }
            };

            BackendConnection::Postgres { pool }
        }
    };

    // Check price_cache freshness
    let price_count = match &backend {
        BackendConnection::Sqlite { conn } => conn
            .query_row(
                "SELECT COUNT(*) FROM price_cache",
                [],
                |row: &rusqlite::Row| row.get::<_, i64>(0),
            )
            .unwrap_or(0),
        BackendConnection::Postgres { pool } => {
            sqlx::query_scalar("SELECT COUNT(*) FROM price_cache")
                .fetch_one(pool)
                .await
                .unwrap_or(0)
        }
    };

    if price_count == 0 {
        return DiagnosticCheck {
            name: "Cache Freshness".to_string(),
            category: "Data Health".to_string(),
            passed: false,
            critical: false,
            message: "No cached prices — run 'pftui refresh' to populate".to_string(),
            duration_ms: None,
        };
    }

    // Check price age
    let price_age: Option<String> = match &backend {
        BackendConnection::Sqlite { conn } => conn
            .query_row(
                "SELECT last_fetch FROM price_cache ORDER BY last_fetch DESC LIMIT 1",
                [],
                |row: &rusqlite::Row| row.get(0),
            )
            .ok(),
        BackendConnection::Postgres { pool } => sqlx::query_scalar(
            "SELECT last_fetch FROM price_cache ORDER BY last_fetch DESC LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten(),
    };

    let status = if let Some(ref last_fetch) = price_age {
        if let Some(dt) = parse_timestamp_flexible(last_fetch) {
            let age = chrono::Utc::now().signed_duration_since(dt);
            let age_mins = age.num_minutes();

            if age_mins < 15 {
                format!(
                    "Fresh (last price update: {}m ago, {} symbols)",
                    age_mins, price_count
                )
            } else if age_mins < 60 {
                format!(
                    "Stale (last price update: {}m ago, {} symbols) — consider refresh",
                    age_mins, price_count
                )
            } else {
                format!(
                    "Stale (last price update: {}h ago, {} symbols) — run 'pftui refresh'",
                    age_mins / 60,
                    price_count
                )
            }
        } else {
            format!("Unknown age ({} symbols)", price_count)
        }
    } else {
        format!("No timestamp available ({} symbols)", price_count)
    };

    DiagnosticCheck {
        name: "Cache Freshness".to_string(),
        category: "Data Health".to_string(),
        passed: true,
        critical: false,
        message: status,
        duration_ms: None,
    }
}

/// BTC series divergence guard.
///
/// The local DB can carry two BTC series: `BTC` (fresh but shallow) and
/// `BTC-USD` (deep but occasionally stale — it has lagged spot by 28%).
/// Compare the latest closes of both where each has data within the last
/// 7 days; divergence above 2% fails the check with both values + dates.
/// One series missing recent data yields a warning naming which.
const BTC_DIVERGENCE_WINDOW_DAYS: i64 = 7;
const BTC_DIVERGENCE_THRESHOLD_PCT: f64 = 2.0;

fn latest_close(conn: &Connection, symbol: &str) -> Option<(String, rust_decimal::Decimal)> {
    conn.query_row(
        "SELECT date, close FROM price_history WHERE symbol = ?1 ORDER BY date DESC LIMIT 1",
        [symbol],
        |row| {
            let date: String = row.get(0)?;
            let close: String = row.get(1)?;
            Ok((date, close))
        },
    )
    .ok()
    .and_then(|(date, close)| {
        close
            .parse::<rust_decimal::Decimal>()
            .ok()
            .map(|c| (date, c))
    })
}

fn btc_series_divergence_check(conn: &Connection) -> DiagnosticCheck {
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::Decimal;

    let mk = |passed: bool, message: String| DiagnosticCheck {
        name: "BTC Series Divergence".to_string(),
        category: "Data Health".to_string(),
        passed,
        critical: false,
        message,
        duration_ms: None,
    };

    let btc = latest_close(conn, "BTC");
    let btc_usd = latest_close(conn, "BTC-USD");
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(BTC_DIVERGENCE_WINDOW_DAYS))
        .format("%Y-%m-%d")
        .to_string();

    match (btc, btc_usd) {
        (None, None) => mk(
            true,
            "Neither BTC nor BTC-USD tracked in price_history — divergence check skipped"
                .to_string(),
        ),
        (Some(_), None) => mk(
            true,
            "Only the BTC series is tracked (no BTC-USD) — divergence check not applicable"
                .to_string(),
        ),
        (None, Some(_)) => mk(
            true,
            "Only the BTC-USD series is tracked (no BTC) — divergence check not applicable"
                .to_string(),
        ),
        (Some((btc_date, btc_close)), Some((usd_date, usd_close))) => {
            let btc_recent = btc_date.as_str() >= cutoff.as_str();
            let usd_recent = usd_date.as_str() >= cutoff.as_str();
            match (btc_recent, usd_recent) {
                (true, true) => {
                    let midpoint = (btc_close + usd_close) / Decimal::from(2);
                    if midpoint == Decimal::ZERO {
                        return mk(false, "Warning: BTC series closes are zero".to_string());
                    }
                    let divergence_pct = ((btc_close - usd_close).abs() / midpoint
                        * Decimal::from(100))
                    .round_dp(2)
                    .to_f64()
                    .unwrap_or(0.0);
                    if divergence_pct > BTC_DIVERGENCE_THRESHOLD_PCT {
                        mk(
                            false,
                            format!(
                                "BTC series diverged {:.2}% (> {:.0}%): BTC {} ({}) vs BTC-USD {} ({}) — one series is stale; refresh or repair before trusting BTC analytics",
                                divergence_pct,
                                BTC_DIVERGENCE_THRESHOLD_PCT,
                                btc_close, btc_date, usd_close, usd_date
                            ),
                        )
                    } else {
                        mk(
                            true,
                            format!(
                                "BTC series agree within {:.2}%: BTC {} ({}) vs BTC-USD {} ({})",
                                divergence_pct, btc_close, btc_date, usd_close, usd_date
                            ),
                        )
                    }
                }
                (true, false) => mk(
                    false,
                    format!(
                        "Warning: BTC-USD has no data in the last {} days (latest {}) — deep series may be stale vs BTC ({})",
                        BTC_DIVERGENCE_WINDOW_DAYS, usd_date, btc_date
                    ),
                ),
                (false, true) => mk(
                    false,
                    format!(
                        "Warning: BTC has no data in the last {} days (latest {}) — spot series may be stale vs BTC-USD ({})",
                        BTC_DIVERGENCE_WINDOW_DAYS, btc_date, usd_date
                    ),
                ),
                (false, false) => mk(
                    true,
                    format!(
                        "No recent data in either BTC series (BTC latest {}, BTC-USD latest {}) — divergence check skipped",
                        btc_date, usd_date
                    ),
                ),
            }
        }
    }
}

/// Registered-series freshness check (R3): every `series_registry` row past
/// 2x its freshness SLA is named in a warning. Within 1x-2x SLA is treated
/// as routine drift (the refresh loop's job); past 2x means a feed has gone
/// dark — the precursor to "stale data infecting the loops".
fn series_registry_staleness_check(conn: &Connection) -> DiagnosticCheck {
    let mk = |passed: bool, message: String| DiagnosticCheck {
        name: "Registered Series Freshness".to_string(),
        category: "Data Health".to_string(),
        passed,
        critical: false,
        message,
        duration_ms: None,
    };

    // Read-only connection: the registry may not exist yet on a DB that has
    // never run the R3 migration. That's a pass, not a failure.
    match crate::db::archive::table_exists(conn, "series_registry") {
        Ok(false) => {
            return mk(
                true,
                "series_registry not initialized yet (created on next normal startup)"
                    .to_string(),
            )
        }
        Err(e) => return mk(false, format!("could not inspect series_registry: {e}")),
        Ok(true) => {}
    }

    let entries = match crate::db::series_registry::list(conn) {
        Ok(entries) => entries,
        Err(e) => return mk(false, format!("could not read series_registry: {e}")),
    };
    let now = chrono::Utc::now();
    let mut past_2x: Vec<String> = Vec::new();
    let mut stale_count = 0usize;
    for entry in &entries {
        match crate::db::series_registry::status_for(conn, entry, now) {
            Ok(status) => {
                if status.stale {
                    stale_count += 1;
                }
                if status.past_2x_sla {
                    let age = status
                        .age_hours
                        .map(|h| format!("{:.0}h old", h))
                        .unwrap_or_else(|| "no data".to_string());
                    past_2x.push(format!(
                        "{} ({age}, SLA {}h)",
                        entry.series_id, entry.freshness_sla_hours
                    ));
                }
            }
            Err(e) => past_2x.push(format!("{} (status error: {e})", entry.series_id)),
        }
    }
    if past_2x.is_empty() {
        mk(
            true,
            format!(
                "{} registered series; {} past SLA, none past 2x SLA",
                entries.len(),
                stale_count
            ),
        )
    } else {
        mk(
            false,
            format!(
                "{} of {} registered series past 2x freshness SLA: {} — run `pftui data refresh` \
                 (detail: pftui data series status)",
                past_2x.len(),
                entries.len(),
                past_2x.join(", ")
            ),
        )
    }
}

/// One-line DB-wide false-value audit summary (read-only; detail lives in
/// `pftui data audit`). Suspect+corrupt findings fail the check (non-critical
/// — the audit is advisory; repair stays manual).
fn data_audit_summary_check(conn: &Connection) -> DiagnosticCheck {
    let (passed, message) = match crate::commands::data_audit::doctor_summary(conn) {
        Ok((passed, message)) => (passed, message),
        Err(e) => (false, format!("data audit failed to run: {e}")),
    };
    DiagnosticCheck {
        name: "Data Audit".to_string(),
        category: "Data Health".to_string(),
        passed,
        critical: false,
        message,
        duration_ms: None,
    }
}

/// One-line thesis evidence-contract summary (read-only; detail lives in
/// `pftui research verify-thesis`). Broken SQL, structural drift, or
/// untagged contract violations fail the check (non-critical — repair is
/// a curated L4 edit, never automatic).
fn thesis_evidence_check(conn: &Connection) -> DiagnosticCheck {
    let (passed, message) = match crate::research::thesis_verify::doctor_summary(conn) {
        Ok((passed, message)) => (passed, message),
        Err(e) => (false, format!("thesis verification failed to run: {e}")),
    };
    DiagnosticCheck {
        name: "Thesis Evidence".to_string(),
        category: "Data Health".to_string(),
        passed,
        critical: false,
        message,
        duration_ms: None,
    }
}

fn output_json(checks: &[DiagnosticCheck]) -> Result<()> {
    let output = serde_json::json!({
        "checks": checks,
        "summary": {
            "total": checks.len(),
            "passed": checks.iter().filter(|c| c.passed).count(),
            "failed": checks.iter().filter(|c| !c.passed).count(),
            "critical_failures": checks.iter().filter(|c| c.critical && !c.passed).count(),
        }
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn output_human(checks: &[DiagnosticCheck]) {
    println!("pftui doctor — System Diagnostics");
    println!("════════════════════════════════════════════════════════════════\n");

    let mut by_category: std::collections::HashMap<&str, Vec<&DiagnosticCheck>> =
        std::collections::HashMap::new();
    for check in checks {
        by_category.entry(&check.category).or_default().push(check);
    }

    let categories = ["Infrastructure", "Data Sources", "Data Health"];
    for category in &categories {
        if let Some(category_checks) = by_category.get(category) {
            println!("{}:", category);
            for check in category_checks {
                let symbol = if check.passed { "✓" } else { "✗" };
                let critical_mark = if check.critical && !check.passed {
                    " [CRITICAL]"
                } else {
                    ""
                };
                println!("  {} {}{}", symbol, check.name, critical_mark);
                println!("    {}", check.message);
                if let Some(duration) = check.duration_ms {
                    println!("    ({}ms)", duration);
                }
            }
            println!();
        }
    }

    let passed = checks.iter().filter(|c| c.passed).count();
    let failed = checks.iter().filter(|c| !c.passed).count();
    let critical_failures = checks.iter().filter(|c| c.critical && !c.passed).count();

    println!("Summary: {}/{} checks passed", passed, checks.len());
    if failed > 0 {
        println!(
            "         {} failed{}",
            failed,
            if critical_failures > 0 {
                format!(" ({} critical)", critical_failures)
            } else {
                String::new()
            }
        );
    }

    if critical_failures > 0 {
        println!("\n⚠️  Critical failures detected. pftui may not function correctly.");
    } else if failed > 0 {
        println!("\n⚠️  Some data sources are unavailable. Core functionality should work.");
    } else {
        println!("\n✓ All systems operational.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        conn
    }

    fn insert_close(conn: &Connection, symbol: &str, date: &str, close: &str) {
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source) VALUES (?1, ?2, ?3, 'test')",
            rusqlite::params![symbol, date, close],
        )
        .unwrap();
    }

    fn recent_date(days_ago: i64) -> String {
        (chrono::Utc::now() - chrono::Duration::days(days_ago))
            .format("%Y-%m-%d")
            .to_string()
    }

    #[test]
    fn btc_divergence_skips_when_untracked() {
        let conn = setup_conn();
        let check = btc_series_divergence_check(&conn);
        assert!(check.passed);
        assert!(check.message.contains("skipped"));
        assert_eq!(check.category, "Data Health");
        assert!(!check.critical);
    }

    #[test]
    fn btc_divergence_single_series_not_applicable() {
        let conn = setup_conn();
        insert_close(&conn, "BTC", &recent_date(0), "100000");
        let check = btc_series_divergence_check(&conn);
        assert!(check.passed);
        assert!(check.message.contains("no BTC-USD"));
    }

    #[test]
    fn btc_divergence_within_threshold_passes() {
        let conn = setup_conn();
        insert_close(&conn, "BTC", &recent_date(0), "100000");
        insert_close(&conn, "BTC-USD", &recent_date(1), "101000");
        let check = btc_series_divergence_check(&conn);
        assert!(check.passed, "message: {}", check.message);
        assert!(check.message.contains("agree"));
    }

    #[test]
    fn btc_divergence_above_threshold_fails_with_values_and_dates() {
        let conn = setup_conn();
        let btc_date = recent_date(0);
        let usd_date = recent_date(2);
        insert_close(&conn, "BTC", &btc_date, "100000");
        // 28% lag — the historically observed failure mode.
        insert_close(&conn, "BTC-USD", &usd_date, "72000");
        let check = btc_series_divergence_check(&conn);
        assert!(!check.passed);
        assert!(!check.critical);
        assert!(check.message.contains("100000"), "message: {}", check.message);
        assert!(check.message.contains("72000"));
        assert!(check.message.contains(&btc_date));
        assert!(check.message.contains(&usd_date));
    }

    #[test]
    fn btc_divergence_warns_when_one_series_stale() {
        let conn = setup_conn();
        insert_close(&conn, "BTC", &recent_date(0), "100000");
        insert_close(&conn, "BTC-USD", "2026-01-01", "72000");
        let check = btc_series_divergence_check(&conn);
        assert!(!check.passed);
        assert!(check.message.contains("Warning"));
        assert!(check.message.contains("BTC-USD has no data"));

        // And the mirrored case.
        let conn = setup_conn();
        insert_close(&conn, "BTC", "2026-01-01", "100000");
        insert_close(&conn, "BTC-USD", &recent_date(0), "98000");
        let check = btc_series_divergence_check(&conn);
        assert!(!check.passed);
        assert!(check.message.contains("BTC has no data"));
    }

    #[test]
    fn btc_divergence_both_stale_skips() {
        let conn = setup_conn();
        insert_close(&conn, "BTC", "2026-01-01", "100000");
        insert_close(&conn, "BTC-USD", "2026-01-02", "72000");
        let check = btc_series_divergence_check(&conn);
        assert!(check.passed);
        assert!(check.message.contains("No recent data"));
    }

    #[test]
    fn series_staleness_passes_when_registry_missing() {
        // A read-only DB that has never run the R3 migration.
        let conn = Connection::open_in_memory().unwrap();
        let check = series_registry_staleness_check(&conn);
        assert!(check.passed);
        assert!(check.message.contains("not initialized"));
    }

    #[test]
    fn series_staleness_names_series_past_2x_sla() {
        let conn = setup_conn();
        // Migrated registry, all series empty -> everything past 2x SLA.
        let check = series_registry_staleness_check(&conn);
        assert!(!check.passed);
        assert!(check.message.contains("past 2x freshness SLA"));
        assert!(
            check.message.contains("gold"),
            "stale series should be named: {}",
            check.message
        );

        // Fresh datapoint clears one series; it must drop out of the warning
        // (cot-gold is still listed — match on the exact list item).
        insert_close(&conn, "GC=F", &recent_date(0), "3350");
        let check = series_registry_staleness_check(&conn);
        assert!(!check.passed); // others still dark
        let flags_gold = check
            .message
            .split(&[',', ':'][..])
            .map(str::trim)
            .any(|part| part.starts_with("gold ("));
        assert!(
            !flags_gold,
            "fresh gold series should not be flagged: {}",
            check.message
        );
    }
}
