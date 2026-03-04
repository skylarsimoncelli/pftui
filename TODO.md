# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Format: `- [ ] **Short title** — Brief description. Files: relevant_file.rs`
> Full analytics spec: `docs/ANALYTICS-SPEC.md`

## P1 — Analytics Foundation

### F6: Unified Alert Engine (price + allocation + indicators)
> **Goal:** One alert system for everything: price targets (inc. watchlist entry levels), allocation drift, indicator thresholds. Check on every refresh. Optional OS notifications. Absorbs watchlist alerts (F9) and allocation drift (F11).
> **Spec:** `docs/ANALYTICS-SPEC.md#f6`

- [ ] **F6.2: `pftui alerts` CLI** — `alerts add "rule"`, `alerts list`, `alerts remove <id>`, `alerts check` (manual one-shot), `alerts check --json` (agent output). Show distance-to-trigger for armed alerts. Files: new `src/commands/alerts.rs`, `cli.rs`
- [ ] **F6.3: Watchlist entry level integration** — `pftui watch TSLA --target 300 --direction below` stores as an alert rule. Watchlist tab [5] shows target column + proximity bar (green=far, yellow=approaching, red=hit). `pftui watchlist --approaching 10%` filters to assets within 10% of target. Files: `tui/views/watchlist.rs`, `commands/watchlist_cli.rs`, `db/watchlist.rs`
- [ ] **F6.4: Allocation target + drift in Positions tab** — `pftui target set gold 25% --band 3%` stores as allocation alert. Positions tab [1] shows target vs actual column, color-coded bands. `pftui drift` shows all positions' drift from targets. `pftui rebalance` suggests trades to restore. Files: `tui/views/positions.rs`, new `src/commands/target.rs`, `alerts/engine.rs`
- [ ] **F6.5: Alert badge in TUI status bar** — `⚠️ 2 alerts` count. Hotkey to expand alerts popup overlay. Per-asset ⚠️ icons on triggered positions/watchlist items. Files: `tui/widgets/status_bar.rs`, new `tui/views/alerts_popup.rs`
- [ ] **F6.6: Alerts in `refresh` output + optional OS notifications** — After price update, report newly triggered alerts in CLI output. `pftui refresh --notify` fires native OS notification via `notify-send` (Linux) or `osascript` (macOS). No daemon required. Files: `commands/refresh.rs`, `alerts/engine.rs`, new `src/notify.rs`

### F10: Portfolio Performance History
> **Goal:** Track portfolio value over time. Compute returns over any period. Benchmark comparison. Requires automated daily snapshots.

- [ ] **F10.1: Automated daily portfolio snapshots** — On every `refresh`, store total portfolio value + per-position values in SQLite. This also fixes the 3M chart "Waiting for data" bug. Files: `db/price_cache.rs`, `commands/refresh.rs`, new `src/db/snapshots.rs`
- [ ] **F10.2: `pftui performance` CLI** — Show MTD, QTD, YTD, since-inception returns. `--since 2026-02-24` for custom period. `--period weekly` for return series. `--vs SPY` for benchmark comparison. `--json` for agents. Files: new `src/commands/performance.rs`, `cli.rs`
- [ ] **F10.3: Performance panel in Positions tab** — Compact return summary in portfolio overview: 1D, 1W, 1M, YTD. Sparkline of portfolio value over selected period. Files: `tui/views/positions.rs`, `tui/widgets/portfolio_stats.rs`

### Other P1

- [ ] **Native multi-currency with live FX conversion** — Store non-USD currencies natively, convert via live FX rates. Show FX rate and currency risk flag. Large effort — split into sub-tasks. Files: `models/position.rs`, `price/mod.rs`, `commands/summary.rs`, `widgets/header.rs`
- [ ] **[Feedback] Populate historical snapshots for 3M chart** — Portfolio 3M chart shows "Waiting for data". Ensure daily portfolio value snapshots are cached for trend analysis. Files: `db/price_cache.rs`, `price_chart.rs`
- [ ] **Ultra-wide layout (160+ cols)** — Third column: market context panel. Layout: 45% positions / 25% market / 30% chart. Files: `tui/ui.rs`, new `widgets/market_context.rs`
- [ ] **thinkorswim UX research** — Research ToS layout, charts, scanners, analytics, shortcuts. Document what translates to TUI. Output: `docs/RESEARCH-THINKORSWIM.md`, then add derived TODO items
- [ ] **Theme visual audit** — Audit all 11 themes across all views. Check: category colors, chart gradients, selection contrast, popup readability. Files: `theme.rs`, all views

## P2 — Analytics Expansion

