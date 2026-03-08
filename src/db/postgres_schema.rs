use anyhow::Result;
use sqlx::PgPool;

pub fn run_migrations(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS pftui_migrations (
                version BIGINT PRIMARY KEY,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;

        // v1: core tables needed by migrated backend-dispatched modules.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS price_cache (
                symbol TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                fetched_at TEXT NOT NULL,
                source TEXT NOT NULL,
                PRIMARY KEY (symbol, currency)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS transactions (
                id BIGSERIAL PRIMARY KEY,
                symbol TEXT NOT NULL,
                category TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                date TEXT NOT NULL,
                notes TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS watchlist (
                id BIGSERIAL PRIMARY KEY,
                symbol TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL,
                group_id BIGINT NOT NULL DEFAULT 1,
                added_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                target_price TEXT,
                target_direction TEXT
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alerts (
                id BIGSERIAL PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'price',
                symbol TEXT NOT NULL,
                direction TEXT NOT NULL,
                threshold TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'armed',
                rule_text TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                triggered_at TIMESTAMPTZ
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS allocation_targets (
                symbol TEXT PRIMARY KEY,
                target_pct TEXT NOT NULL,
                drift_band_pct TEXT NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenarios (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                probability DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                description TEXT,
                asset_impact TEXT,
                triggers TEXT,
                historical_precedent TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_signals (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                signal TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'watching',
                evidence TEXT,
                source TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_history (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                probability DOUBLE PRECISION NOT NULL,
                driver TEXT,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS thesis (
                id BIGSERIAL PRIMARY KEY,
                section TEXT NOT NULL UNIQUE,
                content TEXT NOT NULL,
                conviction TEXT NOT NULL DEFAULT 'medium',
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS thesis_history (
                id BIGSERIAL PRIMARY KEY,
                section TEXT NOT NULL,
                content TEXT NOT NULL,
                conviction TEXT NOT NULL,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_signals_scenario ON scenario_signals(scenario_id)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_history_scenario ON scenario_history(scenario_id)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_thesis_history_section ON thesis_history(section)")
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO pftui_migrations (version) VALUES (1) ON CONFLICT DO NOTHING")
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}
