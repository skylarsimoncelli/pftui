# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Format: `- [ ] **Short title** — Brief description. Files: relevant_file.rs`

## P0

- [ ] **Fix U.UN (Sprott Uranium) chart + price accuracy** — TSX trust unit ticker broken for charts. Additionally, `brief` reports U.UN at +31% gain when actual is ~-4.8% — investigate Yahoo Finance data source accuracy for Canadian-listed securities. Files: `price/yahoo.rs`, `price_chart.rs`, `commands/brief.rs`

## P1

- [ ] **Native multi-currency with live FX conversion** — Store non-USD currencies natively, convert via live FX rates. Show FX rate and currency risk flag. Recurring top request from all 3 testers (GBP stored as USD masks currency exposure). Large effort — split into sub-tasks. Files: `models/position.rs`, `price/mod.rs`, `commands/summary.rs`, `widgets/header.rs`
- [ ] **[Feedback] Populate historical snapshots for 3M chart** — Portfolio 3M chart shows "Waiting for data". Ensure daily portfolio value snapshots are cached for trend analysis. Files: `db/price_cache.rs`, `price_chart.rs`
- [ ] **Ultra-wide layout (160+ cols)** — Third column: market context panel. Layout: 45% positions / 25% market / 30% chart. Files: `tui/ui.rs`, new `widgets/market_context.rs`
- [ ] **thinkorswim UX research** — Research ToS layout, charts, scanners, analytics, shortcuts. Document what translates to TUI. Output: `docs/RESEARCH-THINKORSWIM.md`, then add derived TODO items
- [ ] **Theme visual audit** — Audit all 11 themes across all views. Check: category colors, chart gradients, selection contrast, popup readability. Files: `theme.rs`, all views

## P2

- [ ] **[Feedback] Add "What Changed Today" section to `brief`** — Show largest daily movers, notable threshold crossings, and any triggered alerts in the brief output. Files: `commands/brief.rs`
- [ ] **[Feedback] Technical indicators for held positions** — RSI, SMA50, MACD for positions in `brief`/`summary` output. Would reduce dependency on external scripts. Files: new `src/indicators/`, `commands/brief.rs`, `commands/summary.rs`
- [ ] **[Feedback] Benchmark comparison in `brief`** — Show portfolio performance vs SPY, Gold index, or custom benchmark. Files: `commands/brief.rs`, `price/mod.rs`
- [ ] **News feed integration** — Free RSS/API source (Yahoo Finance RSS, Finnhub). Scrollable list with per-asset filtering. Files: new `src/news/`, new `views/news.rs`
- [ ] **FRED economic data** — Treasury yields, CPI, unemployment via FRED API (free). New DB table, aggressive cache. Files: new `data/fred.rs`, `db/economic_cache.rs`
- [ ] **Candlestick chart variant** — OHLC braille/block candlesticks. Requires OHLC in HistoryRecord. Files: `models/price.rs`, `price/yahoo.rs`, `price_chart.rs`
- [ ] **Web interface (`pftui web`)** — axum/warp server, shared core layer, REST API, lightweight JS frontend. TradingView embedded charts (Advanced Chart Widget) for interactive charting, fallback to SVG. Sub-tasks: 1) Extract core, 2) REST API, 3) Frontend + TradingView, 4) Auth/PID. Files: new `src/web/`, refactor `src/core/`, `cli.rs`
- [ ] **Snap/AUR/Scoop publishing** — Snap: needs Snapcraft account + SNAPCRAFT_TOKEN. AUR: needs account + AUR_SSH_KEY. Scoop: needs Windows binary first. Files: `snap/snapcraft.yaml`, `.github/workflows/release.yml`
- [ ] **Windows build support** — Add x86_64-pc-windows-msvc to release matrix. Files: `.github/workflows/release.yml`

## P3

- [ ] **Portfolio analytics** — Sharpe ratio, max drawdown, volatility, benchmark comparison
- [ ] **Dividend tracking** — Payments, yield, ex-dates
- [ ] **Correlation matrix** — Visual correlation grid between positions
- [ ] **Multi-portfolio support** — Named portfolios with switching
- [ ] **Price alerts** — `pftui alert GC=F above 5500`. CLI + TUI integration. Requested by all 3 testers.
- [ ] **Custom keybinding config** — User-configurable in config.toml
- [ ] **Sector heatmap** — Treemap-style sector performance view
- [ ] **Options chains** — If a free data source exists

## Feedback Summary

**Last reviewed:** 2026-03-03T17:46Z

| Tester | Latest Score | Trend | Key Pain Point |
|---|---|---|---|
| Sentinel Main (TUI) | 78% | ↑ (40→78) | Missing day P&L, sector allocation, benchmarks |
| Evening Planner (CLI) | 38% | → (single point) | Headless features shipped, awaiting re-eval |
| Market Research (CLI) | 72% | → (first review) | U.UN price wrong, no daily P&L in brief, no FX |

**Lowest scorer:** Evening Planner at 38% — however, their top requests (refresh, brief, value, set-cash, what-if, history) have all shipped since their review. Expect significant score increase on re-eval.

**Top 3 priorities from feedback:**
1. **Daily P&L in CLI commands** (P0) — requested by all 3 testers. TUI has it, `brief`/`summary` don't.
2. **Fix U.UN price accuracy** (P0) — Market Research reports +31% gain vs actual -4.8%. Data source issue for Canadian securities.
3. **Native multi-currency FX** (P1) — all 3 testers flag GBP-as-USD as masking currency risk.

**Completed feedback items:** `pftui refresh`, `--period`, `--group-by`, day P&L (TUI header), value/brief/watchlist/set-cash CLI, CSV rounding, base currency config, Markets tab enrichment, `--what-if`, `history --date`, snapshot, import
