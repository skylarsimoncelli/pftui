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

- [ ] **F17.2: Predictions panel in Economy tab [4]** — Right-side panel or sub-view. Show top 10 relevant markets: "Fed rate cut by June?" 34% ↓, "US recession 2026?" 22% ↑, "BTC above $100k by Dec?" 45% →. Color-code by probability (green >60%, red <40%, yellow middle). 24h change arrows. Sort by volume or relevance. Files: `tui/views/economy.rs`
- [ ] **F17.3: `pftui predictions` CLI** — `pftui predictions` (top markets), `--category crypto`, `--search "recession"`, `--json`. Files: new `src/commands/predictions.rs`, `cli.rs`
- [ ] **F17.4: Prediction sparklines in Markets tab** — For key tracked predictions (recession, rate cuts, BTC price), show probability sparkline over 30 days alongside traditional asset charts. Files: `tui/views/markets.rs`

### F18: CFTC Commitments of Traders (COT)
> **Goal:** Show what the smart money is doing. Commercials vs speculators positioning on gold, silver, oil, BTC futures. Updated weekly.
> **Source:** CFTC Socrata API (`publicreporting.cftc.gov`, free, no key, JSON)
> **Data:** Net positions by trader type (commercial, non-commercial/managed money), open interest, week-over-week changes

- [ ] **F18.1: COT data module** — Fetch from CFTC Socrata API. Parse disaggregated futures report. Map contract codes to pftui symbols (GC=Gold, SI=Silver, CL=Oil, BTC=Bitcoin). Cache in SQLite, weekly refresh (data updates every Friday). Files: new `src/data/cot.rs`, new `src/db/cot_cache.rs`
- [ ] **F18.2: COT section in asset detail popup** — When viewing gold/silver/oil/BTC, show: "Managed Money: Net Long 142k contracts (+8k) | Commercials: Net Short -89k (-3k)". Bar chart of net positioning. 4-week trend. Extremes flagged (>90th percentile = crowded trade warning). Files: `tui/views/asset_detail_popup.rs`
- [ ] **F18.3: COT summary in Markets tab** — Compact row per commodity: symbol, managed money net, commercial net, change, signal (🟢 aligned with trend / 🔴 divergence / ⚠️ extreme). Files: `tui/views/markets.rs`
- [ ] **F18.4: `pftui cot` CLI** — `pftui cot` (all tracked), `--symbol gold`, `--weeks 12` (historical), `--json`. Files: new `src/commands/cot.rs`, `cli.rs`

### F19: Fear & Greed Index Integration
> **Goal:** Crypto + traditional market sentiment gauges, natively in the TUI.
> **Sources:** Alternative.me Crypto F&G (free, no key), CNN F&G (scrape — public page)
> **Data:** Index value (0-100), classification (Extreme Fear/Fear/Neutral/Greed/Extreme Greed), historical values

- [ ] **F19.1: Sentiment data module** — Fetch crypto F&G from `https://api.alternative.me/fng/`. Scrape CNN F&G from public page (or derive from VIX + put/call + junk spread + breadth + momentum + safe haven — all calculable from existing data). Cache in SQLite, 1-hour TTL. Files: new `src/data/sentiment.rs`, new `src/db/sentiment_cache.rs`
- [ ] **F19.2: Sentiment gauges in header/status bar** — Compact display: `F&G: 🔴10 Extreme Fear | TradFi: 🟡42 Fear`. Always visible. Color-coded. Files: `tui/widgets/status_bar.rs` or `tui/widgets/header.rs`
- [ ] **F19.3: Sentiment history in Economy tab** — 30-day sparkline of both indices. Overlay with portfolio value sparkline to show correlation/divergence. Files: `tui/views/economy.rs`
- [ ] **F19.4: `pftui sentiment` CLI** — `pftui sentiment` (current), `--history 30` (days), `--json`. Files: new `src/commands/sentiment.rs`, `cli.rs`

### F20: Live News Feed (RSS)
> **Goal:** Zero-cost, zero-key financial news aggregation from the best sources. Filterable by topic, searchable, in a dedicated News view.
> **Sources:** RSS feeds — completely free, no auth, no rate limits.
> **Feeds:** Reuters (`reuters.com/rssfeed/`), CoinDesk (`coindesk.com/arc/outboundfeeds/rss/`), ZeroHedge (`zerohedge.com/fullrss.xml`), The Block, Yahoo Finance, MarketWatch, CNBC, Seeking Alpha, Bloomberg (headlines)

