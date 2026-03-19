pub mod binance;
pub mod coinbase;
pub mod crypto_com;
pub mod ibkr;
pub mod kraken;
pub mod trading212;

use anyhow::Result;
use clap::ValueEnum;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrokerKind {
    Trading212,
    Ibkr,
    Binance,
    Kraken,
    Coinbase,
    #[value(name = "crypto-com")]
    #[serde(rename = "crypto-com")]
    CryptoCom,
}

impl std::fmt::Display for BrokerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrokerKind::Trading212 => write!(f, "trading212"),
            BrokerKind::Ibkr => write!(f, "ibkr"),
            BrokerKind::Binance => write!(f, "binance"),
            BrokerKind::Kraken => write!(f, "kraken"),
            BrokerKind::Coinbase => write!(f, "coinbase"),
            BrokerKind::CryptoCom => write!(f, "crypto-com"),
        }
    }
}

impl std::str::FromStr for BrokerKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "trading212" => Ok(BrokerKind::Trading212),
            "ibkr" => Ok(BrokerKind::Ibkr),
            "binance" => Ok(BrokerKind::Binance),
            "kraken" => Ok(BrokerKind::Kraken),
            "coinbase" => Ok(BrokerKind::Coinbase),
            "crypto-com" | "cryptocom" | "crypto.com" => Ok(BrokerKind::CryptoCom),
            _ => anyhow::bail!(
                "Unknown broker: {s}. Supported: trading212, ibkr, binance, kraken, coinbase, crypto-com"
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerPosition {
    pub symbol: String,
    pub quantity: Decimal,
    pub avg_cost: Decimal,
    pub currency: String,
    pub category: String,
}

#[allow(dead_code)]
pub trait BrokerProvider: Send + Sync {
    fn kind(&self) -> BrokerKind;
    fn is_available(&self) -> Result<()>;
    fn fetch_positions(&self) -> Result<Vec<BrokerPosition>>;
}

pub fn broker_tag(kind: BrokerKind) -> String {
    format!("[broker:{}]", kind)
}

pub fn create_provider(
    kind: BrokerKind,
    config: &crate::config::Config,
) -> Result<Box<dyn BrokerProvider>> {
    match kind {
        BrokerKind::Trading212 => {
            let key = config
                .brokers
                .trading212_api_key
                .as_deref()
                .ok_or_else(|| {
                    anyhow::anyhow!("No Trading212 API key configured. Run: pftui portfolio broker add trading212 --api-key YOUR_KEY")
                })?;
            Ok(Box::new(trading212::Trading212Provider::new(key)))
        }
        BrokerKind::Ibkr => {
            let account_id = config.brokers.ibkr_account_id.clone();
            Ok(Box::new(ibkr::IbkrProvider::new(account_id)))
        }
        BrokerKind::Binance => {
            let api_key = config.brokers.binance_api_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!("No Binance API key configured. Run: pftui portfolio broker add binance --api-key YOUR_KEY --secret YOUR_SECRET")
            })?;
            let secret = config.brokers.binance_secret_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!("No Binance secret key configured. Run: pftui portfolio broker add binance --api-key YOUR_KEY --secret YOUR_SECRET")
            })?;
            Ok(Box::new(binance::BinanceProvider::new(api_key, secret)))
        }
        BrokerKind::Kraken => {
            let api_key = config.brokers.kraken_api_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!("No Kraken API key configured. Run: pftui portfolio broker add kraken --api-key YOUR_KEY --secret YOUR_PRIVATE_KEY")
            })?;
            let private_key =
                config.brokers.kraken_private_key.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("No Kraken private key configured. Run: pftui portfolio broker add kraken --api-key YOUR_KEY --secret YOUR_PRIVATE_KEY")
                })?;
            Ok(Box::new(kraken::KrakenProvider::new(api_key, private_key)))
        }
        BrokerKind::Coinbase => {
            let api_key = config.brokers.coinbase_api_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!("No Coinbase API key configured. Run: pftui portfolio broker add coinbase --api-key YOUR_KEY --secret YOUR_SECRET")
            })?;
            let api_secret =
                config.brokers.coinbase_api_secret.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("No Coinbase API secret configured. Run: pftui portfolio broker add coinbase --api-key YOUR_KEY --secret YOUR_SECRET")
                })?;
            Ok(Box::new(coinbase::CoinbaseProvider::new(
                api_key, api_secret,
            )))
        }
        BrokerKind::CryptoCom => {
            let api_key =
                config.brokers.crypto_com_api_key.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("No Crypto.com API key configured. Run: pftui portfolio broker add crypto-com --api-key YOUR_KEY --secret YOUR_SECRET")
                })?;
            let secret_key =
                config.brokers.crypto_com_secret_key.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("No Crypto.com secret key configured. Run: pftui portfolio broker add crypto-com --api-key YOUR_KEY --secret YOUR_SECRET")
                })?;
            Ok(Box::new(crypto_com::CryptoComProvider::new(
                api_key, secret_key,
            )))
        }
    }
}
