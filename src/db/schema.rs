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
            paired_tx_id INTEGER REFERENCES transactions(id),
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

        CREATE TABLE IF NOT EXISTS consensus_tracker (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source TEXT NOT NULL,
            topic TEXT NOT NULL,
            call_text TEXT NOT NULL,
            call_date TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_consensus_tracker_topic ON consensus_tracker(topic);
        CREATE INDEX IF NOT EXISTS idx_consensus_tracker_date ON consensus_tracker(call_date);

        CREATE TABLE IF NOT EXISTS alerts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL DEFAULT 'price',
            symbol TEXT NOT NULL,
            direction TEXT NOT NULL,
            condition TEXT,
            threshold TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'armed',
            rule_text TEXT NOT NULL,
            recurring INTEGER NOT NULL DEFAULT 0,
            cooldown_minutes INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            triggered_at TEXT
        );

        CREATE TABLE IF NOT EXISTS triggered_alerts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            alert_id INTEGER NOT NULL,
            triggered_at TEXT NOT NULL DEFAULT (datetime('now')),
            trigger_data TEXT NOT NULL DEFAULT '{}',
            acknowledged INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_triggered_alerts_triggered_at ON triggered_alerts(triggered_at);

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

        CREATE TABLE IF NOT EXISTS situation_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            recorded_at TEXT NOT NULL,
            snapshot_json TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_situation_snapshots_recorded_at
            ON situation_snapshots(recorded_at DESC);

        CREATE TABLE IF NOT EXISTS narrative_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            recorded_at TEXT NOT NULL,
            report_json TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_narrative_snapshots_recorded_at
            ON narrative_snapshots(recorded_at DESC);

        CREATE TABLE IF NOT EXISTS allocation_targets (
            symbol TEXT PRIMARY KEY,
            target_pct TEXT NOT NULL,
            drift_band_pct TEXT NOT NULL,
            target_floor_pct TEXT NOT NULL,
            target_ceiling_pct TEXT NOT NULL,
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
            author TEXT NOT NULL DEFAULT 'system',
            created_at TEXT DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_journal_timestamp ON journal(timestamp);
        CREATE INDEX IF NOT EXISTS idx_journal_tag ON journal(tag);
        CREATE INDEX IF NOT EXISTS idx_journal_symbol ON journal(symbol);
        CREATE INDEX IF NOT EXISTS idx_journal_status ON journal(status);
        -- idx_journal_author is created in the author-column migration below
        -- (it cannot be created here because legacy databases need to ALTER
        -- TABLE first, and CREATE INDEX on a missing column would fail).

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

        CREATE TABLE IF NOT EXISTS prediction_market_contracts (
            contract_id TEXT PRIMARY KEY,
            exchange TEXT NOT NULL,
            event_id TEXT NOT NULL,
            event_title TEXT NOT NULL,
            question TEXT NOT NULL,
            category TEXT NOT NULL,
            last_price REAL NOT NULL,
            volume_24h REAL NOT NULL,
            liquidity REAL NOT NULL,
            end_date TEXT,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_pmc_category ON prediction_market_contracts(category);
        CREATE INDEX IF NOT EXISTS idx_pmc_volume ON prediction_market_contracts(volume_24h);
        CREATE INDEX IF NOT EXISTS idx_pmc_exchange ON prediction_market_contracts(exchange);
        CREATE INDEX IF NOT EXISTS idx_pmc_event_id ON prediction_market_contracts(event_id);

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
            source_domain TEXT NOT NULL DEFAULT '',
            source_tier INTEGER NOT NULL DEFAULT 3 CHECK(source_tier BETWEEN 1 AND 4),
            source_tier_inferred INTEGER NOT NULL DEFAULT 1 CHECK(source_tier_inferred IN (0, 1)),
            source_independence TEXT NOT NULL DEFAULT 'unknown'
                CHECK(source_independence IN ('independent','wire','restatement','rumor','unknown')),
            description TEXT NOT NULL DEFAULT '',
            extra_snippets TEXT NOT NULL DEFAULT '[]',
            category TEXT NOT NULL,
            topic TEXT NOT NULL DEFAULT 'other',
            published_at INTEGER NOT NULL,
            fetched_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_news_source ON news_cache(source);
        CREATE INDEX IF NOT EXISTS idx_news_category ON news_cache(category);
        CREATE INDEX IF NOT EXISTS idx_news_published_at ON news_cache(published_at);
        -- Indexes for source_domain, source_tier, source_independence, and topic
        -- are created by ensure_source_tier_schema / ensure_news_cache_topic_column
        -- AFTER the ALTER TABLE migrations that add the columns. Creating them in
        -- this initial batch races the migrations and fails on pre-existing DBs.

        CREATE TABLE IF NOT EXISTS news_source_tiers (
            domain TEXT PRIMARY KEY,
            tier INTEGER NOT NULL CHECK(tier BETWEEN 1 AND 4),
            notes TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS news_topic_markets (
            topic TEXT PRIMARY KEY,
            primary_market_id TEXT NOT NULL,
            secondary_market_id TEXT,
            last_updated TEXT NOT NULL DEFAULT (datetime('now')),
            notes TEXT
        );

        CREATE TABLE IF NOT EXISTS news_topic_market_seed_state (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            seeded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS narrative_money_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
            news_volume REAL NOT NULL,
            news_sentiment REAL NOT NULL,
            market_price REAL,
            market_delta_24h REAL,
            divergence_score REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_narrative_money_history_scenario
            ON narrative_money_history(scenario_id, recorded_at);
        CREATE INDEX IF NOT EXISTS idx_narrative_money_history_recorded
            ON narrative_money_history(recorded_at);

        CREATE TABLE IF NOT EXISTS news_silence_baselines (
            topic TEXT NOT NULL,
            day_of_week INTEGER NOT NULL CHECK(day_of_week BETWEEN 1 AND 7),
            samples_json TEXT NOT NULL DEFAULT '[]',
            median_count REAL NOT NULL DEFAULT 0.0,
            p30_count REAL NOT NULL DEFAULT 0.0,
            p80_count REAL NOT NULL DEFAULT 0.0,
            observed_count INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'insufficient'
                CHECK(status IN ('insufficient','normal','silent','saturated')),
            previous_status TEXT,
            changed_at TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(topic, day_of_week)
        );
        CREATE INDEX IF NOT EXISTS idx_news_silence_baselines_status
            ON news_silence_baselines(status, updated_at);

        CREATE TABLE IF NOT EXISTS rss_feed_health (
            feed_id TEXT PRIMARY KEY,
            last_success_at TEXT,
            last_failure_at TEXT,
            last_failure_reason TEXT,
            consecutive_failures INTEGER NOT NULL DEFAULT 0,
            total_failures INTEGER NOT NULL DEFAULT 0,
            total_successes INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'active'
                CHECK(status IN ('active', 'degraded', 'disabled'))
        );
        CREATE INDEX IF NOT EXISTS idx_rss_feed_health_status ON rss_feed_health(status);

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
            source TEXT NOT NULL DEFAULT 'unknown',
            confidence TEXT NOT NULL DEFAULT 'medium',
            fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
            quarantined INTEGER NOT NULL DEFAULT 0
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
            topic TEXT NOT NULL DEFAULT 'other'
                CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other')),
            confidence REAL,
            source_agent TEXT,
            source_article_id INTEGER REFERENCES news_cache(id),
            target_date TEXT,
            resolution_criteria TEXT,
            outcome TEXT NOT NULL DEFAULT 'pending',
            score_notes TEXT,
            lesson TEXT,
            lessons_applied TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            scored_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_user_predictions_outcome ON user_predictions(outcome);
        CREATE INDEX IF NOT EXISTS idx_user_predictions_symbol ON user_predictions(symbol);
        -- Indexes for `topic` and `source_article_id` are re-created at line ~982
        -- AFTER the ALTER TABLE migrations that add the columns. Creating them in
        -- this initial batch races the migrations and fails on pre-existing DBs.

        CREATE TABLE IF NOT EXISTS prediction_falsification_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prediction_id INTEGER NOT NULL REFERENCES user_predictions(id) ON DELETE CASCADE,
            rule_type TEXT NOT NULL,
            symbol TEXT,
            threshold_value REAL,
            threshold_low REAL,
            threshold_high REAL,
            threshold_text TEXT,
            eval_date_start TEXT,
            eval_date_end TEXT NOT NULL,
            parse_confidence TEXT NOT NULL DEFAULT 'medium',
            auto_score_eligible INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_prediction_falsification_rules_auto
            ON prediction_falsification_rules(auto_score_eligible, eval_date_end, parse_confidence);
        CREATE INDEX IF NOT EXISTS idx_prediction_falsification_rules_prediction
            ON prediction_falsification_rules(prediction_id);

        CREATE TABLE IF NOT EXISTS news_source_accuracy (
            source_domain TEXT NOT NULL,
            topic TEXT NOT NULL
                CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other')),
            n_predictions_implied INTEGER NOT NULL DEFAULT 0,
            n_correct INTEGER NOT NULL DEFAULT 0,
            n_wrong INTEGER NOT NULL DEFAULT 0,
            n_partial INTEGER NOT NULL DEFAULT 0,
            last_updated TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(source_domain, topic)
        );
        CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_topic
            ON news_source_accuracy(topic);

        CREATE TABLE IF NOT EXISTS news_source_accuracy_events (
            prediction_id INTEGER PRIMARY KEY REFERENCES user_predictions(id) ON DELETE CASCADE,
            source_article_id INTEGER REFERENCES news_cache(id),
            source_domain TEXT NOT NULL,
            topic TEXT NOT NULL
                CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other')),
            outcome TEXT NOT NULL CHECK(outcome IN ('correct','partial','wrong')),
            scored_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_events_source
            ON news_source_accuracy_events(source_domain, topic);
        CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_events_scored
            ON news_source_accuracy_events(scored_at);

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
            author TEXT NOT NULL DEFAULT 'system',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_daily_notes_date ON daily_notes(date);
        CREATE INDEX IF NOT EXISTS idx_daily_notes_section ON daily_notes(section);
        -- idx_daily_notes_author is created in the author-column migration below.

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

        CREATE TABLE IF NOT EXISTS technical_snapshots (
            symbol TEXT NOT NULL,
            timeframe TEXT NOT NULL,
            rsi_14 REAL,
            macd REAL,
            macd_signal REAL,
            macd_histogram REAL,
            sma_20 REAL,
            sma_50 REAL,
            sma_200 REAL,
            bollinger_upper REAL,
            bollinger_middle REAL,
            bollinger_lower REAL,
            range_52w_low REAL,
            range_52w_high REAL,
            range_52w_position REAL,
            volume_avg_20 REAL,
            volume_ratio_20 REAL,
            volume_regime TEXT,
            above_sma_20 INTEGER,
            above_sma_50 INTEGER,
            above_sma_200 INTEGER,
            computed_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (symbol, timeframe, computed_at)
        );
        CREATE INDEX IF NOT EXISTS idx_technical_snapshots_symbol_tf
            ON technical_snapshots(symbol, timeframe, computed_at DESC);

        CREATE TABLE IF NOT EXISTS alignment_score_history (
            date TEXT PRIMARY KEY,
            total_alignment_score REAL CHECK(total_alignment_score BETWEEN 0 AND 100),
            components TEXT NOT NULL DEFAULT '[]',
            divergent_assets TEXT NOT NULL DEFAULT '[]',
            regime_state TEXT CHECK(regime_state IN ('high-alignment','mixed','divergent')),
            computed_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_alignment_score_history_date
            ON alignment_score_history(date DESC);

        -- Real-yields curve ingestion (TIPS, breakevens, G10 sovereign 10Y).
        -- Yields are basis-point-precision rates, not money, so REAL is OK here
        -- per the code-standards exception for interest-rate series.
        CREATE TABLE IF NOT EXISTS real_yields_history (
            date TEXT NOT NULL,
            series TEXT NOT NULL,
            value REAL NOT NULL,
            source TEXT NOT NULL,
            fetched_at TEXT NOT NULL,
            PRIMARY KEY (date, series)
        );
        CREATE INDEX IF NOT EXISTS idx_real_yields_history_series_date
            ON real_yields_history(series, date DESC);
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

    // Migration: add OHLC columns to price_history (F48 — OHLCV history)
    let has_open: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('price_history') WHERE name = 'open'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;

    if !has_open {
        conn.execute_batch(
            "ALTER TABLE price_history ADD COLUMN open TEXT;
             ALTER TABLE price_history ADD COLUMN high TEXT;
             ALTER TABLE price_history ADD COLUMN low TEXT;",
        )?;
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

    // Migration: add lessons_applied JSON text to user_predictions
    let has_lessons_applied: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('user_predictions') WHERE name = 'lessons_applied'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_lessons_applied {
        conn.execute_batch(
            "ALTER TABLE user_predictions ADD COLUMN lessons_applied TEXT NOT NULL DEFAULT '[]'",
        )?;
    }

    // Migration: add news-source attribution to user_predictions.
    let has_prediction_topic: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('user_predictions') WHERE name = 'topic'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_prediction_topic {
        conn.execute_batch(
            "ALTER TABLE user_predictions ADD COLUMN topic TEXT NOT NULL DEFAULT 'other'
                CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other'))",
        )?;
    }

    let has_source_article_id: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('user_predictions') WHERE name = 'source_article_id'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_source_article_id {
        conn.execute_batch("ALTER TABLE user_predictions ADD COLUMN source_article_id INTEGER")?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_user_predictions_topic ON user_predictions(topic);
         CREATE INDEX IF NOT EXISTS idx_user_predictions_source_article
            ON user_predictions(source_article_id);",
    )?;
    crate::db::news_source_accuracy::ensure_tables(conn)?;

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
    crate::db::news_topic_markets::ensure_tables(conn)?;
    crate::db::news_topic_markets::ensure_news_cache_topic_column(conn)?;
    crate::db::news_topic_markets::backfill_news_cache_topics(conn)?;

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
    crate::db::news_cache::ensure_source_tier_tables(conn)?;

    let has_alert_condition: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('alerts') WHERE name = 'condition'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_alert_condition {
        conn.execute_batch("ALTER TABLE alerts ADD COLUMN condition TEXT")?;
    }

    let has_alert_recurring: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('alerts') WHERE name = 'recurring'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_alert_recurring {
        conn.execute_batch("ALTER TABLE alerts ADD COLUMN recurring INTEGER NOT NULL DEFAULT 0")?;
    }

    let has_alert_cooldown: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('alerts') WHERE name = 'cooldown_minutes'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_alert_cooldown {
        conn.execute_batch(
            "ALTER TABLE alerts ADD COLUMN cooldown_minutes INTEGER NOT NULL DEFAULT 0",
        )?;
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS triggered_alerts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            alert_id INTEGER NOT NULL,
            triggered_at TEXT NOT NULL DEFAULT (datetime('now')),
            trigger_data TEXT NOT NULL DEFAULT '{}',
            acknowledged INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS idx_triggered_alerts_triggered_at
           ON triggered_alerts(triggered_at);",
    )?;

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

    // F46: Stored market structure and key levels
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS technical_levels (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            level_type TEXT NOT NULL,
            price REAL NOT NULL,
            strength REAL NOT NULL DEFAULT 0.5,
            source_method TEXT NOT NULL,
            timeframe TEXT NOT NULL DEFAULT '1d',
            notes TEXT,
            computed_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_technical_levels_symbol
            ON technical_levels(symbol);
        CREATE INDEX IF NOT EXISTS idx_technical_levels_type
            ON technical_levels(symbol, level_type);",
    )?;

    // F49: Precomputed technical signals
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS technical_signals (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            signal_type TEXT NOT NULL,
            direction TEXT NOT NULL,
            severity TEXT NOT NULL,
            trigger_price REAL,
            description TEXT NOT NULL,
            timeframe TEXT NOT NULL DEFAULT '1d',
            detected_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_technical_signals_symbol
            ON technical_signals(symbol);
        CREATE INDEX IF NOT EXISTS idx_technical_signals_type
            ON technical_signals(signal_type);
        CREATE INDEX IF NOT EXISTS idx_technical_signals_detected
            ON technical_signals(detected_at);",
    )?;

    // Broker connections registry
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS broker_connections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            broker_name TEXT NOT NULL UNIQUE,
            account_id TEXT,
            label TEXT,
            last_sync_at TEXT,
            sync_status TEXT NOT NULL DEFAULT 'configured',
            sync_error TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    // Migration: add OHLCV-aware ATR columns to technical_snapshots (F48 step 2)
    let has_atr_14: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('technical_snapshots') WHERE name = 'atr_14'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_atr_14 {
        conn.execute_batch(
            "ALTER TABLE technical_snapshots ADD COLUMN atr_14 REAL;
             ALTER TABLE technical_snapshots ADD COLUMN atr_ratio REAL;
             ALTER TABLE technical_snapshots ADD COLUMN range_expansion INTEGER;
             ALTER TABLE technical_snapshots ADD COLUMN day_range_ratio REAL;",
        )?;
    }

    // Migration: add source and confidence columns to economic_data
    let has_source: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('economic_data') WHERE name = 'source'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_source {
        conn.execute_batch(
            "ALTER TABLE economic_data ADD COLUMN source TEXT NOT NULL DEFAULT 'unknown';
             ALTER TABLE economic_data ADD COLUMN confidence TEXT NOT NULL DEFAULT 'medium';",
        )?;
    }

    // Migration: add quarantined flag to economic_data (sanity-check
    // quarantine for implausible scraped indicator values).
    let has_quarantined: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('economic_data') WHERE name = 'quarantined'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_quarantined {
        conn.execute_batch(
            "ALTER TABLE economic_data ADD COLUMN quarantined INTEGER NOT NULL DEFAULT 0",
        )?;
    }

    // Migration: add previous_close to price_cache (movers P0 fix)
    let has_previous_close: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('price_cache') WHERE name = 'previous_close'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_previous_close {
        conn.execute_batch("ALTER TABLE price_cache ADD COLUMN previous_close TEXT")?;
    }

    // Migration: add paired transaction linkage for auto cash legs.
    let has_paired_tx_id: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('transactions') WHERE name = 'paired_tx_id'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_paired_tx_id {
        conn.execute_batch(
            "ALTER TABLE transactions ADD COLUMN paired_tx_id INTEGER REFERENCES transactions(id)",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_transactions_paired_tx_id
            ON transactions(paired_tx_id);",
    )?;

    // F53: Situation Engine — add phase/resolved columns to scenarios
    let has_scenario_phase: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('scenarios') WHERE name = 'phase'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_scenario_phase {
        conn.execute_batch(
            "ALTER TABLE scenarios ADD COLUMN phase TEXT NOT NULL DEFAULT 'hypothesis';
             ALTER TABLE scenarios ADD COLUMN resolved_at TEXT;
             ALTER TABLE scenarios ADD COLUMN resolution_notes TEXT;",
        )?;
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_scenarios_phase ON scenarios(phase);")?;

    // F53: Situation Engine — new tables
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS scenario_branches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            probability REAL NOT NULL DEFAULT 0.0,
            description TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE (scenario_id, name)
        );
        CREATE INDEX IF NOT EXISTS idx_scenario_branches_scenario ON scenario_branches(scenario_id);

        CREATE TABLE IF NOT EXISTS scenario_impacts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            branch_id INTEGER REFERENCES scenario_branches(id) ON DELETE CASCADE,
            symbol TEXT NOT NULL,
            direction TEXT NOT NULL,
            tier TEXT NOT NULL DEFAULT 'primary',
            mechanism TEXT,
            parent_id INTEGER REFERENCES scenario_impacts(id) ON DELETE SET NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_scenario_impacts_scenario ON scenario_impacts(scenario_id);
        CREATE INDEX IF NOT EXISTS idx_scenario_impacts_symbol ON scenario_impacts(symbol);
        CREATE INDEX IF NOT EXISTS idx_scenario_impacts_parent ON scenario_impacts(parent_id);

        CREATE TABLE IF NOT EXISTS scenario_indicators (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            branch_id INTEGER REFERENCES scenario_branches(id) ON DELETE CASCADE,
            impact_id INTEGER REFERENCES scenario_impacts(id) ON DELETE SET NULL,
            symbol TEXT NOT NULL,
            metric TEXT NOT NULL DEFAULT 'close',
            operator TEXT NOT NULL,
            threshold TEXT NOT NULL,
            label TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'watching',
            triggered_at TEXT,
            last_value TEXT,
            last_checked TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_scenario_indicators_scenario ON scenario_indicators(scenario_id);
        CREATE INDEX IF NOT EXISTS idx_scenario_indicators_symbol ON scenario_indicators(symbol);
        CREATE INDEX IF NOT EXISTS idx_scenario_indicators_status ON scenario_indicators(status);

        CREATE TABLE IF NOT EXISTS scenario_updates (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            branch_id INTEGER REFERENCES scenario_branches(id) ON DELETE CASCADE,
            headline TEXT NOT NULL,
            detail TEXT,
            severity TEXT NOT NULL DEFAULT 'normal',
            source TEXT,
            source_agent TEXT,
            next_decision TEXT,
            next_decision_at TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_scenario_updates_scenario ON scenario_updates(scenario_id);
        CREATE INDEX IF NOT EXISTS idx_scenario_updates_created ON scenario_updates(created_at);",
    )?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS power_flows (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            date            TEXT NOT NULL,
            event           TEXT NOT NULL,
            source_complex  TEXT NOT NULL,
            direction       TEXT NOT NULL,
            target_complex  TEXT,
            evidence        TEXT NOT NULL,
            magnitude       INTEGER NOT NULL CHECK(magnitude BETWEEN 1 AND 5),
            agent_source    TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_power_flows_date ON power_flows(date);
        CREATE INDEX IF NOT EXISTS idx_power_flows_complex ON power_flows(source_complex);",
    )?;

    // Normalized scenario-set model — guarantee an `Other / Unmodelled`
    // system-managed residual row exists. The row tracks
    // `100 - sum(active modeled scenarios)` and is recomputed deterministically
    // by `crate::db::scenarios::recompute_residual_scenario` on every mutation.
    // See `docs/ANALYTICS-SPEC.md` (Scenario Probability Semantics) for the
    // model. This block is idempotent: re-running just refreshes the status
    // marker / probability.
    //
    // Legacy DBs (pre-v0.28) may be missing some columns that this insert and
    // the surrounding scenarios CRUD depend on. Add any missing columns first
    // so the migration succeeds against the pinned prior-release fixture.
    for (column, ddl) in &[
        ("status", "ALTER TABLE scenarios ADD COLUMN status TEXT NOT NULL DEFAULT 'active'"),
        ("asset_impact", "ALTER TABLE scenarios ADD COLUMN asset_impact TEXT"),
        ("triggers", "ALTER TABLE scenarios ADD COLUMN triggers TEXT"),
        ("historical_precedent", "ALTER TABLE scenarios ADD COLUMN historical_precedent TEXT"),
    ] {
        let exists: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('scenarios') WHERE name = ?1")?
            .query_row([column], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !exists {
            conn.execute_batch(ddl)?;
        }
    }

    {
        let residual_exists: bool = conn
            .prepare("SELECT COUNT(*) FROM scenarios WHERE name = ?1")?
            .query_row([crate::db::scenarios::RESIDUAL_SCENARIO_NAME], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_or(0)
            > 0;
        if !residual_exists {
            // Seed at 100 — when no modeled scenarios exist the residual fills
            // the whole probability space.
            conn.execute(
                "INSERT INTO scenarios (name, probability, description, status, phase)
                 VALUES (?1, 100.0, ?2, ?3, 'active')",
                rusqlite::params![
                    crate::db::scenarios::RESIDUAL_SCENARIO_NAME,
                    "System-managed residual: 100 - sum(active modeled scenarios). Represents outcomes outside the named scenario set.",
                    crate::db::scenarios::RESIDUAL_SCENARIO_STATUS,
                ],
            )?;
        } else {
            // Refresh the status marker on legacy DBs that may have created the
            // row with the default 'active' status.
            conn.execute(
                "UPDATE scenarios SET status = ?1 WHERE name = ?2 AND status != ?1",
                rusqlite::params![
                    crate::db::scenarios::RESIDUAL_SCENARIO_STATUS,
                    crate::db::scenarios::RESIDUAL_SCENARIO_NAME,
                ],
            )?;
        }
        // Recompute against current modeled rows so a freshly-migrated DB
        // already exposes the correct residual.
        crate::db::scenarios::recompute_residual_scenario(conn)?;
    }

    // F55.4: Scenario-contract mappings — link prediction market contracts to scenarios
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS scenario_contract_mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            contract_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(scenario_id, contract_id)
        );
        CREATE INDEX IF NOT EXISTS idx_scm_scenario ON scenario_contract_mappings(scenario_id);
        CREATE INDEX IF NOT EXISTS idx_scm_contract ON scenario_contract_mappings(contract_id);",
    )?;

    let allocation_targets_has_floor: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('allocation_targets') \
             WHERE name = 'target_floor_pct'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !allocation_targets_has_floor {
        conn.execute_batch(
            "ALTER TABLE allocation_targets ADD COLUMN target_floor_pct TEXT NOT NULL DEFAULT '0'",
        )?;
    }

    let allocation_targets_has_ceiling: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('allocation_targets') \
             WHERE name = 'target_ceiling_pct'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !allocation_targets_has_ceiling {
        conn.execute_batch(
            "ALTER TABLE allocation_targets ADD COLUMN target_ceiling_pct TEXT NOT NULL DEFAULT '0'",
        )?;
    }

    conn.execute_batch(
        "UPDATE allocation_targets
         SET target_floor_pct = CAST(
                CAST(COALESCE(NULLIF(target_pct, ''), '0') AS REAL)
              - CAST(COALESCE(NULLIF(drift_band_pct, ''), '0') AS REAL)
              AS TEXT)
         WHERE target_floor_pct = '0' OR target_floor_pct = '';

         UPDATE allocation_targets
         SET target_ceiling_pct = CAST(
                CAST(COALESCE(NULLIF(target_pct, ''), '0') AS REAL)
              + CAST(COALESCE(NULLIF(drift_band_pct, ''), '0') AS REAL)
              AS TEXT)
         WHERE target_ceiling_pct = '0' OR target_ceiling_pct = '';",
    )?;

    // Migrate PPI series from PPIACO (All Commodities) to PPIFIS (Final Demand).
    // PPIFIS matches the headline PPI figure reported by BLS.
    // PPIACO and PPIFIS have different index levels, so we can't copy values —
    // just delete PPIACO data and let the next FRED refresh populate PPIFIS.
    conn.execute_batch(
        "DELETE FROM economic_cache WHERE series_id = 'PPIACO';
         DELETE FROM macro_events WHERE series_id = 'PPIACO';",
    )
    .ok(); // Ignore errors (table may not exist on first run, or data already migrated)

    // Migration: add `author` column to journal and daily_notes, then backfill
    // existing rows by parsing content-prefix conventions used by the
    // timeframe-analyst routines (LOW/MEDIUM/HIGH/MACRO/EVENING/MORNING/NIGHT SHIFT).
    let journal_has_author: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('journal') WHERE name = 'author'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !journal_has_author {
        conn.execute_batch("ALTER TABLE journal ADD COLUMN author TEXT NOT NULL DEFAULT 'system'")?;
        backfill_author_column(conn, "journal")?;
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_journal_author ON journal(author)")?;

    let daily_notes_has_author: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('daily_notes') WHERE name = 'author'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !daily_notes_has_author {
        conn.execute_batch(
            "ALTER TABLE daily_notes ADD COLUMN author TEXT NOT NULL DEFAULT 'system'",
        )?;
        backfill_author_column(conn, "daily_notes")?;
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_daily_notes_author ON daily_notes(author)")?;

    // Migration: lesson half-life curation (status + last_cited_at on
    // prediction_lessons, plus lesson_citations table).
    //
    // The prediction_lessons table is created lazily by
    // `crate::db::prediction_lessons::ensure_table`; we mirror its CREATE
    // here so the ALTER TABLE migrations below have a target on fresh DBs.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS prediction_lessons (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prediction_id INTEGER NOT NULL UNIQUE,
            miss_type TEXT NOT NULL,
            what_predicted TEXT NOT NULL,
            what_happened TEXT NOT NULL,
            why_wrong TEXT NOT NULL,
            signal_misread TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (prediction_id) REFERENCES user_predictions(id)
        );
        CREATE INDEX IF NOT EXISTS idx_prediction_lessons_pid
            ON prediction_lessons(prediction_id);",
    )?;

    let prediction_lessons_has_status: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('prediction_lessons') WHERE name = 'status'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !prediction_lessons_has_status {
        // SQLite ALTER TABLE ADD COLUMN cannot embed a multi-value CHECK
        // constraint with subqueries; the inline CHECK below is allowed
        // because it only references the new column's value.
        conn.execute_batch(
            "ALTER TABLE prediction_lessons ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
                CHECK(status IN ('active','retired','superseded'))",
        )?;
    }
    let prediction_lessons_has_last_cited_at: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('prediction_lessons') WHERE name = 'last_cited_at'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !prediction_lessons_has_last_cited_at {
        conn.execute_batch(
            "ALTER TABLE prediction_lessons ADD COLUMN last_cited_at TEXT",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_prediction_lessons_status
            ON prediction_lessons(status);
         CREATE INDEX IF NOT EXISTS idx_prediction_lessons_last_cited_at
            ON prediction_lessons(last_cited_at);",
    )?;

    // Ensure the lesson_citations table exists. It was originally created
    // during a live-DB enrichment session; this CREATE makes it available
    // on every fresh install for the lesson half-life curation routine.
    crate::db::lesson_citations::ensure_table(conn)?;

    // Migration: add cluster_key to prediction_lessons (live-DB enrichment).
    // Used by the `pftui analytics clusters` and `analytics fragments
    // --for-claim` surfaces to group lessons by taxonomic cluster.
    let prediction_lessons_has_cluster_key: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('prediction_lessons') WHERE name = 'cluster_key'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !prediction_lessons_has_cluster_key {
        conn.execute_batch(
            "ALTER TABLE prediction_lessons ADD COLUMN cluster_key TEXT",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_prediction_lessons_cluster_key
            ON prediction_lessons(cluster_key);",
    )?;

    // Enrichment tables shipped via live-DB sessions (June 1 enrichment pass).
    // Schemas are mirrored here verbatim so fresh installs pick them up
    // and the corresponding `pftui analytics` / `pftui journal` CLIs work
    // from day one.
    crate::db::sources_registry::ensure_table(conn)?;
    crate::db::event_annotations::ensure_table(conn)?;
    crate::db::reasoning_fragments::ensure_table(conn)?;
    crate::db::adversary_views::ensure_table(conn)?;
    crate::db::adversary_synthesis_views::ensure_table(conn)?;
    crate::db::calibration_adjustments::ensure_table(conn)?;
    crate::db::capital_flows::ensure_table(conn)?;
    crate::db::failure_correlations::ensure_table(conn)?;
    crate::db::thesis_dependencies::ensure_table(conn)?;
    crate::db::operator_replies::ensure_table(conn)?;
    crate::db::recommendations::ensure_table(conn)?;
    crate::db::prediction_falsification_rules::ensure_table(conn)?;
    crate::db::regime_history::ensure_table(conn)?;

    // Additional live-DB enrichment tables that are not yet managed by
    // dedicated modules. The `pftui system data-coverage` audit references
    // them; CREATE TABLE IF NOT EXISTS makes a fresh `pftui` install
    // schema-complete. Column shapes are conservative; any agent populating
    // these tables can ALTER TABLE later without conflict.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS calibration_matrix (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            layer TEXT,
            topic TEXT,
            conviction_band TEXT,
            n INTEGER NOT NULL DEFAULT 0,
            hit_rate REAL NOT NULL DEFAULT 0.0,
            stated_confidence REAL,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS risk_factor_mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            factor TEXT NOT NULL,
            direction TEXT NOT NULL DEFAULT 'long',
            exposure_multiplier REAL NOT NULL DEFAULT 1.0,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(symbol, factor)
        );
        CREATE INDEX IF NOT EXISTS idx_risk_factor_mappings_symbol
            ON risk_factor_mappings(symbol);
        CREATE INDEX IF NOT EXISTS idx_risk_factor_mappings_factor
            ON risk_factor_mappings(factor);

        CREATE TABLE IF NOT EXISTS scenario_prediction_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL,
            prediction_id INTEGER NOT NULL,
            link_kind TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(scenario_id, prediction_id)
        );

        CREATE TABLE IF NOT EXISTS lesson_fragment_edges (
            lesson_id INTEGER NOT NULL,
            fragment_id INTEGER NOT NULL,
            edge_weight REAL NOT NULL DEFAULT 1.0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (lesson_id, fragment_id)
        );

        CREATE TABLE IF NOT EXISTS thesis_citations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            thesis_id INTEGER NOT NULL,
            source_type TEXT NOT NULL,
            source_id INTEGER,
            citation_text TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS conviction_durability (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prediction_id INTEGER NOT NULL,
            window_days INTEGER NOT NULL,
            conviction_drift REAL NOT NULL DEFAULT 0.0,
            note TEXT,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS options_chain_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            strike REAL NOT NULL,
            expiry TEXT NOT NULL,
            dte INTEGER NOT NULL,
            oi_calls INTEGER NOT NULL,
            oi_puts INTEGER NOT NULL,
            vol_calls INTEGER NOT NULL,
            vol_puts INTEGER NOT NULL,
            iv_atm REAL,
            fetched_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_options_chain_snapshots_symbol_fetched
            ON options_chain_snapshots(symbol, fetched_at DESC);

        CREATE TABLE IF NOT EXISTS gex_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            gex_flip_strike REAL,
            total_gamma_call REAL NOT NULL,
            total_gamma_put REAL NOT NULL,
            max_pain REAL,
            fetched_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_gex_snapshots_symbol_fetched
            ON gex_snapshots(symbol, fetched_at DESC);",
    )?;

    // Migration: rebuild a drifted `calibration_matrix` to the canonical shape.
    //
    // Legacy DBs created the table with the old scorer shape —
    // `PRIMARY KEY(layer, topic, conviction, window_days)` and a
    // `conviction TEXT NOT NULL` column — and a prior additive migration
    // (#877) appended the canonical analytic columns alongside the legacy
    // ones. That hybrid still breaks `pftui analytics calibration-matrix
    // rebuild`: the INSERT populates only the canonical columns, so the
    // legacy NOT-NULL `conviction` column (no default) fails with
    // "NOT NULL constraint failed: calibration_matrix.conviction".
    //
    // Detect drift (a legacy `conviction` column, or a missing `id` rowid
    // column) and rebuild the table in place to the canonical CREATE above,
    // preserving rows with a best-effort legacy→canonical column mapping.
    // Idempotent: a canonical table has `id` and no `conviction`, so the
    // rebuild never re-fires.
    rebuild_drifted_calibration_matrix(conn)?;

    // Migration: self-heal `calibration_matrix` on legacy DBs.
    //
    // The CREATE TABLE IF NOT EXISTS above is the canonical shape, but DBs
    // created before `conviction_band` (and the other analytic columns) were
    // added to that CREATE still have the old shape on disk — CREATE TABLE IF
    // NOT EXISTS never adds columns to an existing table. Without this,
    // `pftui analytics calibration-matrix rebuild` fails at the INSERT with
    // "table calibration_matrix has no column named conviction_band".
    //
    // Add any missing columns from the canonical set. Idempotent via
    // pragma_table_info check, mirroring the `scenarios` migration above.
    for (column, ddl) in &[
        ("layer", "ALTER TABLE calibration_matrix ADD COLUMN layer TEXT"),
        ("topic", "ALTER TABLE calibration_matrix ADD COLUMN topic TEXT"),
        ("conviction_band", "ALTER TABLE calibration_matrix ADD COLUMN conviction_band TEXT"),
        ("n", "ALTER TABLE calibration_matrix ADD COLUMN n INTEGER NOT NULL DEFAULT 0"),
        ("hit_rate", "ALTER TABLE calibration_matrix ADD COLUMN hit_rate REAL NOT NULL DEFAULT 0.0"),
        ("stated_confidence", "ALTER TABLE calibration_matrix ADD COLUMN stated_confidence REAL"),
        (
            "recorded_at",
            "ALTER TABLE calibration_matrix ADD COLUMN recorded_at TEXT NOT NULL DEFAULT (datetime('now'))",
        ),
    ] {
        let exists: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('calibration_matrix') WHERE name = ?1")?
            .query_row([column], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !exists {
            conn.execute_batch(ddl)?;
        }
    }

    // Migration: normalize analyst-view conviction signs (direction is
    // authoritative). Historical rows were written with positive conviction
    // magnitudes on bear views (e.g. `--direction bear --conviction 3` stored
    // +3), which flipped convergence classification bullish. Idempotent:
    // after one pass no row matches either predicate.
    normalize_analyst_view_conviction_signs(conn)?;

    // Migration (epistemics R4): scenario base rates. Additive — idempotent
    // via pragma_table_info check.
    for (column, ddl) in &[
        ("base_rate", "ALTER TABLE scenarios ADD COLUMN base_rate REAL"),
        (
            "base_rate_reference",
            "ALTER TABLE scenarios ADD COLUMN base_rate_reference TEXT",
        ),
    ] {
        let exists: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('scenarios') WHERE name = ?1")?
            .query_row([column], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !exists {
            conn.execute_batch(ddl)?;
        }
    }

    // Migration (epistemics R4): scenario probability ledger columns on
    // scenario_updates. Every probability update is recorded here with the
    // proposing layer, the evidence cited, the old→new probability move, and
    // (when the daily delta cap was bypassed) the hard data print that
    // justified it. Additive — idempotent via pragma_table_info check.
    for (column, ddl) in &[
        (
            "proposer",
            "ALTER TABLE scenario_updates ADD COLUMN proposer TEXT",
        ),
        (
            "evidence",
            "ALTER TABLE scenario_updates ADD COLUMN evidence TEXT",
        ),
        (
            "old_probability",
            "ALTER TABLE scenario_updates ADD COLUMN old_probability REAL",
        ),
        (
            "new_probability",
            "ALTER TABLE scenario_updates ADD COLUMN new_probability REAL",
        ),
        (
            "hard_print_event",
            "ALTER TABLE scenario_updates ADD COLUMN hard_print_event TEXT",
        ),
    ] {
        let exists: bool = conn
            .prepare(
                "SELECT COUNT(*) FROM pragma_table_info('scenario_updates') WHERE name = ?1",
            )?
            .query_row([column], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !exists {
            conn.execute_batch(ddl)?;
        }
    }

    // Migration (epistemics R4): run_health — one row per report run with
    // the epistemic-health instrumentation (echo risk, blind divergence,
    // panel dispersion, novelty, fallbacks, scenario churn, audit pass rate).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS run_health (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_date TEXT NOT NULL,
            agreement_rate REAL,
            blind_divergence REAL,
            panel_dispersion REAL,
            novelty_rate REAL,
            fallback_warnings INTEGER,
            scenario_delta_total REAL,
            audit_pass_rate REAL,
            agents_spawned INTEGER,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_run_health_run_date
            ON run_health(run_date);",
    )?;

    Ok(())
}

/// Rebuild `calibration_matrix` to the canonical shape when the on-disk
/// table has drifted (legacy `conviction TEXT NOT NULL` column and/or a
/// `PRIMARY KEY(layer, topic, conviction, window_days)` without the `id`
/// rowid column). Rows are preserved with a best-effort legacy→canonical
/// column mapping:
///
///   conviction_band   ← conviction_band, else legacy `conviction`
///   n                 ← n (when non-zero), else legacy `n_scored`
///   hit_rate          ← hit_rate (when non-zero), else legacy `strict_hit_rate`
///   stated_confidence ← stated_confidence, else legacy `avg_confidence`
///   recorded_at       ← recorded_at, else legacy `computed_at`, else now
fn rebuild_drifted_calibration_matrix(conn: &Connection) -> Result<()> {
    let column_exists = |name: &str| -> Result<bool> {
        let n: i64 = conn
            .prepare(
                "SELECT COUNT(*) FROM pragma_table_info('calibration_matrix') WHERE name = ?1",
            )?
            .query_row([name], |row| row.get(0))
            .unwrap_or(0);
        Ok(n > 0)
    };

    let has_conviction = column_exists("conviction")?;
    let has_id = column_exists("id")?;
    if !has_conviction && has_id {
        // Already canonical (or close enough for the additive column loop).
        return Ok(());
    }

    // Build a SELECT that only references columns present on the drifted table.
    let expr = |canonical: &str, legacy_fallbacks: &[&str], default: &str| -> Result<String> {
        let mut parts: Vec<String> = Vec::new();
        if column_exists(canonical)? {
            // NULLIF(x, default) lets a legacy twin win over an appended
            // never-populated DEFAULT column for numeric counters.
            if default == "0" || default == "0.0" {
                parts.push(format!("NULLIF({canonical}, {default})"));
            } else {
                parts.push(canonical.to_string());
            }
        }
        for legacy in legacy_fallbacks {
            if column_exists(legacy)? {
                parts.push((*legacy).to_string());
            }
        }
        parts.push(default.to_string());
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            format!("COALESCE({})", parts.join(", "))
        })
    };

    let layer_expr = expr("layer", &[], "NULL")?;
    let topic_expr = expr("topic", &[], "NULL")?;
    let band_expr = expr("conviction_band", &["conviction"], "NULL")?;
    let n_expr = expr("n", &["n_scored"], "0")?;
    let hit_rate_expr = expr("hit_rate", &["strict_hit_rate"], "0.0")?;
    let conf_expr = expr("stated_confidence", &["avg_confidence"], "NULL")?;
    let recorded_expr = expr("recorded_at", &["computed_at"], "datetime('now')")?;

    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS calibration_matrix_canonical_rebuild;
        CREATE TABLE calibration_matrix_canonical_rebuild (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            layer TEXT,
            topic TEXT,
            conviction_band TEXT,
            n INTEGER NOT NULL DEFAULT 0,
            hit_rate REAL NOT NULL DEFAULT 0.0,
            stated_confidence REAL,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        INSERT INTO calibration_matrix_canonical_rebuild
            (layer, topic, conviction_band, n, hit_rate, stated_confidence, recorded_at)
        SELECT {layer_expr}, {topic_expr}, {band_expr}, COALESCE({n_expr}, 0),
               COALESCE({hit_rate_expr}, 0.0), {conf_expr}, {recorded_expr}
        FROM calibration_matrix;
        DROP TABLE calibration_matrix;
        ALTER TABLE calibration_matrix_canonical_rebuild RENAME TO calibration_matrix;"
    ))?;
    Ok(())
}

/// One-time (idempotent) sign normalization for `analyst_views` and
/// `analyst_view_history`: direction is authoritative, so bear views must
/// carry negative conviction and bull views positive conviction. Neutral
/// rows are left untouched. Tables are created lazily by the analyst-views
/// module, so skip silently when absent.
fn normalize_analyst_view_conviction_signs(conn: &Connection) -> Result<()> {
    for table in ["analyst_views", "analyst_view_history"] {
        let exists: i64 = conn
            .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1")?
            .query_row([table], |row| row.get(0))
            .unwrap_or(0);
        if exists == 0 {
            continue;
        }
        conn.execute(
            &format!(
                "UPDATE {table} SET conviction = -conviction
                 WHERE direction = 'bear' AND conviction > 0"
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "UPDATE {table} SET conviction = -conviction
                 WHERE direction = 'bull' AND conviction < 0"
            ),
            [],
        )?;
    }
    Ok(())
}

/// Backfill the `author` column on a journal-like table by inspecting the
/// `content` column for the historical prefix conventions used by the
/// timeframe-analyst routines. Anything that does not match a known prefix
/// keeps the table default ('system').
///
/// Prefix rules (case-insensitive, anchored at line start of `content`):
///   ^LOW(\s|:|\s+\w)        -> analyst-low
///   ^MEDIUM(\s|:|\s+\w)     -> analyst-medium
///   ^HIGH(\s|:|\s+\w)       -> analyst-high
///   ^MACRO(\s|:|\s+\w)      -> analyst-macro
///   ^EVENING(\s|:|\s+\w)    -> analyst-evening
///   ^MORNING(\s|:|\s+\w)    -> analyst-morning
///   ^NIGHT[ -]SHIFT         -> analyst-night-shift
fn backfill_author_column(conn: &Connection, table: &str) -> Result<()> {
    // Match `LOW ...`, `LOW:`, `LOW WRONG ...` etc. — i.e. the keyword followed
    // by whitespace, colon, or a comma. Use SQLite-native pattern matching so
    // we don't have to load the whole table into Rust. UPPER() gives the
    // case-insensitive match; the leading-prefix anchor is implicit via LIKE
    // 'KEYWORD%'.
    let backfills: &[(&str, &[&str])] = &[
        (
            "analyst-low",
            &["LOW %", "LOW:%", "LOW,%", "LOW\n%", "LOW\t%"],
        ),
        (
            "analyst-medium",
            &["MEDIUM %", "MEDIUM:%", "MEDIUM,%", "MEDIUM\n%", "MEDIUM\t%"],
        ),
        (
            "analyst-high",
            &["HIGH %", "HIGH:%", "HIGH,%", "HIGH\n%", "HIGH\t%"],
        ),
        (
            "analyst-macro",
            &["MACRO %", "MACRO:%", "MACRO,%", "MACRO\n%", "MACRO\t%"],
        ),
        (
            "analyst-evening",
            &[
                "EVENING %",
                "EVENING:%",
                "EVENING,%",
                "EVENING\n%",
                "EVENING\t%",
            ],
        ),
        (
            "analyst-morning",
            &[
                "MORNING %",
                "MORNING:%",
                "MORNING,%",
                "MORNING\n%",
                "MORNING\t%",
            ],
        ),
        ("analyst-night-shift", &["NIGHT SHIFT%", "NIGHT-SHIFT%"]),
    ];

    for (author, patterns) in backfills {
        for pattern in *patterns {
            let sql = format!(
                "UPDATE {table} SET author = ?1 \
                 WHERE author = 'system' AND UPPER(content) LIKE ?2"
            );
            conn.execute(&sql, rusqlite::params![author, pattern])?;
        }
    }

    Ok(())
}
