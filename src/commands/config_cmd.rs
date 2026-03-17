use anyhow::{anyhow, bail, Result};

use crate::config::{load_config, save_config, DatabaseBackend, WatchlistColumn, WorkspaceLayout};

pub fn run(action: &str, field: Option<&str>, value: Option<&str>, json: bool) -> Result<()> {
    match action {
        "list" => list_config(json),
        "get" => get_field(field, json),
        "set" => set_field(field, value),
        _ => bail!("Invalid action '{}'. Use: list, get, set", action),
    }
}

fn list_config(json: bool) -> Result<()> {
    let config = load_config()?;

    if json {
        use serde_json::json;

        let output = json!({
            "database_backend": format_database_backend(config.database_backend),
            "database_url": format_secret(config.database_url.as_deref()),
            "mirror_source_url": format_secret(config.mirror_source_url.as_deref()),
            "postgres_read_only": config.postgres_read_only,
            "postgres_max_connections": config.postgres_max_connections,
            "postgres_connect_timeout_secs": config.postgres_connect_timeout_secs,
            "base_currency": config.base_currency,
            "refresh_interval": config.refresh_interval,
            "auto_refresh": config.auto_refresh,
            "refresh_interval_secs": config.refresh_interval_secs,
            "portfolio_mode": format!("{:?}", config.portfolio_mode).to_lowercase(),
            "theme": config.theme,
            "home_tab": config.home_tab,
            "layout": format_layout(config.layout),
            "fred_api_key": format_secret(config.fred_api_key.as_deref()),
            "brave_api_key": format_secret(config.brave_api_key.as_deref()),
            "news_poll_interval": config.news_poll_interval,
            "custom_news_feeds": config.custom_news_feeds.len(),
            "chart_sma": config.chart_sma,
            "mobile.enabled": config.mobile.enabled,
            "mobile.bind": config.mobile.bind,
            "mobile.port": config.mobile.port,
            "mobile.api_tokens": config.mobile.api_tokens.len(),
            "mobile.cert_path": config.mobile.cert_path,
            "mobile.key_path": config.mobile.key_path,
            "mobile.session_ttl_hours": config.mobile.session_ttl_hours,
            "watchlist_columns": config.watchlist.columns.iter()
                .map(|c| format_watchlist_column(*c))
                .collect::<Vec<_>>(),
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "database_backend = {}",
            format_database_backend(config.database_backend)
        );
        println!(
            "database_url = {}",
            format_secret(config.database_url.as_deref())
        );
        println!(
            "mirror_source_url = {}",
            format_secret(config.mirror_source_url.as_deref())
        );
        println!("postgres_read_only = {}", config.postgres_read_only);
        println!(
            "postgres_max_connections = {}",
            config.postgres_max_connections
        );
        println!(
            "postgres_connect_timeout_secs = {}",
            config.postgres_connect_timeout_secs
        );
        println!("base_currency = {}", config.base_currency);
        println!("refresh_interval = {}", config.refresh_interval);
        println!("auto_refresh = {}", config.auto_refresh);
        println!("refresh_interval_secs = {}", config.refresh_interval_secs);
        println!(
            "portfolio_mode = {}",
            format!("{:?}", config.portfolio_mode).to_lowercase()
        );
        println!("theme = {}", config.theme);
        println!("home_tab = {}", config.home_tab);
        println!("layout = {}", format_layout(config.layout));
        println!(
            "fred_api_key = {}",
            format_secret(config.fred_api_key.as_deref())
        );
        println!(
            "brave_api_key = {}",
            format_secret(config.brave_api_key.as_deref())
        );
        println!("news_poll_interval = {}", config.news_poll_interval);
        println!(
            "custom_news_feeds = {} entries",
            config.custom_news_feeds.len()
        );
        println!("chart_sma = {:?}", config.chart_sma);
        println!("mobile.enabled = {}", config.mobile.enabled);
        println!("mobile.bind = {}", config.mobile.bind);
        println!("mobile.port = {}", config.mobile.port);
        println!("mobile.api_tokens = {}", config.mobile.api_tokens.len());
        println!(
            "mobile.cert_path = {}",
            config.mobile.cert_path.as_deref().unwrap_or("")
        );
        println!(
            "mobile.key_path = {}",
            config.mobile.key_path.as_deref().unwrap_or("")
        );
        println!(
            "mobile.session_ttl_hours = {}",
            config.mobile.session_ttl_hours
        );
        println!(
            "watchlist.columns = [{}]",
            config
                .watchlist
                .columns
                .iter()
                .map(|c| format!("\"{}\"", format_watchlist_column(*c)))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(())
}

fn get_field(field: Option<&str>, json: bool) -> Result<()> {
    let field = field.ok_or_else(|| anyhow!("Missing field. Usage: pftui config get <field>"))?;
    let config = load_config()?;

    if json {
        use serde_json::json;

        let value = match field {
            "database_backend" => json!(format_database_backend(config.database_backend)),
            "database_url" => json!(format_secret(config.database_url.as_deref())),
            "mirror_source_url" => json!(format_secret(config.mirror_source_url.as_deref())),
            "postgres_read_only" => json!(config.postgres_read_only),
            "postgres_max_connections" => json!(config.postgres_max_connections),
            "postgres_connect_timeout_secs" => json!(config.postgres_connect_timeout_secs),
            "base_currency" => json!(config.base_currency),
            "refresh_interval" => json!(config.refresh_interval),
            "auto_refresh" => json!(config.auto_refresh),
            "refresh_interval_secs" => json!(config.refresh_interval_secs),
            "portfolio_mode" => json!(format!("{:?}", config.portfolio_mode).to_lowercase()),
            "theme" => json!(config.theme),
            "home_tab" => json!(config.home_tab),
            "layout" | "workspace_layout" => json!(format_layout(config.layout)),
            "fred_api_key" => json!(format_secret(config.fred_api_key.as_deref())),
            "brave_api_key" => json!(format_secret(config.brave_api_key.as_deref())),
            "news_poll_interval" => json!(config.news_poll_interval),
            "custom_news_feeds" => json!(config.custom_news_feeds.len()),
            "chart_sma" => json!(config.chart_sma),
            "mobile.enabled" => json!(config.mobile.enabled),
            "mobile.bind" => json!(config.mobile.bind),
            "mobile.port" => json!(config.mobile.port),
            "mobile.api_tokens" => json!(config.mobile.api_tokens.len()),
            "mobile.cert_path" => json!(config.mobile.cert_path),
            "mobile.key_path" => json!(config.mobile.key_path),
            "mobile.session_ttl_hours" => json!(config.mobile.session_ttl_hours),
            "watchlist.columns" | "watchlist_columns" => json!(config
                .watchlist
                .columns
                .iter()
                .map(|c| format_watchlist_column(*c))
                .collect::<Vec<_>>()),
            _ => bail!("Unknown field '{}'", field),
        };

        let output = json!({
            "field": field,
            "value": value,
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        match field {
            "database_backend" => println!("{}", format_database_backend(config.database_backend)),
            "database_url" => println!("{}", format_secret(config.database_url.as_deref())),
            "mirror_source_url" => {
                println!("{}", format_secret(config.mirror_source_url.as_deref()))
            }
            "postgres_read_only" => println!("{}", config.postgres_read_only),
            "postgres_max_connections" => println!("{}", config.postgres_max_connections),
            "postgres_connect_timeout_secs" => println!("{}", config.postgres_connect_timeout_secs),
            "base_currency" => println!("{}", config.base_currency),
            "refresh_interval" => println!("{}", config.refresh_interval),
            "auto_refresh" => println!("{}", config.auto_refresh),
            "refresh_interval_secs" => println!("{}", config.refresh_interval_secs),
            "portfolio_mode" => {
                println!("{}", format!("{:?}", config.portfolio_mode).to_lowercase())
            }
            "theme" => println!("{}", config.theme),
            "home_tab" => println!("{}", config.home_tab),
            "layout" | "workspace_layout" => println!("{}", format_layout(config.layout)),
            "fred_api_key" => println!("{}", format_secret(config.fred_api_key.as_deref())),
            "brave_api_key" => println!("{}", format_secret(config.brave_api_key.as_deref())),
            "news_poll_interval" => println!("{}", config.news_poll_interval),
            "custom_news_feeds" => println!("{}", config.custom_news_feeds.len()),
            "chart_sma" => println!("{:?}", config.chart_sma),
            "mobile.enabled" => println!("{}", config.mobile.enabled),
            "mobile.bind" => println!("{}", config.mobile.bind),
            "mobile.port" => println!("{}", config.mobile.port),
            "mobile.api_tokens" => println!("{}", config.mobile.api_tokens.len()),
            "mobile.cert_path" => println!("{}", config.mobile.cert_path.as_deref().unwrap_or("")),
            "mobile.key_path" => println!("{}", config.mobile.key_path.as_deref().unwrap_or("")),
            "mobile.session_ttl_hours" => println!("{}", config.mobile.session_ttl_hours),
            "watchlist.columns" | "watchlist_columns" => println!(
                "[{}]",
                config
                    .watchlist
                    .columns
                    .iter()
                    .map(|c| format!("\"{}\"", format_watchlist_column(*c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => bail!("Unknown field '{}'", field),
        }
    }

    Ok(())
}

fn set_field(field: Option<&str>, value: Option<&str>) -> Result<()> {
    let field =
        field.ok_or_else(|| anyhow!("Missing field. Usage: pftui config set <field> <value>"))?;
    let value =
        value.ok_or_else(|| anyhow!("Missing value. Usage: pftui config set <field> <value>"))?;

    let mut config = load_config()?;
    match field {
        "database_backend" => {
            config.database_backend = parse_database_backend(value)?;
            save_config(&config)?;
            println!(
                "Updated database_backend = {}",
                format_database_backend(config.database_backend)
            );
        }
        "database_url" => {
            let trimmed = value.trim();
            config.database_url = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
            save_config(&config)?;
            println!("Updated database_url");
        }
        "mirror_source_url" => {
            let trimmed = value.trim();
            config.mirror_source_url = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
            save_config(&config)?;
            println!("Updated mirror_source_url");
        }
        "postgres_read_only" => {
            let parsed = match value.trim().to_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => true,
                "false" | "0" | "no" | "off" => false,
                _ => bail!("Invalid postgres_read_only '{}'. Use: true|false", value),
            };
            config.postgres_read_only = parsed;
            save_config(&config)?;
            println!("Updated postgres_read_only = {}", config.postgres_read_only);
        }
        "postgres_max_connections" => {
            let parsed = value
                .trim()
                .parse::<u32>()
                .map_err(|_| anyhow!("Invalid postgres_max_connections '{}'", value))?;
            if parsed == 0 {
                bail!("postgres_max_connections must be > 0");
            }
            config.postgres_max_connections = parsed;
            save_config(&config)?;
            println!(
                "Updated postgres_max_connections = {}",
                config.postgres_max_connections
            );
        }
        "postgres_connect_timeout_secs" => {
            let parsed = value
                .trim()
                .parse::<u64>()
                .map_err(|_| anyhow!("Invalid postgres_connect_timeout_secs '{}'", value))?;
            if parsed == 0 {
                bail!("postgres_connect_timeout_secs must be > 0");
            }
            config.postgres_connect_timeout_secs = parsed;
            save_config(&config)?;
            println!(
                "Updated postgres_connect_timeout_secs = {}",
                config.postgres_connect_timeout_secs
            );
        }
        "brave_api_key" => {
            let trimmed = value.trim();
            config.brave_api_key = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
            save_config(&config)?;
            println!("Updated brave_api_key");
        }
        "layout" | "workspace_layout" => {
            let parsed = match value.trim().to_lowercase().as_str() {
                "compact" => WorkspaceLayout::Compact,
                "split" => WorkspaceLayout::Split,
                "analyst" => WorkspaceLayout::Analyst,
                _ => bail!(
                    "Invalid layout '{}'. Use: compact, split, analyst",
                    value
                ),
            };
            config.layout = parsed;
            save_config(&config)?;
            println!(
                "Updated layout = {}",
                format_layout(config.layout)
            );
        }
        "auto_refresh" => {
            let parsed = match value.trim().to_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => true,
                "false" | "0" | "no" | "off" => false,
                _ => bail!("Invalid auto_refresh '{}'. Use: true|false", value),
            };
            config.auto_refresh = parsed;
            save_config(&config)?;
            println!("Updated auto_refresh = {}", config.auto_refresh);
        }
        "refresh_interval_secs" => {
            let parsed = value
                .trim()
                .parse::<u64>()
                .map_err(|_| anyhow!("Invalid refresh_interval_secs '{}'", value))?;
            if parsed == 0 {
                bail!("refresh_interval_secs must be > 0");
            }
            config.refresh_interval_secs = parsed;
            save_config(&config)?;
            println!("Updated refresh_interval_secs = {}", config.refresh_interval_secs);
        }
        "mobile.enabled" => {
            config.mobile.enabled = parse_bool(value, "mobile.enabled")?;
            save_config(&config)?;
            println!("Updated mobile.enabled = {}", config.mobile.enabled);
        }
        "mobile.bind" => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                bail!("mobile.bind cannot be empty");
            }
            config.mobile.bind = trimmed.to_string();
            save_config(&config)?;
            println!("Updated mobile.bind = {}", config.mobile.bind);
        }
        "mobile.port" => {
            let parsed = value
                .trim()
                .parse::<u16>()
                .map_err(|_| anyhow!("Invalid mobile.port '{}'", value))?;
            if parsed == 0 {
                bail!("mobile.port must be > 0");
            }
            config.mobile.port = parsed;
            save_config(&config)?;
            println!("Updated mobile.port = {}", config.mobile.port);
        }
        "mobile.session_ttl_hours" => {
            let parsed = value
                .trim()
                .parse::<u64>()
                .map_err(|_| anyhow!("Invalid mobile.session_ttl_hours '{}'", value))?;
            if parsed == 0 {
                bail!("mobile.session_ttl_hours must be > 0");
            }
            config.mobile.session_ttl_hours = parsed;
            save_config(&config)?;
            println!(
                "Updated mobile.session_ttl_hours = {}",
                config.mobile.session_ttl_hours
            );
        }
        "watchlist.columns" | "watchlist_columns" => {
            let parsed = parse_watchlist_columns(value)?;
            config.watchlist.columns = parsed;
            save_config(&config)?;
            println!(
                "Updated watchlist.columns = [{}]",
                config.watchlist.columns.iter().map(|c| format!("\"{}\"", format_watchlist_column(*c))).collect::<Vec<_>>().join(", ")
            );
        }
        _ => bail!(
            "Unsupported set field '{}'. Currently supported: database_backend, database_url, mirror_source_url, postgres_read_only, postgres_max_connections, postgres_connect_timeout_secs, brave_api_key, layout, auto_refresh, refresh_interval_secs, mobile.enabled, mobile.bind, mobile.port, mobile.session_ttl_hours, watchlist.columns",
            field
        ),
    }
    Ok(())
}

fn parse_bool(value: &str, field: &str) -> Result<bool> {
    match value.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => bail!("Invalid {} '{}'. Use: true|false", field, value),
    }
}

fn parse_database_backend(value: &str) -> Result<DatabaseBackend> {
    match value.trim().to_lowercase().as_str() {
        "sqlite" => Ok(DatabaseBackend::Sqlite),
        "postgres" | "postgresql" => Ok(DatabaseBackend::Postgres),
        _ => bail!(
            "Invalid database_backend '{}'. Use: sqlite, postgres",
            value
        ),
    }
}

fn format_database_backend(backend: DatabaseBackend) -> &'static str {
    match backend {
        DatabaseBackend::Sqlite => "sqlite",
        DatabaseBackend::Postgres => "postgres",
    }
}

fn format_layout(layout: WorkspaceLayout) -> &'static str {
    match layout {
        WorkspaceLayout::Compact => "compact",
        WorkspaceLayout::Split => "split",
        WorkspaceLayout::Analyst => "analyst",
    }
}

fn format_watchlist_column(column: WatchlistColumn) -> &'static str {
    match column {
        WatchlistColumn::Symbol => "symbol",
        WatchlistColumn::Name => "name",
        WatchlistColumn::Category => "category",
        WatchlistColumn::Price => "price",
        WatchlistColumn::ChangePct => "change_pct",
        WatchlistColumn::Rsi => "rsi",
        WatchlistColumn::Sma50 => "sma50",
        WatchlistColumn::Target => "target",
        WatchlistColumn::Prox => "prox",
    }
}

fn parse_watchlist_columns(value: &str) -> Result<Vec<WatchlistColumn>> {
    let raw = value.trim();
    if raw.is_empty() {
        bail!("watchlist.columns cannot be empty");
    }
    let mut columns = Vec::new();
    for token in raw.split(',') {
        let key = token.trim().trim_matches('"').to_lowercase();
        let col = match key.as_str() {
            "symbol" => WatchlistColumn::Symbol,
            "name" => WatchlistColumn::Name,
            "category" => WatchlistColumn::Category,
            "price" => WatchlistColumn::Price,
            "change_pct" | "change" | "change%" => WatchlistColumn::ChangePct,
            "rsi" => WatchlistColumn::Rsi,
            "sma50" | "sma_50" => WatchlistColumn::Sma50,
            "target" => WatchlistColumn::Target,
            "prox" | "proximity" => WatchlistColumn::Prox,
            _ => bail!("Unknown watchlist column '{}'", token.trim()),
        };
        if !columns.contains(&col) {
            columns.push(col);
        }
    }
    if columns.is_empty() {
        bail!("watchlist.columns cannot be empty");
    }
    Ok(columns)
}

fn format_secret(secret: Option<&str>) -> String {
    match secret {
        None => "(not set)".to_string(),
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                "(not set)".to_string()
            } else {
                mask_secret(trimmed)
            }
        }
    }
}

fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let len = chars.len();
    if len <= 4 {
        "*".repeat(len.max(1))
    } else {
        let suffix: String = chars[len - 4..].iter().collect();
        format!("{}{}", "*".repeat(len - 4), suffix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_secret_but_keeps_suffix() {
        assert_eq!(mask_secret("abc12345"), "****2345");
    }

    #[test]
    fn masks_short_secret() {
        assert_eq!(mask_secret("abc"), "***");
    }

    #[test]
    fn formats_layout() {
        assert_eq!(format_layout(WorkspaceLayout::Compact), "compact");
        assert_eq!(format_layout(WorkspaceLayout::Split), "split");
        assert_eq!(format_layout(WorkspaceLayout::Analyst), "analyst");
    }

    #[test]
    fn formats_database_backend() {
        assert_eq!(format_database_backend(DatabaseBackend::Sqlite), "sqlite");
        assert_eq!(
            format_database_backend(DatabaseBackend::Postgres),
            "postgres"
        );
    }

    #[test]
    fn parses_watchlist_columns_csv() {
        let cols = parse_watchlist_columns("symbol,price,change_pct,rsi").unwrap();
        assert_eq!(
            cols,
            vec![
                WatchlistColumn::Symbol,
                WatchlistColumn::Price,
                WatchlistColumn::ChangePct,
                WatchlistColumn::Rsi
            ]
        );
    }
}
