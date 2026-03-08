# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Brave Search API Integration

> **Goal:** Native Brave Search API support as a single reliable data source that replaces broken scrapers and shallow RSS feeds. Optional API key — pftui works without it, but with a key the data quality jumps dramatically. Free tier gives $5/month in credits (~2000 queries), more than enough for pftui's use case.
>
> **Why:** 4 data integrations are broken (COT, BLS, on-chain, ETF flows). RSS gives headlines but no context. Brave Search API solves both problems with one integration — it can answer ANY financial question via web + news search, returning structured results with full descriptions. Instead of maintaining 10 fragile scrapers, maintain 1 reliable API client.
>
> **API:** Web search (`/res/v1/web/search`) + News search (`/res/v1/news/search`). Auth via `X-Subscription-Token` header. Free tier: $5/month auto-credited, 50 qps. News returns up to 50 results with descriptions, extra_snippets, freshness filtering.

### F26: Brave API Configuration & Client

### F27: Brave-Powered News (Replaces/Supplements RSS)

> When Brave key is configured, news comes from targeted Brave News Search queries instead of generic RSS polling. This gives article summaries (descriptions), not just headlines. Multiple focused queries replace one shallow RSS poll.

### F28: Brave-Powered Economic Data

> Instead of fragile scrapers for BLS/Trading Economics, use Brave Web Search to find latest economic readings. More resilient — when a scraper breaks because a page changed layout, Brave still works because it searches the entire web.

- [ ] **F28.1: Economic data fetcher via Brave** — On `pftui refresh`, if Brave key configured, search for each tracked indicator:
  - `"latest US CPI inflation rate"` → regex-parse value from snippets
  - `"latest US unemployment rate nonfarm payrolls"` → extract NFP + UE
  - `"latest ISM manufacturing PMI services PMI"` → extract both PMIs
  - `"latest FOMC federal funds rate"` → extract rate
  - `"latest US initial jobless claims"` → extract claims
  - `"latest US PPI producer price index"` → extract PPI
  - Parse patterns: "CPI rose 3.2%", "unemployment at 4.4%", "economy added/lost X jobs"
  - Store in new `economic_data` table: `{ indicator, value, previous, change, source_url, fetched_at }`
  - Falls back to BLS API v1 if Brave unavailable
  - ~8-10 Brave API calls per economic refresh
  - Files: new `src/data/economic.rs`, new `src/db/economic_cache.rs`, `src/commands/refresh.rs`

- [ ] **F28.2: Economy tab with real indicator values** — Display actual values in Economy tab [4] instead of `---` or stale data. Show: indicator, current value, previous, direction arrow, release date. Highlight surprises. Files: `src/tui/views/economy.rs`

- [ ] **F28.3: `pftui economy` CLI** — `pftui economy` (all indicators), `--indicator cpi`, `--json`. Agents use this instead of web searching for macro data. Files: new `src/commands/economy.rs`, `cli.rs`, `main.rs`

- [ ] **F28.4: Calendar enrichment** — Search `"next CPI release date"`, `"next FOMC meeting date"` to populate calendar with actual dates and consensus forecasts. Files: `src/data/calendar.rs`

### F29: Brave-Powered Research Command

> A new `pftui research` command — the agent's Swiss Army knife. Instead of falling back to their own web_search tool, agents stay in pftui for ANY financial question.

- [ ] **F29.1: `pftui research "<query>"` CLI** — Brave web search with structured results. Default 5 results with title, URL, description, snippets. Flags: `--news` (news endpoint), `--freshness pd/pw/pm/py`, `--count N`, `--json`. Examples:
  - `pftui research "Iran oil exports sanctions 2026" --news --freshness pw`
  - `pftui research "COMEX silver registered inventory" --json`
  - `pftui research "COT gold managed money positioning" --json`
  - `pftui research "BTC ETF inflows outflows this week" --news`
  - Files: new `src/commands/research.rs`, `cli.rs`, `main.rs`

- [ ] **F29.2: Financial research presets** — Shortcut flags for common patterns:
  - `--fed` → latest Fed statements/speeches
  - `--earnings TSLA` → latest earnings results for symbol
  - `--geopolitics` → geopolitical developments
  - `--cot gold` → COT positioning reports (replaces broken CFTC scraper)
  - `--etf btc` → BTC ETF flow reports (replaces broken CoinGlass scraper)
  - `--opec` → OPEC production/decisions
  - Files: `src/commands/research.rs`

