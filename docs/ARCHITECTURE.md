# Architecture — pftui

Quick-reference for automated agents. Read this FIRST, then only open the files your task needs.
Use `read --offset N --limit M` to read specific line ranges instead of full files.

## app.rs Line Map (6000 lines — DO NOT read in full)

| Lines | Section | When to read |
|-------|---------|-------------|
| 1-285 | Imports, enums (ViewMode, SortField, ChartVariant, MainTab) | Adding views/sort modes/chart types |
| 286-492 | `struct App` fields | Adding new state |
| 493-847 | `App::new()`, `init()`, `init_offline()`, price update handlers | Startup/init changes |
| 848-980 | `recompute_regime()` | Regime signal changes |
| 980-1040 | `compute_portfolio_value_history()` (LOCF forward-fill) | Portfolio chart bugs |
| 1041-1138 | `compute_daily_change()`, `compute_timeframe_gains()` | Gain/loss calculations |
| 1139-1314 | `chart_variants_for_position()` | Chart ratio/variant bugs |
| 1315-1397 | `tick()` — 60fps loop, animation counters | Animation/tick changes |
| 1398-1733 | `handle_key()` — ALL keybindings | Adding/changing keys |
| 1734-2820 | `handle_mouse()`, helpers, sorting, filtering | Mouse/sort/filter changes |
| 2821+ | `#[cfg(test)]` blocks (~3100 lines) | NEVER read unless writing tests |

## price_chart.rs Line Map (1970 lines)

| Lines | Section | When to read |
|-------|---------|-------------|
| 1-40 | Imports, `slice_history()` | — |
| 41-125 | `render()` — main dispatch (single/ratio/multi) | Chart layout changes |
| 126-440 | `render_multi_panel`, `render_single_chart`, `render_ratio_chart`, minis | Specific chart type changes |
| 440-540 | `compute_ratio()`, `compute_sma()`, `compute_bollinger()` | Technical indicator changes |
| 540-870 | `render_braille_chart()` — core braille renderer, SMA/BB overlays, crosshair, area fill | Chart rendering changes |
| 871-990 | `render_braille_mini()`, `area_fill_bg()` | Mini chart / fill changes |
| 991+ | `render_braille_lines()` (embeddable), tests | Embedding charts elsewhere |

## asset_detail_popup.rs Line Map (850 lines)

| Lines | Section | When to read |
|-------|---------|-------------|
| 1-25 | AssetDetailState struct, `render()` dispatch | Adding popup state |
| 25-117 | `render()` — popup frame, shadow, scroll | Popup layout changes |
| 117-708 | `build_lines()` — info, price, chart, technicals, portfolio context | Adding/changing popup sections |
| 709+ | Tests | NEVER read unless writing tests |

## positions.rs Line Map (1450 lines)

| Lines | Section | When to read |
|-------|---------|-------------|
| 1-220 | Helpers: 52W range, change%, row_background, category dividers | Row rendering helpers |
| 218-393 | `render()` dispatch, `render_full_table()` | Full table layout |
| 394-770 | `render_privacy_table()`, watchlist rendering | Privacy/watchlist changes |
| 771+ | Tests | NEVER read unless writing tests |

## Quick Reference

| Task | Read these files (with line ranges) |
|------|-------------------------------------|
| Fix keybinding | `app.rs:1398-1733` |
| Fix chart ratio | `app.rs:1139-1314` |
| Fix portfolio chart | `app.rs:980-1040` |
| Add new widget | `tui/widgets/new.rs` + parent view + `widgets/mod.rs` |
| Add CLI command | `cli.rs` + `commands/new.rs` + `main.rs` |
| Add view/tab | `app.rs:1-285` (ViewMode enum) + `ui.rs` + `help.rs` + `header.rs` |
| Theme changes | `theme.rs` (all 11 themes) |
| Price fetching | `price/yahoo.rs` or `price/coingecko.rs` |
| Mouse handling | `app.rs:1734-2820` |
| Add state field | `app.rs:286-492` (struct) + `app.rs:493-847` (init) |
| Technical indicators | `indicators/mod.rs` re-exports ONLY (never read individual files) |
| Asset detail popup | `asset_detail_popup.rs:117-708` (build_lines sections) |
| FX / currency work | `price/yahoo.rs` (YMetaData.currency, FX rate fetch) |

## How The System Runs (no daemon)

