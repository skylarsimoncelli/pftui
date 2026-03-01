pub mod coingecko;
pub mod yahoo;

use std::sync::mpsc;
use crate::config::Config;
use crate::models::asset::AssetCategory;
use crate::models::price::{HistoryRecord, PriceQuote};

#[derive(Debug)]
pub enum PriceCommand {
    FetchAll(Vec<(String, AssetCategory)>),
    FetchHistory(String, AssetCategory, u32),
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

        // Fetch Yahoo prices
        for sym in &yahoo_symbols {
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

        // Fetch CoinGecko prices (batched), fall back to Yahoo
        if !crypto_symbols.is_empty() {
            match coingecko::fetch_prices(&crypto_symbols).await {
                Ok(quotes) if !quotes.is_empty() => {
                    for quote in quotes {
                        let _ = update_tx.send(PriceUpdate::Quote(quote));
                    }
                }
                _ => {
                    // Fallback: fetch each crypto via Yahoo (SYM-USD)
                    for sym in &crypto_symbols {
                        let yahoo_sym = format!("{}-USD", sym.to_uppercase());
                        match yahoo::fetch_price(&yahoo_sym).await {
                            Ok(mut quote) => {
                                quote.symbol = sym.clone(); // map back to original symbol
                                let _ = update_tx.send(PriceUpdate::Quote(quote));
                            }
                            Err(e) => {
                                let _ = update_tx.send(PriceUpdate::Error(
                                    format!("Yahoo crypto {}: {}", sym, e),
                                ));
                            }
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
        let result = match category {
            AssetCategory::Crypto => {
                // Try CoinGecko first, fall back to Yahoo (BTC-USD format)
                match coingecko::fetch_history(symbol, days).await {
                    Ok(records) if !records.is_empty() => Ok(records),
                    _ => {
                        let yahoo_sym = format!("{}-USD", symbol.to_uppercase());
                        yahoo::fetch_history(&yahoo_sym, days).await
                    }
                }
            }
            AssetCategory::Cash => Ok(Vec::new()),
            _ => yahoo::fetch_history(symbol, days).await,
        };
        match result {
            Ok(records) if !records.is_empty() => {
                let _ = update_tx.send(PriceUpdate::History(symbol.to_string(), records));
            }
            Err(e) => {
                let _ = update_tx.send(PriceUpdate::Error(
                    format!("History {}: {}", symbol, e),
                ));
            }
            _ => {}
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
