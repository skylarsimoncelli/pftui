# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Format: `- [ ] **Short title** — Brief description. Files: relevant_file.rs`
> Full analytics spec: `docs/ANALYTICS-SPEC.md`

## P0 — Data Pipeline Reliability (CRITICAL) ✅

> **Goal:** Every shipped feature must actually populate with real data on `pftui refresh`. Scores dropped across all testers because 40% of tabs/commands show empty states. No new features until these are fixed.

- [x] **`pftui refresh` fetches ALL data sources** — ✅ DONE. Rewritten to fetch all 10 sources with freshness checks.
- [x] **Auto-refresh on TUI launch** — ✅ DONE. Background refresh runs on startup with pulsing status indicator.
- [x] **`pftui status`** — ✅ DONE. Shows freshness for all 10 data sources.
- [x] **Fix movers/watchlist sign discrepancy** — ✅ DONE. Both use (current - yesterday_close) / yesterday_close.
- [x] **Stale data indicator in TUI header** — ✅ DONE. Shows `⚠ Stale (Xh ago)` when >1h old.
- [x] **Standardize `--json` flag across all CLI commands** — ✅ DONE. Added to summary and value.
- [x] **Fix 2 test failures** — ✅ DONE. Both tests now pass.
- [x] **Fix clippy warnings** — ✅ DONE. `cargo clippy --all-targets -- -D warnings` passes.

## P0 — QA Bugs (from 2026-03-06 QA Report)

> **Source:** Opus QA agent ran 52 manual tests + 1105 unit tests. Full report: `QA-REPORT.md`

### P0 Bugs — Critical

- [ ] **P0-1: `brief` and `movers` report contradictory 1D% for same assets** — BTC shows -6.4% in `brief` vs -0.14% in `movers`. Root cause: `brief` uses Yahoo's `regularMarketChangePercent` (real day change from previous close) while `movers` compares last two cached `price_history` entries (which may be minutes apart after multiple refreshes). Fix: `movers` should use the same Yahoo day-change field as `brief`, or compare against the earliest same-day cache entry, not just the previous entry. This is the #1 trust issue — two primary commands show different numbers for the same thing. Files: `src/commands/brief.rs`, `src/commands/movers.rs`, `src/price/mod.rs`
- [ ] **P0-2: `drift` displays raw Decimal values with 30+ decimal places** — Shows `18.718814357195681326649469110` instead of `18.72`. Also affects `summary --json` which outputs `allocation_pct` as raw high-precision strings. Fix: format to 2 decimal places in display, 4 in JSON. Files: `src/commands/drift.rs`, `src/commands/summary.rs`

### P1 Bugs — Significant

- [ ] **P1-1: COT, BLS, On-chain, ETF flows all fail on every refresh** — 4 of the P0 free data integration features don't work in production. COT: all failed. BLS: "Failed to parse BLS value: -". On-chain: "error decoding response body". ETF flows: no data returned. Features marked ✅ COMPLETE but broken. Files: `src/data/cot.rs`, `src/data/bls.rs`, `src/data/onchain.rs`, `src/data/etf_flows.rs` (or equivalent)
- [ ] **P1-2: `pftui global` shows empty data despite 120 cached records** — All 8 country sections display empty headers with no indicator values. World Bank data is cached but the display layer can't read it. Files: `src/commands/global.rs`, `src/db/worldbank_cache.rs`
- [ ] **P1-3: `pftui status` reports COMEX as empty but `pftui supply` shows data** — Inconsistent freshness reporting. Files: `src/commands/status.rs`, `src/db/comex_cache.rs`
- [ ] **P1-4: COMEX registered inventory shows 0 troy ounces** — Both gold and silver show 0 registered, all inventory in eligible. Real COMEX registered gold should be ~16-18M oz. Scraper parsing wrong field or source structure changed. Files: `src/data/comex.rs`
- [ ] **P1-5: USD/JPY and USD/CNY show 1.0000 in macro dashboard** — Known Yahoo Finance FX feed issue. Real values: ~150 (JPY), ~7.2 (CNY). Files: `src/price/yahoo.rs`, `src/commands/macro_cmd.rs`

### P2 Bugs — Minor

