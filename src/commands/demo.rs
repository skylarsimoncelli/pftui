use anyhow::Result;
use std::path::PathBuf;

use crate::config::Config;
use crate::db;

/// (symbol, category, tx_type, quantity, price, currency, date, notes)
type TxRow = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

/// Seed data for a realistic demo portfolio.
const DEMO_TRANSACTIONS: &[TxRow] = &[
    // Commodities — multiple buys at different prices
    (
        "GC=F",
        "commodity",
        "buy",
        "50",
        "1820.00",
        "USD",
        "2024-03-15",
        "Initial gold position",
    ),
    (
        "GC=F",
        "commodity",
        "buy",
        "30",
        "1950.50",
        "USD",
        "2024-09-10",
        "Added on dip",
    ),
    (
        "GC=F",
        "commodity",
        "buy",
        "20",
        "2050.00",
        "USD",
        "2025-04-22",
        "Dollar-cost average",
    ),
    (
        "SI=F",
        "commodity",
        "buy",
        "500",
        "23.50",
        "USD",
        "2024-05-01",
        "Silver position",
    ),
    (
        "SI=F",
        "commodity",
        "buy",
        "300",
        "28.20",
        "USD",
        "2025-01-15",
        "Added silver",
    ),
    (
        "URA",
        "fund",
        "buy",
        "200",
        "22.40",
        "USD",
        "2024-06-20",
        "Uranium ETF",
    ),
    (
        "URA",
        "fund",
        "buy",
        "150",
        "25.80",
        "USD",
        "2025-02-10",
        "Uranium add",
    ),
    (
        "COPX",
        "fund",
        "buy",
        "300",
        "35.60",
        "USD",
        "2024-08-05",
        "Copper miners ETF",
    ),
    (
        "USO",
        "fund",
        "buy",
        "100",
        "72.30",
        "USD",
        "2024-11-12",
        "Oil ETF",
    ),
    // Indices/ETFs — core equity allocation
    (
        "SPY",
        "fund",
        "buy",
        "10",
        "420.50",
        "USD",
        "2024-01-10",
        "S&P 500 core",
    ),
    (
        "SPY",
        "fund",
        "buy",
        "5",
        "445.00",
        "USD",
        "2024-07-15",
        "SPY add",
    ),
    (
        "SPY",
        "fund",
        "buy",
        "3",
        "480.00",
        "USD",
        "2025-03-01",
        "SPY DCA",
    ),
    (
        "QQQ",
        "fund",
        "buy",
        "8",
        "365.00",
        "USD",
        "2024-02-20",
        "Nasdaq exposure",
    ),
    (
        "QQQ",
        "fund",
        "buy",
        "4",
        "410.50",
        "USD",
        "2024-10-30",
        "QQQ add",
    ),
    (
        "IWM",
        "fund",
        "buy",
        "50",
        "195.00",
        "USD",
        "2024-04-15",
        "Small caps",
    ),
    // Crypto — volatile holdings
    (
        "BTC",
        "crypto",
        "buy",
        "0.5",
        "28000.00",
        "USD",
        "2024-01-05",
        "Bitcoin accumulation",
    ),
    (
        "BTC",
        "crypto",
        "buy",
        "0.25",
        "42000.00",
        "USD",
        "2024-06-18",
        "BTC add",
    ),
    (
        "BTC",
        "crypto",
        "buy",
        "0.1",
        "65000.00",
        "USD",
        "2025-01-20",
        "BTC top-up",
    ),
    (
        "ETH",
        "crypto",
        "buy",
        "5",
        "1800.00",
        "USD",
        "2024-02-10",
        "Ethereum position",
    ),
    (
        "ETH",
        "crypto",
        "buy",
        "3",
        "2400.00",
        "USD",
        "2024-08-25",
        "ETH add",
    ),
    (
        "SOL",
        "crypto",
        "buy",
        "100",
        "22.00",
        "USD",
        "2024-03-01",
        "Solana position",
    ),
    (
        "SOL",
        "crypto",
        "buy",
        "50",
        "95.00",
        "USD",
        "2024-12-10",
        "SOL add",
    ),
    // Bonds
    (
        "TLT",
        "fund",
        "buy",
        "40",
        "98.50",
        "USD",
        "2024-05-20",
        "20Y Treasury bond ETF",
    ),
    (
        "TLT",
        "fund",
        "buy",
        "20",
        "92.00",
        "USD",
        "2024-11-05",
        "TLT add on rate move",
    ),
    (
        "SHY",
        "fund",
        "buy",
        "80",
        "82.50",
        "USD",
        "2024-07-01",
        "Short-term treasury",
    ),
    // Individual equities
    (
        "AAPL",
        "equity",
        "buy",
        "20",
        "178.50",
        "USD",
        "2024-03-20",
        "Apple position",
    ),
    (
        "NVDA",
        "equity",
        "buy",
        "15",
        "480.00",
        "USD",
        "2024-04-10",
        "Nvidia",
    ),
    (
        "NVDA",
        "equity",
        "buy",
        "5",
        "650.00",
        "USD",
        "2025-02-01",
        "NVDA add",
    ),
    (
        "PLTR",
        "equity",
        "buy",
        "200",
        "18.50",
        "USD",
        "2024-06-01",
        "Palantir",
    ),
    (
        "PLTR",
        "equity",
        "buy",
        "100",
        "42.00",
        "USD",
        "2025-01-10",
        "PLTR add",
    ),
    // Cash
    (
        "USD",
        "cash",
        "buy",
        "15000",
        "1.00",
        "USD",
        "2024-01-01",
        "Cash reserve",
    ),
];

