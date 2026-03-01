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


## P1 — Chart & Data Enhancements

- [x] **Add timeframe selection to charts** — Currently hardcoded to 90 days. Add timeframe cycling with `h`/`l` (vim left/right) when chart is open: 1W, 1M, 3M, 6M, 1Y, 5Y. Store `chart_timeframe` on App. Pass days to `FetchHistory`. Show timeframe label in chart title. Files: `src/app.rs` (timeframe enum, handle_key), `src/tui/widgets/price_chart.rs` (title), `src/price/mod.rs`. Test: test timeframe cycling.
- [x] **Add equity chart variants** — Regular equities currently get only their own single chart. Add ratio variants: {SYM}/SPX, {SYM}/QQQ (same pattern as BTC/Gold). Makes every equity position more useful. Files: `src/app.rs` (chart_variants_for_position else branch). Test: test equity variant generation.
- [x] **Add volume bars below price chart** — Yahoo history returns OHLCV (volume available). Store volume in HistoryRecord. Render thin volume bars below the price braille chart using block characters. Color: muted version of chart gradient. Files: `src/models/price.rs` (add volume field), `src/price/yahoo.rs` (parse volume), `src/tui/widgets/price_chart.rs` (render volume). Test: test volume parsing.
- [x] **Add moving average overlays** — Compute SMA(20) and SMA(50) from history data. Overlay as a second line on braille charts (using a different color from theme). Files: `src/tui/widgets/price_chart.rs` (SMA computation + overlay rendering). Test: test SMA computation.

## P1 — New Views

- [x] **Add Markets view (tab 3)** — A broad market overview tab showing major indices (SPX, NDX, DJI, RUT), commodities (Gold, Oil, Silver), crypto (BTC, ETH), and forex (DXY, EUR, GBP, JPY). Display as a table: Symbol, Name, Price, Change, Change%. Fetch prices on view activation. Files: new `src/tui/views/markets.rs`, `src/app.rs` (ViewMode::Markets, key `3`), `src/tui/ui.rs` (render dispatch). Test: test market symbol list generation.
- [x] **Add Economy view (tab 4)** — Macro dashboard showing treasury yields (2Y, 5Y, 10Y, 30Y via Yahoo ^TNX etc.), DXY, and key indicators. Start simple: table of economic symbols with current values. Can expand to yield curve chart later. Files: new `src/tui/views/economy.rs`, `src/app.rs` (ViewMode::Economy, key `4`). Test: test economy symbol mapping.
- [x] **Add Watchlist view (tab 5)** — Track assets without holding them. New DB table `watchlist (symbol, category, added_at)`. CLI: `pftui watch <symbol>`, `pftui unwatch <symbol>`. TUI: same chart access as positions. Files: new `src/db/watchlist.rs`, new `src/tui/views/watchlist.rs`, `src/cli.rs`, `src/app.rs`. Test: test watchlist CRUD.

## P1 — Animations & Live Feel

- [ ] **Add price flash with directional arrows** — When a price updates, show a brief ▲/▼ arrow next to the price that fades after ~1s. Currently we flash the price cell bg, but adding a directional indicator makes the update scannable without reading the number. Use `price_flash_ticks` map (already exists) and extend to store direction (up/down/same). Files: `src/tui/views/positions.rs`, `src/app.rs`. Test: verify flash direction stored on update.
- [ ] **Add scrolling ticker tape in header** — Horizontal marquee-style ticker showing top movers from Markets view data: "SPX +1.2% │ BTC -3.4% │ GOLD +0.5%" scrolling left. Renders in the header row using the space after the portfolio value. Uses `app.tick_count` to advance position by 1 char every ~6 ticks (~10 chars/sec). Only active on Positions view to avoid clutter. Files: `src/tui/widgets/header.rs`, `src/app.rs` (market data already available). Test: test ticker text generation and wrap-around.
- [ ] **Add pulsing border on active panel** — Instead of static `border_active` color, pulse the focused panel's border using `pulse_color()` (already exists in theme.rs). Subtle 2-second sine wave between `border_inactive` and `border_active` intensity. Gives the app a "breathing" feel. Only when prices are live (dead/stale = static border). Files: `src/tui/views/positions.rs` (render_table block), `src/tui/widgets/price_chart.rs` (chart block). Test: verify pulse applied only when prices_live.
- [ ] **Add row highlight animation on selection change** — When j/k moves selection, briefly flash the entire new row brighter (lerp from `surface_3` toward `border_accent` then fade back over ~15 ticks). Track `last_selection_change_tick` on App. Files: `src/tui/views/positions.rs` (row_bg calculation), `src/app.rs` (track tick on selection change). Test: test flash decay timing.