- [ ] **P2-1: `add-tx` accepts quantity=0 and price=0** — Zero-quantity buy is meaningless, zero-price corrupts cost basis. Add validation. Files: `src/commands/add_tx.rs`
- [ ] **P2-2: `watch` accepts any string without validation** — No symbol lookup or warning for invalid tickers. At minimum warn if price fetch fails. Files: `src/commands/watch.rs`
- [ ] **P2-3: No rate limiting on concurrent `pftui refresh`** — Two parallel refreshes hit all APIs twice. Add a lock file or deduplication. Files: `src/commands/refresh.rs`
- [ ] **P2-4: Performance shows N/A for MTD despite having March snapshots** — Logic requires snapshot from period start (1st of month). Should calculate from earliest available snapshot in period. Files: `src/commands/performance.rs`
- [ ] **P2-5: `brief --technicals` flag doesn't exist** — README/test plan expected it but brief always includes technicals. Either add the flag or update docs. Files: `src/commands/brief.rs`, `README.md`

---

## P0 — Free Data Integration (No API Keys)

> **Goal:** pftui ships as a zero-config, zero-key terminal for macro-aware investors. Every data source below is completely free and requires NO authentication. A finance enthusiast installs pftui and immediately has prediction markets, COT positioning, sentiment, news, on-chain data, and economic releases — all in one terminal. This is the moat.

### F17: Prediction Markets Panel
> **Goal:** Real-money probability data for macro scenarios, directly in the TUI. This is the single most differentiated feature — no other portfolio TUI shows prediction market odds.
> **Sources:** Polymarket Gamma API (free, no key, JSON REST), Manifold Markets API (free, no key)
> **Data:** Market title, current probability, 24h change, volume, category (geopolitics/economics/crypto/AI)

### F18: CFTC Commitments of Traders (COT)
> **Goal:** Show what the smart money is doing. Commercials vs speculators positioning on gold, silver, oil, BTC futures. Updated weekly.
> **Source:** CFTC Socrata API (`publicreporting.cftc.gov`, free, no key, JSON)
> **Data:** Net positions by trader type (commercial, non-commercial/managed money), open interest, week-over-week changes



### F19: Fear & Greed Index Integration
> **Goal:** Crypto + traditional market sentiment gauges, natively in the TUI.
> **Sources:** Alternative.me Crypto F&G (free, no key), CNN F&G (scrape — public page)
> **Data:** Index value (0-100), classification (Extreme Fear/Fear/Neutral/Greed/Extreme Greed), historical values

### F20: Live News Feed (RSS) ✅ COMPLETE
> **Goal:** Zero-cost, zero-key financial news aggregation from the best sources. Filterable by topic, searchable, in a dedicated News view.
> **Sources:** RSS feeds — completely free, no auth, no rate limits.
> **Feeds:** Reuters (`reuters.com/rssfeed/`), CoinDesk (`coindesk.com/arc/outboundfeeds/rss/`), ZeroHedge (`zerohedge.com/fullrss.xml`), The Block, Yahoo Finance, MarketWatch, CNBC, Seeking Alpha, Bloomberg (headlines)
> **Status:** F20.1-F20.5 complete. News data module, News tab, header ticker, CLI, and per-asset filtering all shipped.

### F21: BTC On-Chain & ETF Flow Data ✅ COMPLETE
> **Goal:** On-chain signals and institutional flow data for BTC — whale movements, exchange flows, ETF inflows/outflows.
> **Sources:** Blockchair (5 req/sec free, no key), CoinGlass (scrape public pages), Whale Alert (limited free tier — scrape public feed)
> **Status:** F21.1 (data module), F21.2 (popup panel), F21.3 (CLI) all shipped.

### F22: COMEX & Commodity Supply Data ✅ COMPLETE
> **Goal:** Physical market data — COMEX inventory, delivery reports, supply/demand signals for metals.
> **Sources:** CME Group public pages (scrapable), World Gold Council public data (scrapable)
> **Status:** F22.1-F22.3 complete (COMEX data module + metals detail popup + CLI). Full integration shipped.

### F23: Economic Release Calendar (Enhanced) ✅ COMPLETE
> **Goal:** Upgrade F12 calendar from sample data to live free sources. Show upcoming releases with countdown, impact ratings, previous/forecast/actual.
> **Sources:** Scrape TradingEconomics calendar (public page), or FRED release schedule API (free), or Finnhub (if user has free key — optional)
> **Status:** F23.1 (TradingEconomics scraper), F23.2 (header countdown), F23.3 (Economy tab panel) all shipped.

### F24: Government Data Direct (BLS + BEA) ✅ COMPLETE
> **Goal:** Pull employment, inflation, and GDP data directly from US government APIs. These are the actual source — not third-party repackaging.
> **Sources:** BLS API v1 (no key, 10 calls/day)
> **Status:** F24.1-F24.2 complete. BLS data module + Economy tab indicators panel shipped.