### F30: Enhanced Refresh & Brief with Brave

> `pftui refresh` becomes a one-command intelligence operation. `pftui brief --agent` becomes the one JSON blob an agent needs.

- [ ] **F30.1: Brave-aware refresh pipeline** — When key configured, `pftui refresh` includes: prices (Yahoo/CoinGecko) → Brave news batch (~12 queries) → Brave economic data (~8 queries) → existing free sources (Polymarket, F&G, etc.). Progress: `✓ Prices (51 symbols) ✓ News (47 articles via Brave) ✓ Economy (8 indicators) ✓ Predictions ✓ Sentiment`. Total: ~20-25 Brave calls per refresh (well within free tier of ~70/day at $5/month). Files: `src/commands/refresh.rs`

- [ ] **F30.2: `brief --agent` with full intelligence** — When Brave configured, `pftui brief --agent --json` includes:
  - `positions` — all held assets with prices, allocation, change
  - `movers` — significant daily moves
  - `macro` — DXY, VIX, yields, commodities, derived ratios
  - `news_summary` — top 10 articles with descriptions (not just titles)
  - `economic_data` — latest CPI, NFP, PMI, Fed rate values
  - `predictions` — top prediction market odds
  - `sentiment` — F&G indices
  - `alerts` — triggered alerts
  - `drift` — allocation drift from targets
  - This is the "one blob" that replaces 4-5 CLI calls + web searching. An agent reading this has 90% of what it needs for a morning brief.
  - Files: `src/commands/brief.rs`

---

## P0 — QA Bugs (from 2026-03-06 QA Report)

> Source: Opus QA agent ran 52 manual tests + 1105 unit tests. Full report: `QA-REPORT.md`

### Critical

### Significant

- [ ] **COT, BLS, On-chain, ETF flows all fail on every refresh** — 4 broken data integrations. Brave API (F28, F29) provides a better path for economic data and research. Fix scrapers where possible, fall back to Brave research presets. Files: `src/data/cot.rs`, `src/data/bls.rs`, `src/data/onchain.rs`

- [ ] **`pftui global` shows empty data despite 120 cached records** — World Bank data cached but display layer can't read it. Files: `src/commands/global.rs`

- [ ] **COMEX registered inventory shows 0 troy ounces** — Scraper parsing wrong field. Files: `src/data/comex.rs`

- [ ] **USD/JPY and USD/CNY show 1.0000** — Yahoo FX feed issue. Files: `src/price/yahoo.rs`

### Minor

- [ ] **`add-tx` accepts quantity=0 and price=0** — Add validation. Files: `src/commands/add_tx.rs`
- [ ] **`watch` accepts invalid symbols without warning** — Validate with price lookup. Files: `src/commands/watch.rs`
- [ ] **No rate limiting on concurrent refreshes** — Add lock file. Files: `src/commands/refresh.rs`
- [ ] **Performance shows N/A for MTD with March data** — Calculate from earliest available snapshot. Files: `src/commands/performance.rs`

---

## P0 — Bugs & Fixes

> Other broken functionality. Fix before shipping.

---

## P1 — Feature Requests

> User-requested features and high-value improvements.

### Data & Display

### CLI Enhancements
- [ ] **"What Changed Today" in `brief`** — Top movers, threshold crossings, triggered alerts. Files: `commands/brief.rs`
- [ ] **Benchmark comparison in `brief`** — Portfolio performance vs SPY or custom benchmark. Files: `commands/brief.rs`
- [ ] **Portfolio stress testing CLI** — `pftui stress-test` with named scenarios. Builds on F4.2 engine. Files: new `commands/stress_test.rs`
- [ ] [Feedback] **`pftui status --data` command** — Per-source data health: prices (✓ fresh 3h ago), predictions (✗ parse error), COT (✗ unavailable), news (✓ 92 articles), BLS (✗ parse error). Makes data availability transparent. Files: new `commands/status_data.rs` or extend `commands/status.rs`
- [ ] [Feedback] **Prediction category filtering** — `pftui predictions --category geopolitics` should filter out sports/entertainment. Add query search for specific topics (e.g., "ceasefire", "Fed rate"). Files: `commands/predictions.rs`, `data/predictions.rs`

