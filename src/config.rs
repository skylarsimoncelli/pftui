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
    Split,
    #[default]
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
pub struct MobileServerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_mobile_bind")]
    pub bind: String,
    #[serde(default = "default_mobile_port")]
    pub port: u16,
    #[serde(default)]
    pub api_tokens: Vec<MobileApiToken>,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default = "default_mobile_session_ttl_hours")]
    pub session_ttl_hours: u64,
}

impl Default for MobileServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_mobile_bind(),
            port: default_mobile_port(),
            api_tokens: Vec::new(),
            cert_path: None,
            key_path: None,
            session_ttl_hours: default_mobile_session_ttl_hours(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonCadenceConfig {
    #[serde(default = "default_daemon_prices_interval_secs")]
    pub prices_interval_secs: u64,
    #[serde(default = "default_daemon_news_interval_secs")]
    pub news_interval_secs: u64,
    #[serde(default = "default_daemon_brave_news_interval_secs")]
    pub brave_news_interval_secs: u64,
    #[serde(default = "default_daemon_predictions_interval_secs")]
    pub predictions_interval_secs: u64,
    #[serde(default = "default_daemon_sentiment_interval_secs")]
    pub sentiment_interval_secs: u64,
    #[serde(default = "default_daemon_calendar_interval_secs")]
    pub calendar_interval_secs: u64,
    #[serde(default = "default_daemon_economy_interval_secs")]
    pub economy_interval_secs: u64,
    #[serde(default = "default_daemon_cot_interval_secs")]
    pub cot_interval_secs: u64,
    #[serde(default = "default_daemon_bls_interval_secs")]
    pub bls_interval_secs: u64,
    #[serde(default = "default_daemon_fred_interval_secs")]
    pub fred_interval_secs: u64,
    #[serde(default = "default_daemon_fedwatch_interval_secs")]
    pub fedwatch_interval_secs: u64,
    #[serde(default = "default_daemon_worldbank_interval_secs")]
    pub worldbank_interval_secs: u64,
    #[serde(default = "default_daemon_comex_interval_secs")]
    pub comex_interval_secs: u64,
    #[serde(default = "default_daemon_onchain_interval_secs")]
    pub onchain_interval_secs: u64,
    #[serde(default = "default_daemon_analytics_interval_secs")]
    pub analytics_interval_secs: u64,
    #[serde(default = "default_daemon_alerts_interval_secs")]
    pub alerts_interval_secs: u64,
    #[serde(default = "default_daemon_cleanup_interval_secs")]
    pub cleanup_interval_secs: u64,
}

impl Default for DaemonCadenceConfig {
    fn default() -> Self {
        Self {
            prices_interval_secs: default_daemon_prices_interval_secs(),
            news_interval_secs: default_daemon_news_interval_secs(),
            brave_news_interval_secs: default_daemon_brave_news_interval_secs(),
            predictions_interval_secs: default_daemon_predictions_interval_secs(),
            sentiment_interval_secs: default_daemon_sentiment_interval_secs(),
            calendar_interval_secs: default_daemon_calendar_interval_secs(),
            economy_interval_secs: default_daemon_economy_interval_secs(),
            cot_interval_secs: default_daemon_cot_interval_secs(),
            bls_interval_secs: default_daemon_bls_interval_secs(),
            fred_interval_secs: default_daemon_fred_interval_secs(),
            fedwatch_interval_secs: default_daemon_fedwatch_interval_secs(),
            worldbank_interval_secs: default_daemon_worldbank_interval_secs(),
            comex_interval_secs: default_daemon_comex_interval_secs(),
            onchain_interval_secs: default_daemon_onchain_interval_secs(),
            analytics_interval_secs: default_daemon_analytics_interval_secs(),
            alerts_interval_secs: default_daemon_alerts_interval_secs(),
            cleanup_interval_secs: default_daemon_cleanup_interval_secs(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default)]
    pub cadence: DaemonCadenceConfig,
}

/// The tracked universe: groups of symbols that get refreshed, priced,
/// and analysed alongside portfolio holdings and watchlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedUniverse {
    #[serde(default = "default_universe_indices")]
    pub indices: Vec<String>,
    #[serde(default = "default_universe_sectors")]
    pub sectors: Vec<String>,
    #[serde(default = "default_universe_commodities")]
    pub commodities: Vec<String>,
    #[serde(default = "default_universe_fx")]
    pub fx: Vec<String>,
    #[serde(default = "default_universe_rates")]
    pub rates: Vec<String>,
    #[serde(default = "default_universe_crypto_majors")]
    pub crypto_majors: Vec<String>,
    #[serde(default)]
    pub custom: Vec<String>,
}

impl Default for TrackedUniverse {
    fn default() -> Self {
        Self {
            indices: default_universe_indices(),
            sectors: default_universe_sectors(),
            commodities: default_universe_commodities(),
            fx: default_universe_fx(),
            rates: default_universe_rates(),
            crypto_majors: default_universe_crypto_majors(),
            custom: Vec::new(),
        }
    }
}

impl TrackedUniverse {
    /// Return all symbols across all groups (deduplicated, order preserved).
    pub fn all_symbols(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for sym in self
            .indices
            .iter()
            .chain(&self.sectors)
            .chain(&self.commodities)
            .chain(&self.fx)
            .chain(&self.rates)
            .chain(&self.crypto_majors)
            .chain(&self.custom)
        {
            if seen.insert(sym.clone()) {
                out.push(sym.clone());
            }
        }
        out
    }

    /// Return all group names.
    pub fn group_names() -> &'static [&'static str] {
        &[
            "indices",
            "sectors",
            "commodities",
            "fx",
            "rates",
            "crypto_majors",
            "custom",
        ]
    }

    /// Get symbols for a named group.
    pub fn group(&self, name: &str) -> Option<&Vec<String>> {
        match name {
            "indices" => Some(&self.indices),
            "sectors" => Some(&self.sectors),
            "commodities" => Some(&self.commodities),
            "fx" => Some(&self.fx),
            "rates" => Some(&self.rates),
            "crypto_majors" => Some(&self.crypto_majors),
            "custom" => Some(&self.custom),
            _ => None,
        }
    }

    /// Get mutable symbols for a named group.
    pub fn group_mut(&mut self, name: &str) -> Option<&mut Vec<String>> {
        match name {
            "indices" => Some(&mut self.indices),
            "sectors" => Some(&mut self.sectors),
            "commodities" => Some(&mut self.commodities),
            "fx" => Some(&mut self.fx),
            "rates" => Some(&mut self.rates),
            "crypto_majors" => Some(&mut self.crypto_majors),
            "custom" => Some(&mut self.custom),
            _ => None,
        }
    }
}