### F25: World Bank & Global Macro ✅ COMPLETE
> **Goal:** Structural macro data for BRICS and major economies. GDP growth, debt/GDP, trade balances, reserves.
> **Source:** World Bank Open Data API (free, no key, unlimited)
> **Status:** F25.1-F25.3 complete. Data module, cache, Economy tab panel, and CLI all shipped.

---

### TUI Layout Vision (Post-P0)

The homepage a finance enthusiast opens every morning:

```
┌─ HEADER ─────────────────────────────────────────────────────────────────────┐
│ pftui  $368.3k +1.4%  │ F&G: 🔴10 Extreme Fear │ TradFi: 🟡42 Fear        │
│ SPX ▼-0.8% │ NDX ▼-1.0% │ VIX ▲+3.5% │ Gold ▼-3.0% │ Oil ▲+1.9% │ BTC ▲+7.6% │
│ 📰 Reuters: Iran threatens Hormuz closure extension │ Next: NFP in 1d 18h   │
├─ [1]Pos [2]Tx [3]Mkt [4]Econ [5]Watch [6]News [7]Journal ──────────────────┤
│                                                                              │
│  POSITIONS (or WATCHLIST if configured)     │  ASSET DETAIL / CHART          │
│  ─────────────────────────────────────────  │  ────────────────────────────── │
│  Asset    Price   Day%  Alloc  RSI  COT    │  Gold (GC=F) — $5,139          │
│  USD      1.00    ---   48.7%  ---  ---    │  ▄▃▅▇█▆▅▃▂▃▅▆▇▅▃  52W: 78%    │
│  Gold     5139   -3.0%  24.9%  56▼  🟢    │                                 │
│  BTC      73705  +4.1%  20.0%  47▲  ---    │  COT: Managed Money Net Long    │
│  Silver   83.64  -4.9%  6.1%   50▼  ⚠️    │  142k (+8k) | Commercials: -89k │
│  U.UN     20.17  -4.0%  0.1%   42▼  ---    │  COMEX: 298M oz registered ▼    │
│                                             │  ETF Flows: n/a (commodity)     │
│  ALLOCATION                                 │                                 │
│  ████████ Cash 49% ███████ Comd 31%        │  PREDICTIONS                    │
│  ██████ Crypto 20%                          │  Gold >$6k by Dec? 38% ↑       │
│                                             │  US recession 2026? 22% →      │
│  MOVERS (>3%)                               │  Fed cut before July? 12% ↓    │
│  🔴 URA -7.7% │ COPX -6.9% │ CCJ -6.6%   │                                 │
├─────────────────────────────────────────────┴─────────────────────────────────┤
│ 📰 LATEST: Iran threatens extended Hormuz closure | BTC ETF +$458M daily    │
│ ISM Services 56.1 beats | ADP +63k, Jan revised to 11k                      │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Key UX decisions:**
- **Header is the pulse:** Portfolio value, F&G gauges, market ticker, news ticker, next economic event countdown — always visible, never need to switch tabs
- **COT column in positions table:** Single emoji signal (🟢 aligned / 🔴 divergent / ⚠️ extreme) — detail in popup on Enter
- **Predictions panel in sidebar:** Top 3-5 relevant prediction market odds, rotates based on which asset is selected (select gold → show gold-related predictions)
- **News tab [6] is the NEW addition** — replaces agent web-scraping for overnight catchup
- **Asset detail popup is the deep dive:** COT positioning, COMEX supply data, ETF flows, per-asset news, prediction markets — all contextual to the selected asset

---

## P1 — Analytics Foundation

### F8: Journal & Decision Log (PROMOTED from P2)
> **Goal:** Structured trade journal in SQLite. Hotkey popup in TUI. Full CLI suite for agents to seed, query, search. Replaces JOURNAL.md as primary decision log for agents.
> **Spec:** `docs/ANALYTICS-SPEC.md#f8`
> **Rationale:** Agents currently read/write a 1000+ line JOURNAL.md with fragile `head`/`tail`/`sed` commands. Evening Planner has consecutive edit failures on large markdown files — same class of problem. SQLite-backed journal eliminates the biggest reliability risk in the agent system. Also enables structured querying (by tag, symbol, date range, conviction) that markdown can never provide.

- [x] **F8.3: JOURNAL.md migration script** — One-time parser that seeds SQLite from existing JOURNAL.md entries with correct timestamps, tags, statuses. Files: `src/commands/migrate_journal.rs`, `src/cli.rs`, `src/main.rs`