### Analytics
- [ ] **Correlation grid in Markets tab** — Color-coded matrix (green/red). Toggle 7d/30d/90d. Files: `views/markets.rs`, new `views/correlation_grid.rs`
- [ ] **Correlations in `brief`** — Top pairs + active breaks. Files: `commands/brief.rs`
- [ ] [Feedback] **`pftui correlations` CLI** — Rolling correlations between held assets (gold-DXY, BTC-SPX, silver-gold). Foundation module already exists in `indicators/correlation.rs`. Files: new `commands/correlations.rs`
- [ ] **Position annotations & thesis tracking** — SQLite table with thesis, invalidation criteria, review dates, targets. CLI: `pftui annotate GC=F --thesis "..."`. Show in detail popup. Files: new `db/annotations.rs`, new `commands/annotate.rs`, `views/asset_detail_popup.rs`
- [ ] **Review date alerts** — Overdue review dates show ⏰ in Positions tab. Auto-creates alert. Files: `alerts/engine.rs`, `views/positions.rs`
- [ ] **Asset groups** — `pftui group create "hard-assets" --symbols GC=F,SI=F,BTC`. Combined allocation + performance. Filter positions by group. Files: new `db/groups.rs`, new `commands/group.rs`

### Infrastructure
- [ ] **PostgreSQL backend support** — Add PostgreSQL as alternative to SQLite via `sqlx` (runtime, not compile-time). `pftui setup` already handles DB choice — add Postgres option to the wizard. Migration uses existing workflow: `pftui export json` → `pftui setup` (pick new backend) → `pftui import`. Files to change:
  - `db/backend.rs` (new) — abstraction layer over `rusqlite`/`sqlx`
  - `db/schema.rs` + `db/*.rs` — abstract all queries to work with both backends
  - `config.rs` — `database.backend` + `database.url` fields
  - `commands/setup.rs` — add Postgres option to wizard
  - `docs/MIGRATING.md` (new) — document the 3-step export/setup/import workflow
  - `README.md` — add "SQLite (default) or PostgreSQL" to features, install section
  - `website/index.html` — update comparison table + features to mention Postgres support
  - `AGENTS.md` — update data model section to explain both backends + how agents should handle it

---

## P2 — Nice to Have

> Future improvements. Lower priority.

### TUI Polish (batch: ~4hrs total)
- [ ] **Command palette** — `:` opens vim-style command mode with autocomplete. Files: new `views/command_palette.rs`
- [ ] **Context-sensitive hotkey hints** — Bottom bar shows available actions for current view. Files: `widgets/status_bar.rs`
- [ ] **Breadcrumb navigation** — Header shows `Positions → AAPL → Detail`. Files: `widgets/header.rs`
- [ ] **Positions sub-modes** — `G`=group by category, `A`=sort by allocation, `P`=sort by performance. Files: `views/positions.rs`
- [ ] **Auto-refresh timer** — Config: `auto_refresh = true`, `refresh_interval_secs = 300`. Files: `config.rs`
- [ ] [Feedback] **Sector grouping in positions** — Toggle to show positions grouped by asset class (Cash, Commodities, Crypto, Equities) with aggregate allocation and performance per group. Files: `views/positions.rs`

### Watchlist (batch: ~2hrs total)
- [ ] **Watchlist column customization** — Config: `watchlist.columns = [...]`. Files: `config.rs`, `views/watchlist.rs`
- [ ] **Watchlist groups** — Multiple named watchlists, switch with `W` + 1/2/3. Files: new `db/watchlist_groups.rs`
- [ ] **Inline watchlist actions** — `a`=alert, `c`=chart, `r`=remove. Files: `views/watchlist.rs`

### Scanner (batch: ~3hrs total)
- [ ] **Scanner with filter DSL** — `pftui scan --filter "allocation_pct > 10"`. Files: new `commands/scan.rs`
- [ ] **Interactive scan builder** — `:scan` modal with add/remove/save/load. Files: new `views/scan_builder.rs`
- [ ] **Saveable scan queries** — SQLite storage. `:scan save my_scan`. Files: new `db/scan_queries.rs`
- [ ] **Scan-triggered alerts** — Alert when scan results change. Files: `alerts/engine.rs`