pftui has no resident process. The system is driven by agent sessions (on this machine: Claude Code invoking the `/pftui-report` skill and ad-hoc commands). Every recurring mechanism — prediction auto-scoring, recommendation forward-return scoring, retroactive forecast scoring, forecast-misalignment detection, regime classification, alert evaluation, housekeeping surfacing — fires in the tail of `pftui data refresh` (`src/commands/refresh.rs`, tail steps after the source fetches). `commands/daemon.rs` still ships an always-on loop but it is LEGACY/optional — it adds cadence, not capability.

## Module Index

### Analytics Layer
`analytics/situation.rs` (canonical Situation Room payload) · `analytics/deltas.rs` (server-owned change radar) · `analytics/catalysts.rs` (ranked event pressure and countdowns) · `analytics/impact.rs` (portfolio impact + opportunities) · `analytics/synthesis.rs` (cross-timeframe alignment/divergence/constraints) · `analytics/narrative.rs` (structured recap + analytical memory)

### TA & Cycle Engines (deterministic, pure over price_history)
`analytics/market_structure.rs` (swing pivots HH/HL/LH/LL, trend class, BOS, MA posture, extension gate — `analytics technicals structure`) · `analytics/cyber/` (composite Cyber Dots engine: Gaussian CyberBands + QB state machine, zone bands, CyberLine, strength dots, Bollinger reversals, Pi Cycle, MTF RSI, hybrid breakouts — `analytics technicals cyber`; PineScript reference in `docs/reference/cyber-dots.pine`) · `analytics/cycle_engine.rs` (multi-degree cycle-theory engine: dated lows, timing bands, translation ledger, FLD/VTL, doctrine in `docs/CYCLE-THEORY.md` — `analytics cycles analyze/ledger`) · `analytics/cycle_clock.rs` (BTC halving/Loukas + gold ~7yr cycle position — `analytics cycles clock`)

