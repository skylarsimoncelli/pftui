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

/// Synthetic daily-OHLC series spec: (symbol, start_price, daily_drift, daily_vol).
/// Seeds ~1.5y of deterministic price history so analytics/chart/risk views
/// render with real-looking data offline. Values are illustrative, NOT real.
const DEMO_HISTORY: &[(&str, f64, f64, f64)] = &[
    ("BTC", 28_000.0, 0.0017, 0.035),
    ("ETH", 1_800.0, 0.0014, 0.040),
    ("SOL", 22.0, 0.0030, 0.055),
    ("GC=F", 1_820.0, 0.0008, 0.010),
    ("SI=F", 23.5, 0.0009, 0.018),
    ("SPY", 420.0, 0.0005, 0.009),
    ("QQQ", 365.0, 0.0007, 0.012),
    ("AAPL", 178.5, 0.0004, 0.015),
    ("NVDA", 480.0, 0.0020, 0.028),
    ("TLT", 98.5, -0.0003, 0.011),
];

/// Number of daily bars to synthesize per symbol.
const DEMO_HISTORY_DAYS: i64 = 540;
/// Fixed end date for the synthetic series (kept constant so renders are
/// reproducible regardless of the wall clock).
const DEMO_HISTORY_END: &str = "2026-06-15";

/// Deterministic 64-bit LCG seeded from the symbol — no RNG dependency, so the
/// demo series is byte-identical on every run (reproducible snapshots).
fn symbol_seed(symbol: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a offset basis
    for b in symbol.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h | 1
}

/// Build a deterministic synthetic OHLC series for one symbol.
fn synth_series(symbol: &str, start: f64, drift: f64, vol: f64) -> Vec<crate::models::price::HistoryRecord> {
    use rust_decimal::prelude::FromPrimitive;
    use rust_decimal::Decimal;

    let end = chrono::NaiveDate::parse_from_str(DEMO_HISTORY_END, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::NaiveDate::from_ymd_opt(2026, 6, 15).unwrap());
    let start_date = end - chrono::Duration::days(DEMO_HISTORY_DAYS - 1);

    let dec = |x: f64| Decimal::from_f64(x).unwrap_or_default().round_dp(4);

    let mut state = symbol_seed(symbol);
    // Uniform noise in [-1, 1] from the LCG.
    let mut next_noise = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    };

    let mut out = Vec::with_capacity(DEMO_HISTORY_DAYS as usize);
    let mut close = start;
    for i in 0..DEMO_HISTORY_DAYS {
        let date = (start_date + chrono::Duration::days(i))
            .format("%Y-%m-%d")
            .to_string();
        let open = close;
        // Geometric step with drift + symmetric noise; floored to stay positive.
        close = (close * (1.0 + drift + vol * next_noise())).max(start * 0.05);
        let hi = open.max(close) * (1.0 + vol * 0.5 * next_noise().abs());
        let lo = open.min(close) * (1.0 - vol * 0.5 * next_noise().abs());
        let volume = 1_000_000.0 * (1.0 + 0.4 * next_noise().abs());
        out.push(crate::models::price::HistoryRecord {
            date,
            close: dec(close),
            volume: Some(volume as u64),
            open: Some(dec(open)),
            high: Some(dec(hi)),
            low: Some(dec(lo)),
        });
    }
    out
}

/// Seed synthetic price history + latest-close cache for the demo symbols.
fn seed_price_history(conn: &rusqlite::Connection) -> Result<()> {
    for &(symbol, start, drift, vol) in DEMO_HISTORY {
        let series = synth_series(symbol, start, drift, vol);
        db::price_history::upsert_history(conn, symbol, "demo", &series)?;
        if let Some(last) = series.last() {
            conn.execute(
                "INSERT OR REPLACE INTO price_cache
                   (symbol, price, currency, fetched_at, source, previous_close)
                 VALUES (?1, ?2, 'USD', ?3, 'demo', ?4)",
                rusqlite::params![
                    symbol,
                    last.close.to_string(),
                    format!("{DEMO_HISTORY_END}T00:00:00Z"),
                    series
                        .get(series.len().saturating_sub(2))
                        .map(|r| r.close.to_string()),
                ],
            )?;
        }
    }
    Ok(())
}

/// Build a demo database at the given path and populate it with realistic data.
pub(crate) fn build_demo_db(path: &std::path::Path) -> Result<()> {
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

    seed_price_history(&conn)?;

    Ok(())
}

/// Create a fresh temporary demo database and return its path. Shared by the
/// interactive `demo` command and the offline `snapshot --demo` renderer so both
/// render identical synthetic data and never touch the real portfolio DB.
pub(crate) fn build_temp_demo_db() -> Result<PathBuf> {
    let demo_dir = std::env::temp_dir().join("pftui-demo");
    std::fs::create_dir_all(&demo_dir)?;
    let db_path: PathBuf = demo_dir.join("demo.db");

    // Always rebuild fresh so the demo is clean
    if db_path.exists() {
        std::fs::remove_file(&db_path)?;
    }

    build_demo_db(&db_path)?;
    Ok(db_path)
}

/// Run the demo: create a temp DB, populate it, launch the TUI.
pub fn run(config: &Config) -> Result<()> {
    let db_path = build_temp_demo_db()?;

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
    fn synth_series_is_deterministic_and_well_formed() {
        let a = synth_series("BTC", 28_000.0, 0.0017, 0.035);
        let b = synth_series("BTC", 28_000.0, 0.0017, 0.035);
        assert_eq!(a.len(), DEMO_HISTORY_DAYS as usize);
        // Deterministic: same symbol/params -> byte-identical series.
        assert_eq!(
            a.iter().map(|r| r.close.to_string()).collect::<Vec<_>>(),
            b.iter().map(|r| r.close.to_string()).collect::<Vec<_>>()
        );
        // All closes positive; dates strictly ascending; OHLC bounds sane.
        for w in a.windows(2) {
            assert!(w[0].date < w[1].date, "dates must be ascending");
        }
        for r in &a {
            assert!(r.close > rust_decimal::Decimal::ZERO, "close must be positive");
            let (o, h, l) = (r.open.unwrap(), r.high.unwrap(), r.low.unwrap());
            assert!(h >= o && h >= r.close, "high must bound open/close");
            assert!(l <= o && l <= r.close, "low must bound open/close");
        }
        // Different symbols diverge (independent seeds).
        let c = synth_series("ETH", 28_000.0, 0.0017, 0.035);
        assert_ne!(a[10].close, c[10].close);
    }

    #[test]
    fn build_demo_db_seeds_price_history() {
        let dir = std::env::temp_dir().join("pftui-test-demo-hist");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hist.db");
        let _ = std::fs::remove_file(&path);

        build_demo_db(&path).unwrap();

        let conn = rusqlite::Connection::open(&path).unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows as usize, DEMO_HISTORY.len() * DEMO_HISTORY_DAYS as usize);
        let cache: i64 = conn
            .query_row("SELECT COUNT(*) FROM price_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cache as usize, DEMO_HISTORY.len());

        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_dir(&dir);
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