## P1 — Header & Status Bar Enhancements

- [ ] **Add day gain/loss to header** — Show today's portfolio change alongside total gain: "$45.2k +1.3% ▲$580 today". Compute from sum of (position.quantity × position.day_change_amount). The "today" figure is the most-checked number in any portfolio app. Files: `src/tui/widgets/header.rs` (add today gain span), `src/app.rs` (compute daily portfolio change from position day changes). Test: test daily change computation.
- [ ] **Add market status indicator to header** — Show "◉ OPEN" (green) or "◎ CLOSED" (muted) based on current UTC time vs US market hours (9:30-16:00 ET, weekdays). Simple timezone offset check, no external dependency. Renders after the clock in the header. Files: `src/tui/widgets/header.rs`. Test: test market open/closed detection for various UTC times.
- [ ] **Add breadcrumb trail to status bar** — When in chart view, show the navigation path: "Positions › AAPL › 3M Chart › AAPL/SPX". When in detail popup: "Positions › AAPL › Detail". Replaces the generic hint bar text with context-aware breadcrumbs. Files: `src/tui/widgets/status_bar.rs`, `src/app.rs` (expose chart variant label). Test: test breadcrumb string generation for each navigation state.

## P1 — Positions Table Visual Density

- [ ] **Add inline mini-sparkline in price column** — Render a 3-char sparkline (▁▃▇) directly after the price number in the Price column, using the last 3 hours of data (or last 3 history points). Gives instant trend context without looking at the separate Trend column. Files: `src/tui/views/positions.rs` (price cell rendering). Test: verify sparkline renders with various history lengths.
- [ ] **Add color-coded category dot before asset name** — Replace the plain text category label approach with a small colored dot (●) in the asset's category color at the start of the Asset column. More scannable than the current approach of coloring the entire row. The `▎` selection marker already uses this column — put the dot after the marker. Files: `src/tui/views/positions.rs` (asset_line construction). Test: verify dot color matches category.
- [ ] **Add gain/loss magnitude bar in Gain% column** — Render the gain% number with a tiny proportional bar behind it: green fill for gains, red for losses, scaled to ±20%. Like a micro horizontal bar chart in each cell. Uses EIGHTH_BLOCKS for sub-character resolution. Files: `src/tui/views/positions.rs` (gain cell rendering). Test: test bar width calculation for various percentages.

## P2 — Visual Polish

- [x] **Add responsive layout** — Detect terminal size in ui.rs. Below 100 columns: hide sidebar, show positions full-width. Below 60 columns: simplify header, reduce column count. Above 160 columns: wider sidebar. Files: `src/tui/ui.rs`, `src/tui/widgets/header.rs`.
- [x] **Add position detail popup** — When pressing Enter on a position, instead of just opening the chart in sidebar, show a full-screen popup with: name, symbol, category, quantity, avg cost, current price, gain, gain%, allocation%, 90d chart, and buy/sell history for that symbol. Esc closes. Files: new `src/tui/views/position_detail.rs`, `src/app.rs`.
- [x] **Improve allocation bars** — Add percentage labels inside bars (when bar is wide enough). Add total portfolio value below the allocation widget. Add subtle animation when allocations change. Files: `src/tui/widgets/allocation_bars.rs`.
- [x] **Add 52-week high/low indicators** — Show distance from 52-week high/low in positions table as a tiny visual bar or percentage. Requires fetching 365-day history (can reuse price_history). Files: `src/models/position.rs` (add high/low fields), `src/tui/views/positions.rs`.

## P2 — Positions Table Enhancements

