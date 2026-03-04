pub mod coingecko;
pub mod yahoo;

use std::sync::mpsc;
use std::time::Duration;
use crate::config::Config;
use crate::models::asset::AssetCategory;
use crate::models::price::{HistoryRecord, PriceQuote};

/// Delay between sequential Yahoo Finance API requests to avoid rate limiting.
const YAHOO_RATE_LIMIT_DELAY: Duration = Duration::from_millis(100);

/// Delay between sequential CoinGecko API requests to avoid rate limiting.
/// CoinGecko free tier is more aggressive with throttling, so use a longer delay.
const COINGECKO_RATE_LIMIT_DELAY: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub enum PriceCommand {
    FetchAll(Vec<(String, AssetCategory)>),
    FetchHistory(String, AssetCategory, u32),
    FetchHistoryBatch(Vec<(String, AssetCategory, u32)>),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum PriceUpdate {
    Quote(PriceQuote),
    History(String, Vec<HistoryRecord>),
    #[allow(dead_code)]
    Error(String),
    FetchComplete,
}


/// Format a crypto symbol for Yahoo Finance (append -USD if not already present)
fn yahoo_crypto_symbol(symbol: &str) -> String {
    let upper = symbol.to_uppercase();
    if upper.ends_with("-USD") {
        upper
    } else {
        format!("{}-USD", upper)
    }
}

/// Fetch history for a single symbol (used by both single and batch paths)
async fn fetch_history_single(
    symbol: &str,
    category: AssetCategory,
    days: u32,
) -> (String, Result<Vec<HistoryRecord>, String>) {
    let result = match category {
        AssetCategory::Crypto => {
            // Try CoinGecko first, fall back to Yahoo (BTC-USD format)
            match coingecko::fetch_history(symbol, days).await {
                Ok(records) if !records.is_empty() => Ok(records),
                _ => {
                    let yahoo_sym = yahoo_crypto_symbol(symbol);
                    yahoo::fetch_history(&yahoo_sym, days)
                        .await
                        .map_err(|e| e.to_string())
                }
            }
        }
        AssetCategory::Cash => Ok(Vec::new()),
        _ => yahoo::fetch_history(symbol, days)
            .await
            .map_err(|e| e.to_string()),
    };
    (symbol.to_string(), result)
}

pub struct PriceService {
    cmd_tx: mpsc::Sender<PriceCommand>,
    update_rx: mpsc::Receiver<PriceUpdate>,
    rt_handle: std::thread::JoinHandle<()>,
}

impl PriceService {
    pub fn start(config: Config) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<PriceCommand>();
        let (update_tx, update_rx) = mpsc::channel::<PriceUpdate>();

        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build tokio runtime");

            rt.block_on(async move {
                Self::run_loop(cmd_rx, update_tx, &config).await;
            });
        });

        PriceService {
            cmd_tx,
            update_rx,
            rt_handle: handle,
        }
    }

    async fn run_loop(
        cmd_rx: mpsc::Receiver<PriceCommand>,
        update_tx: mpsc::Sender<PriceUpdate>,
        config: &Config,
    ) {
        loop {
            match cmd_rx.recv() {
                Ok(PriceCommand::FetchAll(symbols)) => {
                    Self::fetch_all(&symbols, &update_tx, config).await;
                    let _ = update_tx.send(PriceUpdate::FetchComplete);
                }
                Ok(PriceCommand::FetchHistory(symbol, category, days)) => {
                    Self::fetch_history(&symbol, category, days, &update_tx).await;
                }
                Ok(PriceCommand::FetchHistoryBatch(batch)) => {
                    Self::fetch_history_batch(batch, &update_tx).await;
                }
                Ok(PriceCommand::Shutdown) | Err(_) => break,
            }
        }
    }

    async fn fetch_all(
        symbols: &[(String, AssetCategory)],
        update_tx: &mpsc::Sender<PriceUpdate>,
        config: &Config,
    ) {
        // Split by provider
        let mut yahoo_symbols = Vec::new();
        let mut crypto_symbols = Vec::new();

        for (sym, cat) in symbols {
            match cat {
                AssetCategory::Cash => {} // cash is always 1:1, no fetch needed
                AssetCategory::Crypto => crypto_symbols.push(sym.clone()),
                _ => yahoo_symbols.push(sym.clone()),
            }
        }

        // Also fetch forex rates if base currency != USD
        if config.base_currency != "USD" {
            let pair = format!("USD{}=X", config.base_currency);
            yahoo_symbols.push(pair);
        }

        // Fetch Yahoo prices with rate limiting (~100ms between requests)
        for (i, sym) in yahoo_symbols.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(YAHOO_RATE_LIMIT_DELAY).await;
            }
            match yahoo::fetch_price(sym).await {
                Ok(quote) => {
                    let _ = update_tx.send(PriceUpdate::Quote(quote));
                }
                Err(e) => {
                    let _ = update_tx.send(PriceUpdate::Error(
                        format!("Yahoo {}: {}", sym, e),
                    ));
                }
            }
        }

        // Fetch CoinGecko prices (batched), fall back to Yahoo per-symbol
        if !crypto_symbols.is_empty() {
            let cg_result = coingecko::fetch_prices(&crypto_symbols).await;
            let mut cg_ok = false;

            match &cg_result {
                Ok(quotes) if !quotes.is_empty() => {
                    // CoinGecko batch succeeded — send all quotes
                    for quote in quotes {
                        let _ = update_tx.send(PriceUpdate::Quote(quote.clone()));
                    }
                    cg_ok = true;
                }
                Ok(_) => {
                    // CoinGecko returned empty response
                    let _ = update_tx.send(PriceUpdate::Error(
                        "CoinGecko returned empty price data, falling back to Yahoo".to_string(),
                    ));
                }
                Err(e) => {
                    // CoinGecko failed — report why
                    let _ = update_tx.send(PriceUpdate::Error(
                        format!("CoinGecko batch failed: {}, falling back to Yahoo", e),
                    ));
                }
            }

            if !cg_ok {
                // Fallback: fetch each crypto via Yahoo (SYM-USD) with rate limiting
                for (i, sym) in crypto_symbols.iter().enumerate() {
                    if i > 0 {
                        tokio::time::sleep(YAHOO_RATE_LIMIT_DELAY).await;
                    }
                    let yahoo_sym = yahoo_crypto_symbol(sym);
                    match yahoo::fetch_price(&yahoo_sym).await {
                        Ok(mut quote) => {
                            quote.symbol = sym.clone(); // map back to original symbol
                            let _ = update_tx.send(PriceUpdate::Quote(quote));
                        }
                        Err(_e) => {
                            let _ = update_tx.send(PriceUpdate::Error(
                                format!("{}: price fetch failed (CoinGecko + Yahoo)", sym),
                            ));
                        }
                    }
                }
            }
        }
    }

    async fn fetch_history(
        symbol: &str,
        category: AssetCategory,
        days: u32,
        update_tx: &mpsc::Sender<PriceUpdate>,
    ) {
        let (sym, result) = fetch_history_single(symbol, category, days).await;
        match result {
            Ok(records) if !records.is_empty() => {
                let _ = update_tx.send(PriceUpdate::History(sym, records));
            }
            Err(e) => {
                let _ = update_tx.send(PriceUpdate::Error(
                    format!("History {}: {}", sym, e),
                ));
            }
            _ => {}
        }
    }

    async fn fetch_history_batch(
        batch: Vec<(String, AssetCategory, u32)>,
        update_tx: &mpsc::Sender<PriceUpdate>,
    ) {
        // Sequential fetching with rate limiting to avoid API throttling.
        // CoinGecko history gets a longer delay than Yahoo.
        for (i, (symbol, category, days)) in batch.iter().enumerate() {
            if i > 0 {
                let delay = match category {
                    AssetCategory::Crypto => COINGECKO_RATE_LIMIT_DELAY,
                    _ => YAHOO_RATE_LIMIT_DELAY,
                };
                tokio::time::sleep(delay).await;
            }
            let (sym, fetch_result) =
                fetch_history_single(symbol, *category, *days).await;
            match fetch_result {
                Ok(records) if !records.is_empty() => {
                    let _ = update_tx.send(PriceUpdate::History(sym, records));
                }
                Err(e) => {
                    let _ = update_tx.send(PriceUpdate::Error(
                        format!("History {}: {}", sym, e),
                    ));
                }
                _ => {}
            }
        }
    }

    pub fn send_command(&self, cmd: PriceCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn try_recv(&self) -> Option<PriceUpdate> {
        self.update_rx.try_recv().ok()
    }

    pub fn shutdown(self) {
        let _ = self.cmd_tx.send(PriceCommand::Shutdown);
        let _ = self.rt_handle.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yahoo_crypto_symbol_appends_usd_suffix() {
        assert_eq!(yahoo_crypto_symbol("BTC"), "BTC-USD");
        assert_eq!(yahoo_crypto_symbol("eth"), "ETH-USD");
        assert_eq!(yahoo_crypto_symbol("Sol"), "SOL-USD");
    }

    #[test]
    fn yahoo_crypto_symbol_no_double_suffix() {
        assert_eq!(yahoo_crypto_symbol("BTC-USD"), "BTC-USD");
        assert_eq!(yahoo_crypto_symbol("btc-usd"), "BTC-USD");
        assert_eq!(yahoo_crypto_symbol("ETH-USD"), "ETH-USD");
    }

    #[test]
    fn fetch_history_batch_command_variant_exists() {
        // Verify the batch command can be constructed
        let batch = vec![
            ("AAPL".to_string(), AssetCategory::Equity, 90),
            ("BTC".to_string(), AssetCategory::Crypto, 90),
            ("GC=F".to_string(), AssetCategory::Commodity, 90),
        ];
        let cmd = PriceCommand::FetchHistoryBatch(batch);
        // Pattern match to verify variant structure
        if let PriceCommand::FetchHistoryBatch(items) = cmd {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0].0, "AAPL");
            assert_eq!(items[1].2, 90);
        } else {
            panic!("Expected FetchHistoryBatch variant");
        }
    }
}
