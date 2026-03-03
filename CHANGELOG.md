# Changelog

> Reverse chronological. Each entry: date, summary, files changed, tests.
> Automated runs append here after completing TODO items.

### 2026-03-02 — Add crosshair cursor on charts

- What: press `x` in Positions view to toggle a crosshair cursor on the chart. When active, `h`/`l` move the vertical crosshair left/right instead of cycling chart timeframes. A vertical `│` line in `text_accent` color is drawn at the cursor position across all chart rows (including volume and separator). The stats line switches to show the date and price at the cursor position with hint text (`x:off  h/l:move`). Chart title nav hint updates to show crosshair mode. Crosshair resets when changing selected position.
- Why: lets users inspect historical data points on the braille chart without leaving the TUI. Common feature in financial terminals (Bloomberg, TradingView).
- Key: `x` (toggle on/off), `h`/`l` (move cursor left/right when active)
- Files: `src/app.rs` (crosshair_mode, crosshair_x fields, `x` keybinding, h/l override, reset on position change), `src/tui/widgets/price_chart.rs` (CrosshairState struct, vertical line + tooltip rendering, crosshair parameter threading), `src/tui/views/help.rs` (help text for `x` key)
- Tests: 15 new — crosshair toggle on/off, h/l movement, clamp at zero, timeframe unchanged when active, timeframe changes when inactive, no effect in other views, reset on position change, record mapping (leftmost/rightmost/middle), bounds clamping. Total: 486 tests passing.
- TODO: Add crosshair cursor on charts (P2)

### 2026-03-02 — Add `pftui import` command for restoring JSON snapshots

- What: new `pftui import <path> [--mode replace|merge]` command. Imports data from JSON snapshot files produced by `pftui export json`. Two modes: `replace` (default) wipes existing transactions, allocations, and watchlist then inserts from snapshot; `merge` adds new entries without deleting, skipping duplicates. Validates before importing: portfolio mode match, non-empty symbols, positive quantities, non-negative prices, YYYY-MM-DD dates, 0-100 allocation pcts. All inserts run in a single SQLite transaction for atomicity.
- Why: completes the export/import roundtrip. Users can back up, restore, and migrate portfolios between machines. Merge mode enables combining data from multiple sources.
- Files: new `src/commands/import.rs` (717 lines), `src/cli.rs` (Import variant + ImportModeArg enum), `src/main.rs` (dispatch), `src/commands/mod.rs`
- Tests: 15 new tests — replace/merge for transactions, allocations, and watchlist; duplicate skip on merge; validation rejection for mode mismatch, empty symbol, negative quantity, invalid date, invalid allocation pct; empty snapshot; invalid JSON; file not found; full export→import roundtrip. Total: 471 tests passing.
- TODO: Add `pftui import` command (P1)

## Format

```
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

### 2026-03-01 — Fix BTC/crypto price fetching reliability

- What: fixed crypto price fetching by adding proper User-Agent headers to all CoinGecko API requests (missing headers likely caused silent rejections), adding 429 rate-limit retry with 2s backoff, and adding 15s request timeouts. Refactored CoinGecko module with `build_client()` and `get_with_retry()` helpers for consistent HTTP behavior. Separated error paths in the batch price fetcher: CoinGecko empty responses and API failures now produce distinct error messages before falling back to Yahoo. Price errors are no longer silently swallowed — they're stored on `App.last_price_error` and displayed in the status bar with a ⚠ indicator that fades after ~5 seconds.
- Why: BTC price failed to load for at least one user. Root cause was likely CoinGecko rejecting requests without User-Agent headers, combined with zero error visibility (all `PriceUpdate::Error` messages were silently discarded). Users had no way to know why prices failed or which fallback path was taken.
- Files: `src/price/coingecko.rs` (User-Agent, retry, timeout, refactored into helpers), `src/price/mod.rs` (explicit error reporting on CoinGecko failure), `src/app.rs` (store+display price errors), `src/tui/widgets/status_bar.rs` (error indicator)
- Tests: added 7 tests for CoinGecko module (ticker mapping, aliases, response parsing, client construction). Total: 242 tests passing.
- TODO: Fix BTC price fetching (P0)

### 2026-03-01 — Add day gain/loss to header

- What: added daily portfolio change display to the header bar. Shows today's dollar gain/loss with a directional arrow (▲/▼) next to the total gain percentage, e.g. "$45.2k +1.3% ▲$580 today". Computed by comparing each position's current live price to the most recent historical close price (previous trading day). Correctly handles the case where today's date already has a history record by using the second-to-last entry. Skips cash positions (always $1). Hidden in privacy/percentage mode.
- Why: the daily portfolio change is the single most-checked number in any portfolio app. Previously users had to mentally compute it from individual position changes. Now it's immediately visible in the header at a glance.
- Files: `src/app.rs` (new `daily_portfolio_change` field + `compute_daily_change()` method), `src/tui/widgets/header.rs` (display logic + `format_compact_signed()` helper)
- Tests: added 7 tests for daily change computation (no history, with prev close, negative change, multiple positions, cash skip, percentage mode, today record handling) + 5 tests for format_compact_signed helper. Total: 254 tests passing.
- TODO: Add day gain/loss to header (P1)

### 2026-03-02 — Add price flash with directional arrows

- What: extended the price flash animation to show directional arrows (▲/▼) when prices update. Flash background color is now direction-aware: green (`gain_green`) when price goes up, red (`loss_red`) when price goes down, accent color when unchanged. The `price_flash_ticks` map now stores `(tick, PriceFlashDirection)` tuples instead of just ticks, enabling the positions table to render contextual arrows during the ~0.7s flash window. Widened the Price column from 10 to 12 characters to accommodate the arrow indicator without truncating prices.
- Why: the existing price flash only changed the cell background to the accent color, giving no indication of whether the price moved up or down. Users had to read the actual number to understand the direction. Now the flash itself communicates direction at a glance — green ▲ for increases, red ▼ for decreases — making price updates scannable without reading the numbers.
- Files: `src/app.rs` (new `PriceFlashDirection` enum, direction detection in tick handler), `src/tui/views/positions.rs` (directional arrow rendering, column width bump)
- Tests: added 5 tests for flash direction logic (up, down, same, no previous price, storage). Total: 259 tests passing.
- TODO: Add price flash with directional arrows (P1)

### 2026-03-02 — Add scrolling ticker tape in header

- What: added a horizontally scrolling marquee-style ticker tape as a second line in the header, showing live market data with directional arrows and color-coded change percentages. Displays all 18 market symbols (SPX, NDX, DJI, RUT, VIX, Gold, Silver, Oil, NatGas, BTC, ETH, SOL, DXY, EUR, GBP, JPY, 10Y, 2Y) with their daily change. Scrolls left at ~10 chars/sec using `tick_count / 6` for smooth animation. Only active on Positions view in non-compact mode (≥100 columns). Header height is now dynamic: 3 rows when ticker is active, 2 otherwise. Shows "waiting for market data…" placeholder until prices load.
- Why: gives the app a Bloomberg Terminal-like live data feel. The scrolling ticker makes market movements scannable at a glance without switching to the Markets tab. The directional arrows (▲/▼) and color-coding (green/red) provide instant visual context.
- Files: `src/tui/widgets/header.rs` (ticker entry building, styled span windowing with modular scroll, `header_height()` function), `src/tui/ui.rs` (dynamic header height via `header_height()`)
- Tests: added 8 tests for ticker text format, negative format, scroll divisor constant, separator constant, header height (positions/compact/other views), and empty-data behavior. Total: 267 tests passing.
- TODO: Add scrolling ticker tape in header (P1)

### 2026-03-02 — Add GitHub Actions CI workflow

- What: created `.github/workflows/ci.yml` — first CI pipeline for the project. Runs on every push to master and on PRs. Matrix tests on both `ubuntu-latest` and `macos-latest`. Pipeline steps: `cargo clippy --all-targets -- -D warnings` (zero-warning policy), `cargo test` (all tests must pass), `cargo build --release` (verify release build). Caches `~/.cargo/registry`, `~/.cargo/git`, and `target/` directory keyed on `Cargo.lock` hash for fast subsequent runs.
- Why: CI is the foundation for the entire release pipeline. Without it, broken code can be pushed to master undetected. This unblocks the release workflow, crates.io publishing, Homebrew formula, and all other distribution tasks.
- Also: marked "Prepare Cargo.toml for publishing" as complete — all required crates.io metadata (`description`, `license`, `repository`, `homepage`, `readme`, `keywords`, `categories`, `[package.metadata.deb]`, `[package.metadata.generate-rpm]`) was already present in Cargo.toml.
- Files: `.github/workflows/ci.yml` (new)
- Tests: 267 passing, 0 clippy warnings.
- TODO: Create GitHub Actions CI workflow (P0), Prepare Cargo.toml for publishing (P0)

### 2026-03-02 — Add `pftui refresh` headless price command

- What: added a new `pftui refresh` CLI subcommand that fetches and caches current prices for all portfolio symbols without launching the TUI. Collects symbols from transactions (full mode) or allocations (percentage mode) plus watchlist entries, deduplicates across sources, fetches via Yahoo Finance (equities/commodities/forex) and CoinGecko with Yahoo fallback (crypto), writes results to the price_cache DB table, and prints a summary with symbol, price, and source for each fetched price. Handles cash positions statically (always $1). Supports non-USD base currencies by fetching the forex rate.
- Why: this was the #1 requested feature from both beta testers. Without headless price refresh, pftui is a transaction ledger rather than a live portfolio tracker — `pftui summary` shows N/A for all prices unless the TUI has been launched recently. Now agents, scripts, and cron jobs can keep prices fresh: `pftui refresh && pftui summary`.
- Files: new `src/commands/refresh.rs`, `src/commands/mod.rs` (register module), `src/cli.rs` (add `Refresh` subcommand), `src/main.rs` (wire handler)
- Tests: added 10 unit tests for symbol collection (empty DB, from transactions, deduplication with watchlist, percentage mode), price formatting (large/medium/small/very small), and yahoo crypto symbol formatting. Total: 277 tests passing.
- TODO: Add `pftui refresh` headless price command (P0)

### 2026-03-02 — Add `--group-by category` flag to `pftui summary`

- What: added `--group-by category` flag to the `pftui summary` command. When used, groups positions by asset class (Equity, Crypto, Commodity, Cash, Fund, Forex) and displays per-category value, cost, gain%, and allocation%. Each category line is followed by its constituent symbols. Groups are sorted by value descending. Works in both full mode (shows value/cost/gain/alloc) and percentage mode (shows alloc% per category). Added `SummaryGroupBy` enum to CLI with clap `ValueEnum` derive for extensibility.
- Why: this was the #3 most-requested feature from beta testers. Both testers wanted quick category-level allocation breakdowns ("Commodity 41%, Crypto 33%, Cash 26%") for portfolio monitoring and daily briefings. Previously, category allocation could only be inferred by mentally summing individual position allocations.
- Files: `src/cli.rs` (new `SummaryGroupBy` enum, `--group-by` arg on Summary), `src/commands/summary.rs` (category grouping logic, grouped output for full and percentage modes), `src/main.rs` (pass group_by to summary::run)
- Tests: added 6 tests for category grouping (single category, multiple same, mixed categories, no price, format_category, empty positions). Total: 283 tests passing.
- TODO: [Feedback] Add `--group-by category` flag to `pftui summary` (P0)

### YYYY-MM-DD — Summary of change
- What: brief description of what was done
- Why: what problem it solves or what value it adds
- Files: list of files modified
- Tests: tests added or modified
- TODO: which TODO item was completed
```

