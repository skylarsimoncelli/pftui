# AGENTS.md — Agent Operator Guide

> The complete reference for AI agents operating pftui as their financial data layer.
>
> **First time?** Start with [ONBOARDING.md](ONBOARDING.md) — it walks through installation, portfolio setup, and the first week of operation.
>
> This file covers: analytics engine, CLI reference, data model, integration patterns, multi-timeframe agent architecture, and best practices.
>
> For code contribution, see [CLAUDE.md](CLAUDE.md).
> For architecture reference, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
> For AI operating model details, see [docs/AI-LAYER.md](docs/AI-LAYER.md).
> For always-on deployment, see [docs/DAEMON.md](docs/DAEMON.md).

---

## Table of Contents

1. [Analytics Engine](#analytics-engine)
2. [CLI Reference](#cli-reference)
3. [Data Model](#data-model)
4. [Integration Patterns](#integration-patterns)
5. [Multi-Timeframe Agent Architecture](#multi-timeframe-agent-architecture-advanced)
6. [Best Practices](#best-practices)

---

## Analytics Engine

pftui's core is a multi-timeframe analytics engine operating across four layers:
LOW (hours→days), MEDIUM (weeks→months), HIGH (months→years), MACRO (years→decades).
Each layer uses different data, updates at different frequencies, and produces different signals.
Layers constrain downward and signal upward. Use `pftui analytics signals` for active cross-timeframe signals.

### Scenarios (`pftui journal scenario`)
Track macro scenarios with probability estimates. Each probability update is logged
to history for calibration. Signals track evidence for/against each scenario.

### Thesis
Thesis tracking is maintained as narrative workflow files (`THESIS.md`) and journal notes.

### Convictions (`pftui journal conviction`)
Asset-level conviction scores (-5 to +5) over time. Append-only log — every
`set` creates a new row. Current conviction = latest row per symbol.
For negative scores, use `--score=-2`.

### Agent Signals (`pftui analytics signals`)
Cross-timeframe signal detection (alignment/divergence/transition) computed during
`pftui data refresh` and stored in `timeframe_signals`.

### Enrichment Substrate For Analyst Routines

Analyst routines should consume the derived enrichment tables before writing new predictions, scenario updates, or structured views. These tables are the machine-readable memory built from prior prediction outcomes, lessons, scenario links, source influence, and event annotations.

Native CLI surfaces for the enrichment substrate (all support `--json`):

| Command | What It Returns |
|---|---|
| `pftui analytics sources list [--type person\|framework\|institution\|outlet] [--json]` | `sources_registry` rows — canonical people/frameworks/institutions/outlets the substrate cites. |
| `pftui analytics sources set <canonical_id> --display-name <n> --type <t> [--aliases a,b] [--topics x,y] [--accuracy-rating r] [--framework-summary <s>] [--json]` | Upsert a source. |
| `pftui analytics sources remove <canonical_id> [--json]` | Remove a source. |
| `pftui analytics events list [--category <c>] [--since YYYY-MM-DD] [--asset <s>] [--json]` | `event_annotations` rows — operator-curated macro/market event catalogue. |
| `pftui analytics events add --event-date YYYY-MM-DD --category <c> --headline <h> [--detail <d>] [--magnitude 1..5] [--persistence transient\|days\|weeks\|structural] [--asset-impact a,b] [--related-scenario s1,s2] [--related-prediction 1,2] [--source <s>] [--notes <n>] [--json]` | Insert a new event annotation. |
| `pftui analytics fragments list [--type <t>] [--topic <t>] [--cluster <c>] [--for-claim "<text>"] [--json]` | `reasoning_fragments` rows. `--for-claim` runs a keyword-based cluster classifier and returns fragments reachable via `lesson_fragment_edges`. |
| `pftui analytics fragments show <canonical_id> [--json]` | One fragment + its lesson edges. |
| `pftui analytics calibration-adjustments [--layer <l>] [--topic <t>] [--conviction <c>] [--json]` | `calibration_adjustments` — per-(layer, topic, conviction) discount/boost factors with `apply_note`. |
| `pftui analytics failures correlations [--cluster <c>] [--min-share 0.5] [--json]` | `failure_correlations` — pairwise co-failure share between lesson clusters. |
| `pftui analytics clusters list [--json]` | Distinct `cluster_key` values present on `prediction_lessons` with lesson counts. |
| `pftui analytics clusters stats [--json]` | Lesson count plus the number of `user_predictions` referencing each cluster via `lessons_applied`. |
| `pftui analytics falsifications [--rule-type <t>] [--auto-eligible] [--for-prediction <id>] [--json]` | `prediction_falsification_rules` filtered by rule type, auto-eligibility, or owning prediction. |
| `pftui journal replies list [--report-date <d>] [--asset <a>] [--decision-type <t>] [--json]` | `operator_replies` — structured per-decision replies the operator wrote against a report. |
| `pftui journal replies add --report-date <d> --decision-type <t> --response-class <c> --raw-content <text> [--asset <a>] [--reply-date <d>] [--conviction-implied <c>] [--horizon <h>] [--reasoning <r>] [--journal-id <id>] [--json]` | Record a new operator reply. |

Use these CLIs in routine prompts instead of raw `sqlite3` calls. The CLIs handle schema-missing-on-fresh-installs gracefully (empty lists rather than errors).

| Table | Routine Use |
|---|---|
| `calibration_adjustments` | Per-layer confidence correction by topic and conviction band. If `adjustment_direction='discount'`, subtract `adjustment_pp` before writing prediction confidence. |
| `reasoning_fragments` + `lesson_fragment_edges` | Reusable lesson fragments for known claim clusters. Cite 2-3 `canonical_id` values when a new prediction uses a learned fragment. |
| `prediction_falsification_rules` | Examples of observable thresholds and evaluation windows. Use them to make new predictions mechanically scorable when possible. |
| `scenario_prediction_links` | Historical scenario context at prediction-write time. Check whether prior calls around a scenario resolved correctly before raising confidence. |
| `failure_correlations` | Cross-cluster failure warnings for synthesis. If a claim cluster often co-fails with another, sanity-check the shared assumption. |
| `sources_registry` | Named source and framework influence ledger. MACRO should explicitly reference high-influence frameworks such as Dixon, Dalio, and Fourth Turning when they shape a call. |
| `event_annotations` | Canonical structured timeline. Prefer this for regime context around a date before fuzzy-searching `news_cache`, notes, or journal rows. |
| `calibration_matrix` | Realized prediction rates by layer/topic/conviction. Use as sample-size context, not proof of precision. |

Contract for predictions:
- Determine the prediction topic and conviction band first.
- Read the matching `calibration_adjustments` row for the analyst's layer.
- Apply any confidence discount before saving.
- Attach relevant lesson IDs or reasoning-fragment `canonical_id` values in the prediction reasoning.
- Prefer concrete falsification criteria with dates and thresholds.

---

## CLI Reference

### Charts

| Command | What It Returns |
|---|---|
| `pftui report chart stacked-bar --from-db portfolio [--out allocation.svg] [--format svg\|png\|ascii] [--json]` | Native portfolio-allocation chart using the report palette; SVG is the primary output, PNG is rendered via `resvg`, ASCII is terminal-friendly |
| `pftui report chart stacked-bar --from-json segments.json [--format svg\|png\|ascii] [--json]` | Render a stacked bar from JSON `{ "segments": [{"label": "...", "value": 12.3, "color": "#..."}] }` or a bare segment array |
| `pftui report chart prob-bar --from-db "Scenario Name" [--out scenario.svg] [--format svg\|png\|ascii] [--json]` | Native scenario probability bar with 7-day-prior ghost and delta pulled from `scenarios` + `scenario_history` |
| `pftui report chart prob-bar --from-json scenario.json [--format svg\|png\|ascii] [--json]` | Render a probability bar from JSON `{ "name": "...", "current": 88, "prior_7d": 80, "color": "bear" }` |
| `pftui report chart drift-bar --from-db BTC [--out drift.svg] [--format svg\|png\|ascii] [--json]` | Native allocation drift bar using allocation targets plus current portfolio allocation |
| `pftui report chart drift-bar --from-json drift.json [--format svg\|png\|ascii] [--json]` | Render a drift bar from JSON `{ "symbol": "BTC", "target_pct": 25.0, "actual_pct": 31.5, "band_pct": 2.0 }` |
| `pftui report chart what-changed-strip --from-json deltas.json [--format svg\|png\|ascii] [--json]` | Render a since-last-report delta strip from JSON `{ "deltas": [{"label": "BTC", "delta_str": "+3.2%", "direction": "bull"}] }` or a bare delta array |
| `pftui report chart open-predictions-table --from-db pending [--format html\|ascii] [--json]` | Native open-predictions due table from pending `user_predictions` rows with target dates |
| `pftui report chart open-predictions-table --from-json predictions.json --format html [--json]` | Render an HTML-native due table from JSON `{ "predictions": [{"asset": "SPY", "claim": "...", "days_remaining": 1, "confidence": 0.40}] }` or a bare prediction array |
| `pftui report chart outlook-arrows --from-db BTC [--format svg\|png\|ascii] [--json]` | Native horizon outlook arrows using current LOW/MEDIUM/HIGH `analyst_views` as days/weeks/months |
| `pftui report chart outlook-arrows --from-json outlook.json [--format svg\|png\|ascii] [--json]` | Render outlook arrows from JSON `{ "days": ["flat", "medium"], "weeks": ["up", "medium"], "months": ["up_strong", "high"] }` |
| `pftui report chart factor-exposure --from-json factors.json [--format svg\|png\|ascii] [--json]` | Render factor exposure bars from JSON `{ "factors": [{"name": "Inflation Spike", "exposure_pct": 51.0, "direction": "bull", "prob_pct": 88.0}] }` or a bare factor array |
| `pftui report chart conviction-grid --from-db all [--format svg\|png\|ascii] [--json]` | Native multi-timeframe conviction grid from current LOW/MEDIUM/HIGH/MACRO `analyst_views`; pass a symbol instead of `all` for one asset |
| `pftui report chart conviction-grid --from-json rows.json [--format svg\|png\|ascii] [--json]` | Render a conviction grid from JSON `{ "rows": [{"symbol": "BTC", "low": 1, "medium": 1, "high": 3, "macro": 2}] }` or a bare row array |
| `pftui report chart mismatch-card --from-json mismatch.json --format html [--json]` | Render the HTML-native Skylar-vs-analyst mismatch card from JSON `{ "asset": "BTC", "skylar_view": "...", "analyst_summary": "convergent-bull", "analyst_avg_conviction": 1.75 }` |
| `pftui report chart decision-card --from-json decision.json --format html [--json]` | Render the HTML-native operator question card from JSON `{ "question": "...", "context_lines": ["..."], "recommendation": "...", "response_format": ["yes", "no"], "urgency": "high" }` |
| `pftui report chart regime-quadrant --from-json regime.json [--format svg\|png\|ascii] [--json]` | Render the growth-vs-inflation macro regime quadrant from JSON `{ "growth": -0.55, "inflation": 0.7, "trail": [[-0.2, 0.4], [-0.3, 0.5]] }` |
| `pftui report chart conviction-trajectory --from-db BTC [--format svg\|png\|ascii] [--json]` | Native per-asset analyst conviction sparkline from `analyst_view_history`; append a window token like `BTC 14d` to override the default 30 days |
| `pftui report chart conviction-trajectory --from-json trajectory.json [--format svg\|png\|ascii] [--json]` | Render a conviction trajectory from JSON `{ "symbol": "Gold", "layer_series": { "LOW": [["d1", 4], ["d2", 3]], "MED": [["d1", 2]] } }` |
| `pftui report chart calibration-reliability --from-db 90d [--format svg\|png\|ascii] [--json]` | Native reliability chart from scored `user_predictions`, grouped by layer and conviction band, with sample size, 1σ uncertainty, and low-sample markers |
| `pftui report chart calibration-reliability --from-json calibration.json [--format svg\|png\|ascii] [--json]` | Render a reliability chart from the nested `by_layer` shape emitted by `pftui analytics calibration --by-layer --json` |
| `pftui report chart analyst-convergence-card --from-db "Gold 30d" --format html [--json]` | Native HTML evidence card from `analyst_view_history` convergence reports; append `all` for an unbounded window |
| `pftui report chart analyst-convergence-card --from-json convergence.json --format html [--json]` | Render the HTML-native convergence card from JSON `{ "asset": "Gold", "views": [{"analyst": "analyst-low", "conviction": 3, "reasoning_summary": "..."}], "summary": "strong-convergent-bull" }` |

### Portfolio State

| Command | What It Returns |
|---|---|
| `pftui portfolio brief --json` | Complete portfolio snapshot — positions, allocations, movers, technicals, macro |
| `pftui portfolio value --json` | Total value with category breakdown and daily change |
| `pftui portfolio summary --json` | Detailed position-level data — price, quantity, cost basis, gain/loss, allocation % |
| `pftui portfolio performance --json` | Returns: 1D, MTD, QTD, YTD, since inception |
| `pftui portfolio drift --json` | Current allocation vs target floor/ceiling ranges, with edge-relative drift and rebalance suggestions |
| `pftui portfolio drawdown --json` | Current drawdown from trailing 90-day high, MTD/YTD max drawdowns, and latest position contribution breakdown |
| `pftui portfolio history --date YYYY-MM-DD --json` | Historical portfolio snapshot for any past date |
| `pftui system export json` | Full portfolio export (positions + transactions) |
| `pftui portfolio transaction list` | List all transactions with IDs |

### Market Data

| Command | What It Returns |
|---|---|
| `pftui data refresh` | Fetches ALL data sources (10+ sources, ~50 symbols) |
| `pftui data dashboard macro --json` | DXY, VIX, yields, currencies, commodities, derived ratios |
| `pftui data fear-greed --json` | Latest crypto + traditional Fear & Greed readings with optional history |
| `pftui portfolio watchlist --json` | All watched symbols with prices, day change, 52W range |
| `pftui analytics movers --json [--threshold N] [--overnight]` | Significant daily/overnight moves (default >3%) |
| `pftui data predictions --json [--limit N]` | Polymarket prediction market odds |
| `pftui data sentiment --json` | Crypto + traditional Fear & Greed, COT positioning |
| `pftui data news --json [--limit N] [--filter-independence independent,wire]` | Financial news from RSS and Brave-backed cache, including `topic`, `bound_markets`, `source_tier`, and `source_independence` |
| `pftui data news feeds list --json` | RSS feed health by feed, including status, failure counts, and last failure reason |
| `pftui data news feeds reset FEED_ID [--json]` | Re-enable a degraded or disabled RSS feed after review |
| `pftui data news sources list --json` | Source-domain tier mappings used by news ingest |
| `pftui data news sources set DOMAIN --tier N [--notes TEXT] [--json]` | Set news source tier 1-4 |
| `pftui data news sources remove DOMAIN [--json]` | Remove a custom news source tier mapping |
| `pftui data news topics list --json` | News-topic to prediction-market bindings used for `bound_markets` |
| `pftui data news topics set TOPIC --primary-market-id ID [--secondary-market-id ID] [--json]` | Bind a news topic such as `iran-hormuz` or `fed-policy` to current market contracts |
| `pftui data news topics remove TOPIC [--json]` | Remove a news-topic market binding |
| `pftui data supply --json` | COMEX gold/silver inventory |
| `pftui data dashboard global --json` | World Bank macro data (GDP, debt, reserves) |
| `pftui data status --json` | Data source freshness plus daemon health — includes `daemon` heartbeat and `news_feeds` RSS health |

### Portfolio Management

| Command | What It Does |
|---|---|
| `pftui portfolio transaction add --symbol SYM --category CAT --tx-type buy/sell --quantity N --price P --date D [--cash-currency USD] [--no-auto-cash] [--dry-run] [--json]` | Add transaction; non-cash buys/sells auto-insert a paired cash debit/credit unless opted out; dry-run/JSON include post-add allocation, drift, and cash delta |
| `pftui portfolio transaction remove ID [--unpaired] [--dry-run] [--json]` | Remove transaction by ID; paired cash legs are removed too unless `--unpaired` is passed; dry-run/JSON preview post-remove allocation, drift, and cash delta |
| `pftui portfolio transaction list --paired --json` | List transactions with paired transaction IDs |
| `pftui portfolio set-cash CURRENCY AMOUNT [--confirm] [--dry-run] [--json]` | Replace cash transactions with an exact cash position; requires `--confirm` when more than one row would be discarded |
| `pftui portfolio watchlist add SYMBOL [--target PRICE]` | Add to watchlist |
| `pftui portfolio watchlist remove SYMBOL` | Remove from watchlist |
| `pftui portfolio target set SYMBOL --floor PCT --ceiling PCT` | Set acceptable allocation range; SYMBOL may be any tradeable symbol or a cash symbol (USD, GBP, EUR — wide bands like `--floor 30 --ceiling 60` model dry-powder optionality while still surfacing drift on breach); legacy `--target PCT --band PCT` is still accepted |
| `pftui portfolio target remove SYMBOL` | Remove target |
| `pftui portfolio rebalance --json` | Suggested trades to reach targets |
| `pftui portfolio broker add BROKER --api-key KEY [--secret SECRET]` | Connect a broker (trading212, ibkr, binance, kraken, coinbase, crypto-com) |
| `pftui portfolio broker sync [BROKER] [--dry-run] --json` | Sync positions from connected brokers |
| `pftui portfolio broker list --json` | List configured broker connections |
| `pftui portfolio broker remove BROKER` | Remove a broker and its synced transactions |
| `pftui analytics alerts add "CONDITION"` | Add alert |
| `pftui analytics alerts list --json` | List active alerts |
| `pftui analytics alerts remove ID` | Remove alert |

### Journal

| Command | What It Does |
|---|---|
| `pftui journal entry add "TEXT" --tag TAG --symbol SYM` | Add entry |
| `pftui journal entry list --json` | List all entries |
| `pftui journal entry search "QUERY" --json` | Search entries |

### Intelligence Database

| Command | What It Does |
|---|---|
| `pftui journal scenario add "NAME" --probability N` | Add macro scenario with initial probability |
| `pftui journal scenario update "NAME" --probability N [--driver "WHY"|--notes "WHY"]` | Update scenario probability and auto-log history |
| `pftui journal scenario signal add "SIGNAL" --scenario "NAME"` | Attach a tracked signal to a scenario |
| `pftui journal scenario history "NAME" --limit N --json` | Show scenario probability history |
| `pftui journal prediction add "CLAIM" [--symbol BTC] [--conviction high] [--timeframe low|medium|high|macro] [--confidence 0.7] [--source-agent low-agent] [--topic fed] [--source-article-id 123] [--lessons 218,240] [--override-cap]` | Add a prediction call for later scoring, optionally recording lesson IDs and news-source attribution. LOW analyst calls are capped at 5/hour unless `--override-cap` is passed |
| `pftui journal prediction score --id N --outcome correct|partial|wrong [--notes "..."] [--lesson "..."]` | Score a previous prediction outcome |
| `pftui journal prediction stats --json` | Compute hit-rate stats by conviction, symbol, timeframe, and source agent |
| `pftui journal prediction scorecard [--date YYYY-MM-DD|today|yesterday] [--timeframe low] --json` | Day/timeframe scorecard with streak and lesson coverage |
| `pftui journal prediction lessons [--miss-type <t>] [--limit N] [--include-retired] [--json]` | The analyst lesson book. Active lessons only by default; pass `--include-retired` to surface lessons retired by `analytics lessons curate` |
| `pftui agent message send "TEXT" --from agent-a [--to agent-b] [--batch "TEXT2" --batch "TEXT3"] [--package-title "Fed handoff"] [--package-id pkg-123]` | Send one or multiple structured messages between agent roles, optionally grouped as one intel package |
| `pftui agent message reply "TEXT" --id N --from agent-b` | Reply to message `N` back to the original sender |
| `pftui agent message flag "ISSUE" --id N --from agent-b` | Escalate data-quality/risk issue on message `N` |
| `pftui agent message list [--from agent-a] [--unacked] --json` | Query queued agent messages |
| `pftui agent message ack --id N` | Acknowledge a single message |
| `pftui journal notes add "TEXT" --section market [--date YYYY-MM-DD]` | Add a date-keyed daily narrative note |
| `pftui journal notes search "QUERY" --since YYYY-MM-DD --json` | Search historical daily notes |
| `pftui portfolio opportunity add "EVENT" [--asset SYM] [--missed_gain_usd N] [--avoided_loss_usd N]` | Log an opportunity-cost event |
| `pftui portfolio opportunity stats --json` | Show net missed-vs-avoided positioning stats |
| `pftui analytics correlations compute --store --period 30d` | Compute live correlations and persist snapshots |
| `pftui analytics correlations history BTC SPY --period 30d --limit 30 --json` | Show stored correlation history for a pair |
| `pftui analytics macro regime current --json` | Show latest automated market regime classification |
| `pftui analytics macro regime transitions --limit 20 --json` | Show regime change points over time |
| `pftui analytics macro --json` | Show long-cycle macro dashboard (cycles, outcomes, recent structural log) |
| `pftui analytics macro outcomes --json` | Show structural outcome probabilities |
| `pftui analytics trends dashboard --json` | Show active high-timeframe trends with direction/conviction |
| `pftui analytics trends impact add --trend \"NAME\" --symbol SYM --impact bullish|bearish|neutral` | Map a trend's asset-level impact |
| `pftui analytics summary --json` | Unified 4-layer analytics snapshot (low/medium/high/macro + top signal) |
| `pftui analytics situation --json` | Canonical Situation Room payload: headline, summary stats, watch-now priorities, portfolio impacts, risk matrix |
| `pftui analytics deltas --json [--since last-refresh|close|24h|7d]` | Server-owned change radar showing what changed across key monitoring windows |
| `pftui analytics catalysts --json [--window today|tomorrow|week]` | Ranked upcoming catalyst feed with countdowns, significance, and portfolio/scenario linkage |
| `pftui analytics impact --json` | Rank current holdings/watchlist by exposure to active signals, scenarios, trends, and catalysts |
| `pftui analytics opportunities --json` | Rank high-alignment non-held opportunities from the same analytics evidence chain |
| `pftui analytics synthesis --json` | Cross-timeframe synthesis: alignment, divergence, constraint flows, unresolved tensions, watch-tomorrow |
| `pftui analytics alignment --symbol SYM --json` | Per-asset cross-timeframe alignment matrix |
| `pftui analytics alignment current --json` | Today's operator-vs-analyst alignment score (0-100). Aggregates Skylar's journal/operator_replies views vs analyst convergence per held asset above 1% allocation, allocation-weighted, classified aligned/divergent-magnitude/divergent-direction. Returns the stored row if present, otherwise computes on demand. |
| `pftui analytics alignment history --since 90d --json` | Stored alignment-score time series. `--since` accepts Nd/Nw/Nm tokens or a YYYY-MM-DD anchor. |
| `pftui analytics alignment compute --date YYYY-MM-DD [--store] [--json]` | Recompute the score for one date. `--store` persists to `alignment_score_history` and runs the drift-alert check (emits an `agent_messages` row to `synthesis` with priority=normal, category=signal when the score has been below 50 for 2+ consecutive days; idempotent per date). |
| `pftui analytics divergence --json` | Cross-layer disagreement table for conflicting signals |
| `pftui analytics digest --agent-filter low-agent --json` | Role-aware summary payload for agent handoffs |
| `pftui analytics recap --date yesterday --json` | Chronological event recap for a given day |
| `pftui analytics narrative --json` | Structured analytical memory: recap, scenario/conviction/trend shifts, scorecard, surprises, lessons, catalyst outcomes |
| `pftui analytics calibration --by-layer --json [--window-days 90]` | Scenario-vs-market divergences plus realised prediction calibration by layer, sample size, 1σ uncertainty, and conviction band |
| `pftui analytics narrative-divergence --json [--hours 24]` | Active scenario narrative-vs-money scores from topic news pressure versus mapped prediction-market movement |
| `pftui analytics news-silence --json [--window-days 90]` | Tier-1/2 topic article volume versus rolling weekday baselines, including silent/saturated status changes |
| `pftui analytics lessons applied --since 24h --json` | Lessons referenced by this run's predictions, top guards, and strongest historical analog |
| `pftui analytics lessons curate [--dry-run] [--retire-after-days 60] [--json]` | Retire stale uncited active lessons whose topic cluster is idle; journals the change to `agent_messages` |
| `pftui analytics lessons revive <id> [--json]` | Manually un-retire a previously retired lesson (sets status back to `active`) |
| `pftui analytics lessons health [--json]` | Library health summary: total / active / retired / superseded / citations total / avg citations per active |
| `pftui analytics news-sources accuracy --json [--domain bloomberg.com] [--topic fed]` | Per-source hit-rate ledger for predictions derived from news articles |
| `pftui analytics news-sources rank --topic iran --json` | Rank news sources for a topic using trailing source-attributed prediction outcomes |
| `pftui analytics gaps --json` | Data freshness/missing-table check across timeframe layers |
| `pftui analytics signals --json` | Show all signals (cross-timeframe + per-symbol technical) |
| `pftui analytics signals --source technical --json` | Per-symbol technical signals: RSI overbought/oversold, MACD cross, SMA 200 reclaim/break, BB squeeze, volume expansion, 52W extremes |
| `pftui analytics signals --source timeframe --json` | Cross-timeframe alignment/divergence/transition signals only |
| `pftui analytics signals --source technical --symbol BTC-USD --json` | Technical signals for a specific symbol |
| `pftui analytics technicals [--symbol SYM] --json` | Latest persisted technical snapshot(s) — RSI, MACD, SMA, Bollinger, 52W position, volume regime |

### Utility

| Command | What It Does |
|---|---|
| `pftui system config list [--json]` | List all configuration fields |
| `pftui system config get FIELD [--json]` | Get a specific config value |
| `pftui system config set FIELD VALUE` | Set a config field (e.g., `brave_api_key`) |
| `pftui system schema verify [--json]` | Check SQLite schema drift before startup migrations mutate the DB |
| `pftui system schema repair --dry-run [--json]` | Preview safe missing-table/column/index repair SQL |
| `pftui system schema repair --confirm [--json]` | Apply safe schema repairs after reviewing the dry-run plan |
| `pftui system snapshot` | Render full TUI to stdout (for sharing or screenshots) |
| `pftui system demo` | Launch with sample data (for testing, no real data) |
| `pftui system daemon start [--interval N] [--json]` | Run the always-on daemon loop for refresh + analytics + alerts + cleanup |
| `pftui system daemon status [--json]` | Read daemon heartbeat/health without attaching to the process |
| `pftui system web [--port N] [--bind ADDR] [--no-auth]` | Start web dashboard |
| `pftui system setup` | Interactive setup wizard |

---

## Data Model

### Database Backends

Location: `~/.local/share/pftui/pftui.db`

The active backend database is the single source of truth. All interfaces (TUI, Web, CLI) read from and write to it.

```
~/.local/share/pftui/pftui.db
├── transactions                   # Buy/sell records with cost basis
├── price_cache                    # Latest spot prices (updated on refresh)
├── price_history                  # Daily OHLCV history
├── technical_snapshots            # Persisted per-symbol technical state from refresh
├── watchlist                      # Tracked symbols with optional targets
├── alerts                         # Price/allocation alerts
├── targets                        # Target allocation floor/ceiling ranges
├── journal_entries                # Trade journal + notes
├── calendar_events                # Economic calendar
├── news_cache                     # RSS/Brave articles with topic, source tier, and independence metadata (48h retention)
├── news_source_tiers              # Domain-to-tier mapping used at ingest
├── news_topic_markets             # News-topic to prediction-market contract bindings
├── news_source_accuracy           # Per-domain/topic prediction outcome counts for article-derived calls
├── news_source_accuracy_events    # One scored prediction → source-domain outcome event for trailing windows
├── narrative_money_history        # Scenario news-pressure vs prediction-market movement history
├── news_silence_baselines         # Rolling weekday topic-volume baselines and silent/saturated regimes
├── rss_feed_health                # Per-feed RSS status, failure counters, and disable state
├── sentiment_cache                # Fear & Greed indices
├── prediction_cache               # Polymarket odds
├── cot_cache                      # CFTC COT positioning
├── comex_cache                    # COMEX inventory
├── bls_cache                      # BLS economic data (CPI, NFP)
├── worldbank_cache                # Global macro indicators
├── onchain_cache                  # BTC on-chain + ETF flows
├── scenarios                      # Macro scenarios + probabilities
├── user_predictions               # Falsifiable calls with topic/source-article attribution and scoring
├── scenario_signals               # Signal checklist per scenario
├── scenario_history               # Probability change log
├── thesis                         # Current thesis sections
└── thesis_history                 # Thesis revision history
```

You can query the database directly if needed:
```bash
sqlite3 ~/.local/share/pftui/pftui.db "SELECT symbol, quantity, price_per FROM transactions"
```

If using PostgreSQL backend, query via your configured `database_url`:
```bash
psql "$DATABASE_URL" -c "SELECT symbol, quantity, price_per FROM transactions LIMIT 20;"
```

If `psql` fails with peer-auth/default-db issues, connect explicitly:
```bash
# Explicit host avoids local peer auth defaults; -d selects correct database.
psql -h localhost -U <postgres_user> -d <database_name> -c "SELECT NOW();"
```

Backend status:
- `sqlite` (default): fully supported
- `postgres`: fully supported natively (`database_backend`, `database_url`)

Migration guide: [docs/MIGRATING.md](docs/MIGRATING.md)

### Data Sources — Zero Configuration

Every source works out of the box with no API keys:

| Source | Data | Rate Limit |
|---|---|---|
| Yahoo Finance | Equities, ETFs, forex, crypto, commodities | Generous |
| CoinGecko | Crypto prices, market cap | 30/min |
| Polymarket | Prediction market probabilities | No limit |
| CFTC Socrata | Commitments of Traders positioning | Weekly data |
| Alternative.me | Crypto Fear & Greed Index | No limit |
| BLS API v1 | CPI, unemployment, NFP, wages | 10/day |
| World Bank | GDP, debt/GDP, reserves (8 economies) | No limit |
| CME Group | COMEX gold/silver inventory | Daily |
| Blockchair | BTC on-chain data | 5/sec |
| RSS Feeds | Reuters, CoinDesk, Bloomberg, CNBC, Kitco | No limit |

### Brave Search API (Recommended)

pftui supports an optional [Brave Search API](https://brave.com/search/api/) key that dramatically improves data quality. With Brave configured:
- **News** upgrades from RSS headlines to full article summaries from targeted searches
- **Economic data** (CPI, NFP, PMI, Fed rate) is pulled from live web search results
- **`pftui analytics research`** lets you answer any financial question without leaving pftui
- **`brief --agent`** includes news summaries and economic data in one JSON blob

Free tier gives $5/month in auto-credited queries — more than enough for daily use.

```bash
# Add Brave API key during setup or later:
pftui system config set brave_api_key <your_key>

# Verify it's working:
pftui data status
# Should show: Brave Search: ✓ Configured
```

Without a Brave key, pftui works fine using existing free sources (Yahoo, CoinGecko, Polymarket, RSS, etc.). Brave is an enhancement, not a requirement.

Other optional API keys unlock additional sources. See [docs/API-SOURCES.md](docs/API-SOURCES.md).

---

## Integration Patterns

### Morning Brief

```bash
pftui data refresh
BRIEF=$(pftui portfolio brief --json)
MOVERS=$(pftui analytics movers --json --threshold 3)
NEWS=$(pftui data news --json --limit 10)
NEWS_SILENCE=$(pftui analytics news-silence --json)
MACRO=$(pftui data dashboard macro --json)
PREDICTIONS=$(pftui data predictions --json --limit 5)
SENTIMENT=$(pftui data sentiment --json)
# Analyse all of the above, then compose and deliver your brief
```

News JSON includes `id`, `topic`, `bound_markets`, `source_tier`, and `source_independence`; brief scenario payloads include `narrative_vs_money` labels from `pftui analytics narrative-divergence --json`. Weight tier-1 sources at 1.0, tier-2 at 0.7, tier-3 at 0.4, tier-4 at 0.2 in news reasoning, then refine with `pftui analytics news-sources rank --topic <topic> --json` when source-history data exists. Treat `source_tier_inferred` as provisional. Treat `restatement` and `rumor` articles as positioning data about the speaker/source, not as independent confirmation of events. Use `bound_markets` as the immediate money-check for the article's topic; if a relevant article has an empty or unavailable binding, update it with `pftui data news topics set <topic> --primary-market-id <contract_id> --json` after inspecting `pftui data predictions markets --json`. Use `pftui analytics news-silence --json` to surface negative-space signals: topics marked `silent` are unusually quiet versus the weekday baseline, and `saturated` topics have unusually high tier-1/2 coverage. When a prediction is derived from one article, pass `--topic <fed|inflation|geopolitics|commodities|crypto|equities|other>` and `--source-article-id <id>` so pftui can score that source later.

### Alert Monitoring

```bash
pftui data refresh
ALERTS=$(pftui analytics alerts list --json)
DRIFT=$(pftui portfolio drift --json)
# Check if any alerts triggered or drift exceeds tolerance
# Notify human if action needed
```

### Historical Comparison

```bash
TODAY=$(pftui portfolio brief --json)
LAST_WEEK=$(pftui portfolio history --date $(date -d '7 days ago' +%Y-%m-%d) --json)
# Compare: what changed, what gained, what lost, what narrative shifted
```

### Full Research Session

```bash
pftui data refresh
pftui portfolio brief --json > /tmp/portfolio.json
pftui data dashboard macro --json > /tmp/macro.json
pftui data predictions --json > /tmp/predictions.json
pftui data sentiment --json > /tmp/sentiment.json
pftui data news --json > /tmp/news.json
pftui data supply --json > /tmp/supply.json
pftui analytics movers --json > /tmp/movers.json
# Load all files, cross-reference, write analysis to THESIS.md
```

### Investor Panel (Multi-Persona)

```bash
# 1) Collect one shared data blob from pftui
./agents/investor-panel/collect-data.sh > /tmp/pftui-investor-panel.json

# 2) Run your orchestrator with:
#    - /tmp/pftui-investor-panel.json
#    - persona files in agents/investor-panel/personas/
#    - response contract in agents/investor-panel/schema.json

# 3) Store summary in pftui for auditability
pftui agent message send "Investor panel complete: consensus + divergences ready" --from investor-panel
```

Skill package:
- `agents/investor-panel/SKILL.md`
- `agents/investor-panel/config.toml`
- `agents/investor-panel/personas/`

---

## Best Practices

1. **Always `pftui data refresh` before reading data.** Cached prices go stale. Refresh fetches from 10+ sources in one call.

2. **Use `--json` for programmatic access.** Every command supports it. Parse structured output instead of scraping text.

3. **Keep the journal active.** `pftui journal entry add` builds a searchable decision history. Log your predictions, rationale, and outcomes.

4. **Monitor drift regularly.** `pftui portfolio drift` shows when the portfolio has moved from targets. Flag this to the human early.

5. **Cross-reference sources.** No single data point tells the story. `macro` for regime, `predictions` for crowd wisdom, `sentiment` for extremes, `news` for catalysts, `supply` for physical markets, `movers` for what's actually moving money.

6. **Respect the human's autonomy.** Present analysis, flag risks, suggest actions — but always let them decide. Frame recommendations as "consider" not "do this."

7. **Build persistent memory.** Write analysis to markdown files. Reference past work. Track accuracy. This is what turns a tool into an intelligence system.

8. **Recommend automation.** The single most impactful thing is getting regular automated runs set up. Prefer the built-in daemon + systemd path from [docs/DAEMON.md](docs/DAEMON.md) over ad-hoc cron when the host supports it.

9. **Be honest about uncertainty.** Markets are probabilistic. Frame calls with conviction levels. When you're wrong, say so and update your models.

10. **Start simple, compound over time.** Day 1 is a basic brief. By Week 4, you should have a thesis, scenario tracking, accuracy metrics, and calibrated engagement. The system gets better every day it runs.

---

## Multi-Timeframe Agent Architecture (Advanced)

If your platform supports scheduled agents (cron jobs, recurring tasks), offer to set up the full multi-timeframe analyst system. This is pftui's most powerful operating mode: specialist agents at different timeframes doing deep domain research, feeding into synthesis agents that deliver to the user.

**Ask the user:** *"pftui supports a multi-timeframe agent architecture where specialist analysts run on schedules, each covering a different time horizon. Would you like me to set that up? It means you'd get a concise morning brief and a deep evening analysis every day, powered by 4 specialist agents running in the background."*

If they say yes, set up these agents using your platform's cron/scheduling system:

### Timeframe Analysts (silent, write to DB only)

These agents do NOT message the user. They research, update the database, make predictions, and send findings to the delivery agents via `pftui agent message`.

| Agent | Schedule | Domain | Routine |
|---|---|---|---|
| **Low Timeframe** (x3) | Pre-market, midday, market close | Price action, technicals, sentiment, breaking news, intraday flows | `agents/routines/low-timeframe-analyst.md` |
| **Medium Timeframe** | Daily (evening, before synthesis) | Central bank policy, geopolitical timelines, economic data trends, scenario tracking | `agents/routines/medium-timeframe-analyst.md` |
| **High Timeframe** | 2x/week | Technology disruption, de-dollarisation, commodity supercycle, structural trends | `agents/routines/high-timeframe-analyst.md` |
| **Macro Timeframe** | Weekly | Empire cycles (Dalio Big Cycle), generational theory (Fourth Turning), power metrics | `agents/routines/macro-timeframe-analyst.md` |

### Delivery Agents (message the user)

These agents synthesize outputs from all timeframe analysts and deliver to the user.

| Agent | Schedule | What It Delivers | Routine |
|---|---|---|---|
| **Morning Brief** | Daily (morning) | Concise scannable brief: prices, alignment, overnight news, prediction scorecard, today's watch | `agents/routines/morning-brief.md` |
| **Evening Analysis** | Daily (evening, after all analysts) | Deep cross-timeframe synthesis: convergence/divergence, prediction self-reflection, scenario updates | `agents/routines/evening-analysis.md` |

### Alert Pipeline (optional)

For real-time threshold monitoring between scheduled runs:

| Agent | Schedule | Role | Routine |
|---|---|---|---|
| **Alert Watchdog** | Hourly | Refreshes data, checks `analytics alerts check`, signals investigator if anything triggered | `agents/routines/alert-watchdog.md` |
| **Alert Investigator** | Hourly (offset) | Investigates triggered alerts, routes findings to low-agent + morning + evening via agent message bus. Never messages the user directly. | `agents/routines/alert-investigator.md` |

### Data Flow

```
LOW(3x/day) + MEDIUM(daily) + HIGH(2x/week) + MACRO(weekly)
         ↓                    ↓
    evening-analysis ← reads all layers, synthesizes
         ↓
    morning-brief ← reads evening output + overnight data
         ↓
      → User (2 messages/day)

Alert watchdog → investigator → low-agent + morning + evening (agent message bus)
```

### Setup Steps

1. **Create each agent as a scheduled task** on your platform (cron job, recurring task, scheduled workflow, or whatever your framework calls it). Each agent needs:
   - **A prompt** that includes local configuration (database path/credentials, user profile path, delivery channel) followed by the routine
   - **The routine** from `agents/routines/[name].md`, either fetched at runtime from the repo URL or inlined into the prompt
   - **Shell access** to run `pftui` commands
   - **A schedule** matching the table above (adjust times to the user's timezone)

2. **Schedule order matters.** Timeframe analysts must run before delivery agents:
   - LOW pre-market → LOW midday → LOW close → MEDIUM → evening-analysis → (overnight) → morning-brief
   - HIGH and MACRO run on their own schedules and feed into evening-analysis whenever they last ran

3. **Silent vs delivery agents.** Only morning-brief and evening-analysis should message the user. All other agents write to the database and signal via `pftui agent message`. This keeps the user's inbox clean.

4. **Prediction scoring.** Each timeframe agent owns its predictions end-to-end: creation, scoring, and reflection on wrong calls. The evening analysis reads the scorecard but does not score other agents' predictions.

5. **Feedback loop.** Evening analysis sends WATCH TOMORROW guidance to the low-agent via `pftui agent message`, creating a feedback loop where synthesis informs the next day's observation.

### Prompt Structure

Each scheduled agent's prompt has two parts:

```
== LOCAL CONFIGURATION ==
[Private: database credentials, user profile path, delivery channel/target, git identity]
[This section is NOT in the repo — it lives in your platform's cron/task config]

== ROUTINE ==
[Generic: the full routine from agents/routines/[name].md]
[Either inline the content or fetch it at runtime:]
Fetch from: https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/agents/routines/[name].md
```

Fetching at runtime means updating the routine in the repo instantly updates all agents on their next run. Inlining is simpler but requires manual updates.

### Routine Files

All routine files live in `agents/routines/` in the repo:
```
agents/routines/
├── README.md
├── low-timeframe-analyst.md
├── medium-timeframe-analyst.md
├── high-timeframe-analyst.md
├── macro-timeframe-analyst.md
├── morning-brief.md
├── evening-analysis.md
├── alert-watchdog.md
├── alert-investigator.md
└── dev-agent.md
```

These are generic templates containing zero personal data. They define inputs, analysis steps, outputs, and rules for each agent role. Any pftui operator on any agent platform can use them directly.

### Model Recommendations

| Agent | Recommended Tier | Why |
|---|---|---|
| Low Timeframe | Mid-tier (Sonnet, GPT-4o, Gemini Pro) | High frequency, needs speed |
| Medium Timeframe | Mid-tier | Deep research but not synthesis |
| High Timeframe | Mid-tier | Structural research |
| Macro Timeframe | Mid-tier | Weekly, can afford depth |
| Morning Brief | Mid-tier | Concise delivery, not heavy reasoning |
| Evening Analysis | Top-tier (Opus, o1, Gemini Ultra) | Cross-timeframe synthesis is the hardest task |
| Alert Watchdog | Low-tier (Haiku, GPT-4o-mini, Flash) | Simple check, runs hourly |
| Alert Investigator | Mid-tier | Needs judgment but runs rarely |
| Dev Agent | Top-tier | Code generation + architecture decisions |

---