### Strategy Backtester (user-defined trade rules, pure over price_history — `src/analytics/strategy/`)
`analytics/strategy/parser.rs` (hand-rolled Pratt parser for the trade-rule DSL: fields `close`/`open`/`high`/`low`/`volume`, `close(SYM)`, indicators `sma`/`ema`/`rsi`, arithmetic, `> < >= <= ==`, `crosses_above`/`crosses_below`, `and`/`or`/`not`, `@weekly`/`@monthly` timeframe) · `analytics/strategy/resolver.rs` (`SeriesLoader` trait + lookahead-safe projection of any symbol/indicator/timeframe onto the asset's daily master axis: LOCF, weekly value visible only after the week closes, indicators computed over the bucket series; alias map incl. rate symbols `us10y`/`^TNX`, `fedfunds`/`^IRX`) · `analytics/strategy/eval.rs` (untyped tree → numeric/boolean series, `None`-propagating logic) · `analytics/strategy/engine.rs` (rising edge → one-position trades with `hold Nd`/exit-expr, win rate/CAGR/maxDD/time-in-market vs buy-and-hold; regime mask → segmented forward returns) · CLI in `commands/strategy.rs` — `analytics strategy backtest/segment/compare/explain`. Stateless (no tables); returns are f64 statistics over price ratios, not money. See AGENTS.md CLI reference for the full DSL.

### Research Harness (measured signal expectancy — `src/research/`)
`research/registry.rs` (AssetContext + ~27 canonical signal emitters: dated EVENTS, versioned ids) · `research/event_study.rs` (forward returns vs baseline drift, overlap exclusion, binomial significance, walk-forward `as_of`) · `research/forecast_scoring.rs` (retroactive analyst_view_history scoring at canonical horizons; runs in the `data refresh` tail) · `research/shadow_book.rs` (counterfactual SHADOW/ACTUAL/HOLD books from the recommendations ledger — `research shadowbook`) · CLI in `commands/research_harness.rs`, `commands/research_forecasts.rs`, `commands/research_dossier.rs`, `commands/shadow_book.rs` · flow doc: `docs/DATA-ARCHITECTURE.md` § Research harness

### Analyst Views (F57)
`db/analyst_views.rs` (structured per-analyst per-asset views: direction, conviction, reasoning, evidence, blind_spots) · `commands/analyst_views.rs` (set, list, matrix, delete CLI commands)

### Data Layer
`db/schema.rs` (migrations) · `db/transactions.rs` (CRUD) · `db/price_cache.rs` (spot cache) · `db/price_history.rs` (daily history, merge) · `db/technical_snapshots.rs` (persisted technical state) · `db/technical_levels.rs` (market structure levels: support, resistance, MA, swing, range) · `db/technical_signals.rs` (precomputed per-symbol signals: RSI, MACD cross, SMA reclaim, BB squeeze, volume, 52W) · `db/allocations.rs` (% mode) · `db/watchlist.rs` · `db/situation_snapshots.rs` (persisted situation baselines for delta analysis) · `db/narrative_snapshots.rs` (structured recap and analytical memory) · `db/power_flows.rs` (Dixon Power Flow Tracker: FIC/MIC/TIC power shift events, balance aggregation) · `db/prediction_lessons.rs` (structured lessons from wrong predictions: miss type, root cause, signal misread) · `db/news_topic_markets.rs` (news-topic classifier and topic→prediction-market bindings) · `db/news_source_accuracy.rs` (per-source/topic hit-rate ledger for news-derived predictions) · `db/news_silence.rs` (rolling weekday topic-volume baselines and silent/saturated regimes) · `db/debates.rs` (adversarial debate mechanism: debates + debate_rounds tables, bull/bear structured argumentation) · `db/analyst_views.rs` (F57: per-analyst per-asset structured views with direction, conviction, reasoning, evidence, blind_spots)

### Epistemics & Ledger Layer
`db/run_health.rs` (per-run epistemic instrumentation: agreement, blind divergence, panel dispersion, misalignment counts — `analytics epistemics record/show/history`) · `db/standing_rules.rs` (consolidated imperative rules from repeated lessons — `analytics lessons rules`) · `db/forecast_misalignments.rs` (wrong-sign-streak episodes; ACTIVE = probation in convergence + confidence caps — `research misalignments`, detected in the `data refresh` tail) · `db/series_registry.rs` (L1 canonical-series catalog: storage home + freshness SLA — `data series status`, doctor 2×-SLA check) · `db/signal_expectancy.rs` (L2 rebuildable expectancy cache keyed `(signal_id, signal_version, asset, horizon, as_of)` — `research backtest/expectancy/events`) · layer model + table catalog: `docs/DATA-ARCHITECTURE.md` + `docs/db-catalog.toml` (machine-enforced by `tests/schema_conformance.rs`) · epistemic doctrine: `docs/EPISTEMICS.md`

### Report Pipeline (`src/report/`)
`report/build/daily.rs` (BuildContext + section plan; `report build daily`) · `report/sections/` (one renderer per section; notable: `private_epistemic_health.rs` renders the run-health/epistemics block from `run_health` + misalignments, `private_decisions_pending.rs` renders decision cards from `agent_messages` rows with `category='decision-card'`, `adversary_view.rs` quotes adversary counter-cases verbatim) · public output is privacy-audited via `audit_public_markdown` before write

### Prior-release Schema Contract
`tests/fixtures/db/v0.27.0.sqlite` is a synthetic prior-release SQLite fixture.
`cargo test --test prior_release_schema` copies it into an isolated pftui data
directory, forces `db/schema.rs` migrations through `system db-info`, then smokes
representative CLI commands against the migrated database. Any PR that adds an
`ALTER TABLE` migration must consider whether this fixture needs a new old-state
table/column shape to exercise the migration. When a release is cut, refresh the
fixture to the previous released schema so CI keeps testing last-release to
current migrations.

`pftui system schema verify` opens the SQLite database before normal startup
migrations, compares it against a freshly migrated in-memory schema, and exits
non-zero on missing/extra/mismatched tables, columns, or missing indexes.
`pftui system schema repair --dry-run` prints safe repair SQL for missing
tables, columns, and indexes; `--confirm` applies that plan. Destructive drift
such as extra columns, extra tables, or type/default mismatches is reported but
not auto-repaired.

### Models
`models/position.rs` (Position, compute_positions) · `models/transaction.rs` (Transaction, TxType) · `models/asset.rs` (AssetCategory, PriceProvider) · `models/asset_names.rs` (130+ symbols, infer_category, search) · `models/price.rs` (PriceQuote, HistoryRecord)

### Price Service
`price/mod.rs` (PriceService thread + Tokio channels) · `price/yahoo.rs` (Yahoo spot+history, TSX normalization, FX conversion via YMetaData.currency) · `price/coingecko.rs` (CoinGecko 62-coin map, Yahoo fallback)

### TUI Views (signature: `(&mut Frame, Rect, &App)`)
`tui/ui.rs` (root layout) · `views/positions.rs` (positions+watchlist table) · `views/markets.rs` (markets tab) · `views/economy.rs` (economy tab) · `views/transactions.rs` · `views/help.rs` (help popup) · `views/position_detail.rs` · `views/search_overlay.rs` (/ search) · `views/asset_detail_popup.rs` · `views/context_menu.rs` (right-click)

### TUI Widgets
`theme.rs` (28 color slots, 11 themes, animations, shadows) · `widgets/price_chart.rs` (braille charts, SMA, BB, crosshair) · `widgets/header.rs` (top bar) · `widgets/status_bar.rs` (bottom bar) · `widgets/sidebar.rs` (compositor) · `widgets/allocation_bars.rs` · `widgets/portfolio_sparkline.rs` · `widgets/portfolio_stats.rs` · `widgets/asset_header.rs` · `widgets/top_movers.rs` · `widgets/skeleton.rs` · `widgets/regime_bar.rs`

### Indicators (DO NOT read individual files — use mod.rs re-exports)
```
indicators/mod.rs re-exports:
  compute_rsi(&[f64], period) -> Vec<Option<f64>>           // RSI. period=14 standard
  compute_sma(&[f64], period) -> Vec<Option<f64>>           // Simple moving average
  compute_macd(&[f64]) -> MacdResult { macd, signal, histogram: Vec<f64> }
  compute_bollinger(&[f64], period, multiplier) -> BollingerBands { upper, lower, middle, width: Vec<f64> }
  atr::compute_atr(&[Option<f64>], &[Option<f64>], &[f64], period) -> Vec<Option<f64>>  // ATR from OHLCV, Wilder smoothing
  atr::compute_true_range(&[Option<f64>], &[Option<f64>], &[f64]) -> Vec<Option<f64>>   // True Range per bar
```
Color conventions: RSI >70 = red (overbought), <30 = green (oversold), 30-70 = neutral

### Regime
`regime/mod.rs` (9-signal scorer) · `regime/suggestions.rs` (portfolio suggestions)

### CLI Commands
`commands/setup.rs` (wizard) · `commands/summary.rs` (--group-by, --period, --what-if) · `commands/export.rs` (JSON/CSV) · `commands/import.rs` (replace/merge) · `commands/history.rs` (--date) · `commands/brief.rs` · `commands/demo.rs` · `commands/snapshot.rs` · `commands/watchlist_cli.rs` · `commands/set_cash.rs` · `commands/refresh.rs` (source DAG + the recurring tail: auto-score, forecast retro-score, misalignment detection, regime classification, housekeeping line) · `commands/daemon.rs` (LEGACY always-on scheduler + heartbeat — optional, not required; see "How The System Runs") · `commands/status.rs` (source freshness + daemon health) · `commands/value.rs` · `commands/portfolio_status.rs` (consolidated snapshot: allocation + value + daily P&L + unrealized) · `commands/power_flow_conflicts.rs` (FIC/MIC conflict monitor: defense vs energy vs VIX cross-reference) · `commands/narrative_divergence.rs` (scenario narrative-vs-money scoring) · `commands/news_silence.rs` (topic news-volume baseline scoring)

### Shared Intelligence Contract

The current architecture is intentionally server-owned for higher-order analytics:

- CLI exposes canonical analytics payloads
- mobile server reuses the same Rust structs
- web API reuses the same Rust structs
- agents should consume those payloads rather than rebuilding them

If you are deciding whether logic belongs in Swift/JS/prompt code or in Rust, bias toward Rust when the logic defines:

- ranked priorities
- delta detection
- cross-timeframe interpretation
- portfolio impact
- durable analytical memory

## Key Patterns

- **Keybinding**: `app.rs` → `handle_key()` L1398 → match `KeyCode`
- **View/tab**: `ViewMode` enum L1-285 + `ui.rs` + `help.rs` + `header.rs`
- **CLI command**: `cli.rs` (clap) + `commands/new.rs` + `main.rs`
- **Widget**: `widgets/new.rs` + wire into parent view + `mod.rs`
- **Chart**: `price_chart.rs` render, `app.rs` L1139 variant logic
- **Theme**: `theme.rs` — update ALL 11 themes for new color slots
