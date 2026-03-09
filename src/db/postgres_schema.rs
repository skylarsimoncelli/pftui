use anyhow::Result;
use sqlx::PgPool;

pub fn run_migrations(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
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
                price NUMERIC NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                fetched_at TIMESTAMPTZ NOT NULL,
                source TEXT NOT NULL,
                PRIMARY KEY (symbol, currency)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS fx_cache (
                currency TEXT PRIMARY KEY,
                rate TEXT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
                quantity NUMERIC NOT NULL,
                price_per NUMERIC NOT NULL,
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
            "CREATE TABLE IF NOT EXISTS calendar_events (
                id BIGSERIAL PRIMARY KEY,
                date TEXT NOT NULL,
                name TEXT NOT NULL,
                impact TEXT NOT NULL,
                previous TEXT,
                forecast TEXT,
                event_type TEXT NOT NULL DEFAULT 'economic',
                symbol TEXT,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE (date, name)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS cot_cache (
                cftc_code TEXT NOT NULL,
                report_date TEXT NOT NULL,
                open_interest BIGINT NOT NULL,
                managed_money_long BIGINT NOT NULL,
                managed_money_short BIGINT NOT NULL,
                managed_money_net BIGINT NOT NULL,
                commercial_long BIGINT NOT NULL,
                commercial_short BIGINT NOT NULL,
                commercial_net BIGINT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (cftc_code, report_date)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sentiment_cache (
                index_type TEXT PRIMARY KEY,
                value BIGINT NOT NULL,
                classification TEXT NOT NULL,
                timestamp BIGINT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sentiment_history (
                index_type TEXT NOT NULL,
                date TEXT NOT NULL,
                value BIGINT NOT NULL,
                classification TEXT NOT NULL,
                PRIMARY KEY (index_type, date)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS allocation_targets (
                symbol TEXT PRIMARY KEY,
                target_pct NUMERIC NOT NULL,
                drift_band_pct NUMERIC NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS portfolio_allocations (
                id BIGSERIAL PRIMARY KEY,
                symbol TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL,
                allocation_pct NUMERIC NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS portfolio_snapshots (
                date TEXT PRIMARY KEY,
                total_value TEXT NOT NULL,
                cash_value TEXT NOT NULL,
                invested_value TEXT NOT NULL,
                snapshot_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS position_snapshots (
                date TEXT NOT NULL,
                symbol TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                value TEXT NOT NULL,
                PRIMARY KEY (date, symbol)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS worldbank_cache (
                country_code TEXT NOT NULL,
                country_name TEXT NOT NULL,
                indicator_code TEXT NOT NULL,
                indicator_name TEXT NOT NULL,
                year INTEGER NOT NULL,
                value TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (country_code, indicator_code, year)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS comex_cache (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                registered DOUBLE PRECISION NOT NULL,
                eligible DOUBLE PRECISION NOT NULL,
                total DOUBLE PRECISION NOT NULL,
                reg_ratio DOUBLE PRECISION NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (symbol, date)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS onchain_cache (
                metric TEXT NOT NULL,
                date TEXT NOT NULL,
                value TEXT NOT NULL,
                metadata TEXT,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (metric, date)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS economic_data (
                indicator TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                previous TEXT,
                change TEXT,
                source_url TEXT NOT NULL,
                fetched_at TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS bls_cache (
                series_id TEXT NOT NULL,
                year INTEGER NOT NULL,
                period TEXT NOT NULL,
                value TEXT NOT NULL,
                date TEXT NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (series_id, year, period)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scan_queries (
                name TEXT PRIMARY KEY,
                filter_expr TEXT NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scan_alert_state (
                name TEXT PRIMARY KEY,
                last_count BIGINT NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_worldbank_country_indicator ON worldbank_cache(country_code, indicator_code, year)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_cot_report_date ON cot_cache(report_date)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sentiment_history_date ON sentiment_history(date)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_bls_series_date ON bls_cache(series_id, date)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_comex_date ON comex_cache(date)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_comex_symbol ON comex_cache(symbol)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_onchain_date ON onchain_cache(date)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_onchain_metric ON onchain_cache(metric)")
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO pftui_migrations (version) VALUES (1) ON CONFLICT DO NOTHING")
            .execute(pool)
            .await?;
        // v2: remove legacy bridge table from hybrid implementation.
        sqlx::query("DROP TABLE IF EXISTS pftui_sqlite_state")
            .execute(pool)
            .await?;
        sqlx::query("INSERT INTO pftui_migrations (version) VALUES (2) ON CONFLICT DO NOTHING")
            .execute(pool)
            .await?;
        // v3: migrate hot-path numeric/time columns from legacy TEXT types.
        sqlx::query(
            "ALTER TABLE price_cache
               ALTER COLUMN price TYPE NUMERIC
               USING CASE
                    WHEN TRIM(price::TEXT) = '' THEN NULL
                    ELSE price::NUMERIC
               END,
               ALTER COLUMN fetched_at TYPE TIMESTAMPTZ
               USING COALESCE(
                    NULLIF(TRIM(fetched_at::TEXT), '')::TIMESTAMPTZ,
                    NOW()
               )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE transactions
               ALTER COLUMN quantity TYPE NUMERIC
               USING CASE
                    WHEN TRIM(quantity::TEXT) = '' THEN NULL
                    ELSE quantity::NUMERIC
               END,
               ALTER COLUMN price_per TYPE NUMERIC
               USING CASE
                    WHEN TRIM(price_per::TEXT) = '' THEN NULL
                    ELSE price_per::NUMERIC
               END",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE portfolio_allocations
               ALTER COLUMN allocation_pct TYPE NUMERIC
               USING CASE
                    WHEN TRIM(allocation_pct::TEXT) = '' THEN NULL
                    ELSE allocation_pct::NUMERIC
               END",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE allocation_targets
               ALTER COLUMN target_pct TYPE NUMERIC
               USING CASE
                    WHEN TRIM(target_pct::TEXT) = '' THEN NULL
                    ELSE target_pct::NUMERIC
               END,
               ALTER COLUMN drift_band_pct TYPE NUMERIC
               USING CASE
                    WHEN TRIM(drift_band_pct::TEXT) = '' THEN NULL
                    ELSE drift_band_pct::NUMERIC
               END",
        )
        .execute(pool)
        .await?;
        sqlx::query("INSERT INTO pftui_migrations (version) VALUES (3) ON CONFLICT DO NOTHING")
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}