- [x] **Add daily change % column to positions** — Show how each position moved today as a Day% column between Price and Gain%. Compute from last two price history entries (same pattern as Markets/Economy/Watchlist views). Gain-aware coloring (green/red). Privacy-safe (percentage only). Files: `src/tui/views/positions.rs`. Test: test compute_change_pct logic.

## P2 — Chart Visual Enhancements

- [ ] **Add crosshair cursor on charts** — When chart detail is open, pressing `c` enables a crosshair mode: j/k moves a vertical line across the chart, showing the date and price at that point in a tooltip overlay. Renders as a vertical column of `│` characters in `text_accent` color with a data label. Files: `src/tui/widgets/price_chart.rs` (crosshair rendering), `src/app.rs` (crosshair_mode, crosshair_x fields, c keybinding). Test: test crosshair bounds clamping.
- [ ] **Add chart area fill with gradient** — Instead of just braille dots for the line, fill the area below the line with a fading gradient using BLOCK characters at very low intensity (10-20% alpha via dark versions of chart colors). Creates a "filled area chart" effect common in financial dashboards. Files: `src/tui/widgets/price_chart.rs` (render_braille_chart area fill pass). Test: verify fill doesn't exceed chart line position.
- [ ] **Add Bollinger Bands overlay** — Compute 20-period SMA ± 2 standard deviations. Render as faint dotted braille lines above and below the SMA(20). When price touches a band, highlight the touch point. Shows volatility and overbought/oversold conditions. Files: `src/tui/widgets/price_chart.rs` (compute_bollinger, overlay rendering). Test: test band computation with known data.

## P2 — Layout & Visual Polish

- [ ] **Add Unicode box-drawing panel separators** — Replace the default ratatui `Rounded` border type with custom double-line top (═══) and single-line sides (│). Use `╔═══╗` style for the active panel and `┌───┐` for inactive. Gives a more premium, Bloomberg-like feel. Files: `src/tui/views/positions.rs`, `src/tui/widgets/price_chart.rs`, `src/tui/widgets/allocation_bars.rs`, `src/tui/widgets/portfolio_sparkline.rs`. Test: visual verification only.
- [ ] **Add shadow effect on popups** — When the detail popup or help overlay renders, draw a 1-cell shadow on the right and bottom edges using `surface_0` with slight offset. Creates a floating/elevated look. Files: `src/tui/views/position_detail.rs`, `src/tui/views/help.rs`. Test: verify shadow doesn't exceed terminal bounds.
- [ ] **Add section divider lines between position groups** — When sorted by category, insert thin separator lines (─── Crypto ───) between position groups. Uses `border_subtle` color. Only appears when sort field is Category. Files: `src/tui/views/positions.rs`. Test: test divider insertion logic.
- [ ] **Add ultra-wide layout (160+ columns)** — When terminal is very wide, show a third column: market context panel with major indices and the portfolio sparkline below the positions table, with sidebar remaining as the chart panel. Three-column layout: 45% positions / 25% market context / 30% chart. Files: `src/tui/ui.rs` (new layout branch), new `src/tui/widgets/market_context.rs`. Test: test layout thresholds.

## P2 — Sidebar & Sparkline Enhancements

- [ ] **Add portfolio sparkline period selector** — The sparkline is hardcoded to 90d. Allow cycling with `[`/`]` keys (when sidebar is focused) through 1W, 1M, 3M, 6M, 1Y. Show period label in the sparkline panel title. Reuse the `ChartTimeframe` enum. Files: `src/tui/widgets/portfolio_sparkline.rs` (accept timeframe), `src/app.rs` (sparkline_timeframe field, `[`/`]` keybindings). Test: test timeframe cycling.
- [ ] **Add allocation change indicators** — Show ▲/▼ arrows next to allocation percentages when they've changed since the previous day (based on price movements shifting allocation weights). Helps identify rebalancing needs. Files: `src/tui/widgets/allocation_bars.rs`, `src/app.rs` (store previous day allocations for comparison). Test: test change detection logic.

## P2 — Micro-Interactions & Feedback