### F4: Portfolio Risk & Scenario Engine (PROMOTED from P2)
> **Goal:** Portfolio-level risk metrics + multi-asset "what-if" scenario modeling with cascading impacts.
> **Spec:** `docs/ANALYTICS-SPEC.md#f4`
> **Rationale:** The user holds extreme views both ways on every asset and maintains 8 named macro scenarios. Making scenario analysis computational ("what is portfolio value if BTC $40k + Gold $6k" vs "BTC $150k + S&P -40%") maps directly to the decision framework. Currently lives as prose in SCENARIOS.md — should be interactive.

- [x] **F4.1: Risk metrics module** — Annualized volatility, max drawdown, Sharpe ratio (vs FFR), historical VaR (95%), Herfindahl concentration index. Files: new `src/analytics/{mod,risk}.rs`
- [x] **F4.2: Scenario engine** — Named macro scenarios with per-asset impact multipliers. Presets: "Oil $100", "BTC $40k", "Gold $6000", "2008 GFC", "1973 Oil Crisis". Custom: `--what-if "gold:-10%,btc:-20%"`. Files: new `src/analytics/scenarios.rs`, modify `commands/summary.rs`
- [x] **F4.3: Analytics tab [6] in TUI** — New tab. Risk panel (gauges + color coding), concentration chart, scenario selector with interactive parameter tweaking, projected portfolio value. Files: new `tui/views/analytics.rs`, `app.rs` (add ViewMode::Analytics)
- [x] **F4.4: Risk summary in `brief`** — 1-line risk summary: volatility, VaR, concentration flag. Files: `commands/brief.rs`

### F15: Configurable Homepage & Tab Layout
> **Goal:** First-run setup lets user choose their default homepage (Portfolio or Watchlist). The non-default view becomes a sub-tab on tab [1]. Not all users are portfolio-first — some want a watchlist/market scanner as their primary view.

- [x] **F15.1: First-run homepage prompt** — On first launch (no config exists), prompt: "Default homepage: [P]ortfolio or [W]atchlist?" Store choice in config.toml or SQLite settings table. Files: `src/config.rs` or `src/db/settings.rs`, `src/app.rs`
- [x] **F15.2: Dual sub-tabs on homepage** — Tab [1] gets two sub-views accessible via `Tab` key or left/right arrows: the default view (Portfolio or Watchlist) and the secondary view. Both share the same tab position but swap content. Header shows active sub-tab indicator. Files: `src/app.rs`, `src/tui/ui.rs`, `src/tui/views/positions.rs`, `src/tui/views/watchlist.rs`

### F16: Full Chart Search (Enhanced `/` Search)
> **Goal:** The `/` search overlay becomes the primary interface for looking up ANY symbol — not just held/watched assets. Searching "TSLA" should show a full chart + key data even if TSLA isn't in your portfolio or watchlist. Think Bloomberg's `TSLA <GO>`.

- [x] **F16.1: Search with live price fetch** — When `/` search matches a symbol not in portfolio or watchlist, fetch price data on-the-fly from Yahoo Finance. Show: current price, day change, 52W range. Files: `src/tui/views/search_overlay.rs`, `src/price/mod.rs`
- [x] **F16.2: Search result chart popup** — After selecting a search result, open a full-screen chart popup (reuse existing price_chart widget) with braille price history, RSI, volume if available. Same quality as the chart shown for held positions. `Esc` returns to previous view. Files: `src/tui/views/search_overlay.rs`, new `src/tui/views/search_chart_popup.rs`, `src/tui/widgets/price_chart.rs`
- [x] **F16.3: Quick-add from search** — From the search chart popup, `w` to add to watchlist, `a` to add a transaction. Seamless flow: search → chart → decide → add. Files: `src/tui/views/search_chart_popup.rs`, `src/db/watchlist.rs`, `src/commands/add_tx.rs`

### Other P1

- [ ] **Native multi-currency with live FX conversion** — Store non-USD currencies natively, convert via live FX rates. Show FX rate and currency risk flag. Large effort — split into sub-tasks. Files: `models/position.rs`, `price/mod.rs`, `commands/summary.rs`, `widgets/header.rs`
- [ ] **Ultra-wide layout (160+ cols)** — Third column: market context panel. Layout: 45% positions / 25% market / 30% chart. Files: `tui/ui.rs`, new `widgets/market_context.rs`
- [x] **thinkorswim UX research** — ✅ DONE. Research ToS layout, charts, scanners, analytics, shortcuts. Document what translates to TUI. Output: `docs/RESEARCH-THINKORSWIM.md` (2026-03-06)
- [x] **Theme visual audit** — ✅ DONE. Audited all 11 themes. Fixed 12 issues: gain/loss distinguishability (5 themes), text_muted visibility (7 themes). All themes now pass contrast and color separation thresholds. Files: `theme.rs`