fn default_universe_indices() -> Vec<String> {
    vec![
        "SPY".into(),
        "QQQ".into(),
        "DIA".into(),
        "IWM".into(),
    ]
}

fn default_universe_sectors() -> Vec<String> {
    vec![
        "XLE".into(),
        "XLF".into(),
        "XLK".into(),
        "XLV".into(),
        "XLY".into(),
        "XLP".into(),
        "XLI".into(),
        "XLU".into(),
        "XLB".into(),
        "XLRE".into(),
        "XLC".into(),
    ]
}

fn default_universe_commodities() -> Vec<String> {
    vec![
        "GC=F".into(),
        "SI=F".into(),
        "CL=F".into(),
        "HG=F".into(),
        "URA".into(),
    ]
}

fn default_universe_fx() -> Vec<String> {
    vec![
        "DX-Y.NYB".into(),
        "EURUSD=X".into(),
        "GBPUSD=X".into(),
        "USDJPY=X".into(),
    ]
}

fn default_universe_rates() -> Vec<String> {
    vec!["^TNX".into(), "^TYX".into()]
}

fn default_universe_crypto_majors() -> Vec<String> {
    vec!["BTC-USD".into(), "ETH-USD".into()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MobileTokenPermission {
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobileApiToken {
    pub name: String,
    pub prefix: String,
    pub token_hash: String,
    pub permission: MobileTokenPermission,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Database backend selector.
    #[serde(default)]
    pub database_backend: DatabaseBackend,
    /// Database connection URL used when `database_backend = "postgres"`.
    #[serde(default)]
    pub database_url: Option<String>,
    /// Optional remote Postgres source used to mirror into a local SQLite database.
    #[serde(default)]
    pub mirror_source_url: Option<String>,
    /// When true, PostgreSQL sessions are opened in read-only mode and startup skips migrations.
    #[serde(default)]
    pub postgres_read_only: bool,
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
    /// EIA API key for oil inventory and SPR data.
    /// Register (free) at: https://www.eia.gov/opendata/register.php
    #[serde(default)]
    pub eia_api_key: Option<String>,
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
    /// Native mobile API server configuration.
    #[serde(default)]
    pub mobile: MobileServerConfig,
    /// Default minimum cooldown (in minutes) for recurring alerts when no per-alert
    /// cooldown is set. Prevents flapping when conditions toggle rapidly.
    /// Set to 0 to disable the default cooldown floor. Default: 30 minutes.
    #[serde(default = "default_alert_cooldown_minutes")]
    pub alert_default_cooldown_minutes: i64,
    /// Background daemon cadence controls for per-source scheduling.
    #[serde(default)]
    pub daemon: DaemonConfig,
    /// Broker API credentials for each supported integration.
    #[serde(default)]
    pub brokers: BrokerCredentials,
    /// Tracked universe: groups of symbols refreshed and analysed
    /// alongside portfolio holdings and watchlist.
    #[serde(default)]
    pub tracked_universe: TrackedUniverse,
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

fn default_alert_cooldown_minutes() -> i64 {
    30
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

fn default_mobile_bind() -> String {
    "127.0.0.1".to_string()
}

fn default_mobile_port() -> u16 {
    9443
}

fn default_mobile_session_ttl_hours() -> u64 {
    12
}

fn default_daemon_prices_interval_secs() -> u64 {
    300
}

fn default_daemon_news_interval_secs() -> u64 {
    600
}

fn default_daemon_brave_news_interval_secs() -> u64 {
    4 * 60 * 60
}

fn default_daemon_predictions_interval_secs() -> u64 {
    60 * 60
}

fn default_daemon_sentiment_interval_secs() -> u64 {
    60 * 60
}

fn default_daemon_calendar_interval_secs() -> u64 {
    24 * 60 * 60
}

fn default_daemon_economy_interval_secs() -> u64 {
    6 * 60 * 60
}

fn default_daemon_cot_interval_secs() -> u64 {
    7 * 24 * 60 * 60
}

fn default_daemon_bls_interval_secs() -> u64 {
    30 * 24 * 60 * 60
}

fn default_daemon_fred_interval_secs() -> u64 {
    24 * 60 * 60
}

fn default_daemon_fedwatch_interval_secs() -> u64 {
    60 * 60
}

fn default_daemon_worldbank_interval_secs() -> u64 {
    30 * 24 * 60 * 60
}

fn default_daemon_comex_interval_secs() -> u64 {
    24 * 60 * 60
}

fn default_daemon_onchain_interval_secs() -> u64 {
    24 * 60 * 60
}

fn default_daemon_analytics_interval_secs() -> u64 {
    300
}

fn default_daemon_alerts_interval_secs() -> u64 {
    60
}

fn default_daemon_cleanup_interval_secs() -> u64 {
    24 * 60 * 60
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrokerCredentials {
    #[serde(default)]
    pub trading212_api_key: Option<String>,
    #[serde(default)]
    pub ibkr_account_id: Option<String>,
    #[serde(default)]
    pub binance_api_key: Option<String>,
    #[serde(default)]
    pub binance_secret_key: Option<String>,
    #[serde(default)]
    pub kraken_api_key: Option<String>,
    #[serde(default)]
    pub kraken_private_key: Option<String>,
    #[serde(default)]
    pub coinbase_api_key: Option<String>,
    #[serde(default)]
    pub coinbase_api_secret: Option<String>,
    #[serde(default)]
    pub crypto_com_api_key: Option<String>,
    #[serde(default)]
    pub crypto_com_secret_key: Option<String>,
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
            mirror_source_url: None,
            postgres_read_only: false,
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
            eia_api_key: None,
            news_poll_interval: default_news_poll_interval(),
            custom_news_feeds: Vec::new(),
            brave_news_queries: default_brave_news_queries(),
            chart_sma: default_chart_sma(),
            watchlist: WatchlistConfig::default(),
            keybindings: KeybindingsConfig::default(),
            mobile: MobileServerConfig::default(),
            alert_default_cooldown_minutes: default_alert_cooldown_minutes(),
            daemon: DaemonConfig::default(),
            brokers: BrokerCredentials::default(),
            tracked_universe: TrackedUniverse::default(),
        }
    }
}

impl Config {
    pub fn is_percentage_mode(&self) -> bool {
        self.portfolio_mode == PortfolioMode::Percentage
    }

    fn postgres_host(&self) -> Option<String> {
        let url = self.database_url.as_deref()?.trim();
        let (_, rest) = url.split_once("://")?;
        let authority = rest.split('/').next().unwrap_or(rest);
        let host_port = authority.rsplit('@').next().unwrap_or(authority);

        if let Some(stripped) = host_port.strip_prefix('[') {
            let end = stripped.find(']')?;
            return Some(stripped[..end].to_string());
        }

        Some(host_port.split(':').next().unwrap_or(host_port).to_string())
    }

    pub fn uses_remote_postgres_profile(&self) -> bool {
        if self.database_backend != DatabaseBackend::Postgres {
            return false;
        }

        match self.postgres_host() {
            Some(host) => {
                let normalized = host.trim().to_ascii_lowercase();
                if normalized == "localhost" {
                    return false;
                }
                normalized
                    .parse::<std::net::IpAddr>()
                    .map(|ip| !ip.is_loopback())
                    .unwrap_or(true)
            }
            None => false,
        }
    }

    pub fn is_remote_postgres_read_only(&self) -> bool {
        self.database_backend == DatabaseBackend::Postgres
            && (self.postgres_read_only || self.uses_remote_postgres_profile())
    }

    pub fn effective_postgres_read_only(&self) -> bool {
        self.is_remote_postgres_read_only()
    }

    pub fn effective_postgres_max_connections(&self) -> u32 {
        if self.is_remote_postgres_read_only() {
            1
        } else {
            self.postgres_max_connections.max(1)
        }
    }

    pub fn effective_postgres_connect_timeout_secs(&self) -> u64 {
        if self.is_remote_postgres_read_only() {
            self.postgres_connect_timeout_secs.max(30)
        } else {
            self.postgres_connect_timeout_secs.max(1)
        }
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
        assert_eq!(config.mirror_source_url, None);
        assert!(!config.postgres_read_only);
        assert_eq!(config.postgres_max_connections, 5);
        assert_eq!(config.postgres_connect_timeout_secs, 10);
        assert_eq!(config.base_currency, "USD");
        assert_eq!(config.refresh_interval, 60);
        assert!(config.auto_refresh);
        assert_eq!(config.refresh_interval_secs, 300);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
        assert_eq!(config.home_tab, "positions");
        assert_eq!(config.layout, WorkspaceLayout::Analyst);
        assert!(!config.mobile.enabled);
        assert_eq!(config.mobile.bind, "127.0.0.1");
        assert_eq!(config.mobile.port, 9443);
        assert!(config.mobile.api_tokens.is_empty());
    }

    #[test]
    fn is_percentage_mode_full() {
        let config = Config::default();
        assert!(!config.is_percentage_mode());
    }

    #[test]
    fn is_percentage_mode_percentage() {
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Default::default()
        };
        assert!(config.is_percentage_mode());
    }

    #[test]
    fn config_roundtrip_toml() {
        let config = Config {
            database_backend: DatabaseBackend::Postgres,
            database_url: Some("postgres://localhost:5432/pftui".to_string()),
            mirror_source_url: Some("postgres://mirror.example/pftui".to_string()),
            postgres_read_only: true,
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
            eia_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: default_brave_news_queries(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
            keybindings: crate::config::KeybindingsConfig::default(),
            mobile: crate::config::MobileServerConfig {
                enabled: true,
                bind: "0.0.0.0".to_string(),
                port: 10443,
                api_tokens: vec![crate::config::MobileApiToken {
                    name: "iphone".to_string(),
                    prefix: "pftm_read_1234".to_string(),
                    token_hash: "hash".to_string(),
                    permission: crate::config::MobileTokenPermission::Read,
                    created_at: "2026-03-16T00:00:00Z".to_string(),
                }],
                cert_path: Some("/tmp/cert.pem".to_string()),
                key_path: Some("/tmp/key.pem".to_string()),
                session_ttl_hours: 24,
            },
            alert_default_cooldown_minutes: 45,
            daemon: DaemonConfig::default(),
            brokers: BrokerCredentials::default(),
            tracked_universe: TrackedUniverse::default(),
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.database_backend, DatabaseBackend::Postgres);
        assert_eq!(
            loaded.database_url,
            Some("postgres://localhost:5432/pftui".to_string())
        );
        assert_eq!(
            loaded.mirror_source_url,
            Some("postgres://mirror.example/pftui".to_string())
        );
        assert!(loaded.postgres_read_only);
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
        assert!(loaded.mobile.enabled);
        assert_eq!(loaded.mobile.bind, "0.0.0.0");
        assert_eq!(loaded.mobile.port, 10443);
        assert_eq!(loaded.mobile.session_ttl_hours, 24);
        assert_eq!(loaded.mobile.api_tokens.len(), 1);
        assert_eq!(loaded.alert_default_cooldown_minutes, 45);
    }

    #[test]
    fn config_deserialize_missing_fields_uses_defaults() {
        let toml_str = r#"base_currency = "GBP""#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.database_backend, DatabaseBackend::Sqlite);
        assert_eq!(config.database_url, None);
        assert_eq!(config.mirror_source_url, None);
        assert!(!config.postgres_read_only);
        assert_eq!(config.postgres_max_connections, 5);
        assert_eq!(config.postgres_connect_timeout_secs, 10);
        assert_eq!(config.base_currency, "GBP");
        assert_eq!(config.refresh_interval, 60);
        assert!(config.auto_refresh);
        assert_eq!(config.refresh_interval_secs, 300);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
        assert_eq!(config.home_tab, "positions");
        assert_eq!(config.layout, WorkspaceLayout::Analyst);
        assert_eq!(config.keybindings.quit, "q");
        assert_eq!(config.keybindings.search, "/");
        assert!(!config.mobile.enabled);
        assert_eq!(config.mobile.bind, "127.0.0.1");
        assert_eq!(config.mobile.port, 9443);
        assert!(config.mobile.api_tokens.is_empty());
        assert_eq!(config.alert_default_cooldown_minutes, 30);
    }

    #[test]
    fn config_deserialize_empty_uses_all_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.database_backend, DatabaseBackend::Sqlite);
        assert_eq!(config.database_url, None);
        assert_eq!(config.mirror_source_url, None);
        assert!(!config.postgres_read_only);
        assert_eq!(config.postgres_max_connections, 5);
        assert_eq!(config.postgres_connect_timeout_secs, 10);
        assert_eq!(config.base_currency, "USD");
        assert_eq!(config.refresh_interval, 60);
        assert!(config.auto_refresh);
        assert_eq!(config.refresh_interval_secs, 300);
        assert_eq!(config.portfolio_mode, PortfolioMode::Full);
        assert_eq!(config.theme, "midnight");
        assert_eq!(config.home_tab, "positions");
        assert_eq!(config.layout, WorkspaceLayout::Analyst);
        assert_eq!(config.keybindings.help, "?");
        assert!(!config.mobile.enabled);
        assert_eq!(config.mobile.bind, "127.0.0.1");
        assert!(config.mobile.api_tokens.is_empty());
        assert_eq!(config.alert_default_cooldown_minutes, 30);
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
        let eur_config = Config {
            base_currency: "EUR".to_string(),
            ..Default::default()
        };
        assert_eq!(eur_config.currency_symbol(), "€");
    }

    #[test]
    fn remote_postgres_read_only_uses_remote_runtime_profile() {
        let config = Config {
            database_backend: DatabaseBackend::Postgres,
            database_url: Some("postgres://example".to_string()),
            postgres_read_only: true,
            postgres_max_connections: 9,
            postgres_connect_timeout_secs: 5,
            ..Default::default()
        };

        assert!(config.is_remote_postgres_read_only());
        assert_eq!(config.effective_postgres_max_connections(), 1);
        assert_eq!(config.effective_postgres_connect_timeout_secs(), 30);
    }

    #[test]
    fn local_postgres_keeps_configured_runtime_profile() {
        let config = Config {
            database_backend: DatabaseBackend::Postgres,
            database_url: Some("postgres://localhost/pftui".to_string()),
            postgres_read_only: false,
            postgres_max_connections: 9,
            postgres_connect_timeout_secs: 5,
            ..Default::default()
        };

        assert!(!config.is_remote_postgres_read_only());
        assert_eq!(config.effective_postgres_max_connections(), 9);
        assert_eq!(config.effective_postgres_connect_timeout_secs(), 5);
    }

    #[test]
    fn remote_postgres_url_implies_remote_profile() {
        let config = Config {
            database_backend: DatabaseBackend::Postgres,
            database_url: Some("postgres://user:pass@37.27.248.245:50498/pftui".to_string()),
            postgres_read_only: false,
            ..Default::default()
        };

        assert!(config.uses_remote_postgres_profile());
        assert!(config.effective_postgres_read_only());
        assert_eq!(config.effective_postgres_max_connections(), 1);
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
