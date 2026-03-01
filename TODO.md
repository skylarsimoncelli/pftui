# TODO — pftui

> Pick the highest-priority unclaimed `[ ]` item. Mark `[~]` while working, `[x]` when done.
> Each item is scoped to ~1 hour. If it's bigger, split it. Update CHANGELOG.md when done.

## P0 — Bugs & Regressions

- [x] **Fix sequential history fetching** — `PriceService::run_loop` processes `FetchHistory` commands one at a time. When a chart needs 4-5 symbols, they fetch sequentially (4-5 HTTP round-trips in series). Use `tokio::join!` or `JoinSet` to fetch all history concurrently within a single `FetchHistory` batch command. Files: `src/price/mod.rs`. Test: verify concurrent fetch completes faster (or at minimum, restructure to batch).
- [x] **Fix CoinGecko→Yahoo fallback double-suffix** — In `fetch_history`, when category is Crypto and CoinGecko fails, Yahoo fallback does `format!("{}-USD", symbol)`. But chart variant symbols like `BTC-USD` already have the suffix → becomes `BTC-USD-USD`. Guard: if symbol already ends with `-USD`, don't append. Files: `src/price/mod.rs`. Test: add test for suffix logic.
- [x] **Show "Loading..." on blank mini ratio panels** — In `price_chart.rs`, `render_mini_ratio` silently returns when `compute_ratio` produces < 2 points, leaving a blank panel. Should show "Loading {label}..." like `render_mini_chart` does. Files: `src/tui/widgets/price_chart.rs`.
- [x] **Fix clippy warnings** — There are 8 dead code warnings (unused fields, unused functions). Either use them or remove them. Run `cargo clippy` and resolve all warnings. Files: various.

## P1 — Vim Motions & Core UX

- [x] **Add gg/G vim motions** — `gg` jumps to first row, `G` jumps to last row. Implementation: track a `g_pending` bool on App. If `g` is pressed and `g_pending` is true → jump to top + clear. If `g` is pressed and `g_pending` is false → set `g_pending = true`. Any other key clears `g_pending`. `G` (shift+g) currently sorts by total gain — reassign total gain sort to a different key (e.g., `$`) and use `G` for jump-to-bottom per vim convention. Files: `src/app.rs` (handle_key), `src/tui/views/help.rs`. Test: test gg/G index changes.
- [x] **Add Ctrl+d / Ctrl+u half-page scroll** — Ctrl+d moves cursor down by half the visible height, Ctrl+u moves up. Need to know the visible row count (pass terminal height to App or compute from layout). Files: `src/app.rs`, `src/tui/ui.rs` (may need to store visible_rows on App). Test: test scroll bounds.
- [x] **Add / search filter** — Pressing `/` enters a search mode: a text input appears in the status bar, typing filters positions/transactions by symbol or name substring (case-insensitive). `Esc` clears search and exits search mode. `Enter` confirms filter. Files: `src/app.rs` (new search_mode, search_query fields), `src/tui/widgets/status_bar.rs` (render input), positions/transactions views (apply filter). Test: test filter logic.
- [x] **Add Esc to close detail panel** (already implemented) — `Esc` already closes help overlay, but when detail (chart) panel is open and help is not, `Esc` should close the chart panel too. Currently only `Enter` toggles it. Files: `src/app.rs` (handle_key Esc branch). Test: verify Esc closes detail.
- [x] **Improve help overlay** — Group keybindings by context (Navigation, Sorting, Views, Charts, Other). Show vim motions prominently. Add brief descriptions. Make it a scrollable panel if content exceeds terminal height. Files: `src/tui/views/help.rs`.

## P1 — Chart & Data Enhancements

- [x] **Add timeframe selection to charts** — Currently hardcoded to 90 days. Add timeframe cycling with `h`/`l` (vim left/right) when chart is open: 1W, 1M, 3M, 6M, 1Y, 5Y. Store `chart_timeframe` on App. Pass days to `FetchHistory`. Show timeframe label in chart title. Files: `src/app.rs` (timeframe enum, handle_key), `src/tui/widgets/price_chart.rs` (title), `src/price/mod.rs`. Test: test timeframe cycling.
- [x] **Add equity chart variants** — Regular equities currently get only their own single chart. Add ratio variants: {SYM}/SPX, {SYM}/QQQ (same pattern as BTC/Gold). Makes every equity position more useful. Files: `src/app.rs` (chart_variants_for_position else branch). Test: test equity variant generation.
- [x] **Add volume bars below price chart** — Yahoo history returns OHLCV (volume available). Store volume in HistoryRecord. Render thin volume bars below the price braille chart using block characters. Color: muted version of chart gradient. Files: `src/models/price.rs` (add volume field), `src/price/yahoo.rs` (parse volume), `src/tui/widgets/price_chart.rs` (render volume). Test: test volume parsing.
- [x] **Add moving average overlays** — Compute SMA(20) and SMA(50) from history data. Overlay as a second line on braille charts (using a different color from theme). Files: `src/tui/widgets/price_chart.rs` (SMA computation + overlay rendering). Test: test SMA computation.