### ToS-Inspired UX Improvements (from RESEARCH-THINKORSWIM.md)
> Research complete 2026-03-06. thinkorswim UX excellence: linked views, extreme customization, keyboard-first, filter-based discovery. Items ranked by impact × feasibility.

**High Priority (Quick Wins — 3-4 hours total):**
- [ ] **Symbol linking (30 min, P1)** — Selected position propagates to chart/detail/watchlist. Add `selected_symbol: Option<String>` to App state. All views read it. Files: `app.rs`, `views/positions.rs`, `widgets/price_chart.rs`, `views/watchlist.rs`
- [ ] **Hotkeys for existing features (15 min, P1)** — `B`=benchmark overlay, `<`/`>`=cycle timeframes, `D`=detail pane, `A`=alert, `T`=target, `J`=journal. Files: `app.rs`
- [ ] **Benchmark comparison chart (45 min, P1)** — Plot position vs SPY, dual braille lines (green + gray). Toggle with `B`. Files: `widgets/price_chart.rs`, `price/mod.rs`
- [ ] **Persist chart timeframe (30 min, P1)** — Save per-position in `chart_state` table. Restore on next view. Files: `db/schema.rs`, `app.rs`
- [ ] **Split-pane view (1 hr, P1)** — Bottom 30% = chart + txs + news for selected position. Toggle with `D`. Files: `tui/ui.rs`, new `views/position_detail_pane.rs`, `app.rs`

**Medium Priority (High Value — 6-8 hours total):**
- [ ] **Scanner with filter DSL (1 hr, P2)** — `pftui scan --filter "allocation_pct > 10" --filter "day_change_pct > 2"`. Files: new `commands/scan.rs`, `cli.rs`
- [ ] **Interactive scan builder (1 hr, P2)** — `:scan` opens modal, [A]dd/[R]emove/[S]ave/[L]oad filters. Files: new `views/scan_builder.rs`, `app.rs`
- [ ] **Saveable scan queries (45 min, P2)** — Store in `scan_queries` table. `:scan save my_scan`, `:scan load my_scan`. Files: `db/schema.rs`, new `db/scan_queries.rs`, `commands/scan.rs`
- [ ] **SMA overlay on charts (45 min, P2)** — Config: `chart_sma = [20, 50, 200]`. Faint braille lines. Files: `widgets/price_chart.rs`, `config.rs`
- [ ] **Volume sub-chart (45 min, P2)** — Below price (3 rows), braille bars. Toggle with `V`. Files: `widgets/price_chart.rs`, `models/price.rs`
- [ ] **Watchlist column customization (30 min, P2)** — Config: `watchlist.columns = [...]`. Files: `config.rs`, `views/watchlist.rs`
- [ ] **Watchlist groups (45 min, P2)** — Multiple named watchlists, switch with `W` + 1/2/3. Files: `db/schema.rs`, new `db/watchlist_groups.rs`, `app.rs`
- [ ] **Positions sub-modes (30 min, P2)** — `G`=group by category, `A`=sort by allocation, `P`=sort by performance. Files: `app.rs`, `views/positions.rs`
- [ ] **Command palette (1 hr, P2)** — `:` opens vim-style command mode. Autocomplete. Files: new `views/command_palette.rs`, `app.rs`
- [ ] **Context-sensitive hotkey hints (30 min, P2)** — Bottom bar shows available actions. Files: `widgets/status_bar.rs`, `app.rs`
- [ ] **Breadcrumb nav (20 min, P2)** — Top shows `Positions → AAPL → Detail`. Files: `widgets/header.rs`, `app.rs`
- [ ] **Auto-refresh timer (45 min, P2)** — Config: `auto_refresh = true`, `refresh_interval_secs = 300`. Files: `price/mod.rs`, `config.rs`

**Low Priority (Nice-to-Have — 3-4 hours total):**
- [ ] **Workspace presets (1 hr, P3)** — Config: `layout = "compact" | "split" | "analyst"`. Files: `config.rs`, `tui/ui.rs`
- [ ] **"Chart All Positions" grid (1.5 hr, P3)** — Mini braille charts, 6-9 per screen. New view mode `8`. Files: new `views/chart_grid.rs`
- [ ] **Link watchlist to main (30 min, P3)** — Select watchlist symbol → opens detail. Files: `views/watchlist.rs`, `app.rs`
- [ ] **Inline watchlist actions (30 min, P3)** — `a`=alert, `c`=chart, `r`=remove. Files: `views/watchlist.rs`, `app.rs`
- [ ] **Scan-triggered alerts (45 min, P3)** — Alert when scan results change. Files: `alerts/engine.rs`, `db/scan_queries.rs`
- [ ] **Onboarding tour (1 hr, P3)** — First-run walkthrough. Check `~/.config/pftui/onboarding_complete`. Files: new `views/onboarding.rs`