- [ ] **F20.1: RSS aggregator module** — Poll configured RSS feeds on 10-min interval. Parse titles, links, published dates, source. Deduplicate by URL. Store in SQLite with 48-hour retention. Default feed list ships with pftui (user can add/remove via config). Files: new `src/data/rss.rs`, new `src/db/news_cache.rs`, `src/config.rs`
- [ ] **F20.2: News tab [6] in TUI** — New tab. Scrollable list: timestamp, source icon/tag, headline. Color-code by source category (macro=blue, crypto=orange, commodities=yellow, geopolitics=red). `Enter` to open URL in browser. `/` to search headlines. `f` to filter by source or category. Files: new `src/tui/views/news.rs`, `src/app.rs` (add ViewMode::News, bind key `6`)
- [ ] **F20.3: News ticker in header** — Scrolling one-line news ticker below the market bar showing latest 3 headlines. Cycles every 10 seconds. Files: `tui/widgets/header.rs` or new `tui/widgets/news_ticker.rs`
- [ ] **F20.4: `pftui news` CLI** — `pftui news` (latest 20), `--source coindesk`, `--search "bitcoin"`, `--hours 4`, `--json`. Files: new `src/commands/news.rs`, `cli.rs`
- [ ] **F20.5: Per-asset news in detail popup** — When viewing a position or watchlist item, filter news by asset name/ticker. Show last 5 relevant headlines. Files: `tui/views/asset_detail_popup.rs`

### F21: BTC On-Chain & ETF Flow Data
> **Goal:** On-chain signals and institutional flow data for BTC — whale movements, exchange flows, ETF inflows/outflows.
> **Sources:** Blockchair (5 req/sec free, no key), CoinGlass (scrape public pages), Whale Alert (limited free tier — scrape public feed)

- [ ] **F21.1: On-chain data module** — Fetch BTC exchange net flows from Blockchair (`https://api.blockchair.com/bitcoin/`). Scrape CoinGlass BTC ETF flow page for daily net inflows by fund. Parse Whale Alert public feed for transactions >$10M. Cache in SQLite. Files: new `src/data/onchain.rs`, new `src/db/onchain_cache.rs`
- [ ] **F21.2: BTC intelligence panel in asset detail** — When viewing BTC: ETF daily net flow (+$458M), 7-day cumulative, top fund flows (IBIT, FBTC, GBTC). Exchange net flow (negative = accumulation). Large whale transactions today. Files: `tui/views/asset_detail_popup.rs`
- [ ] **F21.3: `pftui etf-flows` CLI** — `pftui etf-flows` (today), `--days 7`, `--fund IBIT`, `--json`. Files: new `src/commands/etf_flows.rs`, `cli.rs`

### F22: COMEX & Commodity Supply Data
> **Goal:** Physical market data — COMEX inventory, delivery reports, supply/demand signals for metals.
> **Sources:** CME Group public pages (scrapable), World Gold Council public data (scrapable)

- [ ] **F22.1: COMEX data module** — Scrape CME daily bulletin for COMEX gold + silver registered/eligible inventory, delivery notices, warehouse stocks. Parse World Gold Council public data for central bank purchases (quarterly). Cache in SQLite. Files: new `src/data/comex.rs`, new `src/db/comex_cache.rs`
- [ ] **F22.2: Supply data in metals detail popup** — When viewing GC=F or SI=F: COMEX registered inventory (oz), registered/eligible ratio, daily delivery notices, trend (drawing down / building). For gold: CB net purchases last quarter. Files: `tui/views/asset_detail_popup.rs`
- [ ] **F22.3: `pftui supply` CLI** — `pftui supply gold` (COMEX + CB data), `pftui supply silver`, `--json`. Files: new `src/commands/supply.rs`, `cli.rs`

### F23: Economic Release Calendar (Enhanced)
> **Goal:** Upgrade F12 calendar from sample data to live free sources. Show upcoming releases with countdown, impact ratings, previous/forecast/actual.
> **Sources:** Scrape TradingEconomics calendar (public page), or FRED release schedule API (free), or Finnhub (if user has free key — optional)

- [ ] **F23.1: Calendar scraper** — Scrape public economic calendar pages for upcoming releases (FOMC, CPI, NFP, PPI, GDP, PMI, JOLTS, jobless claims). Parse: date, event name, previous value, forecast, impact level. Store in SQLite calendar_events table (F12.1 schema already exists). Files: `src/data/calendar.rs` (upgrade from sample data)
- [ ] **F23.2: Calendar countdown in header** — "Next: NFP in 2d 4h | CPI Mar 12". Always visible. High-impact events only. Files: `tui/widgets/header.rs`
- [ ] **F23.3: Calendar view in Economy tab** — 7-day forward view. Impact color-coded. Countdown timers. Previous/forecast columns. Actual filled in post-release. Files: `tui/views/economy.rs`

### F24: Government Data Direct (BLS + BEA)
> **Goal:** Pull employment, inflation, and GDP data directly from US government APIs. These are the actual source — not third-party repackaging.
> **Sources:** BLS API v2 (no key for v1: 10 calls/day, or free key for v2: 500/day), BEA API (free key required — SKIP for no-key constraint, but v1 BLS works without)