## P1 — New Views

- [x] **Add Markets view (tab 3)** — A broad market overview tab showing major indices (SPX, NDX, DJI, RUT), commodities (Gold, Oil, Silver), crypto (BTC, ETH), and forex (DXY, EUR, GBP, JPY). Display as a table: Symbol, Name, Price, Change, Change%. Fetch prices on view activation. Files: new `src/tui/views/markets.rs`, `src/app.rs` (ViewMode::Markets, key `3`), `src/tui/ui.rs` (render dispatch). Test: test market symbol list generation.
- [x] **Add Economy view (tab 4)** — Macro dashboard showing treasury yields (2Y, 5Y, 10Y, 30Y via Yahoo ^TNX etc.), DXY, and key indicators. Start simple: table of economic symbols with current values. Can expand to yield curve chart later. Files: new `src/tui/views/economy.rs`, `src/app.rs` (ViewMode::Economy, key `4`). Test: test economy symbol mapping.
- [x] **Add Watchlist view (tab 5)** — Track assets without holding them. New DB table `watchlist (symbol, category, added_at)`. CLI: `pftui watch <symbol>`, `pftui unwatch <symbol>`. TUI: same chart access as positions. Files: new `src/db/watchlist.rs`, new `src/tui/views/watchlist.rs`, `src/cli.rs`, `src/app.rs`. Test: test watchlist CRUD.

## P2 — Visual Polish

- [x] **Add responsive layout** — Detect terminal size in ui.rs. Below 100 columns: hide sidebar, show positions full-width. Below 60 columns: simplify header, reduce column count. Above 160 columns: wider sidebar. Files: `src/tui/ui.rs`, `src/tui/widgets/header.rs`.
- [ ] **Add position detail popup** — When pressing Enter on a position, instead of just opening the chart in sidebar, show a full-screen popup with: name, symbol, category, quantity, avg cost, current price, gain, gain%, allocation%, 90d chart, and buy/sell history for that symbol. Esc closes. Files: new `src/tui/views/position_detail.rs`, `src/app.rs`.
- [ ] **Improve allocation bars** — Add percentage labels inside bars (when bar is wide enough). Add total portfolio value below the allocation widget. Add subtle animation when allocations change. Files: `src/tui/widgets/allocation_bars.rs`.
- [ ] **Add 52-week high/low indicators** — Show distance from 52-week high/low in positions table as a tiny visual bar or percentage. Requires fetching 365-day history (can reuse price_history). Files: `src/models/position.rs` (add high/low fields), `src/tui/views/positions.rs`.

## P2 — Data & Infrastructure

- [ ] **Add news feed integration** — Fetch financial news from a free RSS/API source (e.g., Yahoo Finance RSS, Finnhub free tier). Display as a scrollable list: timestamp, headline, source. Per-asset filtering. Files: new `src/news/` module, new `src/tui/views/news.rs`. Research: find best free news API that works without API key.
- [ ] **Add FRED economic data** — FRED API (free with API key) for treasury yields, CPI, unemployment, Fed funds rate. Store in new DB table. Cache aggressively (economic data updates daily at most). Files: new `src/data/fred.rs`, `src/db/economic_cache.rs`.
- [ ] **Increase test coverage** — Add tests for: `config.rs` (load/save/defaults), `asset_names.rs` (infer_category, search_names), `theme.rs` (lerp_color, gradient_3, gain_intensity_color), `price_chart.rs` (compute_ratio, resample). Files: respective test modules.
- [ ] **Add candlestick chart variant** — OHLC candlestick rendering using braille/block characters. Green body for close > open, red for close < open. Wicks as thin lines. Requires OHLC data in HistoryRecord. Files: `src/models/price.rs`, `src/price/yahoo.rs`, `src/tui/widgets/price_chart.rs`.

## P3 — Future

- [ ] **Portfolio analytics** — Sharpe ratio, max drawdown, volatility metrics, benchmark comparison
- [ ] **Dividend tracking** — Track dividend payments, show yield, ex-dates
- [ ] **Correlation matrix** — Visual correlation grid between portfolio positions
- [ ] **Multi-portfolio support** — Multiple named portfolios with switching
- [ ] **Price alerts** — Configurable threshold alerts with terminal notification
- [ ] **Custom keybinding config** — User-configurable keybindings in config.toml
- [ ] **Sector heatmap** — Treemap-style sector/industry performance view
- [ ] **Options chains** — Options display if a free data source exists