## P2 — Analytics Expansion

### F2: Correlation Matrix
> **Goal:** Rolling Pearson correlation between assets. Identify diversification, crowded trades, correlation breaks.
> **Spec:** `docs/ANALYTICS-SPEC.md#f2`


- [ ] **F2.2: Correlation grid in Markets tab** — Color-coded matrix (green=positive, red=negative). Held assets + key macro indicators. Toggle 7d/30d/90d. Files: `tui/views/markets.rs`, new `tui/views/correlation_grid.rs`
- [ ] **F2.3: Correlations in `brief --correlations`** — Top pairs + any active breaks. Files: `commands/brief.rs`

### F12: Economic Calendar
> **Goal:** Upcoming market-moving events (FOMC, CPI, NFP, earnings) with impact ratings. Integrates into existing Economy tab [4].
- [ ] **F12.2: Calendar in Economy tab [4]** — Right-side panel or sub-view showing next 7 days of events. Impact color-coded (🔴 high, 🟡 medium, ⚪ low). Countdown to next event. Earnings dates for watchlist stocks highlighted. Files: `tui/views/economy.rs`
- [ ] **F12.3: `pftui calendar` CLI** — `pftui calendar` (next 7 days), `--days 30`, `--impact high`, `--json`. Files: new `src/commands/calendar.rs`, `cli.rs`

### F13: Position Annotations & Thesis Tracking
> **Goal:** Attach entry thesis, invalidation criteria, review dates, and target levels to positions. Per-position structured notes that agents can query instead of reading JOURNAL.md open calls.

- [ ] **F13.1: Annotations DB + CLI** — SQLite table: symbol, thesis, invalidation, review_date, target_add, target_sell, conviction, updated_at. CLI: `pftui annotate GC=F --thesis "..." --invalidate "..." --review-date 2026-03-20 --target-sell 6000`. `pftui annotate GC=F --json` returns full annotation. Files: new `src/db/annotations.rs`, new `src/commands/annotate.rs`, `cli.rs`
- [ ] **F13.2: Thesis section in position detail popup** — Existing asset detail popup gains "Thesis" section: entry thesis, invalidation, review date (color-coded if approaching/overdue), target levels with distance. Editable inline. Files: `tui/views/asset_detail_popup.rs`, `tui/views/position_detail.rs`
- [ ] **F13.3: Review date alerts** — Positions with overdue review dates show ⏰ icon in Positions tab. Integrates with F6 alert engine — auto-creates alert when review date is set. Files: `alerts/engine.rs`, `tui/views/positions.rs`

### F14: Tag-Based Asset Groups
> **Goal:** Group assets by theme for combined performance tracking.

- [ ] **F14.1: Groups DB + CLI** — SQLite table: group_name, symbols (comma-separated). CLI: `pftui group create "hard-assets" --symbols GC=F,SI=F,BTC`, `pftui group list`, `pftui group "hard-assets"` (combined allocation + performance), `--json`. Files: new `src/db/groups.rs`, new `src/commands/group.rs`, `cli.rs`
- [ ] **F14.2: Group filter in Positions tab** — Filter positions by group. Allocation bars show group-level allocation. Files: `tui/views/positions.rs`, `tui/widgets/allocation_bars.rs`

### F15 & F16: See P1
> F15 (Configurable Homepage) and F16 (Full Chart Search) are defined in P1.