- [ ] **F24.1: BLS data module (no-key mode)** — BLS API v1 requires no registration. Fetch series: CPI-U (CUUR0000SA0), unemployment rate (LNS14000000), NFP (CES0000000001), average hourly earnings (CES0500000003). 10 calls/day limit — cache aggressively (data only changes monthly). Files: new `src/data/bls.rs`, new `src/db/bls_cache.rs`
- [ ] **F24.2: Enhanced Economy tab indicators** — Replace "sample" economic data with live BLS data. Show: CPI (YoY%, MoM%), unemployment rate, NFP (last + revision), average hourly earnings. Trend arrows. Last release date + next release countdown. Files: `tui/views/economy.rs`

### F25: World Bank & Global Macro
> **Goal:** Structural macro data for BRICS and major economies. GDP growth, debt/GDP, trade balances, reserves.
> **Source:** World Bank Open Data API (free, no key, unlimited)

- [ ] **F25.1: World Bank data module** — Fetch key indicators: GDP growth (NY.GDP.MKTP.KD.ZG), debt/GDP (GC.DOD.TOTL.GD.ZS), current account (BN.CAB.XOKA.GD.ZS), reserves (FI.RES.TOTL.CD) for US, China, India, Russia, Brazil, SA, UK, EU. Cache in SQLite with monthly refresh (data updates quarterly). Files: new `src/data/worldbank.rs`, new `src/db/worldbank_cache.rs`
- [ ] **F25.2: Global macro panel in Economy tab** — Compact table: Country, GDP Growth, Debt/GDP, Reserves trend. BRICS aggregate row. Color-code: green (expanding), red (contracting). Files: `tui/views/economy.rs`
- [ ] **F25.3: `pftui global` CLI** — `pftui global` (all tracked countries), `--country US`, `--indicator gdp`, `--json`. Files: new `src/commands/global.rs`, `cli.rs`

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

- [ ] **F8.2: Journal tab [7] in TUI** — New tab in numbered menu. Scrollable list: date, content (truncated), tag columns. `a` to add entry inline, Enter to expand full text, `/` to search within journal. Files: new `src/tui/views/journal.rs`, `src/app.rs` (add ViewMode::Journal, bind key `7`)
- [ ] **F8.3: JOURNAL.md migration script** — One-time parser that seeds SQLite from existing JOURNAL.md entries with correct timestamps, tags, statuses. Files: new `src/commands/migrate_journal.rs` or standalone script

### F4: Portfolio Risk & Scenario Engine (PROMOTED from P2)
> **Goal:** Portfolio-level risk metrics + multi-asset "what-if" scenario modeling with cascading impacts.
> **Spec:** `docs/ANALYTICS-SPEC.md#f4`
> **Rationale:** The user holds extreme views both ways on every asset and maintains 8 named macro scenarios. Making scenario analysis computational ("what is portfolio value if BTC $40k + Gold $6k" vs "BTC $150k + S&P -40%") maps directly to the decision framework. Currently lives as prose in SCENARIOS.md — should be interactive.

- [ ] **F4.1: Risk metrics module** — Annualized volatility, max drawdown, Sharpe ratio (vs FFR), historical VaR (95%), Herfindahl concentration index. Files: new `src/analytics/{mod,risk}.rs`
- [ ] **F4.2: Scenario engine** — Named macro scenarios with per-asset impact multipliers. Presets: "Oil $100", "BTC $40k", "Gold $6000", "2008 GFC", "1973 Oil Crisis". Custom: `--what-if "gold:-10%,btc:-20%"`. Files: new `src/analytics/scenarios.rs`, modify `commands/summary.rs`
- [ ] **F4.3: Analytics tab [6] in TUI** — New tab. Risk panel (gauges + color coding), concentration chart, scenario selector with interactive parameter tweaking, projected portfolio value. Files: new `tui/views/analytics.rs`, `app.rs` (add ViewMode::Analytics)
- [ ] **F4.4: Risk summary in `brief`** — 1-line risk summary: volatility, VaR, concentration flag. Files: `commands/brief.rs`

### F15: Configurable Homepage & Tab Layout
> **Goal:** First-run setup lets user choose their default homepage (Portfolio or Watchlist). The non-default view becomes a sub-tab on tab [1]. Not all users are portfolio-first — some want a watchlist/market scanner as their primary view.

- [ ] **F15.1: First-run homepage prompt** — On first launch (no config exists), prompt: "Default homepage: [P]ortfolio or [W]atchlist?" Store choice in config.toml or SQLite settings table. Files: `src/config.rs` or `src/db/settings.rs`, `src/app.rs`
- [ ] **F15.2: Dual sub-tabs on homepage** — Tab [1] gets two sub-views accessible via `Tab` key or left/right arrows: the default view (Portfolio or Watchlist) and the secondary view. Both share the same tab position but swap content. Header shows active sub-tab indicator. Files: `src/app.rs`, `src/tui/ui.rs`, `src/tui/views/positions.rs`, `src/tui/views/watchlist.rs`

