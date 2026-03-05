use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortfolioMode {
    #[default]
    Full,
    Percentage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_base_currency")]
    pub base_currency: String,
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval: u64,
    #[serde(default)]
    pub portfolio_mode: PortfolioMode,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// FRED API key for fetching economic indicators.
    /// Register at: https://fred.stlouisfed.org/docs/api/api_key.html
    #[serde(default)]
    pub fred_api_key: Option<String>,
    /// RSS news feed polling interval in seconds (default: 600 = 10 minutes)
    #[serde(default = "default_news_poll_interval")]
    pub news_poll_interval: u64,
    /// Custom RSS feeds (name, url, category). If empty, uses default feeds.
    #[serde(default)]
    pub custom_news_feeds: Vec<CustomNewsFeed>,
}

fn default_base_currency() -> String {
    "USD".to_string()
}

fn default_refresh_interval() -> u64 {
    60
}

fn default_theme() -> String {
    "midnight".to_string()
}

fn default_news_poll_interval() -> u64 {
    600 // 10 minutes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomNewsFeed {
    pub name: String,
    pub url: String,
    pub category: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            base_currency: default_base_currency(),
            refresh_interval: default_refresh_interval(),
            portfolio_mode: PortfolioMode::default(),
            theme: default_theme(),
            fred_api_key: None,
            news_poll_interval: default_news_poll_interval(),
            custom_news_feeds: Vec::new(),
        }
    }
}

impl Config {
    pub fn is_percentage_mode(&self) -> bool {
        self.portfolio_mode == PortfolioMode::Percentage
    }

    /// Return the currency symbol for the configured base currency.
    pub fn currency_symbol(&self) -> &str {
        currency_symbol(&self.base_currency)
    }
}

/// Map a currency code to its display symbol.
/// Returns the code itself for unknown currencies.
pub fn currency_symbol(code: &str) -> &str {
    match code {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "CNY" => "¥",
        "KRW" => "₩",
        "INR" => "₹",
        "RUB" => "₽",
        "BRL" => "R$",
        "CHF" => "CHF",
        "CAD" => "C$",
        "AUD" => "A$",
        "NZD" => "NZ$",
        "SEK" => "kr",
        "NOK" => "kr",
        "DKK" => "kr",
        "PLN" => "zł",
        "THB" => "฿",
        "TRY" => "₺",
        "MXN" => "MX$",
        "ZAR" => "R",
        "HKD" => "HK$",
        "SGD" => "S$",
        "TWD" => "NT$",
        "ILS" => "₪",
        _ => code,
    }
}

/// Supported currencies for selection in the setup wizard.
pub const SUPPORTED_CURRENCIES: &[(&str, &str)] = &[
    ("USD", "US Dollar ($)"),
    ("EUR", "Euro (€)"),
    ("GBP", "British Pound (£)"),
    ("JPY", "Japanese Yen (¥)"),
    ("CAD", "Canadian Dollar (C$)"),
    ("AUD", "Australian Dollar (A$)"),
    ("CHF", "Swiss Franc (CHF)"),
    ("CNY", "Chinese Yuan (¥)"),
    ("INR", "Indian Rupee (₹)"),
    ("KRW", "South Korean Won (₩)"),
    ("BRL", "Brazilian Real (R$)"),
    ("SEK", "Swedish Krona (kr)"),
    ("NOK", "Norwegian Krone (kr)"),
    ("NZD", "New Zealand Dollar (NZ$)"),
    ("MXN", "Mexican Peso (MX$)"),
    ("SGD", "Singapore Dollar (S$)"),
    ("HKD", "Hong Kong Dollar (HK$)"),
    ("ZAR", "South African Rand (R)"),
    ("TRY", "Turkish Lira (₺)"),
    ("PLN", "Polish Złoty (zł)"),
];

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pftui")
        .join("config.toml")
}

pub fn load_config() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    } else {
        let config = Config::default();
        save_config(&config)?;
        Ok(config)
    }
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = Config::default();
        assert_eq!(config.base_currency, "USD");
        assert_eq!(config.refresh_interval, 60);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
    }

    #[test]
    fn is_percentage_mode_full() {
        let config = Config::default();
        assert!(!config.is_percentage_mode());
    }

    #[test]
    fn is_percentage_mode_percentage() {
        let config = Config { portfolio_mode: PortfolioMode::Percentage, ..Default::default() };
        assert!(config.is_percentage_mode());
    }

    #[test]
    fn config_roundtrip_toml() {
        let config = Config {
            base_currency: "EUR".to_string(),
            refresh_interval: 30,
            portfolio_mode: PortfolioMode::Percentage,
            theme: "nord".to_string(),
            fred_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.base_currency, "EUR");
        assert_eq!(loaded.refresh_interval, 30);
        assert_eq!(loaded.portfolio_mode, PortfolioMode::Percentage);
        assert_eq!(loaded.theme, "nord");
    }

    #[test]
    fn config_deserialize_missing_fields_uses_defaults() {
        let toml_str = r#"base_currency = "GBP""#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.base_currency, "GBP");
        assert_eq!(config.refresh_interval, 60);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
    }

    #[test]
    fn config_deserialize_empty_uses_all_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.base_currency, "USD");
        assert_eq!(config.refresh_interval, 60);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
    }

    #[test]
    fn portfolio_mode_serialization() {
        assert_eq!(
            serde_json::to_string(&PortfolioMode::Full).unwrap(),
            "\"full\""
        );
        assert_eq!(
            serde_json::to_string(&PortfolioMode::Percentage).unwrap(),
            "\"percentage\""
        );
    }

    #[test]
    fn config_path_ends_with_pftui() {
        let path = config_path();
        assert!(path.ends_with("pftui/config.toml"));
    }

    #[test]
    fn currency_symbol_known_currencies() {
        assert_eq!(currency_symbol("USD"), "$");
        assert_eq!(currency_symbol("EUR"), "€");
        assert_eq!(currency_symbol("GBP"), "£");
        assert_eq!(currency_symbol("JPY"), "¥");
        assert_eq!(currency_symbol("CAD"), "C$");
        assert_eq!(currency_symbol("AUD"), "A$");
        assert_eq!(currency_symbol("CHF"), "CHF");
        assert_eq!(currency_symbol("INR"), "₹");
        assert_eq!(currency_symbol("BRL"), "R$");
    }

    #[test]
    fn currency_symbol_unknown_returns_code() {
        assert_eq!(currency_symbol("XYZ"), "XYZ");
        assert_eq!(currency_symbol("FAKE"), "FAKE");
    }

    #[test]
    fn config_currency_symbol_method() {
        let config = Config::default();
        assert_eq!(config.currency_symbol(), "$");
        let eur_config = Config { base_currency: "EUR".to_string(), ..Default::default() };
        assert_eq!(eur_config.currency_symbol(), "€");
    }

    #[test]
    fn supported_currencies_contains_major() {
        let codes: Vec<&str> = SUPPORTED_CURRENCIES.iter().map(|(c, _)| *c).collect();
        assert!(codes.contains(&"USD"));
        assert!(codes.contains(&"EUR"));
        assert!(codes.contains(&"GBP"));
        assert!(codes.contains(&"JPY"));
    }
}