### F7: Enhanced Agent Output
> **Goal:** Single JSON entry point for all agent-consumable data. Replaces multiple CLI calls.
> **Spec:** `docs/ANALYTICS-SPEC.md#f7`

- [ ] **F7.1: `brief --agent` mode** — Single JSON blob: positions, prices, technicals, macro, alerts, regime. Optional `--sections` filter. Files: `commands/brief.rs`

### F2: Correlation Matrix
> **Goal:** Rolling Pearson correlation between assets. Identify diversification, crowded trades, correlation breaks.
> **Spec:** `docs/ANALYTICS-SPEC.md#f2`

- [ ] **F2.1: Correlation math module** — Pearson on daily returns. 7/30/90-day rolling windows. Break detection (|Δ30d-90d| > 0.3). Files: new `src/indicators/correlation.rs`
- [ ] **F2.2: Correlation grid in Markets tab** — Color-coded matrix (green=positive, red=negative). Held assets + key macro indicators. Toggle 7d/30d/90d. Files: `tui/views/markets.rs`, new `tui/views/correlation_grid.rs`
- [ ] **F2.3: Correlations in `brief --correlations`** — Top pairs + any active breaks. Files: `commands/brief.rs`

### F4: Portfolio Risk & Scenario Engine
> **Goal:** Portfolio-level risk metrics + multi-asset "what-if" scenario modeling with cascading impacts.
> **Spec:** `docs/ANALYTICS-SPEC.md#f4`

- [ ] **F4.1: Risk metrics module** — Annualized volatility, max drawdown, Sharpe ratio (vs FFR), historical VaR (95%), Herfindahl concentration index. Files: new `src/analytics/{mod,risk}.rs`
- [ ] **F4.2: Scenario engine** — Named macro scenarios with per-asset impact multipliers. Presets: "Oil $100", "BTC $40k", "Gold $6000", "2008 GFC", "1973 Oil Crisis". Custom: `--what-if "gold:-10%,btc:-20%"`. Files: new `src/analytics/scenarios.rs`, modify `commands/summary.rs`
- [ ] **F4.3: Analytics tab [6] in TUI** — New tab. Risk panel (gauges + color coding), concentration chart, scenario selector with interactive parameter tweaking, projected portfolio value. Files: new `tui/views/analytics.rs`, `app.rs` (add ViewMode::Analytics)
- [ ] **F4.4: Risk summary in `brief`** — 1-line risk summary: volatility, VaR, concentration flag. Files: `commands/brief.rs`

### F8: Journal & Decision Log
> **Goal:** Structured trade journal in SQLite. Hotkey popup in TUI. Full CLI suite for agents to seed, query, search. Replaces JOURNAL.md as primary decision log for agents.
> **Spec:** `docs/ANALYTICS-SPEC.md#f8`

- [ ] **F8.1: Journal DB schema + CLI command suite** — SQLite table (timestamp, content, tag, symbol, conviction, status). Full CLI: `pftui journal add/list/search/update/remove/tags/stats`. All commands support `--json`. Files: new `src/db/journal.rs`, new `src/commands/journal.rs`, `cli.rs`
- [ ] **F8.2: Journal tab [7] in TUI** — New tab in numbered menu. Scrollable list: date, content (truncated), tag columns. `a` to add entry inline, Enter to expand full text, `/` to search within journal. Files: new `src/tui/views/journal.rs`, `src/app.rs` (add ViewMode::Journal, bind key `7`)
- [ ] **F8.3: JOURNAL.md migration script** — One-time parser that seeds SQLite from existing JOURNAL.md entries with correct timestamps, tags, statuses. Files: new `src/commands/migrate_journal.rs` or standalone script

### F12: Economic Calendar
> **Goal:** Upcoming market-moving events (FOMC, CPI, NFP, earnings) with impact ratings. Integrates into existing Economy tab [4].

- [ ] **F12.1: Calendar data source + cache** — Free API integration (Finnhub free tier or Trading Economics free or Forex Factory RSS). Fetch upcoming events, cache in SQLite with: date, event name, impact (high/medium/low), previous value, forecast, actual. Refresh daily. Files: new `src/data/calendar.rs`, new `src/db/calendar_cache.rs`
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

### Other P2

- [ ] **[Feedback] `pftui movers` command** — `pftui movers --threshold 3%` scans all held + watchlist symbols and returns only those exceeding the daily change threshold. Replaces manual scanning of 40+ symbols. Files: new `src/commands/movers.rs`, `cli.rs`
- [ ] **[Feedback] P&L attribution in `brief`** — Show which position contributed most to daily P&L (e.g., "Gold: -$5.2k, BTC: +$3.1k"). Files: `src/commands/brief.rs`
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
