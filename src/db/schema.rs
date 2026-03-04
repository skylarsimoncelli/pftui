use anyhow::Result;
use rusqlite::Connection;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS transactions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            category TEXT NOT NULL,
            tx_type TEXT NOT NULL,
            quantity TEXT NOT NULL,
            price_per TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'USD',
            date TEXT NOT NULL,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS price_cache (
            symbol TEXT NOT NULL,
            price TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'USD',
            fetched_at TEXT NOT NULL,
            source TEXT NOT NULL,
            PRIMARY KEY (symbol, currency)
        );

        CREATE TABLE IF NOT EXISTS price_history (
            symbol TEXT NOT NULL,
            date TEXT NOT NULL,
            close TEXT NOT NULL,
            source TEXT NOT NULL,
            PRIMARY KEY (symbol, date)
        );

        CREATE TABLE IF NOT EXISTS portfolio_allocations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL UNIQUE,
            category TEXT NOT NULL,
            allocation_pct TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS watchlist (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL UNIQUE,
            category TEXT NOT NULL,
            added_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS economic_cache (
            series_id TEXT NOT NULL,
            date TEXT NOT NULL,
            value TEXT NOT NULL,
            fetched_at TEXT NOT NULL,
            PRIMARY KEY (series_id, date)
        );

        CREATE TABLE IF NOT EXISTS alerts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL DEFAULT 'price',
            symbol TEXT NOT NULL,
            direction TEXT NOT NULL,
            threshold TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'armed',
            rule_text TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            triggered_at TEXT
        );

        CREATE TABLE IF NOT EXISTS portfolio_snapshots (
            date TEXT PRIMARY KEY,
            total_value TEXT NOT NULL,
            cash_value TEXT NOT NULL,
            invested_value TEXT NOT NULL,
            snapshot_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS position_snapshots (
            date TEXT NOT NULL,
            symbol TEXT NOT NULL,
            quantity TEXT NOT NULL,
            price TEXT NOT NULL,
            value TEXT NOT NULL,
            PRIMARY KEY (date, symbol)
        );

        CREATE TABLE IF NOT EXISTS allocation_targets (
            symbol TEXT PRIMARY KEY,
            target_pct TEXT NOT NULL,
            drift_band_pct TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        ",
    )?;

    // Migration: add volume column to price_history (added in v0.2)
    // SQLite ALTER TABLE ADD COLUMN is idempotent-safe via checking pragma
    let has_volume: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('price_history') WHERE name = 'volume'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;

    if !has_volume {
        conn.execute_batch("ALTER TABLE price_history ADD COLUMN volume TEXT")?;
    }

    // Migration: add target_price and target_direction to watchlist (F6.3)
    let has_target_price: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('watchlist') WHERE name = 'target_price'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;

    if !has_target_price {
        conn.execute_batch(
            "ALTER TABLE watchlist ADD COLUMN target_price TEXT;
             ALTER TABLE watchlist ADD COLUMN target_direction TEXT;"
        )?;
    }

    Ok(())
}