### Distribution
- [ ] **Snap/AUR/Scoop publishing** — Needs accounts + secrets for each store
- [ ] **Windows build support** — Add x86_64-pc-windows-msvc to release matrix
- [ ] **Homebrew Core** — Needs 50+ GitHub stars (currently 1)

### Other
- [ ] **Workspace presets** — Config: `layout = "compact" | "split" | "analyst"`. Files: `config.rs`, `tui/ui.rs`
- [ ] **Chart grid view** — Mini braille charts for all positions (6-9 per screen). New view `8`. Files: new `views/chart_grid.rs`
- [ ] **Onboarding tour** — First-run walkthrough for new users. Files: new `views/onboarding.rs`
- [ ] **Calendar in Economy tab** — 7-day forward view with impact color-coding. Files: `views/economy.rs`
- [ ] [Feedback] **Economy tab data gaps** — CPI, unemployment, NFP show `---`. BLS parse errors. Global macro section empty. Need data feed reliability improvements. Files: `data/bls.rs`, `views/economy.rs`
- [ ] [Feedback] **Day P&L dollar column in TUI positions** — Show absolute daily P&L in dollars alongside percentage. Currently only total P&L shown. Every Sentinel review requests this. Files: `views/positions.rs`

---

## P3 — Long Term

- [ ] **Sovereign holdings tracker** — CB gold (WGC), government BTC, COMEX silver. No other TUI tracks this. Files: new `data/sovereign.rs`
- [ ] **Dividend tracking** — Payments, yield, ex-dates
- [ ] **Multi-portfolio support** — Named portfolios with switching
- [ ] **Custom keybinding config** — User-configurable in config.toml
- [ ] **Sector heatmap** — Treemap-style sector performance view
- [ ] **Options chains** — If a free data source exists
- [ ] [Feedback] **Oil-specific dashboard** — `pftui oil` showing WTI, Brent, spread, RSI, OPEC+ context, Hormuz status. Niche but high-value during geopolitical crises.
- [ ] [Feedback] **War/crisis mode dashboard** — Configurable crisis dashboard tracking oil, VIX, defense sector, safe havens, shipping rates in one view.

---

## Feedback Summary

> Updated: 2026-03-07

### Current Scores (latest per tester)

| Tester | Usefulness | Overall | Trend |
|--------|-----------|---------|-------|
| Market Research | 78% | 74% | ↑ (40→72→78→78→74) |
| Eventuality Planner | 82% | 80% | ↑ (38→85→92→85→80) |
| Sentinel (Portfolio Analyst) | 78% | 82% | → (78→82→82→78→82) |
| Market Close | 92% | 88% | ↑ (68→80→72→88) |
| UX Analyst | — | 73% | → (78→68→72→73) |

### Score Trends

- **Market Research:** Strong recovery from 40→74. Plateaued around 74-78. Main blocker: still needs Python script for RSI/MACD/SMA on macro assets.
- **Eventuality Planner:** Best improvement arc (38→92 peak). Slight dip to 80 on Mar 7. Macro dashboard is the star feature. ETF flows failure and prediction markets filtering are pain points.
- **Sentinel (Portfolio Analyst):** Stable at 78-82. Consistently asks for day P&L dollar column, sector grouping, and enhanced watchlist signals. TUI visual quality highly praised.
- **Market Close:** Strongest recent score (92/88). `brief + movers + macro` pipeline now covers most of the routine. Python script nearly eliminated. Wants correlations and sector heatmap.
- **UX Analyst:** Lowest scorer at 73. Focus is on CLI consistency (--json gaps), data pipeline reliability (predictions/COT/BLS parse errors), and feature discoverability. Watchlist --json was fixed (Mar 7).

### Top 3 Priorities (Feedback-Driven)

1. **Fix data pipeline stubs** (P0) — Predictions returns sports only, ETF flows fails, COT/BLS intermittent. Half the advertised features show "no data". This is the UX Analyst's core complaint and the biggest trust issue.
2. **Add technicals to macro + watchlist** (P1) — RSI/MACD/SMA on macro dashboard and watchlist. This single feature eliminates the Python script dependency that 3/4 testers still rely on. Highest-leverage feature for score improvement.
3. **`pftui status --data` command** (P1) — Per-source data health transparency. Makes it clear which integrations work vs which are broken, instead of silent failures.
