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

        CREATE TABLE IF NOT EXISTS journal (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            content TEXT NOT NULL,
            tag TEXT,
            symbol TEXT,
            conviction TEXT,
            status TEXT DEFAULT 'open',
            created_at TEXT DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_journal_timestamp ON journal(timestamp);
        CREATE INDEX IF NOT EXISTS idx_journal_tag ON journal(tag);
        CREATE INDEX IF NOT EXISTS idx_journal_symbol ON journal(symbol);
        CREATE INDEX IF NOT EXISTS idx_journal_status ON journal(status);

        CREATE TABLE IF NOT EXISTS calendar_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            name TEXT NOT NULL,
            impact TEXT NOT NULL,
            previous TEXT,
            forecast TEXT,
            event_type TEXT NOT NULL DEFAULT 'economic',
            symbol TEXT,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(date, name)
        );

        CREATE TABLE IF NOT EXISTS prediction_cache (
            market_id TEXT PRIMARY KEY,
            question TEXT NOT NULL,
            outcome_yes_price TEXT NOT NULL,
            outcome_no_price TEXT NOT NULL,
            volume TEXT NOT NULL,
            category TEXT NOT NULL,
            end_date TEXT NOT NULL,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_prediction_category ON prediction_cache(category);
        CREATE INDEX IF NOT EXISTS idx_prediction_volume ON prediction_cache(volume);

        CREATE TABLE IF NOT EXISTS predictions_cache (
            id TEXT PRIMARY KEY,
            question TEXT NOT NULL,
            probability REAL NOT NULL,
            volume_24h REAL NOT NULL,
            category TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_predictions_category ON predictions_cache(category);
        CREATE INDEX IF NOT EXISTS idx_predictions_volume ON predictions_cache(volume_24h);
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