---


### 2026-03-01 — Visual/UX brainstorm: 20+ new TODO items

- What: reviewed the entire codebase (11k+ lines across 40 source files) and brainstormed 20+ new visual/UX improvement ideas, each scoped to ~1 hour. Added them as well-structured TODO items across 8 new categories: Animations & Live Feel (4 items — price flash arrows, scrolling ticker tape, pulsing active border, row highlight animation), Header & Status Bar Enhancements (3 items — daily portfolio change, market open/closed indicator, breadcrumb navigation), Positions Table Visual Density (3 items — inline mini-sparklines, category dots, gain magnitude bars), Chart Visual Enhancements (3 items — crosshair cursor, area fill gradient, Bollinger Bands), Layout & Visual Polish (4 items — double-line borders, popup shadows, category dividers, ultra-wide 3-column layout), Sidebar & Sparkline Enhancements (2 items — sparkline period selector, allocation change arrows), Micro-Interactions & Feedback (3 items — keystroke echo, sort animation, loading skeletons), Theme & Color Enhancements (2 items — theme toast notification, dynamic header tinting). Every item includes specific file references, implementation approach, and test strategy.
- Why: owner requested a brainstorm run to generate ideas that make pftui more visually impressive, aesthetic, high-tech, and polished — "Bloomberg Terminal meets cyberpunk." The existing TODO was running low on unclaimed visual/UX items. These 20+ items provide a deep backlog of polish work for future automated runs, each independently implementable in a single session.
- Files: `TODO.md`
- Tests: no code changes, all 204 tests still passing
- TODO: Brainstorm visual/UX improvements (P0)


### 2026-03-01 — Add daily change % column to positions table

- What: added a Day% column to both the full and privacy positions tables, showing each position's daily price change as a percentage. Computed from the last two entries in the price history (same approach used by Markets, Economy, and Watchlist views). Column sits between Price and Gain% in the full table, and between Price and Alloc% in the privacy table. Uses gain-intensity coloring (green gradient for gains, red for losses) via `theme::gain_intensity_color`. Added `compute_change_pct()` public function for reuse. Added `format_change_pct()` helper. Privacy-safe — shows only percentage change, no absolute dollar values. Updated help overlay with Day% column note. Updated README positions view description.
- Why: daily change % is one of the most essential portfolio metrics — it tells you immediately how each position performed today. The app showed total gain % but not the day's move, which is what most users check first. Markets, Economy, and Watchlist views all had daily change; positions was the only view missing it.
- Files: `src/tui/views/positions.rs` (compute_change_pct, format_change_pct, Day% column in full + privacy tables, 10 tests), `src/tui/views/help.rs` (Day% note), `docs/README.md` (positions description)
- Tests: added 10 tests — compute_change_pct_basic, compute_change_pct_negative, compute_change_pct_no_change, compute_change_pct_uses_last_two_entries, compute_change_pct_single_record, compute_change_pct_no_history, compute_change_pct_zero_prev_close, format_change_pct_positive, format_change_pct_negative, format_change_pct_none. Total: 204 tests passing.
- TODO: Add daily change % column to positions (P2)

### 2026-03-01 — Rewrite README, extract Architecture and Keybindings docs

- What: rewrote `docs/README.md` from a dense technical reference into an engaging, punchy project overview that sells the tool — focused on features, quick start, and visual appeal. Extracted the full keybinding reference (navigation, views, charts, sorting, actions) into a new `docs/KEYBINDINGS.md`. Extracted all architecture content (component diagram, data flow, price routing, layout diagrams, chart system, database schema, configuration, technology table, file map) into a new `docs/ARCHITECTURE.md`. README now links to both docs for deep dives instead of inlining everything. README covers: why pftui, quick start, usage, views overview, charts, themes, essential keybindings (with link to full reference), and a brief architecture summary (with link to full docs).
- Why: the README was a 500-line technical reference document that buried the lede. Nobody scrolls through database schemas to decide if they want to try a tool. The new README hooks readers immediately, shows what makes pftui special, and gets them to `cargo build` in seconds. Technical details are preserved and properly organized in dedicated docs.
- Files: `docs/README.md` (full rewrite), new `docs/KEYBINDINGS.md`, new `docs/ARCHITECTURE.md`
- Tests: no code changes, all 194 tests still passing
- TODO: Rewrite README.md (P0)

### 2026-03-01 — Increase test coverage across 4 modules

- What: added comprehensive test suites to 4 previously untested modules: `config.rs` (8 tests — default values, TOML roundtrip serialization, deserialization with missing fields, empty TOML defaults, PortfolioMode serialization, is_percentage_mode, config_path), `asset_names.rs` (14 tests — resolve_name known/unknown, infer_category for all 6 asset categories plus case insensitivity, search_names by ticker prefix, name prefix, exact match priority, no match, case insensitivity), `theme.rs` (21 tests — lerp_color at 0/0.5/1/clamping/non-RGB fallback, gradient_3 at 0/0.25/0.5/1, pulse_intensity range check, gain_intensity_color positive/negative/zero/saturation, all themes load by name, unknown theme fallback, next_theme cycling/wrapping, category_color all variants), `price_chart.rs` (10 new tests — compute_ratio basic/missing dates/zero denominator/empty inputs, resample identity/upscale/downscale/empty/zero target/single value).
- Why: these 4 modules had zero test coverage despite containing core business logic (config parsing, asset classification, color math, chart data computation). Adding tests catches regressions in financial data categorization, theme color interpolation, chart ratio computation, and config serialization — all areas where silent breakage would be hard to notice.
- Files: `src/config.rs` (8 tests), `src/models/asset_names.rs` (14 tests), `src/tui/theme.rs` (21 tests), `src/tui/widgets/price_chart.rs` (10 new tests)
- Tests: added 53 new tests. Total: 194 tests passing.
- TODO: Increase test coverage (P2)


### 2026-03-01 — Add 52-week high/low range indicators

- What: added a 52-week range indicator column to the positions table (both full and privacy modes). Each position shows a visual range bar (`━━━●━━━`) with a colored dot indicating where the current price sits between the 52-week low and high, plus a percentage distance from the 52-week high (e.g. `-12%`, or `ATH` when at the high). The dot color uses a red→neutral→green gradient based on position within range. Also added 52-week range info to the position detail popup (Enter), showing the numeric low—high range and distance from high with color coding (green at high, neutral near high, red when >10% below). The `compute_52w_range()` function takes price history records and limits analysis to the most recent 365 entries, includes the current live price in high/low calculations, and handles edge cases (flat prices, no data, new highs/lows). Reduced Qty column from 10→8 chars to accommodate the new 52W column (11 chars). Column header is `52W`.
- Why: 52-week high/low is one of the most commonly referenced metrics for any asset — it tells you instantly whether something is near its peak, at a bottom, or somewhere in between. The visual range bar makes this scannable across an entire portfolio at a glance, and the from-high percentage quantifies the distance. Together with gain% and sparkline trend, this gives three different temporal perspectives on each position.
- Files: `src/tui/views/positions.rs` (Range52W struct, compute_52w_range function, build_52w_spans function, 52W column in full and privacy tables, 8 tests), `src/tui/views/position_detail.rs` (52W range in Performance section), `src/tui/views/help.rs` (52W help note), `docs/README.md` (52W feature bullets)
- Tests: added 8 tests — compute_52w_range_basic, compute_52w_range_at_high, compute_52w_range_at_low, compute_52w_range_no_records, compute_52w_range_single_record, compute_52w_range_no_price, compute_52w_range_flat_price, compute_52w_range_limits_to_365_records. Total: 141 tests passing.
- TODO: Add 52-week high/low indicators (P2)


