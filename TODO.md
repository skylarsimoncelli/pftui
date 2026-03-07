# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Bugs & Fixes

> Broken existing functionality. Fix before shipping.

### UX Cohesion
- [ ] **Regime suggestions should be portfolio-aware** — Economy tab regime advice is generic ("consider defensive positioning"). Should reference actual holdings ("your 25% gold allocation is well-positioned for..."). Files: `tui/views/economy.rs`

### Data Pipeline Reliability
- [ ] [Feedback] **Fix predictions data source** — Polymarket parser returns only sports/NHL markets, no geopolitical or financial predictions. Category filtering doesn't surface macro-relevant markets. Testers need ceasefire odds, rate cut probabilities, recession odds. Files: `data/predictions.rs`, `commands/predictions.rs`
- [ ] [Feedback] **Fix ETF flows command** — `pftui etf-flows` fails with exit code 1 or returns empty. BTC ETF flow data (IBIT, FBTC, ARKB daily flows) is critical for crypto analysis. CoinGlass JS rendering blocks scraping. Files: `data/onchain.rs`, `commands/etf_flows.rs`
- [ ] [Feedback] **Fix COT data availability** — COT data shows "unavailable" for some testers despite field name fix. Verify CFTC API returns data consistently. Files: `data/cot.rs`, `db/cot_cache.rs`

---

## P1 — Feature Requests

> User-requested features and high-value improvements.

### Data & Display
- [ ] **Candlestick chart variant** — OHLC data layer is done. Implement braille/block candlestick renderer using new open/high/low fields. New variant `ChartVariant::Candlestick`, keybinding, renderer. Files: `app.rs`, `widgets/price_chart.rs`
- [ ] **SMA overlay on charts** — Configurable `chart_sma = [20, 50, 200]`. Faint braille lines. Files: `widgets/price_chart.rs`, `config.rs`
- [ ] **Volume sub-chart** — 3-row braille bars below price. Toggle with `V`. Files: `widgets/price_chart.rs`
- [ ] **After-hours / pre-market prices** — Yahoo provides extended hours data. Show in watchlist and brief. Files: `price/yahoo.rs`, `commands/brief.rs`
- [ ] **Brent crude + WTI spread in macro** — Key metric for geopolitical crises. Files: `commands/macro_cmd.rs`, `views/economy.rs`
- [ ] [Feedback] **Technicals on macro dashboard** — Add RSI(14), MACD, SMA(50) for macro instruments (DXY, VIX, oil, 10Y, copper). This is the #1 remaining reason testers use the external Python script. Files: `commands/macro_cmd.rs`
- [ ] [Feedback] **Technicals on watchlist** — Add RSI/SMA50 columns for all watchlist symbols, not just held positions. Eliminates Python script dependency entirely. Files: `commands/watchlist_cli.rs`, `tui/views/watchlist.rs`

### CLI Enhancements
- [ ] **`pftui eod` command** — Market close command combining brief + movers + macro + sentiment. Files: new `commands/eod.rs`
- [ ] **`pftui sector` command** — Sector ETF performance (XLE, XLF, IGV, etc.). Files: new `commands/sector.rs`
- [ ] **`pftui calendar` CLI** — `pftui calendar` (next 7 days), `--days 30`, `--impact high`, `--json`. Files: new `commands/calendar.rs`
- [ ] **Alerts in `brief` output** — Show triggered/near-threshold alerts. Files: `commands/brief.rs`
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
