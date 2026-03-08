use anyhow::{anyhow, bail, Result};

use crate::config::{load_config, save_config, WatchlistColumn, WorkspaceLayout};

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
            "watchlist_columns": config.watchlist.columns.iter()
                .map(|c| format_watchlist_column(*c))
                .collect::<Vec<_>>(),
        });
        
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("base_currency = {}", config.base_currency);
        println!("refresh_interval = {}", config.refresh_interval);
        println!("auto_refresh = {}", config.auto_refresh);
        println!("refresh_interval_secs = {}", config.refresh_interval_secs);
        println!("portfolio_mode = {}", format!("{:?}", config.portfolio_mode).to_lowercase());
        println!("theme = {}", config.theme);
        println!("home_tab = {}", config.home_tab);
        println!("layout = {}", format_layout(config.layout));
        println!("fred_api_key = {}", format_secret(config.fred_api_key.as_deref()));
        println!("brave_api_key = {}", format_secret(config.brave_api_key.as_deref()));
        println!("news_poll_interval = {}", config.news_poll_interval);
        println!("custom_news_feeds = {} entries", config.custom_news_feeds.len());
        println!("chart_sma = {:?}", config.chart_sma);
        println!(
            "watchlist.columns = [{}]",
            config.watchlist.columns.iter().map(|c| format!("\"{}\"", format_watchlist_column(*c))).collect::<Vec<_>>().join(", ")
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
            "watchlist.columns" | "watchlist_columns" => json!(
                config.watchlist.columns.iter()
                    .map(|c| format_watchlist_column(*c))
                    .collect::<Vec<_>>()
            ),
            _ => bail!("Unknown field '{}'", field),
        };
        
        let output = json!({
            "field": field,
            "value": value,
        });
        
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        match field {
            "base_currency" => println!("{}", config.base_currency),
            "refresh_interval" => println!("{}", config.refresh_interval),
            "auto_refresh" => println!("{}", config.auto_refresh),
            "refresh_interval_secs" => println!("{}", config.refresh_interval_secs),
            "portfolio_mode" => println!("{}", format!("{:?}", config.portfolio_mode).to_lowercase()),
            "theme" => println!("{}", config.theme),
            "home_tab" => println!("{}", config.home_tab),
            "layout" | "workspace_layout" => println!("{}", format_layout(config.layout)),
            "fred_api_key" => println!("{}", format_secret(config.fred_api_key.as_deref())),
            "brave_api_key" => println!("{}", format_secret(config.brave_api_key.as_deref())),
            "news_poll_interval" => println!("{}", config.news_poll_interval),
            "custom_news_feeds" => println!("{}", config.custom_news_feeds.len()),
            "chart_sma" => println!("{:?}", config.chart_sma),
            "watchlist.columns" | "watchlist_columns" => println!(
                "[{}]",
                config.watchlist.columns.iter().map(|c| format!("\"{}\"", format_watchlist_column(*c))).collect::<Vec<_>>().join(", ")
            ),
            _ => bail!("Unknown field '{}'", field),
        }
    }
    
    Ok(())
}

fn set_field(field: Option<&str>, value: Option<&str>) -> Result<()> {
    let field = field.ok_or_else(|| anyhow!("Missing field. Usage: pftui config set <field> <value>"))?;
    let value = value.ok_or_else(|| anyhow!("Missing value. Usage: pftui config set <field> <value>"))?;

    let mut config = load_config()?;
    match field {
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
            "Unsupported set field '{}'. Currently supported: brave_api_key, layout, auto_refresh, refresh_interval_secs, watchlist.columns",
            field
        ),
    }
    Ok(())
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
