# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Format: `- [ ] **Short title** — Brief description. Files: relevant_file.rs`
> Full analytics spec: `docs/ANALYTICS-SPEC.md`

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
- [ ] **F16.2: Search result chart popup** — After selecting a search result, open a full-screen chart popup (reuse existing price_chart widget) with braille price history, RSI, volume if available. Same quality as the chart shown for held positions. `Esc` returns to previous view. Files: `src/tui/views/search_overlay.rs`, new `src/tui/views/search_chart_popup.rs`, `src/tui/widgets/price_chart.rs`
- [ ] **F16.3: Quick-add from search** — From the search chart popup, `w` to add to watchlist, `a` to add a transaction. Seamless flow: search → chart → decide → add. Files: `src/tui/views/search_chart_popup.rs`, `src/db/watchlist.rs`, `src/commands/add_tx.rs`

### Other P1

- [ ] **Native multi-currency with live FX conversion** — Store non-USD currencies natively, convert via live FX rates. Show FX rate and currency risk flag. Large effort — split into sub-tasks. Files: `models/position.rs`, `price/mod.rs`, `commands/summary.rs`, `widgets/header.rs`
- [ ] **Ultra-wide layout (160+ cols)** — Third column: market context panel. Layout: 45% positions / 25% market / 30% chart. Files: `tui/ui.rs`, new `widgets/market_context.rs`
- [ ] **thinkorswim UX research** — Research ToS layout, charts, scanners, analytics, shortcuts. Document what translates to TUI. Output: `docs/RESEARCH-THINKORSWIM.md`, then add derived TODO items
- [ ] **Theme visual audit** — Audit all 11 themes across all views. Check: category colors, chart gradients, selection contrast, popup readability. Files: `theme.rs`, all views

## P2 — Analytics Expansion

### F2: Correlation Matrix
> **Goal:** Rolling Pearson correlation between assets. Identify diversification, crowded trades, correlation breaks.
> **Spec:** `docs/ANALYTICS-SPEC.md#f2`

- [ ] **F2.1: Correlation math module** — Pearson on daily returns. 7/30/90-day rolling windows. Break detection (|Δ30d-90d| > 0.3). Files: new `src/indicators/correlation.rs`
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

**Last reviewed:** 2026-03-05T03:00Z

| Tester | Latest Score | Trend | Key Pain Point |
|---|---|---|---|
| Sentinel Main (TUI) | 82% | ↑→ (40→78→82→82) | P&L dollar amounts, sector grouping, economy tab expansion |
| Evening Planner (CLI) | 92% | ↑↑ (38→85→92) | RSI/MACD/SMA for watchlist, stress testing, sector rotation |
| Market Research (CLI) | 78% | ↑ (40→72→78) | Movers 1D calc bug, RSI/MACD/SMA, F&G indices, news |
| Market Close (CLI) | 80% | ↑ (68→80) | Expand watchlist (11→50+), technicals on macro, fix USD/JPY+CNY, after-hours |

**Lowest scorer:** Market Research at 78% — top pain points: movers command shows multi-day changes instead of true 1D (bug), no RSI/MACD/SMA50 (still using fetch_prices.py), missing F&G indices and news integration.

**Score trajectory:** All testers now in 78-92% range. Evening Planner hit 92% — highest score ever — driven by macro dashboard being "THE most useful feature." Market Close jumped +12 points after macro, movers, and history improvements shipped. Sentinel Main plateaued at 82% — needs P&L dollar amounts and economy tab enrichment to break through.

**Top 3 priorities from feedback:**
1. **F19 Sentiment gauges (F&G indices)** (P0) — F19.1 data module done, need F19.2 (header display), F19.3 (history sparklines), F19.4 (CLI). Requested by Market Research and Evening Planner.
2. **Fix USD/JPY and USD/CNY data** (P2, bug) — Market Close reports both showing 1.0000 in macro dashboard. Broken data source needs investigation.
3. **Add RSI/MACD/SMA indicators to CLI commands** — Market Research and Evening Planner both requested technicals for watchlist/macro symbols.

**Completed since last review:** F17.2-F17.4 (predictions panel + sparklines + CLI), F18.1-F18.4 (COT data + popup + Markets column + CLI), F19.1 (sentiment data module), F23.2 (calendar countdown in header), F8.2 (journal tab), UX overhaul (unified timeframe control, clickable selector, P&L/Value columns), web dashboard

**Release status:** v0.4.1 is current. Since then: F17.2-F17.4 (predictions), F18.1-F18.4 (COT), F19.1 (sentiment), F23.2 (calendar countdown), F8.2 (journal tab), UX overhaul (timeframe selector, P&L/Value columns), website improvements. Tests: 1019 passing, clippy clean. **Significant feature work since v0.4.1 — ready to release as v0.5.0.**

**Homebrew Core:** 1 star — needs 50+ for homebrew-core submission. Not eligible yet.