### Other P2
- [ ] **[Feedback] Fix USD/JPY and USD/CNY in macro dashboard** — Yahoo Finance FX feed for JPY=X and CNY=X is broken (returns 1.00). Upgraded yahoo_finance_api to v4 (didn't fix it). Solution: add fallback FX API module using exchangerate-api.com (free, 1500/mo) or frankfurter.app (free, unlimited). Files: new `src/data/fx_fallback.rs`, `src/price/mod.rs` (fallback logic), `src/commands/refresh.rs`
- [ ] **[Feedback] Alerts in `brief` output** — Show any triggered or near-threshold alerts in the brief command output. Connects alert engine to the primary agent-consumed command. Files: `commands/brief.rs`, `alerts/engine.rs`
- [ ] **[Feedback] After-hours / pre-market prices** — Show AH/pre-market prices in watchlist and brief for market close routines. Yahoo Finance provides extended hours data. Files: `src/price/yahoo.rs`, `commands/brief.rs`, `commands/watchlist_cli.rs`
- [ ] **[Feedback] `pftui sector` command** — Show sector ETF performance (XLE, ITA, XLF, IGV, etc.) for tracking sector-level moves and capital flow identification during regime shifts. Files: new `src/commands/sector.rs`, `cli.rs`
- [ ] **[Feedback] Add "What Changed Today" section to `brief`** — Show largest daily movers, notable threshold crossings, and any triggered alerts in the brief output. Files: `commands/brief.rs`
- [ ] **[Feedback] Benchmark comparison in `brief`** — Show portfolio performance vs SPY, Gold index, or custom benchmark. Files: `commands/brief.rs`, `price/mod.rs`
- [ ] **News feed integration** — Free RSS/API source (Yahoo Finance RSS, Finnhub). Scrollable list with per-asset filtering. Files: new `src/news/`, new `views/news.rs`
- [ ] **Candlestick chart variant** — OHLC braille/block candlesticks. Requires OHLC in HistoryRecord. Files: `models/price.rs`, `price/yahoo.rs`, `price_chart.rs`
- [x] **Web interface (`pftui web`)** — axum server with REST API, lightweight vanilla JS/HTML/CSS frontend embedded in binary. TradingView Advanced Chart Widget for interactive charting. Bearer token auth (auto-generated, optional --no-auth). Dark theme, responsive layout, 9 API endpoints, click-to-chart, auto-refresh. Completed 2026-03-04. Files: `src/web/{mod,api,auth,server}.rs`, `src/web/static/index.html`, `Cargo.toml`, `cli.rs`, `main.rs`. **Note:** Core layer was NOT extracted — web API directly uses existing db/models functions. No code duplication. Future: Add API endpoint tests, PID file management, systemd service template.
- [ ] **[Feedback] Fix data pipeline for "complete" P0 features** — ✅ Predictions FIXED (2026-03-06 18:42 UTC). ✅ News FIXED (2026-03-06 19:30 UTC). Remaining: etf-flows (F21), COT (F18), on-chain still failing. Must audit remaining: verify fetch → parse → cache → display works with live data sources. Files: `src/commands/refresh.rs`, `src/data/onchain.rs`, `src/data/cot.rs`
- [ ] **[Feedback] Standardize CLI `--json` flag** — Three different conventions exist: `--json` (export, macro), `--agent` (brief), and no JSON support (summary, value, performance). Unify all data-output commands to support `--json` with consistent structure (top-level `data`, `meta`, `timestamp` keys). Deprecate `--agent` in favor of `--json`. Files: `src/cli.rs`, `src/commands/brief.rs`, `src/commands/summary.rs`, `src/commands/value.rs`, `src/commands/performance.rs`
- [ ] **[Feedback] `pftui status` or `pftui refresh --status`** — Show data health: last refresh timestamp, cached data age per source (prices, predictions, news, COT, BLS, calendar), pending alerts count. Agent health check. Files: new `src/commands/status.rs`, `cli.rs`
- [ ] **[Feedback] Stale data indicator in TUI header** — Show ⚠ Stale (Xh ago) if cached data >1h old, with hint 'Press r to refresh'. Files: `src/tui/widgets/header.rs`
- [ ] **[Feedback] `pftui close` / `pftui eod` command** — Purpose-built market close command combining brief + movers + macro + sentiment in one output. Files: new `src/commands/eod.rs`, `cli.rs`
- [ ] **[Feedback] Brent crude alongside WTI in macro dashboard** — Add Brent-WTI spread as key metric during geopolitical crises. Files: `src/commands/macro_cmd.rs`, `src/tui/views/economy.rs`
- [ ] **[Feedback] Portfolio stress testing via CLI** — `pftui stress-test` showing portfolio impact under named scenarios (DXY 100, oil $90, BTC $40k). Builds on F4.2 scenario engine. Files: new `src/commands/stress_test.rs`, `src/analytics/scenarios.rs`
- [ ] **Snap/AUR/Scoop publishing** — Snap: needs Snapcraft account + SNAPCRAFT_TOKEN. AUR: needs account + AUR_SSH_KEY. Scoop: needs Windows binary first. Files: `snap/snapcraft.yaml`, `.github/workflows/release.yml`
- [ ] **Windows build support** — Add x86_64-pc-windows-msvc to release matrix. Files: `.github/workflows/release.yml`

## P3 — Intelligence Layer

### F5: Central Bank & Sovereign Holdings Tracker
> **Goal:** The differentiator. No other TUI tracks institutional gold/BTC/silver flows.
> **Spec:** `docs/ANALYTICS-SPEC.md#f5`

- [ ] **F5.1: Sovereign data module** — Curated data store for CB gold (WGC monthly), government BTC (bitcointreasuries.net API), COMEX silver inventory (CME). Update cadence: monthly for gold, weekly for BTC, daily for COMEX. Files: new `src/data/{sovereign,comex,wgc}.rs`, new `src/db/sovereign_cache.rs`
- [ ] **F5.2: Sovereign Holdings in Economy tab** — Gold CB bar chart + purchase streak. BTC government + corporate holdings bar. Silver COMEX registered + coverage ratio. Gold-USD crossover progress bar ($5,790 threshold). Files: `tui/views/economy.rs`
- [ ] **F5.3: Sovereign data in `macro` CLI** — `pftui macro --sovereign` or default inclusion. Files: `commands/macro_cmd.rs`

### Other P3

- [ ] **Dividend tracking** — Payments, yield, ex-dates
- [ ] **Multi-portfolio support** — Named portfolios with switching
- [ ] **Custom keybinding config** — User-configurable in config.toml
- [ ] **Sector heatmap** — Treemap-style sector performance view
- [ ] **Options chains** — If a free data source exists

## Feedback Summary

**Last reviewed:** 2026-03-06T03:00Z

| Tester | Latest Score | Trend | Key Pain Point |
|---|---|---|---|
| Sentinel Main (TUI) | 78% | ↑↓ (40→78→82→82→78) | P&L dollar amounts, economy tab expansion, data loading issues |
| Evening Planner (CLI) | 85% | ↑↓ (38→85→92→85) | BTC ETF flow data, stress testing, FRED integration |
| Market Research (CLI) | 78% | → (40→72→78→78) | RSI/MACD/SMA, etf-flows/news/predictions not populating |
| Market Close (CLI) | 72% | ↑↓ (68→80→72) | Movers/watchlist sign discrepancy (BUG), RSI/MACD, stub features empty |
| UX Analyst (NEW) | 68% | NEW | Data availability gap, CLI JSON flag inconsistency, stub features |

**Lowest scorer:** UX Analyst at 68% — new tester focused on UX cohesion. Primary concern: features marked "complete" (predictions, news, etf-flows, COT, performance) don't populate with real data in practice, creating a trust gap.

**Score trajectory:** REGRESSION across the board. Sentinel Main dropped 82→78, Evening Planner dropped 92→85, Market Close dropped 80→72. Root cause: data availability gap. Many P0 features are marked complete but don't actually populate data reliably (predictions empty, news empty, etf-flows empty, COT unavailable, performance N/A). Testers hit "No cached data. Run pftui refresh" loops where refresh doesn't fix the issue. Scores will not recover until data actually flows through the features that were shipped.

**Top 3 priorities from feedback:**
1. **FIX DATA PIPELINE** (P0, critical) — Predictions, news, etf-flows, COT all return empty after `pftui refresh`. These are marked ✅ COMPLETE but don't work end-to-end. Trust-breaking. Must verify data flows from source → cache → display for every "complete" P0 feature.
2. **Standardize CLI JSON output** (P2) — UX Analyst flagged three different conventions: `--json` (export, macro), `--agent` (brief), and no JSON support (summary, value, performance). Standardize all data-output commands to `--json`.
3. **Fix USD/JPY and USD/CNY in macro dashboard** (P2) — Yahoo Finance FX feed broken for these pairs, returns 1.00. Need fallback FX API.

**Completed since last review:** F19.2-F19.4 (sentiment header + sparklines + CLI), F20.1-F20.5 (RSS news full stack), F21.1-F21.3 (on-chain + ETF flows), F22.1-F22.3 (COMEX supply), F23.1-F23.3 (economic calendar), F24.1-F24.2 (BLS data), F25.1-F25.3 (World Bank data), F4.1-F4.4 (risk + scenarios + analytics tab), F15.1-F15.2 (configurable homepage), F16.1-F16.3 (full chart search), F8.3 (journal migration), F2.1 (correlation math), web parity phases, movers 1D fix, UX overhaul

**Release status:** v0.5.0 already tagged and released. Since then: 86 commits including F2.1 (correlations), F4.1-F4.4 (analytics), F15-F16 (homepage + search), F8.3 (journal migration), web parity (auth + SSE + overlays + contract tests). However, 2 test failures and 7 clippy warnings currently block a release. **Not release-ready — fix tests and clippy first, then release as v0.6.0.**

**Homebrew Core:** 1 star — needs 50+ for homebrew-core submission. Not eligible yet.