### F16: Full Chart Search (Enhanced `/` Search)
> **Goal:** The `/` search overlay becomes the primary interface for looking up ANY symbol — not just held/watched assets. Searching "TSLA" should show a full chart + key data even if TSLA isn't in your portfolio or watchlist. Think Bloomberg's `TSLA <GO>`.

- [ ] **F16.1: Search with live price fetch** — When `/` search matches a symbol not in portfolio or watchlist, fetch price data on-the-fly from Yahoo Finance. Show: current price, day change, 52W range. Files: `src/tui/views/search_overlay.rs`, `src/price/mod.rs`
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

- [ ] **[Feedback] `pftui sector` command** — Show sector ETF performance (XLE, ITA, XLF, IGV, etc.) for tracking sector-level moves. Files: new `src/commands/sector.rs`, `cli.rs`
- [ ] **[Feedback] Add "What Changed Today" section to `brief`** — Show largest daily movers, notable threshold crossings, and any triggered alerts in the brief output. Files: `commands/brief.rs`
- [ ] **[Feedback] Benchmark comparison in `brief`** — Show portfolio performance vs SPY, Gold index, or custom benchmark. Files: `commands/brief.rs`, `price/mod.rs`
- [ ] **News feed integration** — Free RSS/API source (Yahoo Finance RSS, Finnhub). Scrollable list with per-asset filtering. Files: new `src/news/`, new `views/news.rs`
- [ ] **Candlestick chart variant** — OHLC braille/block candlesticks. Requires OHLC in HistoryRecord. Files: `models/price.rs`, `price/yahoo.rs`, `price_chart.rs`
- [ ] **Web interface (`pftui web`)** — axum/warp server, shared core layer, REST API, lightweight JS frontend. TradingView embedded charts (Advanced Chart Widget) for interactive charting, fallback to SVG. Sub-tasks: 1) Extract core, 2) REST API, 3) Frontend + TradingView, 4) Auth/PID. Files: new `src/web/`, refactor `src/core/`, `cli.rs`
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

**Last reviewed:** 2026-03-04T03:00Z

| Tester | Latest Score | Trend | Key Pain Point |
|---|---|---|---|
| Sentinel Main (TUI) | 82% | ↑↑ (40→78→82) | P&L dollar amounts, sector grouping, cost basis in positions |
| Evening Planner (CLI) | 85% | ↑↑ (38→85) | Macro command, RSI/MACD for watchlist, correlations CLI |
| Market Research (CLI) | 72% | → (single point) | FX support, U.UN data accuracy, daily P&L, technicals |
| Market Close (CLI) | 68% | → (first review) | Macro dashboard, bulk watchlist, history cash, watchlist 1D% |

**Lowest scorer:** Market Close at 68% — top pain points: no macro dashboard (still using fetch_prices.py for DXY/VIX/10Y/oil/copper), no bulk watchlist add (20 separate calls needed), history omits cash (misleading totals), watchlist missing daily change column.

**Score trajectory:** All testers now in 68-85% range. Evening Planner had the biggest jump (+47 points) after headless features shipped (brief, refresh, value, watchlist, what-if, history). Sentinel Main continues climbing with TUI polish.

**Top 3 priorities from feedback:**
1. **Macro dashboard / `pftui macro`** (P1, F3.3-F3.4) — requested by 3 of 4 testers. Would eliminate fetch_prices.py dependency entirely. F3.1-F3.2 (FRED + refresh integration) already shipped.
2. **History cash inclusion** (P0) — Market Close reports `history --date` shows $184k instead of $362k because cash is omitted. Misleading for portfolio value tracking.
3. **Alert engine** (P1, F6) — all 4 testers want price/threshold alerts. Most impactful for workflow integration.

**Completed feedback items:** `pftui refresh`, `--period`, `--group-by`, day P&L (TUI + CLI), value/brief/watchlist/set-cash CLI, CSV rounding, base currency config, Markets tab enrichment, `--what-if`, `history --date`, snapshot, import, U.UN FX fix, `--technicals` flag, RSI column in positions/watchlist, MACD/RSI gauge in detail popup, rate limiting, macro symbols in refresh

**Release status:** v0.2.0 is current. Since then: F1.3 (RSI columns), F1.4 (--technicals), F3.1 (FRED API), F3.2 (macro refresh), rate limiting fix, install.sh. Tests: 855 passing, clippy clean. **Ready to release as v0.3.0.**

**Homebrew Core:** 1 star — needs 50+ for homebrew-core submission. Not eligible yet.
