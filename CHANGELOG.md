# Changelog

> Reverse chronological. Each entry: date, summary, files changed, tests.
> Automated runs append here after completing TODO items.

## Format

```
_Older entries archived in CHANGELOG-archive.md_

### 2026-03-01 — Add market status indicator to header

- What: added a live US market status indicator to the header bar. Shows "◉ OPEN" in green during NYSE/NASDAQ trading hours (Mon-Fri 9:30 AM - 4:00 PM ET) and "◎ CLOSED" in muted color outside hours. Handles EST/EDT transitions via DST approximation (second Sunday March - first Sunday November). Hidden in compact mode (<100 cols) to preserve space. Renders between the UTC clock and theme name.
- Why: the most-glanced indicator in any trading app. Instantly tells you whether price movements are live or stale without mental timezone math.
- Files: `src/tui/widgets/header.rs` (added `is_us_market_open()`, `is_us_market_open_at()`, `is_us_eastern_dst()`, market indicator rendering)
- Tests: added 10 tests — weekday open/closed before/during/after hours, Saturday, Sunday, exact open/close boundaries, DST summer open/closed, Friday afternoon. Total: 214 tests passing.
- TODO: Add market status indicator to header (P1)

### 2026-03-01 — Add gg/G vim motions for jump-to-top/bottom

- What: implemented `gg` (jump to first row) and `G` (jump to last row) vim motions. Added `g_pending` state to App for two-key sequence detection. Reassigned gain% sort from `g` to `%` and total gain sort from `G` to `$` to free up the vim-standard keys. Both motions work in Positions and Transactions views. `g_pending` is cleared on any non-g keypress.
- Why: vim-native navigation is a core design principle. `gg`/`G` are fundamental vim motions for jumping to list boundaries, critical for efficient keyboard-driven navigation in large portfolios.
- Files: `src/app.rs` (g_pending field, handle_key logic, jump_to_top/jump_to_bottom methods), `src/tui/views/help.rs` (updated keybinding display), `docs/README.md` (updated keybinding docs)
- Tests: added 6 tests — gg jumps to top, g_pending cleared by other key, G jumps to bottom, gg from bottom, gg/G on empty list, gg/G in transactions view. Total: 30 tests passing.
- TODO: Add gg/G vim motions (P1)


### 2026-03-01 — Fix all clippy warnings (22 → 0)

- What: resolved all 22 clippy warnings across the codebase. Removed unused `PriceProvider` enum and `price_provider()` method from `asset.rs`. Removed unused `build_price_map()` from `price/mod.rs`. Added `#[allow(dead_code)]` for legitimately unused-but-tested functions (`delete_all_allocations`, `get_cached_price`, `Transaction::cost_basis`), future-facing structs (`PortfolioSummary`, `Theme` name/chart_line fields), and enum variants (`Resize`, `PriceUpdate::Error`). Collapsed consecutive `.replace()` calls to `.replace([',', '$'], "")` in `setup.rs`. Replaced manual `Default` impl for `PortfolioMode` with derive. Fixed needless borrows, redundant closures, and identical if-branches in `positions.rs`. Replaced `map_or(false, ...)` with `is_some_and(...)` in `sidebar.rs`. Added `#[allow(clippy::too_many_arguments)]` to `add_tx::run`.
- Why: clean compiler output, better code hygiene, removal of dead code paths
- Files: `src/models/asset.rs`, `src/models/portfolio.rs`, `src/models/transaction.rs`, `src/price/mod.rs`, `src/db/allocations.rs`, `src/db/price_cache.rs`, `src/tui/event.rs`, `src/tui/theme.rs`, `src/tui/views/positions.rs`, `src/tui/widgets/price_chart.rs`, `src/tui/widgets/sidebar.rs`, `src/commands/add_tx.rs`, `src/commands/setup.rs`, `src/config.rs`
- Tests: all 22 existing tests pass, no changes needed
- TODO: Fix clippy warnings (P0)
### 2026-02-28 — Initial project documentation and chart fixes

- What: added CLAUDE.md, docs/README.md, docs/VISION.md, TODO.md, CHANGELOG.md. Fixed non-USD fiat chart variants (DXY was shown as standalone single chart; now shows {CCY}/DXY ratio). Fixed chart history pre-fetching (comparison indices like ^GSPC, GC=F, BTC-USD, DX-Y.NYB were only fetched on-demand; now pre-fetched at startup so charts are ready immediately).
- Why: repo had zero documentation. Fiat charts showed irrelevant DXY standalone instead of meaningful ratio. Charts showed "Loading..." until user manually opened them.
- Files: `CLAUDE.md`, `docs/README.md`, `docs/VISION.md`, `TODO.md`, `CHANGELOG.md`, `src/app.rs`
- Tests: added 9 chart variant tests (BTC, Gold, USD cash, non-USD cash EUR/GBP, equity, crypto, fetch dedup, DXY inclusion). Total: 22 tests passing.

### 2026-02-28 — Initial commit

- What: full pftui implementation — TUI portfolio tracker with live prices, braille charts, 6 themes, privacy mode, CLI commands
- Files: all src/ files, Cargo.toml
- Tests: 13 tests (db/transactions, db/allocations, db/price_history, db/price_cache, models/position)

### 2026-03-01 — Fix crypto Yahoo fallback double-suffix & blank ratio panels

