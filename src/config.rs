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

impl Default for Config {
    fn default() -> Self {
        Config {
            base_currency: default_base_currency(),
            refresh_interval: default_refresh_interval(),
            portfolio_mode: PortfolioMode::default(),
            theme: default_theme(),
        }
    }
}

impl Config {
    pub fn is_percentage_mode(&self) -> bool {
        self.portfolio_mode == PortfolioMode::Percentage
    }
}

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
