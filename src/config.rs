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
}