- What: (1) Added `yahoo_crypto_symbol()` helper that checks if a symbol already ends with `-USD` before appending the suffix. Fixes `BTC-USD` becoming `BTC-USD-USD` when CoinGecko fails and Yahoo fallback is used for chart variant symbols. Applied to both `fetch_history` and `fetch_all` crypto fallback paths. (2) Fixed `render_ratio_mini` in `price_chart.rs` to show "Loading {num}/{den}..." when `compute_ratio` produces fewer than 2 data points, instead of silently rendering a blank panel.
- Why: (1) Chart variant symbols like `BTC-USD` were being double-suffixed, causing Yahoo Finance lookups to fail silently. (2) Blank mini ratio panels in the "All" chart view gave no feedback about loading state, inconsistent with how `render_single_mini` handles the same case.
- Files: `src/price/mod.rs`, `src/tui/widgets/price_chart.rs`
- Tests: added 2 tests for `yahoo_crypto_symbol` (suffix append + no double-suffix). Total: 24 tests passing.
- TODO: Fix CoinGecko→Yahoo fallback double-suffix (P0), Show "Loading..." on blank mini ratio panels (P0)

### 2026-03-01 — Fix chart timeframe selection (all timeframes now work)

- What: fixed the P0 bug where only 3M chart timeframe loaded data reliably. Root cause: `request_all_history()` only fetched `chart_timeframe.days()` (default 90) from APIs, and incoming history data replaced the in-memory cache entirely — so switching to 6M/1Y/5Y either had insufficient data or lost existing longer-range data from DB cache. Three fixes: (1) all initial history fetches now request 5Y (1825 days) so every timeframe has data to slice from; (2) `request_history_for_symbol()` also uses 5Y; (3) new `merge_history_into()` free function merges incoming records with existing in-memory data using a BTreeMap (union of dates, newer prices win for overlaps), preventing shorter re-fetches from discarding cached data.
- Why: 1W, 1M, 6M, 1Y, 5Y timeframes showed empty charts or failed to load. Only 3M worked because it matched the default fetch size. This was a major UX regression.
- Files: `src/app.rs`
- Tests: added 4 tests for `merge_history_into` (empty map, preserves older data, adds new dates, existing empty vec). Total: 218 tests passing.
- TODO: Fix chart timeframe selection (P0)

### 2026-03-01 — Switch to on-demand chart history fetching

- What: changed initial history fetch from 5Y (1825 days) for every asset to 3M (90 days). Added per-symbol fetch tracking (`fetched_history_days` HashMap) so longer timeframes are only fetched from APIs when the user actually switches to them (6M, 1Y, 5Y). DB cache still loads all available data from previous sessions, providing immediate display for longer timeframes without fresh API calls. New `request_history_if_needed()` method checks whether we've already fetched sufficient data before issuing an API request, preventing redundant network calls.
- Why: fetching 5Y of daily data for every asset on every startup was wasteful — slow startup, excessive API calls, risk of CoinGecko rate limiting. Most users only view 3M charts. Now startup is faster, API usage is minimal, and longer ranges are fetched on-demand when needed.
- Files: `src/app.rs`
- Tests: added 5 tests for on-demand fetch tracking (empty initial state, tracks days, skips when sufficient, upgrades when more needed, exact match skips). Total: 223 tests passing.
- TODO: Switch to on-demand chart history fetching (P0)

### 2026-03-01 — Restructure layout: left=portfolio overview, right=asset chart

- What: restructured the Positions view layout to establish clear L/R pane separation. Left pane (57%) now shows the positions table (top) with portfolio overview below it (allocation bars + sparkline). Right pane (43%) now always shows the per-asset price chart for the selected position — no longer hidden behind Enter toggle. Chart navigation keybindings (J/K for chart variant, h/l for timeframe) now work immediately in Positions view without needing to "open" the detail view first. Enter now simply toggles the detail popup overlay. Position navigation (j/k, gg, G, Ctrl-d/u) auto-fetches chart data for the newly selected asset.
- Why: the old layout put allocation bars and portfolio sparkline in the right pane as a "sidebar", and the asset chart only appeared after pressing Enter. This meant the most important real-time data (per-asset chart) was hidden by default, and portfolio overview widgets competed for space with the asset detail. The new layout matches the intended Bloomberg-style information density: portfolio overview on the left, selected asset detail always visible on the right.
- Files: `src/tui/ui.rs` (layout restructure with dynamic overview height), `src/app.rs` (simplified Enter handler, view-mode-based chart keybindings, auto-fetch on selection change), `src/tui/views/positions.rs` (border color logic)
- Tests: updated 3 tests (renamed to reflect view-mode guards, test for non-Positions view). Total: 223 tests passing.
- TODO: Fix layout: allocation bars belong in left pane (P0), Fix layout: portfolio chart on left, asset chart on right (P0)

### 2026-03-01 — Add permanent asset detail header in right pane

- What: created new `asset_header` widget that renders a compact, always-visible info panel above the price chart in the right pane. Shows: category-colored dot, symbol (bold), name, current price with currency, gain/loss percentage with directional arrow (▲/▼), dollar gain/loss, quantity, current value, and allocation percentage. Respects privacy mode (hides quantity, value, and gain in percentage-only mode). Right pane layout now splits into a 5-row header + remaining chart area, with graceful fallback to chart-only when terminal height is insufficient (< header + 6 rows).
- Why: the asset overview info was only accessible via the Enter popup, forcing users to toggle a popup to see basic position data. Now the most-checked info (price, gain, value) is always visible alongside the chart, matching the Bloomberg-style density the app targets.
- Files: new `src/tui/widgets/asset_header.rs`, `src/tui/widgets/mod.rs` (register module), `src/tui/ui.rs` (right pane layout split)
- Tests: added 12 tests for format helpers (format_price, format_money, format_qty) and height constant. Total: 235 tests passing.
- TODO: Make asset detail info permanent in right pane header (P0)
