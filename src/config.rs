use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortfolioMode {
    #[default]
    Full,
    Percentage,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseBackend {
    #[default]
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceLayout {
    Compact,
    #[default]
    Split,
    Analyst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchlistColumn {
    Symbol,
    Name,
    Category,
    Price,
    ChangePct,
    Rsi,
    Sma50,
    Target,
    Prox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistConfig {
    #[serde(default = "default_watchlist_columns")]
    pub columns: Vec<WatchlistColumn>,
}

impl Default for WatchlistConfig {
    fn default() -> Self {
        Self {
            columns: default_watchlist_columns(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingsConfig {
    #[serde(default = "default_key_quit")]
    pub quit: String,
    #[serde(default = "default_key_help")]
    pub help: String,
    #[serde(default = "default_key_command_palette")]
    pub command_palette: String,
    #[serde(default = "default_key_refresh")]
    pub refresh: String,
    #[serde(default = "default_key_search")]
    pub search: String,
    #[serde(default = "default_key_theme_cycle")]
    pub theme_cycle: String,
    #[serde(default = "default_key_privacy_toggle")]
    pub privacy_toggle: String,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            quit: default_key_quit(),
            help: default_key_help(),
            command_palette: default_key_command_palette(),
            refresh: default_key_refresh(),
            search: default_key_search(),
            theme_cycle: default_key_theme_cycle(),
            privacy_toggle: default_key_privacy_toggle(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Database backend selector.
    #[serde(default)]
    pub database_backend: DatabaseBackend,
    /// Database connection URL used when `database_backend = "postgres"`.
    #[serde(default)]
    pub database_url: Option<String>,
    /// PostgreSQL max pool connections.
    #[serde(default = "default_postgres_max_connections")]
    pub postgres_max_connections: u32,
    /// PostgreSQL connect timeout in seconds.
    #[serde(default = "default_postgres_connect_timeout_secs")]
    pub postgres_connect_timeout_secs: u64,
    #[serde(default = "default_base_currency")]
    pub base_currency: String,
    /// Legacy refresh interval (seconds) used by older config versions.
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval: u64,
    /// Enable automatic periodic refresh in the TUI.
    #[serde(default = "default_auto_refresh")]
    pub auto_refresh: bool,
    /// Auto-refresh interval in seconds.
    #[serde(default = "default_refresh_interval_secs")]
    pub refresh_interval_secs: u64,
    #[serde(default)]
    pub portfolio_mode: PortfolioMode,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Preferred home tab when opening TUI/Web.
    /// Allowed values: "positions" or "watchlist".
    #[serde(default = "default_home_tab")]
    pub home_tab: String,
    /// Workspace layout preset for Positions view.
    /// compact: table-first, no right pane
    /// split: standard two-column layout on wide terminals
    /// analyst: enables ultra-wide 3-column layout when available
    #[serde(default, alias = "workspace_layout")]
    pub layout: WorkspaceLayout,
    /// FRED API key for fetching economic indicators.
    /// Register at: https://fred.stlouisfed.org/docs/api/api_key.html
    #[serde(default)]
    pub fred_api_key: Option<String>,
    /// Brave Search API key for enhanced news/research/economic data.
    /// Register at: https://brave.com/search/api/
    #[serde(default)]
    pub brave_api_key: Option<String>,
    /// RSS news feed polling interval in seconds (default: 600 = 10 minutes)
    #[serde(default = "default_news_poll_interval")]
    pub news_poll_interval: u64,
    /// Custom RSS feeds (name, url, category). If empty, uses default feeds.
    #[serde(default)]
    pub custom_news_feeds: Vec<CustomNewsFeed>,
    /// Brave news search query presets used during refresh.
    #[serde(default = "default_brave_news_queries")]
    pub brave_news_queries: Vec<String>,
    /// SMA periods to overlay on price charts (default: [20, 50])
    #[serde(default = "default_chart_sma")]
    pub chart_sma: Vec<usize>,
    /// Watchlist table customization.
    #[serde(default)]
    pub watchlist: WatchlistConfig,
    /// User-configurable global keybindings.
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
}

fn default_brave_news_queries() -> Vec<String> {
    vec![
        "stock market today".to_string(),
        "federal reserve interest rates monetary policy".to_string(),
        "bitcoin cryptocurrency regulation".to_string(),
        "gold silver precious metals price".to_string(),
        "oil OPEC energy crude".to_string(),
        "geopolitics international trade war sanctions".to_string(),
    ]
}

fn default_chart_sma() -> Vec<usize> {
    vec![20, 50]
}

fn default_watchlist_columns() -> Vec<WatchlistColumn> {
    vec![
        WatchlistColumn::Symbol,
        WatchlistColumn::Name,
        WatchlistColumn::Category,
        WatchlistColumn::Price,
        WatchlistColumn::ChangePct,
        WatchlistColumn::Rsi,
        WatchlistColumn::Sma50,
        WatchlistColumn::Target,
        WatchlistColumn::Prox,
    ]
}

fn default_base_currency() -> String {
    "USD".to_string()
}

fn default_refresh_interval() -> u64 {
    60
}

fn default_auto_refresh() -> bool {
    true
}

fn default_refresh_interval_secs() -> u64 {
    300
}

fn default_theme() -> String {
    "midnight".to_string()
}

fn default_home_tab() -> String {
    "positions".to_string()
}

fn default_news_poll_interval() -> u64 {
    600 // 10 minutes
}

fn default_key_quit() -> String {
    "q".to_string()
}

fn default_key_help() -> String {
    "?".to_string()
}

fn default_key_command_palette() -> String {
    ":".to_string()
}

fn default_key_refresh() -> String {
    "r".to_string()
}

fn default_key_search() -> String {
    "/".to_string()
}

fn default_key_theme_cycle() -> String {
    "t".to_string()
}

fn default_key_privacy_toggle() -> String {
    "p".to_string()
}

fn default_postgres_max_connections() -> u32 {
    5
}

fn default_postgres_connect_timeout_secs() -> u64 {
    10
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
            database_backend: DatabaseBackend::default(),
            database_url: None,
            postgres_max_connections: default_postgres_max_connections(),
            postgres_connect_timeout_secs: default_postgres_connect_timeout_secs(),
            base_currency: default_base_currency(),
            refresh_interval: default_refresh_interval(),
            auto_refresh: default_auto_refresh(),
            refresh_interval_secs: default_refresh_interval_secs(),
            portfolio_mode: PortfolioMode::default(),
            theme: default_theme(),
            home_tab: default_home_tab(),
            layout: WorkspaceLayout::default(),
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: default_news_poll_interval(),
            custom_news_feeds: Vec::new(),
            brave_news_queries: default_brave_news_queries(),
            chart_sma: default_chart_sma(),
            watchlist: WatchlistConfig::default(),
            keybindings: KeybindingsConfig::default(),
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

/// Load config, prompting first-run users for home tab preference.
///
/// Prompt appears only when `config.toml` does not exist yet.
pub fn load_config_with_first_run_prompt() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        return load_config();
    }

    let config = Config {
            home_tab: prompt_first_run_home_tab()?,
            layout: WorkspaceLayout::default(),
            brave_api_key: prompt_optional_brave_api_key()?,
            ..Config::default()
        };
    save_config(&config)?;
    Ok(config)
}

fn prompt_first_run_home_tab() -> Result<String> {
    println!();
    println!("  First launch setup");
    println!("  Default homepage: [P]ortfolio or [W]atchlist?");

    loop {
        print!("  Choose [P/w] (default: P): ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if let Some(home_tab) = parse_home_tab_input(&input) {
            return Ok(home_tab.to_string());
        }
        println!("  Please enter P or W.");
    }
}

fn parse_home_tab_input(input: &str) -> Option<&'static str> {
    match input.trim().to_lowercase().as_str() {
        "" | "p" | "portfolio" => Some("positions"),
        "w" | "watchlist" => Some("watchlist"),
        _ => None,
    }
}

fn prompt_optional_brave_api_key() -> Result<Option<String>> {
    println!();
    println!("  Optional: Brave Search API key");
    println!("  For richer news, economic data, and market intelligence, add a Brave Search API key (free tier: $5/month credits).");
    println!("  Get one at https://brave.com/search/api/");
    print!("  Enter key (or press Enter to skip): ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let key = input.trim().to_string();
    if key.is_empty() {
        Ok(None)
    } else {
        Ok(Some(key))
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
        assert_eq!(config.database_backend, DatabaseBackend::Sqlite);
        assert_eq!(config.database_url, None);
        assert_eq!(config.postgres_max_connections, 5);
        assert_eq!(config.postgres_connect_timeout_secs, 10);
        assert_eq!(config.base_currency, "USD");
        assert_eq!(config.refresh_interval, 60);
        assert!(config.auto_refresh);
        assert_eq!(config.refresh_interval_secs, 300);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
        assert_eq!(config.home_tab, "positions");
        assert_eq!(config.layout, WorkspaceLayout::Split);
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
            database_backend: DatabaseBackend::Postgres,
            database_url: Some("postgres://localhost:5432/pftui".to_string()),
            postgres_max_connections: 12,
            postgres_connect_timeout_secs: 20,
            base_currency: "EUR".to_string(),
            refresh_interval: 30,
            auto_refresh: false,
            refresh_interval_secs: 120,
            portfolio_mode: PortfolioMode::Percentage,
            theme: "nord".to_string(),
            home_tab: "watchlist".to_string(),
            layout: WorkspaceLayout::Analyst,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: default_brave_news_queries(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
            keybindings: crate::config::KeybindingsConfig::default(),
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.database_backend, DatabaseBackend::Postgres);
        assert_eq!(
            loaded.database_url,
            Some("postgres://localhost:5432/pftui".to_string())
        );
        assert_eq!(loaded.postgres_max_connections, 12);
        assert_eq!(loaded.postgres_connect_timeout_secs, 20);
        assert_eq!(loaded.base_currency, "EUR");
        assert_eq!(loaded.refresh_interval, 30);
        assert!(!loaded.auto_refresh);
        assert_eq!(loaded.refresh_interval_secs, 120);
        assert_eq!(loaded.portfolio_mode, PortfolioMode::Percentage);
        assert_eq!(loaded.theme, "nord");
        assert_eq!(loaded.home_tab, "watchlist");
        assert_eq!(loaded.layout, WorkspaceLayout::Analyst);
    }

    #[test]
    fn config_deserialize_missing_fields_uses_defaults() {
        let toml_str = r#"base_currency = "GBP""#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.database_backend, DatabaseBackend::Sqlite);
        assert_eq!(config.database_url, None);
        assert_eq!(config.postgres_max_connections, 5);
        assert_eq!(config.postgres_connect_timeout_secs, 10);
        assert_eq!(config.base_currency, "GBP");
        assert_eq!(config.refresh_interval, 60);
        assert!(config.auto_refresh);
        assert_eq!(config.refresh_interval_secs, 300);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
        assert_eq!(config.home_tab, "positions");
        assert_eq!(config.layout, WorkspaceLayout::Split);
        assert_eq!(config.keybindings.quit, "q");
        assert_eq!(config.keybindings.search, "/");
    }

    #[test]
    fn config_deserialize_empty_uses_all_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.database_backend, DatabaseBackend::Sqlite);
        assert_eq!(config.database_url, None);
        assert_eq!(config.postgres_max_connections, 5);
        assert_eq!(config.postgres_connect_timeout_secs, 10);
        assert_eq!(config.base_currency, "USD");
        assert_eq!(config.refresh_interval, 60);
        assert!(config.auto_refresh);
        assert_eq!(config.refresh_interval_secs, 300);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
        assert_eq!(config.home_tab, "positions");
        assert_eq!(config.layout, WorkspaceLayout::Split);
        assert_eq!(config.keybindings.help, "?");
    }

    #[test]
    fn keybindings_deserialize_custom_values() {
        let toml_str = r#"
[keybindings]
quit = "x"
help = "h"
command_palette = ";"
refresh = "R"
search = "s"
theme_cycle = "T"
privacy_toggle = "P"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.keybindings.quit, "x");
        assert_eq!(config.keybindings.command_palette, ";");
        assert_eq!(config.keybindings.privacy_toggle, "P");
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
    fn workspace_layout_serialization() {
        assert_eq!(
            serde_json::to_string(&WorkspaceLayout::Compact).unwrap(),
            "\"compact\""
        );
        assert_eq!(
            serde_json::to_string(&WorkspaceLayout::Split).unwrap(),
            "\"split\""
        );
        assert_eq!(
            serde_json::to_string(&WorkspaceLayout::Analyst).unwrap(),
            "\"analyst\""
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

    #[test]
    fn parse_home_tab_input_accepts_portfolio_variants() {
        assert_eq!(parse_home_tab_input(""), Some("positions"));
        assert_eq!(parse_home_tab_input("p"), Some("positions"));
        assert_eq!(parse_home_tab_input("Portfolio"), Some("positions"));
    }

    #[test]
    fn parse_home_tab_input_accepts_watchlist_variants() {
        assert_eq!(parse_home_tab_input("w"), Some("watchlist"));
        assert_eq!(parse_home_tab_input("Watchlist"), Some("watchlist"));
    }

    #[test]
    fn parse_home_tab_input_rejects_invalid() {
        assert_eq!(parse_home_tab_input("x"), None);
    }
}
