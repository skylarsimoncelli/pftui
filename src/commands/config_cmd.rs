use anyhow::{anyhow, bail, Result};

use crate::config::{load_config, save_config, WorkspaceLayout};

pub fn run(action: &str, field: Option<&str>, value: Option<&str>) -> Result<()> {
    match action {
        "list" => list_config(),
        "get" => get_field(field),
        "set" => set_field(field, value),
        _ => bail!("Invalid action '{}'. Use: list, get, set", action),
    }
}

fn list_config() -> Result<()> {
    let config = load_config()?;
    println!("base_currency = {}", config.base_currency);
    println!("refresh_interval = {}", config.refresh_interval);
    println!("portfolio_mode = {}", format!("{:?}", config.portfolio_mode).to_lowercase());
    println!("theme = {}", config.theme);
    println!("home_tab = {}", config.home_tab);
    println!("layout = {}", format_layout(config.layout));
    println!("fred_api_key = {}", format_secret(config.fred_api_key.as_deref()));
    println!("brave_api_key = {}", format_secret(config.brave_api_key.as_deref()));
    println!("news_poll_interval = {}", config.news_poll_interval);
    println!("custom_news_feeds = {} entries", config.custom_news_feeds.len());
    println!("chart_sma = {:?}", config.chart_sma);
    Ok(())
}

fn get_field(field: Option<&str>) -> Result<()> {
    let field = field.ok_or_else(|| anyhow!("Missing field. Usage: pftui config get <field>"))?;
    let config = load_config()?;
    match field {
        "base_currency" => println!("{}", config.base_currency),
        "refresh_interval" => println!("{}", config.refresh_interval),
        "portfolio_mode" => println!("{}", format!("{:?}", config.portfolio_mode).to_lowercase()),
        "theme" => println!("{}", config.theme),
        "home_tab" => println!("{}", config.home_tab),
        "layout" | "workspace_layout" => println!("{}", format_layout(config.layout)),
        "fred_api_key" => println!("{}", format_secret(config.fred_api_key.as_deref())),
        "brave_api_key" => println!("{}", format_secret(config.brave_api_key.as_deref())),
        "news_poll_interval" => println!("{}", config.news_poll_interval),
        "custom_news_feeds" => println!("{}", config.custom_news_feeds.len()),
        "chart_sma" => println!("{:?}", config.chart_sma),
        _ => bail!("Unknown field '{}'", field),
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
        _ => bail!(
            "Unsupported set field '{}'. Currently supported: brave_api_key, layout",
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
}
