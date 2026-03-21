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
        .get("https://www.cftc.gov/files/dea/cotarchives/2024/futures/deacot2024.zip")
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
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(last_fetch) {
            let age = chrono::Utc::now().signed_duration_since(dt.with_timezone(&chrono::Utc));
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