### 2026-03-01 — Improve allocation bars with inline labels and total value

- What: enhanced the allocation bars widget with two improvements. (1) Percentage labels are now rendered inside the filled portion of bars when the bar is wide enough (>= 5 cells) — e.g. a 42% equity bar shows "42%" overlaid in bold black text on the colored bar background, making it instantly readable without scanning to the right-side label. When bars are too narrow, they render as before (solid fill). (2) Total portfolio value is displayed below the allocation bars as "Total: $XX.XK" using compact formatting ($2.50M, $456.7K, $12,345, $999.00). The total value line respects privacy mode — hidden when percentage mode is active or privacy view is toggled. Updated sidebar layout to allocate an extra row for the total value line when present. Refactored `fractional_bar()` into `fractional_bar_with_label()` with centered label placement and width-preserving rendering.
- Why: the allocation bars showed percentages only in the right-side label column, wasting the visual space of the bar itself. Bloomberg-style inline labels make allocation magnitudes immediately scannable. The total value display provides essential portfolio context (the one number every user wants to see at a glance) without taking up a separate widget.
- Files: `src/tui/widgets/allocation_bars.rs` (inline labels, total value line, format_compact_value, fractional_bar_with_label, 9 tests), `src/tui/widgets/sidebar.rs` (extra row allocation for total value)
- Tests: added 9 tests — format_compact_value_millions, format_compact_value_hundred_thousands, format_compact_value_thousands, format_compact_value_small, fractional_bar_label_shown_when_wide, fractional_bar_label_hidden_when_narrow, fractional_bar_zero_width, fractional_bar_full_width, fractional_bar_preserves_total_width. Total: 133 tests passing.
- TODO: Improve allocation bars (P2)


### 2026-03-01 — Add position detail popup

- What: added a full-screen position detail popup that appears when pressing Enter on a position. Shows comprehensive info: symbol, name, category, current price, quantity, avg cost, cost basis, current value, gain, gain%, allocation%, and the most recent 10 buy/sell transactions for that symbol (sorted newest first). Respects privacy mode — hides quantity, cost, gain, and transaction history when privacy is active. Uses theme colors throughout including gain-aware coloring for performance metrics and category-colored badge. Transaction rows show BUY (green) / SELL (red) with date, quantity, and price. Popup is centered, 64 columns wide, and auto-sizes to content. Enter from popup transitions to the chart view in the sidebar. Esc closes the popup. Help overlay updated (Enter shows "Position detail / chart"). Status bar hint updated from "Chart" to "Detail". Added PositionExt trait with name_or_symbol() helper. Popup closes automatically when switching views (tabs 2-5).
- Why: pressing Enter only opened the price chart in the sidebar, which showed one dimension of data. A detail popup gives a comprehensive view of a position at a glance — price info, cost basis analysis, gain/loss metrics, and full transaction history — without leaving the positions view. This is the first P2 visual polish item from the TODO.
- Files: new `src/tui/views/position_detail.rs` (render function, build_detail_lines, format helpers, PositionExt trait, 10 tests), `src/app.rs` (detail_popup_open field, updated Enter handler with 3-state flow, Esc handler for popup, popup close on view switch), `src/tui/ui.rs` (position_detail popup render dispatch), `src/tui/views/mod.rs` (position_detail module), `src/tui/views/help.rs` (Enter keybinding text), `src/tui/widgets/status_bar.rs` (Enter hint text), `TODO.md`
- Tests: added 10 tests — detail_lines_contain_symbol, detail_lines_contain_price_info, detail_lines_contain_gain_info, detail_lines_privacy_hides_values, detail_lines_contain_category, detail_lines_show_transactions, detail_lines_privacy_hides_transactions, format_money_large, format_money_medium, format_money_small. Total: 124 tests passing.
- TODO: Add position detail popup (P2)




### 2026-03-01 — Add responsive layout for narrow terminals

- What: added responsive layout that adapts to terminal width. Below 100 columns, the sidebar (allocation bars, portfolio sparkline, price chart panel) is hidden and positions use the full terminal width. Header abbreviates tab names ("Econ"→"Ec", "Watch"→"Wl") and hides the clock and theme indicator. Status bar shows only essential hints (Help, Search) instead of the full hint bar. Added `terminal_width` field to App (default 120, updated from `crossterm::terminal::size()` on startup and resize). Replaced `set_terminal_height` with `set_terminal_size(w, h)`. Exported `COMPACT_WIDTH` constant (100) from `ui.rs` so header and status bar can reference the same threshold.
- Why: the app assumed wide terminals (100+ columns). On narrow terminals, the 57/43 split made both panels too small to be useful — positions got truncated and the sidebar was unreadable. Hiding the sidebar on narrow terminals gives positions room to display properly. This is the first P2 polish item from the backlog.
- Files: `src/app.rs` (terminal_width field, set_terminal_size method, removed set_terminal_height, 5 responsive tests), `src/tui/mod.rs` (set width on startup and resize), `src/tui/ui.rs` (COMPACT_WIDTH const, conditional sidebar hiding, 1 test), `src/tui/widgets/header.rs` (compact mode: abbreviate tabs, hide clock/theme), `src/tui/widgets/status_bar.rs` (compact mode: essential hints only), `docs/README.md` (responsive layout section, updated layout diagram)
- Tests: added 5 tests — terminal_width_default, terminal_height_default, set_terminal_size_updates_both, set_terminal_size_narrow, set_terminal_size_wide. Added 1 test — compact_width_threshold_is_100. Total: 114 tests passing.
- TODO: Add responsive layout (P2)


### 2026-03-01 — Add Watchlist view (tab 5) with CLI commands

- What: added a Watchlist view accessible via the `5` key. Users can track assets without holding them in their portfolio. New DB table `watchlist (id, symbol, category, added_at)` with unique constraint on symbol. CLI commands: `pftui watch <SYMBOL>` (auto-detects category or accepts `--category`) and `pftui unwatch <SYMBOL>`. TUI displays a table with symbol, name, category (color-coded), live price, and daily change % with gain-aware coloring. Empty state shows usage instructions. Symbols stored uppercase, all operations case-insensitive. Full vim navigation (j/k, gg/G, Ctrl+d/Ctrl+u) works. Header shows `[5]Watch` tab. Help overlay updated with `5` keybinding. Prices and 30-day history fetched on tab activation. Watchlist reloads from DB on each tab switch so CLI-added symbols appear immediately.
- Why: the VISION roadmap lists Watchlist as a core view — tracking assets you're interested in but don't hold is essential for research and monitoring. This completes the P1 New Views category (Markets, Economy, Watchlist all done).
- Files: new `src/db/watchlist.rs` (WatchlistEntry struct, add/remove/list/get_symbols/is_watched CRUD, 7 tests), new `src/tui/views/watchlist.rs` (render function, yahoo_symbol_for helper, format_price, compute_change_pct, empty state, 7 tests), `src/db/schema.rs` (watchlist table migration), `src/db/mod.rs` (watchlist module), `src/cli.rs` (Watch/Unwatch subcommands), `src/main.rs` (Watch/Unwatch handlers with category auto-detection), `src/app.rs` (ViewMode::Watchlist, watchlist_selected_index, watchlist_entries, load_watchlist, request_watchlist_data, key 5 handler, Watchlist arms in all 6 navigation methods), `src/tui/views/mod.rs` (watchlist module), `src/tui/ui.rs` (Watchlist render dispatch), `src/tui/widgets/header.rs` (Watchlist tab display), `src/tui/views/help.rs` (key 5 entry), `docs/README.md` (Watchlist features, keybinding, CLI commands, DB table, file map)
- Tests: added 14 tests — db: add_and_list, upsert_same_symbol, remove, remove_nonexistent, is_watched, case_insensitive_operations, get_watchlist_symbols; view: yahoo_symbol_for_crypto, yahoo_symbol_for_crypto_already_suffixed, yahoo_symbol_for_equity, yahoo_symbol_for_commodity, format_price_large, format_price_medium, format_price_small. Total: 108 tests passing.
- TODO: Add Watchlist view (tab 5) (P1)

### 2026-03-01 — Add Economy dashboard view (tab 4)

- What: added a new Economy view accessible via the `4` key. Displays a macro dashboard with 14 economic indicators across 4 groups: Treasury Yields (2Y, 5Y, 10Y, 30Y via ^IRX, ^FVX, ^TNX, ^TYX), Currency (DXY, EUR, GBP, JPY, CNY), Commodities (Gold, Oil, Copper, NatGas), and Volatility (VIX). Each row shows symbol, name, group (color-coded), live value, and daily change % with gain-aware coloring. Yields are formatted with % suffix (e.g. "4.325%") while currencies/commodities use standard price formatting. Visual group separators (blank rows) divide sections. Prices and 30-day history fetched at startup and on tab activation. Full vim navigation (j/k, gg/G, Ctrl+d/Ctrl+u) works. Header shows `[4]Econ` tab. Also fixed Markets tab being incorrectly nested inside `if !pct_mode` block in header — now always visible. Help overlay updated with `4` keybinding.
- Why: the Markets tab shows broad market instruments but lacks macro economic context. Treasury yields, the dollar index, and commodity prices are essential for understanding the economic environment — interest rate expectations, inflation signals, currency strength. This is the second new view tab from the VISION roadmap.
- Files: new `src/tui/views/economy.rs` (EconomyItem struct, EconomyGroup enum, economy_symbols list, render function, format_value, compute_change_pct, category_for_group, 9 tests), `src/app.rs` (ViewMode::Economy, economy_selected_index, key 4 handler, request_economy_data method, Economy arms in all 6 navigation methods), `src/tui/views/mod.rs` (economy module), `src/tui/ui.rs` (Economy render dispatch), `src/tui/widgets/header.rs` (Economy tab display, fixed Markets tab brace nesting), `src/tui/views/help.rs` (key 4 entry), `docs/README.md` (Economy features, keybinding, file map)
- Tests: added 9 tests — economy_symbols_has_expected_count, economy_symbols_has_all_groups, economy_symbols_yahoo_symbols_unique, economy_symbols_yields_first, format_value_yields_shows_percent, format_value_currency_large, format_value_commodity_large, format_value_currency_small, category_for_group_mapping. Total: 94 tests passing.
- TODO: Add Economy view (tab 4) (P1)

