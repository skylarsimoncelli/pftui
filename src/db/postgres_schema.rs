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
                previous_close NUMERIC,
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
                condition TEXT,
                threshold TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'armed',
                rule_text TEXT NOT NULL,
                recurring BOOLEAN NOT NULL DEFAULT FALSE,
                cooldown_minutes BIGINT NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                triggered_at TIMESTAMPTZ
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS triggered_alerts (
                id BIGSERIAL PRIMARY KEY,
                alert_id BIGINT NOT NULL,
                triggered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                trigger_data TEXT NOT NULL DEFAULT '{}',
                acknowledged BOOLEAN NOT NULL DEFAULT FALSE
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_triggered_alerts_triggered_at
             ON triggered_alerts(triggered_at)",
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
            "CREATE TABLE IF NOT EXISTS power_metrics (
                id BIGSERIAL PRIMARY KEY,
                country TEXT NOT NULL,
                metric TEXT NOT NULL,
                score DOUBLE PRECISION,
                rank INTEGER,
                trend TEXT NOT NULL DEFAULT 'stable',
                notes TEXT,
                source TEXT,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_power_metrics_country ON power_metrics(country)",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_power_metrics_metric ON power_metrics(metric)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS power_metrics_history (
                id BIGSERIAL PRIMARY KEY,
                country TEXT NOT NULL,
                metric TEXT NOT NULL,
                decade INTEGER NOT NULL,
                score DOUBLE PRECISION NOT NULL,
                notes TEXT,
                source TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(country, metric, decade)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_pmh_country ON power_metrics_history(country)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_pmh_decade ON power_metrics_history(decade)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS structural_cycles (
                id BIGSERIAL PRIMARY KEY,
                cycle_name TEXT NOT NULL UNIQUE,
                current_stage TEXT NOT NULL,
                stage_entered TEXT,
                description TEXT,
                evidence TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS structural_outcomes (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                probability DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                time_horizon TEXT,
                description TEXT,
                historical_parallel TEXT,
                asset_implications TEXT,
                key_signals TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS structural_outcome_history (
                id BIGSERIAL PRIMARY KEY,
                outcome_id BIGINT NOT NULL REFERENCES structural_outcomes(id) ON DELETE CASCADE,
                probability DOUBLE PRECISION NOT NULL,
                driver TEXT,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_structural_outcome_history ON structural_outcome_history(outcome_id)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS historical_parallels (
                id BIGSERIAL PRIMARY KEY,
                period TEXT NOT NULL,
                event TEXT NOT NULL,
                parallel_to TEXT NOT NULL,
                similarity_score INTEGER CHECK(similarity_score BETWEEN 1 AND 10),
                asset_outcome TEXT,
                notes TEXT,
                source TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS structural_log (
                id BIGSERIAL PRIMARY KEY,
                date TEXT NOT NULL,
                development TEXT NOT NULL,
                cycle_impact TEXT,
                outcome_shift TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_structural_log_date ON structural_log(date)")
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
            "CREATE TABLE IF NOT EXISTS situation_snapshots (
                id BIGSERIAL PRIMARY KEY,
                recorded_at TIMESTAMPTZ NOT NULL,
                snapshot_json TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_situation_snapshots_recorded_at
             ON situation_snapshots(recorded_at DESC)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS narrative_snapshots (
                id BIGSERIAL PRIMARY KEY,
                recorded_at TIMESTAMPTZ NOT NULL,
                report_json TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_narrative_snapshots_recorded_at
             ON narrative_snapshots(recorded_at DESC)",
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
            "CREATE TABLE IF NOT EXISTS macro_events (
                series_id TEXT NOT NULL,
                event_date TEXT NOT NULL,
                expected TEXT NOT NULL,
                actual TEXT NOT NULL,
                surprise_pct TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (series_id, event_date)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS fedwatch_cache (
                id BIGSERIAL PRIMARY KEY,
                source_label TEXT NOT NULL,
                source_url TEXT NOT NULL,
                no_change_pct DOUBLE PRECISION NOT NULL,
                verified BOOLEAN NOT NULL DEFAULT TRUE,
                warning TEXT,
                snapshot_json TEXT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS consensus_tracker (
                id BIGSERIAL PRIMARY KEY,
                source TEXT NOT NULL,
                topic TEXT NOT NULL,
                call_text TEXT NOT NULL,
                call_date TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS correlation_snapshots (
                id BIGSERIAL PRIMARY KEY,
                symbol_a TEXT NOT NULL,
                symbol_b TEXT NOT NULL,
                correlation DOUBLE PRECISION NOT NULL,
                period TEXT NOT NULL DEFAULT '30d',
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS technical_snapshots (
                symbol TEXT NOT NULL,
                timeframe TEXT NOT NULL,
                rsi_14 DOUBLE PRECISION,
                macd DOUBLE PRECISION,
                macd_signal DOUBLE PRECISION,
                macd_histogram DOUBLE PRECISION,
                sma_20 DOUBLE PRECISION,
                sma_50 DOUBLE PRECISION,
                sma_200 DOUBLE PRECISION,
                bollinger_upper DOUBLE PRECISION,
                bollinger_middle DOUBLE PRECISION,
                bollinger_lower DOUBLE PRECISION,
                range_52w_low DOUBLE PRECISION,
                range_52w_high DOUBLE PRECISION,
                range_52w_position DOUBLE PRECISION,
                volume_avg_20 DOUBLE PRECISION,
                volume_ratio_20 DOUBLE PRECISION,
                volume_regime TEXT,
                above_sma_20 BOOLEAN,
                above_sma_50 BOOLEAN,
                above_sma_200 BOOLEAN,
                computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (symbol, timeframe, computed_at)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_technical_snapshots_symbol_tf
             ON technical_snapshots(symbol, timeframe, computed_at DESC)",
        )
        .execute(pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_signals_scenario ON scenario_signals(scenario_id)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_history_scenario ON scenario_history(scenario_id)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_thesis_history_section ON thesis_history(section)",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_worldbank_country_indicator ON worldbank_cache(country_code, indicator_code, year)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_cot_report_date ON cot_cache(report_date)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_sentiment_history_date ON sentiment_history(date)",
        )
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
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_macro_events_event_date ON macro_events(event_date)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_fedwatch_cache_fetched_at ON fedwatch_cache(fetched_at DESC)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_consensus_tracker_topic ON consensus_tracker(topic)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_consensus_tracker_date ON consensus_tracker(call_date)",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_corr_snap_pair ON correlation_snapshots(symbol_a, symbol_b)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_corr_snap_date ON correlation_snapshots(recorded_at)",
        )
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
        sqlx::query("ALTER TABLE alerts ADD COLUMN IF NOT EXISTS condition TEXT")
            .execute(pool)
            .await?;
        sqlx::query(
            "ALTER TABLE alerts ADD COLUMN IF NOT EXISTS recurring BOOLEAN NOT NULL DEFAULT FALSE",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE alerts ADD COLUMN IF NOT EXISTS cooldown_minutes BIGINT NOT NULL DEFAULT 0",
        )
        .execute(pool)
        .await?;

        // F46: Stored market structure and key levels
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS technical_levels (
                id BIGSERIAL PRIMARY KEY,
                symbol TEXT NOT NULL,
                level_type TEXT NOT NULL,
                price DOUBLE PRECISION NOT NULL,
                strength DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                source_method TEXT NOT NULL,
                timeframe TEXT NOT NULL DEFAULT '1d',
                notes TEXT,
                computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_technical_levels_symbol ON technical_levels(symbol)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_technical_levels_type ON technical_levels(symbol, level_type)",
        )
        .execute(pool)
        .await?;

        // F49: Precomputed technical signals
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS technical_signals (
                id BIGSERIAL PRIMARY KEY,
                symbol TEXT NOT NULL,
                signal_type TEXT NOT NULL,
                direction TEXT NOT NULL,
                severity TEXT NOT NULL,
                trigger_price DOUBLE PRECISION,
                description TEXT NOT NULL,
                timeframe TEXT NOT NULL DEFAULT '1d',
                detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_technical_signals_symbol ON technical_signals(symbol)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_technical_signals_type ON technical_signals(signal_type)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_technical_signals_detected ON technical_signals(detected_at)",
        )
        .execute(pool)
        .await?;

        // Migration: add OHLCV-aware ATR columns to technical_snapshots (F48 step 2)
        for col in &[
            ("atr_14", "DOUBLE PRECISION"),
            ("atr_ratio", "DOUBLE PRECISION"),
            ("range_expansion", "BOOLEAN"),
            ("day_range_ratio", "DOUBLE PRECISION"),
        ] {
            let check = format!(
                "SELECT COUNT(*) FROM information_schema.columns WHERE table_name = 'technical_snapshots' AND column_name = '{}'",
                col.0
            );
            let exists: (i64,) = sqlx::query_as(&check).fetch_one(pool).await?;
            if exists.0 == 0 {
                let alter = format!(
                    "ALTER TABLE technical_snapshots ADD COLUMN {} {}",
                    col.0, col.1
                );
                sqlx::query(&alter).execute(pool).await?;
            }
        }

        // Migration: add previous_close to price_cache (movers P0 fix)
        sqlx::query("ALTER TABLE price_cache ADD COLUMN IF NOT EXISTS previous_close NUMERIC")
            .execute(pool)
            .await?;

        // F53: Situation Engine — add phase/resolved columns to scenarios
        sqlx::query("ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS phase TEXT NOT NULL DEFAULT 'hypothesis'")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMPTZ")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS resolution_notes TEXT")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenarios_phase ON scenarios(phase)")
            .execute(pool)
            .await?;

        // F53: Situation Engine — new tables
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_branches (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                probability DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                description TEXT,
                sort_order INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE (scenario_id, name)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_branches_scenario ON scenario_branches(scenario_id)")
            .execute(pool)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_impacts (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id BIGINT REFERENCES scenario_branches(id) ON DELETE CASCADE,
                symbol TEXT NOT NULL,
                direction TEXT NOT NULL,
                tier TEXT NOT NULL DEFAULT 'primary',
                mechanism TEXT,
                parent_id BIGINT REFERENCES scenario_impacts(id) ON DELETE SET NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_impacts_scenario ON scenario_impacts(scenario_id)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_scenario_impacts_symbol ON scenario_impacts(symbol)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_scenario_impacts_parent ON scenario_impacts(parent_id)",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_indicators (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id BIGINT REFERENCES scenario_branches(id) ON DELETE CASCADE,
                impact_id BIGINT REFERENCES scenario_impacts(id) ON DELETE SET NULL,
                symbol TEXT NOT NULL,
                metric TEXT NOT NULL DEFAULT 'close',
                operator TEXT NOT NULL,
                threshold TEXT NOT NULL,
                label TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'watching',
                triggered_at TIMESTAMPTZ,
                last_value TEXT,
                last_checked TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_indicators_scenario ON scenario_indicators(scenario_id)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_indicators_symbol ON scenario_indicators(symbol)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_indicators_status ON scenario_indicators(status)")
            .execute(pool)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_updates (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id BIGINT REFERENCES scenario_branches(id) ON DELETE CASCADE,
                headline TEXT NOT NULL,
                detail TEXT,
                severity TEXT NOT NULL DEFAULT 'normal',
                source TEXT,
                source_agent TEXT,
                next_decision TEXT,
                next_decision_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_updates_scenario ON scenario_updates(scenario_id)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_updates_created ON scenario_updates(created_at DESC)")
            .execute(pool)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS power_flows (
                id              BIGSERIAL PRIMARY KEY,
                date            TEXT NOT NULL,
                event           TEXT NOT NULL,
                source_complex  TEXT NOT NULL,
                direction       TEXT NOT NULL,
                target_complex  TEXT,
                evidence        TEXT NOT NULL,
                magnitude       INTEGER NOT NULL CHECK(magnitude BETWEEN 1 AND 5),
                agent_source    TEXT,
                created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_power_flows_date ON power_flows(date)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_power_flows_complex ON power_flows(source_complex)",
        )
        .execute(pool)
        .await?;

        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}