- [ ] **Add keystroke echo in status bar** — Briefly flash the last pressed key in the status bar corner: shows "k" for 0.3s when you press k, "gg" for the two-key sequence, "Ctrl+d" etc. Helps users learn keybindings and confirms input was received. Render in `text_muted` with quick fade. Files: `src/tui/widgets/status_bar.rs`, `src/app.rs` (last_key_display, last_key_tick). Test: test key echo text generation.
- [ ] **Add sort indicator animation** — When user changes sort order (s/n/c/% etc.), briefly animate the sort arrow (▲/▼) by flashing it in `text_accent` then fading to normal. Confirms the sort happened. Track `last_sort_change_tick` on App. Files: `src/tui/views/positions.rs` (sort indicator styling), `src/app.rs`. Test: test flash timing.
- [ ] **Add loading skeleton for empty states** — When a view is loading data, show shimmer/skeleton placeholder rows instead of "Waiting for data...". Render 5-6 rows of `░░░░░░` block characters in `text_muted` with a wave animation (phase offset per row). Makes loading feel fast and intentional. Files: `src/tui/views/positions.rs`, `src/tui/views/markets.rs`, `src/tui/views/economy.rs`. Test: verify skeleton row count matches expected.

## P2 — Theme & Color Enhancements

- [ ] **Add theme preview on cycle** — When pressing `t`, show a brief (1.5s) toast notification in the status bar: "◆ Midnight" (with theme name in that theme's accent color). Currently the theme just changes with no feedback about which theme you're on unless you look at the header. Files: `src/tui/widgets/status_bar.rs`, `src/app.rs` (theme_toast_tick). Test: test toast display timing.
- [ ] **Add dynamic header accent based on portfolio performance** — Tint the header border/accent color slightly green when portfolio is up today, slightly red when down. Subtle (5-10% blend) so it doesn't clash with theme, but gives an instant ambient mood indicator. Files: `src/tui/widgets/header.rs`. Test: test color blending for positive/negative days.

## P2 — Data & Infrastructure

- [ ] **Add news feed integration** — Fetch financial news from a free RSS/API source (e.g., Yahoo Finance RSS, Finnhub free tier). Display as a scrollable list: timestamp, headline, source. Per-asset filtering. Files: new `src/news/` module, new `src/tui/views/news.rs`. Research: find best free news API that works without API key.
- [ ] **Add FRED economic data** — FRED API (free with API key) for treasury yields, CPI, unemployment, Fed funds rate. Store in new DB table. Cache aggressively (economic data updates daily at most). Files: new `src/data/fred.rs`, `src/db/economic_cache.rs`.
- [x] **Increase test coverage** — Add tests for: `config.rs` (load/save/defaults), `asset_names.rs` (infer_category, search_names), `theme.rs` (lerp_color, gradient_3, gain_intensity_color), `price_chart.rs` (compute_ratio, resample). Files: respective test modules.
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

## P0 — README Rewrite (Owner Request)

- [x] **Rewrite README.md** — The README should be an engaging, fun overview of pftui and how to install it. It should sell the tool, not document internals. Specifically:
  - Make it punchy and visually appealing — hook readers immediately
  - Focus on: what it is, why it's cool, screenshots/examples, installation, quick start
  - **Move the full keybinding reference** to `docs/KEYBINDINGS.md` and link it from the README
  - **Move architecture/technical notes** (component diagram, data flow, price routing, etc.) to `docs/ARCHITECTURE.md` and link from the README
  - Keep the README lean — link to docs for deep dives, don't inline everything
  - Files: `docs/README.md`, new `docs/KEYBINDINGS.md`, new `docs/ARCHITECTURE.md`

## P0 — Visual & UX Brainstorm (Owner Request)

- [x] **Brainstorm visual/UX improvements** — Reviewed entire codebase and UI. Added 20+ new TODO items across P1 and P2 covering animations, data density, micro-interactions, layout polish, and "wow factor" features. See new sections: Animations & Live Feel, Header & Status Bar Enhancements, Positions Table Visual Density, Chart Visual Enhancements, Layout & Visual Polish, Sidebar & Sparkline Enhancements, Micro-Interactions & Feedback, Theme & Color Enhancements.