/// Watchlist entries for the demo: (symbol, category)
const DEMO_WATCHLIST: &[(&str, &str)] = &[
    ("MSFT", "equity"),
    ("AMZN", "equity"),
    ("TSLA", "equity"),
    ("XOM", "equity"),
    ("DX-Y.NYB", "forex"),
    ("^VIX", "equity"),
];

/// Build a demo database at the given path and populate it with realistic data.
fn build_demo_db(path: &std::path::Path) -> Result<()> {
    let conn = db::open_db(path)?;

    for &(symbol, category, tx_type, quantity, price, currency, date, notes) in DEMO_TRANSACTIONS {
        conn.execute(
            "INSERT INTO transactions (symbol, category, tx_type, quantity, price_per, currency, date, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![symbol, category, tx_type, quantity, price, currency, date, notes],
        )?;
    }

    for &(symbol, category) in DEMO_WATCHLIST {
        conn.execute(
            "INSERT INTO watchlist (symbol, category) VALUES (?1, ?2)",
            rusqlite::params![symbol, category],
        )?;
    }

    Ok(())
}

/// Run the demo: create a temp DB, populate it, launch the TUI.
pub fn run(config: &Config) -> Result<()> {
    let demo_dir = std::env::temp_dir().join("pftui-demo");
    std::fs::create_dir_all(&demo_dir)?;
    let db_path: PathBuf = demo_dir.join("demo.db");

    // Always rebuild fresh so the demo is clean
    if db_path.exists() {
        std::fs::remove_file(&db_path)?;
    }

    build_demo_db(&db_path)?;

    eprintln!(
        "🎮 pftui demo mode — using temporary portfolio at {}",
        db_path.display()
    );
    eprintln!("   Your real portfolio is untouched.");
    eprintln!();

    // Launch TUI with the demo DB
    let mut app = crate::app::App::new(config, db_path);
    app.init();
    let result = crate::tui::run(&mut app);
    app.shutdown();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_transactions_have_valid_categories() {
        let valid = ["commodity", "fund", "crypto", "equity", "cash", "forex"];
        for &(_, cat, _, _, _, _, _, _) in DEMO_TRANSACTIONS {
            assert!(valid.contains(&cat), "Invalid category: {cat}");
        }
    }

    #[test]
    fn demo_transactions_have_valid_tx_types() {
        let valid = ["buy", "sell"];
        for &(_, _, tx_type, _, _, _, _, _) in DEMO_TRANSACTIONS {
            assert!(valid.contains(&tx_type), "Invalid tx_type: {tx_type}");
        }
    }

    #[test]
    fn demo_transactions_quantities_are_positive() {
        for &(symbol, _, _, qty, _, _, _, _) in DEMO_TRANSACTIONS {
            let q: rust_decimal::Decimal = qty.parse().expect("quantity must parse");
            assert!(
                q > rust_decimal::Decimal::ZERO,
                "Non-positive quantity for {symbol}"
            );
        }
    }

    #[test]
    fn demo_transactions_prices_are_positive() {
        for &(symbol, _, _, _, price, _, _, _) in DEMO_TRANSACTIONS {
            let p: rust_decimal::Decimal = price.parse().expect("price must parse");
            assert!(
                p > rust_decimal::Decimal::ZERO,
                "Non-positive price for {symbol}"
            );
        }
    }

    #[test]
    fn demo_transactions_dates_are_valid() {
        for &(symbol, _, _, _, _, _, date, _) in DEMO_TRANSACTIONS {
            assert!(
                chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok(),
                "Invalid date for {symbol}: {date}"
            );
        }
    }

    #[test]
    fn demo_has_diverse_categories() {
        let cats: std::collections::HashSet<&str> = DEMO_TRANSACTIONS.iter().map(|t| t.1).collect();
        assert!(cats.contains("commodity"), "Missing commodity");
        assert!(cats.contains("crypto"), "Missing crypto");
        assert!(cats.contains("fund"), "Missing fund");
        assert!(cats.contains("equity"), "Missing equity");
        assert!(cats.contains("cash"), "Missing cash");
    }

    #[test]
    fn demo_has_multiple_transactions_per_asset() {
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for &(symbol, _, _, _, _, _, _, _) in DEMO_TRANSACTIONS {
            *counts.entry(symbol).or_default() += 1;
        }
        let multi_tx: Vec<_> = counts.iter().filter(|(_, &c)| c > 1).collect();
        assert!(
            multi_tx.len() >= 5,
            "Need at least 5 assets with multiple transactions for realism"
        );
    }

    #[test]
    fn demo_watchlist_entries_valid() {
        let valid = ["commodity", "fund", "crypto", "equity", "cash", "forex"];
        for &(symbol, cat) in DEMO_WATCHLIST {
            assert!(!symbol.is_empty(), "Empty watchlist symbol");
            assert!(valid.contains(&cat), "Invalid watchlist category: {cat}");
        }
    }

    #[test]
    fn build_demo_db_creates_valid_database() {
        let dir = std::env::temp_dir().join("pftui-test-demo");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test-demo.db");
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }

        build_demo_db(&path).unwrap();

        let conn = rusqlite::Connection::open(&path).unwrap();
        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count as usize, DEMO_TRANSACTIONS.len());

        let wl_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM watchlist", [], |r| r.get(0))
            .unwrap();
        assert_eq!(wl_count as usize, DEMO_WATCHLIST.len());

        // Verify unique symbols
        let symbol_count: i64 = conn
            .query_row("SELECT COUNT(DISTINCT symbol) FROM transactions", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            symbol_count >= 10,
            "Need at least 10 unique symbols, got {symbol_count}"
        );

        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_dir(&dir);
    }
}