### 2026-03-01 — Add Markets overview view (tab 3)

- What: added a new Markets view accessible via the `3` key. Displays a table of 18 major market symbols across 5 categories: indices (SPX, NDX, DJI, RUT, VIX), commodities (Gold, Silver, Oil, NatGas), crypto (BTC, ETH, SOL), forex (DXY, EUR, GBP, JPY), and bonds (10Y, 2Y Treasury). Each row shows symbol, name, category (color-coded), live price, and daily change % with gain-aware coloring. Prices and 30-day history are fetched at startup and on tab activation for change % calculation. Full vim navigation (j/k, gg/G, Ctrl+d/Ctrl+u) works in the Markets view. Header shows `[3]Mkt` tab with active/inactive styling. Help overlay updated with `3` keybinding.
- Why: the app had no way to view broad market data beyond your own portfolio. A Markets tab is essential for context — seeing how indices, commodities, crypto, and forex are performing alongside your positions. This is the first of the new view tabs from the VISION roadmap.
- Files: new `src/tui/views/markets.rs` (MarketItem struct, market_symbols list, render function, format_price, compute_change_pct, 8 tests), `src/app.rs` (ViewMode::Markets, markets_selected_index, key 3 handler, request_market_data method, Markets arms in all 6 navigation methods), `src/tui/views/mod.rs` (markets module), `src/tui/ui.rs` (Markets render dispatch), `src/tui/widgets/header.rs` (Markets tab display), `src/tui/views/help.rs` (key 3 entry), `docs/README.md` (Markets features, keybinding, file map)
- Tests: added 8 tests — market_symbols_has_expected_count, market_symbols_has_all_categories, market_symbols_yahoo_symbols_unique, market_symbols_spx_is_first, format_price_large, format_price_medium, format_price_ones, format_price_small. Total: 85 tests passing.
- TODO: Add Markets view (tab 3) (P1)

### 2026-03-01 — Add SMA(20) and SMA(50) moving average overlays

- What: added Simple Moving Average (SMA) computation and braille overlay rendering on single-symbol price charts. SMA(20) renders as a thin braille dot line in `text_accent` color, SMA(50) in `border_accent` color. Added `compute_sma()` function using a sliding window sum for O(n) computation. Added `braille_bits()` (refactored from `braille_char`) and `braille_dot_bits()` helper for single-dot overlay rendering. SMA dots are composited with price area bits using bitwise OR, with color priority: price gradient dominates when both are present, SMA color shows through in empty cells. SMA legend ("─SMA20 ─SMA50") appended to the stats line below the chart. SMAs only appear on single-symbol full charts — not on ratio charts, mini panels, or "All" multi-panel views where they are not meaningful. NaN values in SMA (the leading `period-1` entries) are preserved through resampling so the line starts only where valid data exists.
- Why: Moving averages are foundational technical analysis indicators. SMA(20) shows short-term trend, SMA(50) shows medium-term trend. Crossovers between the two (golden cross / death cross) are widely-used trading signals. Without SMAs, charts showed only raw price action with no trend context.
- Files: `src/tui/widgets/price_chart.rs` (compute_sma, braille_bits, braille_dot_bits, SMA overlay in render_braille_chart, SMA legend in stats line, 9 new tests), `src/tui/views/help.rs` (SMA note in Charts section), `docs/README.md` (SMA feature bullet + rendering docs)
- Tests: added 9 tests — compute_sma_basic, compute_sma_period_1, compute_sma_period_zero, compute_sma_empty_input, compute_sma_period_larger_than_data, braille_dot_bits_single_dot, braille_dot_bits_no_dot_outside_row, braille_dot_bits_both_columns, braille_dot_bits_none_is_empty. Total: 77 tests passing.
- TODO: Add moving average overlays (P1)

### 2026-03-01 — Add volume bars below price charts

- What: added volume data to the price history pipeline and rendered volume bars below braille price charts. Added `volume: Option<u64>` to `HistoryRecord`. DB migration adds `volume` column to `price_history` table. Yahoo Finance history now captures volume from OHLCV data. CoinGecko history now parses `total_volumes` from market_chart endpoint. Volume bars render as a single row of block characters (▁▂▃▄▅▆▇█) between the braille chart and the stats line, using muted theme-aware coloring (60/40 blend of text_muted and surface). Volume is shown only on single-symbol charts (not ratio or "All" multi-panel views, where volume is not meaningful). DB upsert uses COALESCE to preserve existing volume when new data has None.
- Why: volume is one of the most important technical indicators — high volume on a price move confirms the move, low volume suggests weakness. Without volume display, charts were missing critical context. Yahoo already returns volume data; it just was not being captured or displayed.
- Files: `src/models/price.rs` (volume field), `src/db/schema.rs` (migration), `src/db/price_history.rs` (store/load volume), `src/price/yahoo.rs` (parse volume), `src/price/coingecko.rs` (parse total_volumes), `src/tui/widgets/price_chart.rs` (volume bar rendering, muted_color helper, build_volume_line)
- Tests: added 8 tests — volume_blocks_levels, build_volume_line_all_zero, build_volume_line_scaling, build_volume_line_resamples, compute_ratio_has_no_volume, muted_color_blends, muted_color_non_rgb_passthrough, upsert_preserves_volume_when_null. Total: 68 tests passing.
- TODO: Add volume bars below price chart (P1)
## Log

### 2026-03-01 — Add equity, fund, crypto, and commodity chart ratio variants

- What: expanded chart variants for equities, funds, non-BTC crypto, and non-gold commodities. Equities and funds now get All + {SYM}/USD + {SYM}/SPX + {SYM}/QQQ (4 variants, cyclable with J/K). Non-BTC crypto gets All + {SYM}/USD + {SYM}/BTC + {SYM}/SPX. Non-gold commodities get All + {SYM}/USD + {SYM}/SPX + {SYM}/QQQ. Smart deduplication: SPY/VOO skip the SPX ratio (would be ~1.0), QQQ/TQQQ skip the QQQ ratio. Forex retains single chart (no meaningful index ratio). Comparison symbols (^GSPC, QQQ, BTC-USD) are pre-fetched at startup via existing batch fetch infrastructure.
- Why: equities and other non-special assets only had a single price chart with no way to compare performance against benchmarks. Ratio charts (e.g., AAPL/SPX) show whether a stock is outperforming or underperforming the market — essential for portfolio analysis. This brings feature parity with BTC and Gold which already had rich variant sets.
- Files: `src/app.rs` (chart_variants_for_position else-branch rewrite, 4 new tests + 2 updated tests), `docs/README.md` (variants by asset table)
- Tests: updated `test_regular_equity_has_ratio_variants`, `test_crypto_non_btc_has_ratio_variants`. Added `test_spy_skips_spx_ratio`, `test_qqq_skips_qqq_ratio`, `test_fund_has_ratio_variants`. Total: 60 tests passing.
- TODO: Add equity chart variants (P1)


### 2026-03-01 — Add chart timeframe selection (1W–5Y)

- What: added `ChartTimeframe` enum with 6 timeframes (1W, 1M, 3M, 6M, 1Y, 5Y). Default is 3M (preserving existing behavior). When a chart detail panel is open, `h` cycles to shorter timeframe, `l` cycles to longer (vim left/right convention). Timeframe label shown in chart title bar. Chart navigation hint updated to show "h/l" alongside "J/K". All chart render functions (`render_single_chart`, `render_ratio_chart`, `render_single_mini`, `render_ratio_mini`) now slice history data to the selected timeframe via `slice_history()` helper. Cache loads up to 5Y of data so timeframe switching is instant for cached data; new data is fetched on demand when switching to a longer timeframe. Help overlay updated with `h / l` keybinding.
- Why: charts were hardcoded to 90 days with no way to zoom in/out. Timeframe selection is essential for analyzing different market periods — 1W for recent price action, 1Y/5Y for long-term trends.
- Files: `src/app.rs` (ChartTimeframe enum, chart_timeframe field, h/l keybindings, refetch_chart_history method, 8 tests), `src/tui/widgets/price_chart.rs` (slice_history helper, timeframe-aware rendering in all 4 render functions, dynamic title), `src/tui/views/help.rs` (h/l keybinding entry), `docs/README.md` (keybinding table, chart docs), `TODO.md`
- Tests: added 8 tests — timeframe days values, labels, next/prev cycling (wrap-around), default is 3M, l cycles forward when detail open, h cycles backward when detail open, h/l no effect when detail closed. Total: 57 tests passing.
- TODO: Add timeframe selection to charts (P1)


