# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Bugs & Fixes

> Broken existing functionality. Fix before shipping.



---

## P1 — Feature Requests

> User-requested features and high-value improvements.

### Data & Display
- [ ] **Candlestick chart variant** — OHLC braille/block candlesticks. New variant `ChartVariant::Candlestick`, keybinding, renderer. Files: `app.rs`, `widgets/price_chart.rs`
- [ ] **SMA overlay on charts** — Configurable `chart_sma = [20, 50, 200]`. Faint braille lines. Files: `widgets/price_chart.rs`, `config.rs`
- [ ] **Volume sub-chart** — 3-row braille bars below price. Toggle with `V`. Files: `widgets/price_chart.rs`
- [ ] **After-hours / pre-market prices** — Yahoo provides extended hours data. Show in watchlist and brief. Files: `price/yahoo.rs`, `commands/brief.rs`
- [ ] **Brent crude + WTI spread in macro** — Key metric for geopolitical crises. Files: `commands/macro_cmd.rs`, `views/economy.rs`

### CLI Enhancements
- [ ] **`pftui eod` command** — Market close command combining brief + movers + macro + sentiment. Files: new `commands/eod.rs`
- [ ] **`pftui sector` command** — Sector ETF performance (XLE, XLF, IGV, etc.). Files: new `commands/sector.rs`
- [ ] **`pftui calendar` CLI** — `pftui calendar` (next 7 days), `--days 30`, `--impact high`, `--json`. Files: new `commands/calendar.rs`
- [ ] **Alerts in `brief` output** — Show triggered/near-threshold alerts. Files: `commands/brief.rs`
- [ ] **"What Changed Today" in `brief`** — Top movers, threshold crossings, triggered alerts. Files: `commands/brief.rs`
- [ ] **Benchmark comparison in `brief`** — Portfolio performance vs SPY or custom benchmark. Files: `commands/brief.rs`
- [ ] **Portfolio stress testing CLI** — `pftui stress-test` with named scenarios. Builds on F4.2 engine. Files: new `commands/stress_test.rs`

### Analytics
- [ ] **Correlation grid in Markets tab** — Color-coded matrix (green/red). Toggle 7d/30d/90d. Files: `views/markets.rs`, new `views/correlation_grid.rs`
- [ ] **Correlations in `brief`** — Top pairs + active breaks. Files: `commands/brief.rs`
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

---

## P3 — Long Term

- [ ] **Sovereign holdings tracker** — CB gold (WGC), government BTC, COMEX silver. No other TUI tracks this. Files: new `data/sovereign.rs`
- [ ] **Dividend tracking** — Payments, yield, ex-dates
- [ ] **Multi-portfolio support** — Named portfolios with switching
- [ ] **Custom keybinding config** — User-configurable in config.toml
- [ ] **Sector heatmap** — Treemap-style sector performance view
- [ ] **Options chains** — If a free data source exists
