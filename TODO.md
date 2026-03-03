# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Format: `- [ ] **Short title** — Brief description. Files: relevant_file.rs`

## P0


- [ ] **Fix U.UN (Sprott Uranium) chart** — TSX trust unit ticker broken. `normalize_yahoo_symbol()` may need fixing or graceful "No chart data" if unresolvable. Files: `price/yahoo.rs`, `price_chart.rs` (empty state)
- [ ] **Show 24h % change in search results** — Each `/` search result row should display 24h change color-coded. Files: `search_overlay.rs`

## P1

- [ ] **Native multi-currency with live FX conversion** — Store non-USD currencies natively, convert via live FX rates. Show FX rate and currency risk. Large effort — split into sub-tasks. Files: `models/position.rs`, `price/mod.rs`, `commands/summary.rs`, `widgets/header.rs`
- [ ] **Ultra-wide layout (160+ cols)** — Third column: market context panel. Layout: 45% positions / 25% market / 30% chart. Files: `tui/ui.rs`, new `widgets/market_context.rs`
- [ ] **thinkorswim UX research** — Research ToS layout, charts, scanners, analytics, shortcuts. Document what translates to TUI. Output: `docs/RESEARCH-THINKORSWIM.md`, then add derived TODO items
- [ ] **Theme visual audit** — Audit all 11 themes across all views. Check: category colors, chart gradients, selection contrast, popup readability. Files: `theme.rs`, all views

## P2

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
- [ ] **Price alerts** — `pftui alert GC=F above 5500`. CLI + TUI integration
- [ ] **Custom keybinding config** — User-configurable in config.toml
- [ ] **Sector heatmap** — Treemap-style sector performance view
- [ ] **Options chains** — If a free data source exists

## Feedback Summary

**Last reviewed:** 2026-03-03 | Sentinel TUI: 78% | Evening Planner: 38% (top requests all shipped, awaiting re-eval) | Portfolio Analyst: no data

**Key gaps:** Multi-currency FX, correlation/benchmarks/risk metrics (professional tool gap), third tester activation

**Completed feedback items:** `pftui refresh`, `--period`, `--group-by`, day P&L header, value/brief/watchlist/set-cash CLI, CSV rounding, base currency config, Markets tab enrichment