### 2026-03-01 — Improve help overlay with grouped sections and scroll support

- What: restructured the help overlay into 5 logically grouped sections (Navigation, Views, Charts, Sorting, Actions) with visual section headers and separator lines. Added scroll support — j/k, gg/G, Ctrl+d/Ctrl+u all work when help is open. Title bar shows scroll percentage when content overflows. Footer hint tells users how to scroll/close. Extracted `build_help_lines()` as a public function for testability. Changed `ui::render` to accept `&mut App` so the help renderer can clamp scroll bounds.
- Why: the old help overlay was a flat unsorted list of keybindings with no grouping, no scrollability, and no visual hierarchy. On small terminals, keybindings at the bottom were cut off with no way to see them. The new version is organized, scannable, and fully navigable.
- Files: `src/tui/views/help.rs` (full rewrite with sections, scroll, tests), `src/app.rs` (help_scroll field, scroll key handling in help mode), `src/tui/ui.rs` (render signature `&App` → `&mut App`), `TODO.md`
- Tests: added 4 tests — sections present, vim motions present, scroll hint in footer, help_scroll defaults to zero. Total: 49 tests passing.
- TODO: Improve help overlay (P1)


### 2026-03-01 — Add / search filter for positions and transactions

- What: implemented vim-style `/` search mode. Pressing `/` enters search mode with a text input in the status bar, typing filters positions and transactions by symbol or name substring (case-insensitive). `Enter` confirms the filter (stays active after exiting search mode), `Esc` clears search and exits, `Backspace` removes characters. All normal keybindings are blocked while search mode is active (can't accidentally quit by typing 'q'). Status bar shows `[/]Search` hint and an active filter indicator when a search is confirmed. Help overlay updated with `/` keybinding.
- Why: `/` is the standard vim search key. Essential for navigating portfolios with many positions — lets users quickly find specific assets by typing part of the symbol or name instead of scrolling through the entire list.
- Files: `src/app.rs` (search_mode, search_query fields, key handling, apply_filter_and_sort integration, 9 tests), `src/tui/widgets/status_bar.rs` (search input rendering, filter indicator, [/]Search hint), `src/tui/views/help.rs` (/ keybinding entry)
- Tests: added 9 tests — slash enters search mode, filters by symbol, filters by name (case-insensitive), Esc clears and exits, Enter confirms filter, backspace removes char, no match shows empty, resets selection index, blocks normal keys (q doesn't quit). Total: 45 tests passing.
- TODO: Add / search filter (P1)


### 2026-03-01 — Add Ctrl+d/Ctrl+u half-page scroll

- What: implemented vim-standard `Ctrl+d` (scroll down half page) and `Ctrl+u` (scroll up half page) motions. Added `terminal_height` field to App, set from `crossterm::terminal::size()` on startup and updated on terminal resize events. Half-page step computed as `(terminal_height - 4) / 2` (subtracting header and status bar rows), minimum 1. Works in both Positions and Transactions views with bounds clamping. Also marked "Add Esc to close detail panel" as already implemented (was done in prior gg/G commit).
- Why: Ctrl+d/Ctrl+u are essential vim navigation motions for quickly moving through long lists without holding j/k. Completes the core vim motion set (j/k, gg/G, Ctrl+d/Ctrl+u).
- Files: `src/app.rs` (terminal_height field, half_page method, scroll_down_half_page/scroll_up_half_page methods, Ctrl+d/Ctrl+u keybindings, 5 new tests), `src/tui/mod.rs` (set initial height, update on resize), `src/tui/views/help.rs` (Ctrl+d/Ctrl+u entries), `docs/README.md` (keybinding table), `TODO.md`
- Tests: added 5 tests — ctrl_d scrolls down, ctrl_u scrolls up, empty list safety, small terminal, transactions view. Total: 36 tests passing.
- TODO: Add Ctrl+d / Ctrl+u half-page scroll (P1), Add Esc to close detail panel (P1, already done)

### 2026-03-01 — Concurrent history fetching with FetchHistoryBatch

- What: added `FetchHistoryBatch` command variant that uses `tokio::JoinSet` to fetch all price history concurrently. Extracted shared `fetch_history_single()` helper used by both single and batch code paths. Changed `request_all_history()` in `app.rs` to collect all symbols into a Vec and send a single `FetchHistoryBatch` command instead of N individual `FetchHistory` commands.
- Why: previously, startup chart loading sent individual `FetchHistory` commands processed sequentially — a portfolio with 10 symbols + 5 comparison indices meant 15 sequential HTTP round-trips. Now all 15 fetch concurrently via `JoinSet`, reducing wall-clock time from O(n × latency) to O(latency).
- Files: `src/price/mod.rs` (FetchHistoryBatch variant, fetch_history_single helper, fetch_history_batch method, new test), `src/app.rs` (request_all_history batch collection)
- Tests: added `fetch_history_batch_command_variant_exists` test. Total: 31 tests passing.
- TODO: Fix sequential history fetching (P0)



### 2026-03-02 — Add `pftui value` CLI command

- What: implemented `pftui value` (aliased in CLI as `value`). Outputs a single compact line with total portfolio value, gain/loss amount and percentage, plus a category allocation breakdown. Supports both full mode (transaction-based with cost basis) and percentage mode (allocation-based). Numbers are comma-formatted for readability. Respects `base_currency` config setting for non-USD portfolios. Warns when positions are missing cached prices.
- Why: #1 feedback request from testers — headless CLI access to portfolio value without launching the TUI. Enables integration with daily briefings, scripts, and agent workflows. Example output: `Portfolio: $368,613.28 (+4,513.53 / +1.2%)` with `Cash 49%, Commodity 33%, Crypto 18%` breakdown.
- Files: new `src/commands/value.rs` (run, run_full, run_percentage, format_with_commas, 10 tests), `src/commands/mod.rs`, `src/cli.rs` (Value variant), `src/main.rs` (dispatch)
- Tests: added 10 tests — format_with_commas (6 variants: basic, small, large, negative, zero, no-decimals), value_empty_db, value_with_positions_no_prices, value_with_positions_and_prices, value_percentage_mode_no_prices. Total: 293 tests passing.
- TODO: Add `pftui value` / `pftui worth` command (P0)


### 2026-03-02 — Add `--period` flag to `pftui summary`

- What: implemented `--period` flag for the `summary` command, supporting 5 periods: `today` (alias `1d`), `1w`, `1m`, `3m`, `1y`. Computes P&L by comparing current cached prices against historical prices at the start of the chosen period. Works in all output modes: default table, `--group-by category`, and percentage mode. Added `get_price_at_date` and `get_prices_at_date` helper functions to the `price_history` DB module that find the closest price on or before a given date. Also updated `docs/README.md` to document the new `summary --period` flag, and added missing `refresh` and `value` commands to the CLI usage section.
- Why: #2 feedback priority from testers — both requested daily/weekly/monthly P&L instead of only total gain from cost basis. Critical for daily briefings and agent-driven monitoring routines. Example: `pftui summary --period 1w` shows per-position weekly change%, or `pftui summary --group-by category --period 1m` shows monthly P&L by category.
- Files: `src/cli.rs` (SummaryPeriod enum with days_back/label methods, --period arg on Summary), `src/commands/summary.rs` (6 new render functions for period variants, CategoryPctPeriodGroup/SymbolPriceData structs, 4 new tests), `src/db/price_history.rs` (get_price_at_date, get_prices_at_date, 5 new tests), `src/main.rs` (pass period arg), `docs/README.md` (usage examples)
- Tests: added 9 tests — get_price_at_date_exact, get_price_at_date_falls_back, get_price_at_date_no_data, get_prices_at_date_multiple, period_days_back, period_label, summary_with_period_no_history, summary_with_period_and_history, summary_with_period_and_group_by. Total: 302 tests passing.
- TODO: Add `--period` flag to `pftui summary` (P0)

### 2026-03-02 — Round CSV export decimals, add --notes to list-tx

- What: (1) CSV export now rounds all decimal fields (allocation_pct, gain_pct, gain, current_value, current_price, avg_cost, total_cost) to 2 decimal places using `Decimal::round_dp(2)`. Added `round2()` helper function with 5 unit tests. Both full and percentage mode CSV exports are fixed. (2) Added `--notes` flag to `list-tx` command. When passed, an extra "Notes" column is displayed showing transaction notes (previously stored but never shown in CLI output).
- Why: (1) Feedback bug — CSV export showed up to 27 decimal places on allocation percentages, making output unreadable and breaking downstream parsing. (2) Feedback request — transaction notes are stored in the DB but `list-tx` never displayed them, making the notes field effectively useless from the CLI.
- Files: `src/commands/export.rs` (round2 helper, rounded all CSV decimal fields, 5 new tests), `src/commands/list_tx.rs` (accept show_notes bool, conditional Notes column rendering), `src/cli.rs` (ListTx now has `--notes` flag), `src/main.rs` (pass notes arg through)
- Tests: added 5 tests — round2_basic, round2_rounds_up, round2_whole_number, round2_small, round2_negative. Total: 307 tests passing.
- TODO: Round CSV export allocation percentages (P1), Add --notes flag to list-tx (P1)

### 2026-03-02 — Add `pftui brief` markdown summary command

- What: implemented `pftui brief` — outputs a complete markdown-formatted portfolio brief designed for agent consumption, daily reports, and script integration. The output includes: date-stamped header, total portfolio value with gain/loss, category allocation breakdown (e.g. "**Cash** 49% · **Commodity** 33% · **Crypto** 18%"), top movers section with 1-day price changes sorted by absolute movement (capped at 5, with 📈/📉 indicators and resolved asset names), and a full positions markdown table with symbol, category, qty, price, value, gain%, and allocation%. Supports both full mode (transaction-based with cost basis) and percentage mode (allocation-based). Respects `base_currency` config setting. Non-USD currencies use suffix format (e.g. "1,234.56 GBP"). Warns when positions are missing cached prices.
- Why: P1 feedback request — testers and agents need a structured, parseable portfolio overview without launching the TUI. Markdown format renders natively in Telegram, GitHub, and most agent pipelines. Enables `pftui refresh && pftui brief` as a one-liner for daily briefings.
- Files: new `src/commands/brief.rs` (run, run_full, run_percentage, fmt_commas, fmt_currency, pct_change, print_category_allocation, print_category_allocation_pct, print_top_movers, print_position_table_full, format_category, 16 tests), `src/commands/mod.rs`, `src/cli.rs` (Brief variant), `src/main.rs` (dispatch), `docs/README.md` (CLI usage)
- Tests: added 16 tests — fmt_commas (4: basic, small, negative, zero), fmt_currency (2: usd, gbp), pct_change (3: positive, negative, zero_base), brief_empty_db, brief_with_positions_no_prices, brief_with_positions_and_prices, brief_percentage_mode, brief_percentage_mode_no_prices, top_movers_sorts_by_absolute_change, category_allocation_groups_correctly. Total: 323 tests passing.
- TODO: Add `pftui brief` markdown summary command (P1)

### 2026-03-02 — Upgrade asset search to ranked fuzzy matching

- What: replaced prefix-only `search_names()` with a 5-tier ranked fuzzy scoring system. Match tiers: exact (100), prefix (80), word-start (65), substring (50), subsequence (10-30). Results sorted by match quality then alphabetically. The setup wizard's `resolve_symbol()` now benefits from fuzzy finding — typing "depot" finds Home Depot, "old" finds Gold/GLD, "slna" finds Solana via subsequence matching.
- Why: P0 setup wizard fuzzy finder — the symbol entry flow previously only matched prefix, missing relevant results. Fuzzy matching makes the setup wizard significantly more usable for discovering assets by partial name, abbreviation, or substring.
- Files: `src/models/asset_names.rs` (new `fuzzy_score()` function, rewritten `search_names()`, 14 new tests)
- Tests: added 14 tests — fuzzy_score_exact, fuzzy_score_prefix, fuzzy_score_word_start, fuzzy_score_substring, fuzzy_score_subsequence, fuzzy_score_no_match, search_names_substring_match, search_names_word_start_match, search_names_subsequence_match, search_names_empty_query, search_names_ranking_exact_over_prefix, search_names_ranking_prefix_over_substring. Total: 335 tests passing.
- TODO: Setup wizard fuzzy finder (P0) — core matching done; inline-as-you-type display is a future enhancement

### 2026-03-02 — Add keystroke echo in status bar

- What: added keystroke echo that briefly flashes the last pressed key in the status bar for ~0.3s (18 ticks at 60fps). Displays formatted key names: regular keys ("j", "k"), sequences ("gg"), modifier keys ("Ctrl+d"), and special keys ("Enter", "↑", "↓"). Fades from `text_secondary` to `text_muted` color over the display period. Only echoes keys outside of search mode (search input is already visible).
- Why: P2 micro-interaction — helps users learn keybindings and confirms input was received. Especially useful for multi-key sequences like "gg" where there's no immediate visual feedback that the first key registered.
- Files: `src/app.rs` (new `record_keystroke()` method, `last_key_display`/`last_key_tick` fields, call in `handle_key`, 8 new tests), `src/tui/widgets/status_bar.rs` (render keystroke echo with fade)
- Tests: added 8 tests — test_record_regular_key, test_record_ctrl_key, test_record_shift_g, test_record_gg_sequence, test_record_enter_key, test_record_esc_key, test_record_arrow_keys, test_key_echo_text_generation. Total: 343 tests passing.
- TODO: Keystroke echo in status bar (P2)

### 2026-03-02 — Add breadcrumb trail to status bar

- What: added a context-aware breadcrumb trail at the left side of the status bar. Shows the current navigation path based on view mode, selected position, chart variant, and detail popup state. Examples: "Positions › AAPL", "Positions › AAPL › 3M › AAPL/SPX", "Positions › BTC › Detail", "Markets", "Economy". Breadcrumb is styled with `text_accent` color in bold, separated from key hints by a `│` divider in `border_subtle` color.
- Why: P1 status bar enhancement — the generic key hint bar gave no context about where you are in the app. The breadcrumb instantly communicates the current view, selected asset, active chart variant, and timeframe without requiring the user to look at multiple parts of the screen.
- Files: `src/app.rs` (new `breadcrumb()` method with 10 tests), `src/tui/widgets/status_bar.rs` (render breadcrumb before key hints)
- Tests: added 10 tests — test_breadcrumb_positions_no_selection, test_breadcrumb_positions_with_selection, test_breadcrumb_detail_popup, test_breadcrumb_chart_variant, test_breadcrumb_transactions_view, test_breadcrumb_markets_view, test_breadcrumb_economy_view, test_breadcrumb_watchlist_view, test_breadcrumb_detail_overrides_chart, test_breadcrumb_chart_timeframe_label. Total: 353 tests passing.
- TODO: Add breadcrumb trail to status bar (P1)

### 2026-03-02 — Add pulsing border on active panel

- What: active panel borders now gently pulse with a 2-second sine wave when price data is live. The focused panel's border breathes between `border_active` and `border_inactive` colors using the existing `pulse_color()` utility. When prices are stale or the panel is inactive, borders remain static. Applies to positions table, price chart, and asset header panels. Added `PULSE_PERIOD_BORDER` constant (120 ticks = 2s at 60fps) separate from the existing live-dot pulse period. Extracted `positions_border_color()` helper function for clean testability.
- Why: P1 animation — gives the app a subtle "alive" breathing feel when connected to live price data. The static border when prices are stale provides an additional ambient indicator of data freshness beyond the live dot in the status bar.
- Files: `src/tui/theme.rs` (new `PULSE_PERIOD_BORDER` constant, 2 tests), `src/tui/views/positions.rs` (extracted `positions_border_color()` helper, pulse logic, 3 tests), `src/tui/widgets/price_chart.rs` (chart border pulse), `src/tui/widgets/asset_header.rs` (header border pulse)
- Tests: added 5 tests — test_positions_border_pulse_when_active_and_live, test_positions_border_static_when_active_and_stale, test_positions_border_inactive_when_not_active, pulse_intensity_border_period_range, pulse_border_period_produces_variation. Total: 358 tests passing.
- TODO: Add pulsing border on active panel (P1)

### 2026-03-02 — Add row highlight flash on selection change

- What: when j/k moves the selection in the positions table, the newly selected row briefly flashes from `border_accent` color and smoothly fades back to `surface_3` over ~0.25s (15 ticks at 60fps). The flash uses `lerp_color()` for smooth decay. Non-selected rows are completely unaffected. Flash is suppressed on initial state (tick 0) to prevent a flash when the app first opens.
- Why: P1 animation — gives immediate, satisfying visual feedback when navigating the positions list. The brief color flash draws the eye to the new selection, making navigation feel responsive and polished. Complements the existing pulsing border and keystroke echo animations.
- Files: `src/app.rs` (new `last_selection_change_tick` field, set in `on_position_selection_changed`), `src/tui/theme.rs` (new `SELECTION_FLASH_DURATION` constant), `src/tui/views/positions.rs` (new `row_background()` helper, 6 tests)
- Tests: added 6 tests — test_flash_at_start_returns_accent_color, test_flash_decays_to_surface_3, test_flash_midpoint_is_between_accent_and_surface, test_non_selected_rows_unaffected_by_flash, test_no_flash_on_initial_state, test_flash_well_past_duration. Total: 364 tests passing.
- TODO: Add row highlight animation on selection change (P1)

### 2026-03-02 — Add color-coded category dot before asset name

- What: replaced cell-level category coloring with a discrete colored dot (●) before each asset name in the positions table. The dot renders in the asset's category color (equity, crypto, commodity, forex, fund, cash) between the selection marker (▎) and the asset name. Asset text now uses `text_primary` instead of the category color for better readability. The dot provides instant visual category scanning — much more scannable than coloring the entire row text.
- Why: P1 visual density — makes categories immediately identifiable at a glance without sacrificing text readability. Each category has its own distinct color from the theme, so users can instantly spot crypto vs commodity vs equity positions.
- Files: `src/tui/views/positions.rs` (asset_line construction in both full and privacy tables, removed cell-level cat_color styling, 3 new tests)
- Tests: added 3 tests — test_category_dot_uses_category_color, test_category_dot_is_single_char, test_asset_line_structure_with_dot. Total: 367 tests passing.
- TODO: Add color-coded category dot before asset name (P1)

### 2026-03-02 — Add inline mini-sparkline in price column

- What: added a 3-char sparkline (▁▃▇) directly after the price number in the Price column, using the last 3 history data points. Reuses the existing `build_sparkline_spans()` with `count=3`. Gives instant trend context without needing the separate Trend column. Price column widened from 12 to 16 chars. During a price flash (▲/▼), the mini sparkline still appears after the flash indicator. When no history is available, the price renders without a sparkline (graceful degradation).
- Why: P1 visual density — the price column was just a number. Now it immediately communicates direction with a tiny trend indicator right next to the price, making the positions table more information-dense at a glance.
- Files: `src/tui/views/positions.rs` (mini sparkline construction in full table, price cell spans, column width, 5 new tests)
- Tests: added 5 tests — test_mini_sparkline_three_points, test_mini_sparkline_uses_last_three_of_many, test_mini_sparkline_fewer_than_three_records, test_mini_sparkline_empty_history, test_mini_sparkline_flat_prices. Total: 372 tests passing.
- TODO: Add inline mini-sparkline in price column (P1)

### 2026-03-02 — Add theme toast notification on cycle

- What: when pressing `t` to cycle themes, a brief toast now appears in the status bar showing "◆ Midnight" (or the new theme name) in the theme's accent color. The toast lasts ~1.5s (90 ticks at 60fps) with a smooth two-phase fade: full accent brightness for the first half, then gradually fading to muted via `lerp_color()`. Toast is suppressed on initial state (tick 0) to prevent flashing on app launch. Added `capitalize_first()` helper for display-friendly theme names.
- Why: P2 theme enhancement — previously pressing `t` cycled the theme with no feedback about which theme you landed on. Now there's a clear, stylish confirmation that fades naturally.
- Files: `src/tui/theme.rs` (new `THEME_TOAST_DURATION` constant), `src/app.rs` (new `theme_toast_tick` field, set in `cycle_theme()`), `src/tui/widgets/status_bar.rs` (toast rendering with fade logic, `capitalize_first()` helper, 7 tests)
- Tests: added 7 tests — test_capitalize_first_basic, test_capitalize_first_empty, test_capitalize_first_already_capitalized, test_capitalize_first_single_char, test_theme_toast_timing, test_theme_toast_not_shown_on_init, test_theme_toast_fade_phases. Total: 393 tests passing.
- TODO: Add theme preview on cycle (P2)

### 2026-03-02 — Fix chart ratio variants for USD, GBP, and all commodities

- What: fixed three chart ratio bugs reported by owner. (1) USD position showed a BTC/USD single chart — replaced with USD/BTC ratio (DXY divided by BTC-USD), correctly showing the dollar's value in BTC terms. (2) Non-USD cash positions (GBP, EUR, etc.) showed Gold and BTC as standalone single charts — replaced with proper {CURRENCY}/Gold and {CURRENCY}/BTC ratio charts (e.g. GBP/Gold = GBPUSD=X / GC=F). (3) Silver and other non-Gold commodities (SLV, COPX, URA, USO) were missing a /BTC ratio variant — Gold had Gold/BTC but no other commodity did. All commodities now get {SYM}/BTC alongside their existing /SPX and /QQQ ratios.
- Why: P0 bug fix — chart ratios should always show SELECTED_ASSET/BENCHMARK. The old code was showing inverted or raw benchmark charts instead of computing the actual ratio, making the data misleading.
- Files: `src/app.rs` (chart_variants_for_position: USD branch, non-USD cash branch, commodity branch in else block)
- Tests: added 2 new tests (test_commodity_non_gold_has_btc_ratio, test_equity_has_no_btc_ratio), updated 3 existing tests (test_usd_cash_variants, test_non_usd_cash_variants_ratio_dxy, test_gbp_cash_variants). Total: 395 tests passing.
- TODO: Fix USD chart ratio (P0), Fix GBP chart ratio (P0), Fix silver/commodities missing BTC ratio (P0)

### 2026-03-02 — Fix Sprott Uranium (U.UN) chart and improve empty state handling

- What: fixed U.UN showing no chart data by adding Yahoo Finance symbol normalization for Toronto Stock Exchange trust units. TSX tickers with `.UN` suffix (e.g. `U.UN`) are now converted to Yahoo's format (`U-UN.TO`). Also added proper empty state handling: charts now distinguish between "still loading" and "fetch attempted but no data available." Previously, failed fetches showed "Loading..." indefinitely; now they show "No chart data available for {SYM}" (or "No data" in mini panels).
- Why: P0 bug fix — U.UN is Sprott Physical Uranium Trust on the TSX. Yahoo Finance doesn't recognize the `.UN` dot notation; it uses `{PREFIX}-UN.TO` format. Without normalization, both price and history fetches silently failed, leaving the chart in a permanent loading state.
- Files: `src/price/yahoo.rs` (new `normalize_yahoo_symbol()` function, applied in `fetch_price` and `fetch_history`), `src/app.rs` (new `history_attempted: HashSet<String>` field, populated on batch send and individual fetch), `src/tui/widgets/price_chart.rs` (updated all 6 chart render functions to show "No chart data" vs "Loading..." based on `history_attempted`)
- Tests: added 5 new tests (test_normalize_tsx_trust_unit, test_normalize_tsx_trust_unit_multi_char_prefix, test_normalize_regular_symbol_unchanged, test_normalize_already_to_suffix_unchanged, test_normalize_preserves_original_for_non_tsx). Total: 400 tests passing.
- TODO: Fix Sprott Uranium (U.UN) chart — no data (P0)

### 2026-03-02 — Enhance portfolio chart with multi-timeframe gains and larger layout

- What: rewrote the portfolio sparkline panel to show multi-timeframe gain/loss indicators (1D, 1W, 1M, 3M) below the braille chart. Each timeframe shows a colored arrow (▲/▼), absolute change (e.g. $+1.5k), and percentage change. Gains auto-wrap to two rows on narrow terminals. The panel title now dynamically shows current portfolio value (e.g. "Portfolio  $125.3k"). Increased minimum sparkline panel height from 6→10 rows and overall overview panel minimum from 10→14, giving the chart significantly more vertical space to render a meaningful braille graph.
- Why: P0 fix — the portfolio chart was "broken" per owner report: too small to render meaningful data (often just 2-3 braille rows), and showed only a single total change percentage with no timeframe context. Now the chart gets proper vertical space and immediately communicates portfolio performance across multiple periods, matching the request for "hourly, daily, weekly, monthly, yearly" gains.
- Files: `src/tui/widgets/portfolio_sparkline.rs` (full rewrite: multi-timeframe gains, dynamic title, `compute_timeframe_gains()`, `build_gain_lines()`, `format_compact_change()`, 16 tests), `src/tui/widgets/sidebar.rs` (increased sparkline min height), `src/tui/ui.rs` (increased MIN_OVERVIEW_HEIGHT and sparkline allocation)
- Tests: added 16 tests (timeframe gain computation for empty/short/1d/multi/negative, compact formatting for change/value/short, braille char, resample). Total: 416 tests passing.
- TODO: Fix main portfolio chart (P0)

### 2026-03-02 — Add inline transaction form and delete confirmation from TUI

- What: added two new keybindings for managing transactions directly from the TUI positions view. `A` (Shift+a) opens an inline add-transaction form in the status bar area, pre-filled with the selected position's symbol and today's date. The form has 4 fields: Type (Buy/Sell toggle), Quantity, Price per unit, and Date (YYYY-MM-DD). Tab/Enter advances between fields, Esc cancels, and Enter on the Date field submits. Validates that quantity > 0, price > 0, and date matches YYYY-MM-DD format. `X` (Shift+x) shows a delete confirmation prompt for all transactions of the selected position, with `y` to confirm and any other key to cancel. Both keybindings only appear in Positions view with Full portfolio mode.
- Why: P0 feature — previously the only way to add or remove transactions was editing the config file or using the CLI. This was the biggest UX gap in the TUI, making portfolio management cumbersome. Now users can add buys/sells and remove positions without leaving the TUI.
- Files: `src/app.rs` (TxFormField enum, TxFormState struct, DeleteConfirmState struct, form/delete handlers, keybindings, 20 tests), `src/tui/widgets/status_bar.rs` (form + delete confirmation rendering), `src/tui/views/help.rs` (updated help with A/X keybindings)
- Tests: added 20 new tests (field cycling, form state defaults, open/cancel/advance, type toggle, digit input, backspace, form-eats-keys, validation errors, delete confirm cancel, mode guards). Total: 436 tests passing.
- TODO: Add easy position modification (P0)

### 2026-03-02 — Add `pftui watchlist` CLI command

- What: added a new `pftui watchlist` CLI subcommand that displays all watched symbols with their current cached prices in a formatted table. Output includes symbol, resolved name, category, price (with comma-separated thousands and adaptive decimal places), and a relative timestamp showing when the price was last fetched (e.g. "3h ago", "2d ago"). Symbols are sorted alphabetically. Shows "N/A" for symbols without cached prices and suggests running `pftui refresh`.
- Why: P1 feedback item — the TUI `watch` command existed but there was no CLI equivalent. Headless/agent workflows need to query watchlist prices without launching the TUI.
- Files: new `src/commands/watchlist_cli.rs`, `src/commands/mod.rs`, `src/cli.rs` (Watchlist variant), `src/main.rs` (dispatch)
- Tests: added 13 new tests (price formatting for large/medium/small/zero/very-large values, relative timestamp formatting for recent/minutes/hours/days/invalid, empty DB, entries without prices, entries with prices). Total: 449 tests passing.
- TODO: Add `pftui watchlist` CLI command (P1)

### 2026-03-02 — Enhance export to full database snapshot with --output flag

- What: enhanced the `pftui export` command. JSON export now produces a complete portfolio snapshot including config, transactions, allocations, watchlist entries, and computed positions — a full backup/restore format. CSV export now includes `name` and `currency` columns. Both formats support a new `--output <path>` (`-o`) flag to write to a file instead of stdout. Added structured types (FullSnapshot, ConfigExport, WatchlistExport) for clean JSON serialization.
- Why: P1 feature (Import/Export, Owner Request) — the existing export only dumped computed position data. A full snapshot enables backup, migration, and the upcoming `pftui import` command to restore from it. File output support makes it practical for scripts and cron jobs.
- Files: `src/commands/export.rs` (full rewrite: snapshot types, file output, JSON/CSV), `src/cli.rs` (added `--output` arg to Export), `src/main.rs` (pass output arg)
- Tests: added 6 new tests (config_export_from_config, json_snapshot_serializes, json_snapshot_with_data, export_json_full_db_snapshot, export_csv_to_file, get_writer_stdout). Total: 456 tests passing.
- TODO: Enhance `pftui export` to full database snapshot (P1)

### 2026-03-02 — Add `pftui set-cash` command

- What: added a dedicated `pftui set-cash <SYMBOL> <AMOUNT>` command for managing cash positions. Instead of the cumbersome `pftui add-tx --symbol USD --category cash --tx-type buy --quantity 45000 --price 1.00`, users can now run `pftui set-cash USD 45000`. The command replaces all existing transactions for that currency with a single buy at price 1.00. Setting amount to 0 clears the position entirely. Validates input (rejects negative amounts, handles decimals), warns on unrecognized currency codes, and is blocked in percentage mode.
- Why: P1 CLI feedback item — both testers flagged that managing cash positions via `add-tx` was unintuitive. Cash is conceptually "I have X dollars" not "I bought X dollars at $1 each". This command matches that mental model.
- Files: new `src/commands/set_cash.rs` (run, delete_all_for_symbol, KNOWN_CASH list, 10 tests), `src/commands/mod.rs`, `src/cli.rs` (SetCash variant), `src/main.rs` (dispatch + percentage mode guard)
- Tests: added 10 new tests (create position, replace existing, clear with zero, clear nonexistent, uppercase normalization, decimal amounts, negative rejection, invalid amount, multiple currencies, isolation from non-cash symbols). Total: 496 tests passing.

### 2026-03-02 — Add portfolio sparkline timeframe selector

- What: added `[`/`]` keybindings to cycle the portfolio sparkline's timeframe through 1W, 1M, 3M, 6M, 1Y, 5Y. Previously the sparkline was hardcoded to show all available history (~90 days). Now the title shows the active timeframe (e.g. "Portfolio 3M $42.5k") and the gain/loss periods below the chart automatically adapt to only show periods that fit within the selected window. Extended the gain display to include 6M and 1Y periods when enough data exists.
- Why: P2 sidebar enhancement — the hardcoded 90-day sparkline was limiting. Users viewing short-term vs long-term trends needed to zoom in/out. This reuses the existing `ChartTimeframe` enum for consistency with the per-asset chart `h`/`l` cycling.
- Files: `src/app.rs` (sparkline_timeframe field, `[`/`]` keybindings, 3 tests), `src/tui/widgets/portfolio_sparkline.rs` (timeframe filtering, dynamic period selection, title label, 2 tests), `src/tui/views/help.rs` (keybinding documentation)
- Tests: 5 new tests (default timeframe, forward cycling, backward cycling, larger periods, 1W-only periods). Total: 501 tests passing.
- TODO: Add portfolio sparkline period selector (P2)

### 2026-03-02 — Add dynamic header border tint based on portfolio performance

- What: the header's bottom border now subtly tints toward green when the portfolio is up for the day, or toward red when it's down. Uses a 15% `lerp_color` blend from `border_subtle` toward `gain_green` or `loss_red`. Falls back to the neutral `border_subtle` when there's no daily change data (e.g. no price history yet, or zero change). Works across all themes since it uses each theme's own color values.
- Why: P2 theme/color enhancement — provides an ambient mood indicator. Users can instantly sense portfolio direction from the header border without reading numbers. The blend is subtle enough (15%) to avoid clashing with any theme.
- Files: `src/tui/widgets/header.rs` (border color computation in `render()`, added `lerp_color` import, 4 new tests)
- Tests: 4 new tests (positive tint, negative tint, zero change unchanged, no data unchanged). Total: 505 tests passing.
- TODO: Add dynamic header accent based on portfolio performance (P2)

### 2026-03-02 — Add section divider lines between position groups

- What: when positions are sorted by Category (`c` keybinding), thin divider lines like `─── Crypto ───` are inserted between each category group in the positions table. Dividers appear in both full and privacy/percentage table modes. Category names are capitalized in the divider label. Uses theme's `border_subtle` color for a clean, unobtrusive look. Only shown when sort field is Category — no dividers for other sort modes.
- Why: P2 visual enhancement — makes it easier to visually scan grouped positions. When sorting by category, users can instantly see where one group ends and the next begins without reading the category dot colors.
- Files: `src/tui/views/positions.rs` (added `category_divider_row()`, `capitalize_category()`, interleaved dividers in both `render_full_table` and `render_privacy_table`, 6 new tests)
- Tests: 6 new tests (capitalize basic/empty/already-capitalized, divider row column count, all categories, privacy column count). Total: 511 tests passing.
- TODO: Add section divider lines between position groups (P2)

### 2026-03-02 — Add sort indicator flash animation

- What: when the user changes sort order via any sort keybinding (`a`, `n`, `c`, `%`, `$`, `d`, `Tab`), the sort indicator in the positions table title (e.g. `[alloc%▼]`) briefly flashes from `text_primary` to `text_accent` with bold styling, fading over ~0.5s (30 ticks at 60fps). Provides immediate visual confirmation that the sort changed. Uses `lerp_color` for smooth color transition. The `last_sort_change_tick` guard ensures the flash only triggers on actual sort actions, not on initial render.
- Why: P2 micro-interaction enhancement — confirms sort actions visually. Without this, changing sort is silent and users must scan the indicator text to verify. The flash draws the eye to the indicator and makes the action feel responsive.
- Files: `src/app.rs` (added `last_sort_change_tick` field, set on all sort key handlers), `src/tui/theme.rs` (added `SORT_FLASH_DURATION` constant), `src/tui/views/positions.rs` (flash logic in `render_table`, 4 new style tests)
- Tests: 9 new tests (5 in app.rs: starts at zero, updates on name/tab/category/allocation sort; 4 in positions.rs: bold during flash, normal after duration, color starts at primary, color ends at accent). Total: 520 tests passing.
- TODO: Add sort indicator animation (P2)

### 2026-03-02 — Add allocation change indicators (▲/▼) in sidebar

- What: the allocation bars sidebar now shows ▲/▼ arrows next to category allocation percentages when they've shifted since the previous day. For example, if Crypto was 48% yesterday and is 50% today, it shows `50% ▲2.0`. Change is computed from price history — uses the second-to-last close price per symbol to estimate yesterday's allocation weights. Indicators only appear when the shift is >= 0.1 percentage points, avoiding noise from rounding. Colored green (▲ increase) or red (▼ decrease) using theme colors.
- Why: P2 sidebar enhancement — helps identify rebalancing needs and shows how price movements are shifting portfolio allocation weights day-to-day. Previously allocation bars were static snapshots with no sense of direction.
- Files: `src/app.rs` (added `prev_day_cat_allocations` field, `compute_prev_day_cat_allocations()` method called from `recompute()`, 5 new tests), `src/tui/widgets/allocation_bars.rs` (added `allocation_change()` and `allocation_change_span()` functions, integrated into render loop)
- Tests: 5 new tests (no history → empty, second-to-last close usage, single record insufficient, cash priced at 1.0, multi-category aggregation). Total: 525 tests passing.
- TODO: Add allocation change indicators (P2)

### 2026-03-03 — Add configurable base currency with symbol display

- What: the setup wizard now asks users to choose their base currency (from 20 supported currencies) before selecting portfolio mode. All currency display throughout the app uses the configured currency symbol instead of hardcoded `$`. Added `currency_symbol()` mapping for 25+ currencies (USD→$, EUR→€, GBP→£, JPY→¥, CAD→C$, AUD→A$, etc.). Unknown currencies fall back to their 3-letter code as prefix. Existing installations default to USD/$, fully backward compatible.
- Why: P0 setup enhancement — users with non-USD portfolios previously saw `$` everywhere, which was misleading. A UK user tracking GBP positions now sees `£` in the header, sparkline, allocation bars, and all CLI commands. This was the second P0 setup wizard improvement, addressing the "configurable primary fiat currency" TODO item.
- Files: `src/config.rs` (added `currency_symbol()` free function, `Config::currency_symbol()` method, `SUPPORTED_CURRENCIES` constant), `src/commands/setup.rs` (currency selection step with 2-column display), `src/tui/widgets/header.rs` (format_compact/format_compact_signed take sym param), `src/tui/widgets/portfolio_sparkline.rs` (format_compact_value/format_compact_change take sym param), `src/tui/widgets/allocation_bars.rs` (Total line uses currency symbol), `src/commands/value.rs` (uses currency_symbol), `src/commands/refresh.rs` (format_price takes sym param), `src/commands/brief.rs` (simplified fmt_currency using currency_symbol), `src/commands/watchlist_cli.rs` (accepts config, uses currency symbol), `src/main.rs` (passes config to watchlist)
- Tests: 11 new tests (currency_symbol known/unknown, Config method, SUPPORTED_CURRENCIES contains major, format_compact euro, format_compact_signed gbp, format_price euro, format_compact_value euro, format_compact_change gbp, fmt_currency eur/unknown). Total: 536 tests passing.
- TODO: Setup wizard: configurable primary fiat currency (P0)
