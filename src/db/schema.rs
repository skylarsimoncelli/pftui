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
            group_id INTEGER NOT NULL DEFAULT 1,
            added_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        -- NOTE: idx_watchlist_group_id index is created in the migration
        -- section below to handle existing databases without group_id column.

        CREATE TABLE IF NOT EXISTS watchlist_groups (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS economic_cache (
            series_id TEXT NOT NULL,
            date TEXT NOT NULL,
            value TEXT NOT NULL,
            fetched_at TEXT NOT NULL,
            PRIMARY KEY (series_id, date)
        );

        CREATE TABLE IF NOT EXISTS macro_events (
            series_id TEXT NOT NULL,
            event_date TEXT NOT NULL,
            expected TEXT NOT NULL,
            actual TEXT NOT NULL,
            surprise_pct TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (series_id, event_date)
        );
        CREATE INDEX IF NOT EXISTS idx_macro_events_event_date ON macro_events(event_date);

        CREATE TABLE IF NOT EXISTS fedwatch_cache (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_label TEXT NOT NULL,
            source_url TEXT NOT NULL,
            no_change_pct REAL NOT NULL,
            verified INTEGER NOT NULL DEFAULT 1,
            warning TEXT,
            snapshot_json TEXT NOT NULL,
            fetched_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_fedwatch_cache_fetched_at ON fedwatch_cache(fetched_at DESC);

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

        CREATE TABLE IF NOT EXISTS thesis (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            section TEXT NOT NULL UNIQUE,
            content TEXT NOT NULL,
            conviction TEXT NOT NULL DEFAULT 'medium',
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS thesis_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            section TEXT NOT NULL,
            content TEXT NOT NULL,
            conviction TEXT NOT NULL,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_thesis_history_section ON thesis_history(section);

        CREATE TABLE IF NOT EXISTS dividends (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            amount_per_share TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'USD',
            ex_date TEXT,
            pay_date TEXT NOT NULL,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_dividends_symbol ON dividends(symbol);
        CREATE INDEX IF NOT EXISTS idx_dividends_pay_date ON dividends(pay_date);

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

        CREATE TABLE IF NOT EXISTS cot_cache (
            cftc_code TEXT NOT NULL,
            report_date TEXT NOT NULL,
            open_interest INTEGER NOT NULL,
            managed_money_long INTEGER NOT NULL,
            managed_money_short INTEGER NOT NULL,
            managed_money_net INTEGER NOT NULL,
            commercial_long INTEGER NOT NULL,
            commercial_short INTEGER NOT NULL,
            commercial_net INTEGER NOT NULL,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (cftc_code, report_date)
        );
        CREATE INDEX IF NOT EXISTS idx_cot_report_date ON cot_cache(report_date);

        CREATE TABLE IF NOT EXISTS predictions_history (
            id TEXT NOT NULL,
            date TEXT NOT NULL,
            probability REAL NOT NULL,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (id, date)
        );
        CREATE INDEX IF NOT EXISTS idx_predictions_history_date ON predictions_history(date);

        CREATE TABLE IF NOT EXISTS sentiment_cache (
            index_type TEXT PRIMARY KEY,
            value INTEGER NOT NULL,
            classification TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS sentiment_history (
            index_type TEXT NOT NULL,
            date TEXT NOT NULL,
            value INTEGER NOT NULL,
            classification TEXT NOT NULL,
            PRIMARY KEY (index_type, date)
        );
        CREATE INDEX IF NOT EXISTS idx_sentiment_history_date ON sentiment_history(date);

        CREATE TABLE IF NOT EXISTS news_cache (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            url TEXT NOT NULL UNIQUE,
            source TEXT NOT NULL,
            source_type TEXT NOT NULL DEFAULT 'rss',
            symbol_tag TEXT,
            description TEXT NOT NULL DEFAULT '',
            extra_snippets TEXT NOT NULL DEFAULT '[]',
            category TEXT NOT NULL,
            published_at INTEGER NOT NULL,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_news_source ON news_cache(source);
        CREATE INDEX IF NOT EXISTS idx_news_category ON news_cache(category);
        CREATE INDEX IF NOT EXISTS idx_news_published_at ON news_cache(published_at);

        CREATE TABLE IF NOT EXISTS onchain_cache (
            metric TEXT NOT NULL,
            date TEXT NOT NULL,
            value TEXT NOT NULL,
            metadata TEXT,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (metric, date)
        );
        CREATE INDEX IF NOT EXISTS idx_onchain_date ON onchain_cache(date);
        CREATE INDEX IF NOT EXISTS idx_onchain_metric ON onchain_cache(metric);

        CREATE TABLE IF NOT EXISTS comex_cache (
            symbol TEXT NOT NULL,
            date TEXT NOT NULL,
            registered REAL NOT NULL,
            eligible REAL NOT NULL,
            total REAL NOT NULL,
            reg_ratio REAL NOT NULL,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (symbol, date)
        );
        CREATE INDEX IF NOT EXISTS idx_comex_date ON comex_cache(date);
        CREATE INDEX IF NOT EXISTS idx_comex_symbol ON comex_cache(symbol);

        CREATE TABLE IF NOT EXISTS bls_cache (
            series_id TEXT NOT NULL,
            year INTEGER NOT NULL,
            period TEXT NOT NULL,
            value TEXT NOT NULL,
            date TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (series_id, year, period)
        );
        CREATE INDEX IF NOT EXISTS idx_bls_series_date ON bls_cache(series_id, date);

        CREATE TABLE IF NOT EXISTS worldbank_cache (
            country_code TEXT NOT NULL,
            country_name TEXT NOT NULL,
            indicator_code TEXT NOT NULL,
            indicator_name TEXT NOT NULL,
            year INTEGER NOT NULL,
            value TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (country_code, indicator_code, year)
        );
        CREATE INDEX IF NOT EXISTS idx_worldbank_country_indicator 
            ON worldbank_cache(country_code, indicator_code, year);

        CREATE TABLE IF NOT EXISTS chart_state (
            symbol TEXT PRIMARY KEY,
            timeframe TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS fx_cache (
            currency TEXT PRIMARY KEY,
            rate TEXT NOT NULL,
            fetched_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS economic_data (
            indicator TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            previous TEXT,
            change TEXT,
            source_url TEXT NOT NULL,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS annotations (
            symbol TEXT PRIMARY KEY,
            thesis TEXT NOT NULL DEFAULT '',
            invalidation TEXT,
            review_date TEXT,
            target_price TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_annotations_review_date ON annotations(review_date);

        CREATE TABLE IF NOT EXISTS groups (
            name TEXT PRIMARY KEY,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS group_members (
            group_name TEXT NOT NULL,
            symbol TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (group_name, symbol),
            FOREIGN KEY (group_name) REFERENCES groups(name) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_group_members_symbol ON group_members(symbol);

        CREATE TABLE IF NOT EXISTS scan_queries (
            name TEXT PRIMARY KEY,
            filter_expr TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS scan_alert_state (
            name TEXT PRIMARY KEY,
            last_count INTEGER NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS scenarios (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            probability REAL NOT NULL DEFAULT 0.0,
            description TEXT,
            asset_impact TEXT,
            triggers TEXT,
            historical_precedent TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS scenario_signals (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            signal TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'watching',
            evidence TEXT,
            source TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_scenario_signals_scenario ON scenario_signals(scenario_id);

        CREATE TABLE IF NOT EXISTS scenario_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            probability REAL NOT NULL,
            driver TEXT,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_scenario_history_scenario ON scenario_history(scenario_id);

        CREATE TABLE IF NOT EXISTS thesis (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            section TEXT NOT NULL UNIQUE,
            content TEXT NOT NULL,
            conviction TEXT NOT NULL DEFAULT 'medium',
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS thesis_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            section TEXT NOT NULL,
            content TEXT NOT NULL,
            conviction TEXT NOT NULL,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_thesis_history_section ON thesis_history(section);

        CREATE TABLE IF NOT EXISTS convictions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            score INTEGER NOT NULL CHECK(score BETWEEN -5 AND 5),
            notes TEXT,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_convictions_symbol ON convictions(symbol);
        CREATE INDEX IF NOT EXISTS idx_convictions_recorded ON convictions(recorded_at);

        CREATE TABLE IF NOT EXISTS research_questions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            question TEXT NOT NULL,
            evidence_tilt TEXT NOT NULL DEFAULT 'neutral',
            key_signal TEXT,
            evidence TEXT,
            first_raised TEXT NOT NULL DEFAULT (datetime('now')),
            last_updated TEXT NOT NULL DEFAULT (datetime('now')),
            status TEXT NOT NULL DEFAULT 'open',
            resolution TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_research_questions_status ON research_questions(status);
        CREATE INDEX IF NOT EXISTS idx_research_questions_updated ON research_questions(last_updated);

        CREATE TABLE IF NOT EXISTS user_predictions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            claim TEXT NOT NULL,
            symbol TEXT,
            conviction TEXT NOT NULL DEFAULT 'medium',
            timeframe TEXT NOT NULL DEFAULT 'medium',
            confidence REAL,
            source_agent TEXT,
            target_date TEXT,
            resolution_criteria TEXT,
            outcome TEXT NOT NULL DEFAULT 'pending',
            score_notes TEXT,
            lesson TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            scored_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_user_predictions_outcome ON user_predictions(outcome);
        CREATE INDEX IF NOT EXISTS idx_user_predictions_symbol ON user_predictions(symbol);

        CREATE TABLE IF NOT EXISTS agent_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_agent TEXT NOT NULL,
            to_agent TEXT,
            package_id TEXT,
            package_title TEXT,
            priority TEXT NOT NULL DEFAULT 'normal',
            content TEXT NOT NULL,
            category TEXT,
            layer TEXT,
            acknowledged INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            acknowledged_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_agent_messages_to ON agent_messages(to_agent);
        CREATE INDEX IF NOT EXISTS idx_agent_messages_ack ON agent_messages(acknowledged);

        CREATE TABLE IF NOT EXISTS daily_notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            section TEXT NOT NULL DEFAULT 'general',
            content TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_daily_notes_date ON daily_notes(date);
        CREATE INDEX IF NOT EXISTS idx_daily_notes_section ON daily_notes(section);

        CREATE TABLE IF NOT EXISTS opportunity_cost (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            event TEXT NOT NULL,
            asset TEXT,
            missed_gain_pct REAL,
            missed_gain_usd REAL,
            avoided_loss_pct REAL,
            avoided_loss_usd REAL,
            was_rational INTEGER NOT NULL DEFAULT 1,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_opportunity_cost_date ON opportunity_cost(date);
        CREATE INDEX IF NOT EXISTS idx_opportunity_cost_asset ON opportunity_cost(asset);

        CREATE TABLE IF NOT EXISTS correlation_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol_a TEXT NOT NULL,
            symbol_b TEXT NOT NULL,
            correlation REAL NOT NULL,
            period TEXT NOT NULL DEFAULT '30d',
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_corr_snap_pair ON correlation_snapshots(symbol_a, symbol_b);
        CREATE INDEX IF NOT EXISTS idx_corr_snap_date ON correlation_snapshots(recorded_at);

        CREATE TABLE IF NOT EXISTS regime_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            regime TEXT NOT NULL,
            confidence REAL,
            drivers TEXT,
            vix REAL,
            dxy REAL,
            yield_10y REAL,
            oil REAL,
            gold REAL,
            btc REAL,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_regime_snapshots_recorded ON regime_snapshots(recorded_at);

        CREATE TABLE IF NOT EXISTS power_metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            country TEXT NOT NULL,
            metric TEXT NOT NULL,
            score REAL,
            rank INTEGER,
            trend TEXT NOT NULL DEFAULT 'stable',
            notes TEXT,
            source TEXT,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_power_metrics_country ON power_metrics(country);
        CREATE INDEX IF NOT EXISTS idx_power_metrics_metric ON power_metrics(metric);

        CREATE TABLE IF NOT EXISTS power_metrics_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            country TEXT NOT NULL,
            metric TEXT NOT NULL,
            decade INTEGER NOT NULL,
            score REAL NOT NULL,
            notes TEXT,
            source TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(country, metric, decade)
        );
        CREATE INDEX IF NOT EXISTS idx_pmh_country ON power_metrics_history(country);
        CREATE INDEX IF NOT EXISTS idx_pmh_decade ON power_metrics_history(decade);

        CREATE TABLE IF NOT EXISTS structural_cycles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cycle_name TEXT NOT NULL UNIQUE,
            current_stage TEXT NOT NULL,
            stage_entered TEXT,
            description TEXT,
            evidence TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS structural_outcomes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            probability REAL NOT NULL DEFAULT 0.0,
            time_horizon TEXT,
            description TEXT,
            historical_parallel TEXT,
            asset_implications TEXT,
            key_signals TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS structural_outcome_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            outcome_id INTEGER NOT NULL REFERENCES structural_outcomes(id) ON DELETE CASCADE,
            probability REAL NOT NULL,
            driver TEXT,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_structural_outcome_history ON structural_outcome_history(outcome_id);

        CREATE TABLE IF NOT EXISTS historical_parallels (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            period TEXT NOT NULL,
            event TEXT NOT NULL,
            parallel_to TEXT NOT NULL,
            similarity_score INTEGER CHECK(similarity_score BETWEEN 1 AND 10),
            asset_outcome TEXT,
            notes TEXT,
            source TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS structural_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            development TEXT NOT NULL,
            cycle_impact TEXT,
            outcome_shift TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_structural_log_date ON structural_log(date);

        CREATE TABLE IF NOT EXISTS trend_tracker (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            timeframe TEXT NOT NULL DEFAULT 'high',
            direction TEXT NOT NULL DEFAULT 'neutral',
            conviction TEXT NOT NULL DEFAULT 'medium',
            category TEXT,
            description TEXT,
            asset_impact TEXT,
            key_signal TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS trend_evidence (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trend_id INTEGER NOT NULL REFERENCES trend_tracker(id) ON DELETE CASCADE,
            date TEXT NOT NULL,
            evidence TEXT NOT NULL,
            direction_impact TEXT,
            source TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_trend_evidence_trend ON trend_evidence(trend_id);

        CREATE TABLE IF NOT EXISTS trend_asset_impact (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trend_id INTEGER NOT NULL REFERENCES trend_tracker(id) ON DELETE CASCADE,
            symbol TEXT NOT NULL,
            impact TEXT NOT NULL,
            mechanism TEXT,
            timeframe TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_trend_asset_trend ON trend_asset_impact(trend_id);

        CREATE TABLE IF NOT EXISTS timeframe_signals (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            signal_type TEXT NOT NULL,
            layers TEXT NOT NULL,
            assets TEXT NOT NULL,
            description TEXT NOT NULL,
            severity TEXT NOT NULL,
            detected_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_timeframe_signals_detected ON timeframe_signals(detected_at);
        CREATE INDEX IF NOT EXISTS idx_timeframe_signals_type ON timeframe_signals(signal_type);
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
             ALTER TABLE watchlist ADD COLUMN target_direction TEXT;",
        )?;
    }

    // Migration: add group_id column to watchlist and ensure watchlist_groups seed rows
    let has_group_id: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('watchlist') WHERE name = 'group_id'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_group_id {
        conn.execute_batch("ALTER TABLE watchlist ADD COLUMN group_id INTEGER NOT NULL DEFAULT 1")?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS watchlist_groups (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
         );
         INSERT OR IGNORE INTO watchlist_groups (id, name) VALUES
            (1, 'Core'),
            (2, 'Opportunistic'),
            (3, 'Research');
         CREATE INDEX IF NOT EXISTS idx_watchlist_group_id ON watchlist(group_id);",
    )?;

    // Migration: add resolution_criteria to user_predictions
    let has_resolution_criteria: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('user_predictions') WHERE name = 'resolution_criteria'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_resolution_criteria {
        conn.execute_batch("ALTER TABLE user_predictions ADD COLUMN resolution_criteria TEXT")?;
    }

    // Migration: add source_type column to news_cache (rss|brave)
    let has_news_source_type: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('news_cache') WHERE name = 'source_type'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_news_source_type {
        conn.execute_batch(
            "ALTER TABLE news_cache ADD COLUMN source_type TEXT NOT NULL DEFAULT 'rss'",
        )?;
    }

    // Migration: add Brave-rich news fields
    let has_news_description: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('news_cache') WHERE name = 'description'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_news_description {
        conn.execute_batch(
            "ALTER TABLE news_cache ADD COLUMN description TEXT NOT NULL DEFAULT ''",
        )?;
    }

    let has_news_snippets: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('news_cache') WHERE name = 'extra_snippets'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_news_snippets {
        conn.execute_batch(
            "ALTER TABLE news_cache ADD COLUMN extra_snippets TEXT NOT NULL DEFAULT '[]'",
        )?;
    }

    let has_agent_package_id: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('agent_messages') WHERE name = 'package_id'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_agent_package_id {
        conn.execute_batch(
            "ALTER TABLE agent_messages ADD COLUMN package_id TEXT;
             ALTER TABLE agent_messages ADD COLUMN package_title TEXT;",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_agent_messages_package ON agent_messages(package_id);",
    )?;

    let has_news_symbol_tag: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('news_cache') WHERE name = 'symbol_tag'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_news_symbol_tag {
        conn.execute_batch("ALTER TABLE news_cache ADD COLUMN symbol_tag TEXT")?;
    }

    // Migration guard: ensure thesis tables exist on upgraded databases.
    let has_thesis: bool = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='thesis'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_thesis {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS thesis (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                section TEXT NOT NULL UNIQUE,
                content TEXT NOT NULL,
                conviction TEXT NOT NULL DEFAULT 'medium',
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )?;
    }

    let has_thesis_history: bool = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='thesis_history'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_thesis_history {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS thesis_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                section TEXT NOT NULL,
                content TEXT NOT NULL,
                conviction TEXT NOT NULL,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_thesis_history_section ON thesis_history(section)",
        )?;
    }

    Ok(())
}
