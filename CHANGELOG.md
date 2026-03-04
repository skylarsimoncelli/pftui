# Changelog

> Reverse chronological. Each entry: date, summary, files changed, tests.
> Automated runs append here after completing TODO items.

### 2026-03-04 — Drift and rebalance CLI commands (F6.4 continued)

- What: Two new CLI commands complete F6.4 CLI layer. `pftui drift [--json]` shows allocation drift vs targets: target %, actual %, drift %, drift band, and status (✓ in range / ⚠️ out of band). Sorted by absolute drift descending. `pftui rebalance [--json]` suggests buy/sell trades to bring out-of-band positions back to targets: current value, target value, diff, action (BUY/SELL). Both read allocation targets from DB, compute positions with current prices, support JSON.
- Why: Completes CLI layer for allocation management. Enables agents to query drift status and get rebalance suggestions programmatically. Next step: TUI integration in positions view to show target/actual/drift columns.
- Files: new `src/commands/drift.rs`, new `src/commands/rebalance.rs`, `src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`
- Tests: 973 passing (no new tests; commands are thin wrappers over DB + positions logic), clippy clean
- TODO: F6.4 partial (DB + CLI done; next: TUI positions view drift columns)

### 2026-03-04 — Allocation target storage and CLI (F6.4 foundation)

- What: New `allocation_targets` DB table and `pftui target` CLI command suite. `pftui target set GC=F --target 25% --band 3%` stores target allocation percentage and drift band. `pftui target list [--json]` shows all targets. `pftui target remove SYMBOL` deletes. Default drift band is 2%. Validates target 0-100%, band 0-50%.
- Why: Foundation for F6.4 (allocation target + drift in Positions tab). Enables setting portfolio allocation targets and drift tolerance bands, which will be used to compute drift, show target vs actual columns in TUI, and suggest rebalance trades.
- Files: new `src/db/allocation_targets.rs` (CRUD), `src/db/schema.rs` (allocation_targets table), `src/commands/target.rs` (CLI), `src/cli.rs`, `src/main.rs`, `src/db/mod.rs`, `src/commands/mod.rs`
- Tests: 973 passing (+4 new: set_target, update_target, list_targets, remove_target), clippy clean
- TODO: F6.4 partial (storage + CLI done, next: drift calculation, positions view update, rebalance suggestions)

### 2026-03-04 — `pftui movers` command

- What: New `pftui movers` command that scans all held positions + watchlist symbols, computes daily change % from cached price history, and shows those exceeding a threshold (default 3%). Sorted by absolute change descending. `--threshold 5` for custom threshold, `--json` for agent output. Deduplicates symbols in both held and watchlist, skips cash.
- Why: Replaces manual scanning of 40+ symbols. Requested by feedback testers — quick way to spot significant daily moves across the entire universe.
- Files: new `src/commands/movers.rs`, `src/cli.rs`, `src/main.rs`, `src/commands/mod.rs`
- Tests: 13 new tests (empty DB, no history, below/above threshold, custom threshold, JSON output, cash skip, negative change, dedup, helpers). Total: 969 passing, clippy clean.
- TODO: `[Feedback] pftui movers command` (P2)

### 2026-03-04 — F10.2: `pftui performance` CLI command

- What: New `pftui performance` command showing portfolio returns across standard periods (1D, 1W, 1M, MTD, QTD, YTD, since inception). `--since 2026-02-24` for custom period with best/worst day analysis. `--period weekly` for return series. `--json` for agent consumption. Uses daily snapshots from `pftui refresh`.
- Why: Completes F10.2 from the analytics spec — enables tracking portfolio returns over any period without manual calculation.
- Files: new `src/commands/performance.rs`, `src/cli.rs`, `src/main.rs`, `src/commands/mod.rs`, `src/db/snapshots.rs` (new `get_all_portfolio_snapshots`, `get_portfolio_snapshots_since` functions)
- Tests: 12 new tests (956 total), clippy clean

### 2026-03-04 — F6.3: Watchlist entry level integration

- What: `pftui watch TSLA --target 300 --direction below` stores a target price on the watchlist entry and auto-creates an alert rule. Watchlist CLI and TUI views show Target and Proximity columns when any entry has a target. Proximity is color-coded: red (<3%), yellow (<10%), green (>10%), 🎯 HIT when reached. `pftui watchlist --approaching 10%` filters to symbols within N% of target. DB migration adds `target_price` and `target_direction` columns to watchlist table.
- Why: Connects the watchlist and alert systems — set entry levels on watched assets and get notified when they're hit, without manually creating separate alerts.
- Files: `db/schema.rs` (migration), `db/watchlist.rs` (set_watchlist_target), `cli.rs` (--target, --direction, --approaching flags), `main.rs` (watch/watchlist handler updates), `commands/watchlist_cli.rs` (target/proximity columns, --approaching filter), `tui/views/watchlist.rs` (target/proximity TUI columns with color-coded proximity bars)
- Tests: 942 passing (+2 new: set_watchlist_target, set_target_nonexistent_symbol), clippy clean

### 2026-03-04 — F10.1: Automated daily portfolio snapshots

- What: On every `pftui refresh`, compute positions from current prices and store a daily portfolio snapshot in SQLite. New `portfolio_snapshots` table (date, total_value, cash_value, invested_value) and `position_snapshots` table (date, symbol, quantity, price, value). Upserts by date so multiple refreshes per day update the same snapshot. Includes reader functions for F10.2/F10.3.
- Why: Foundation for portfolio performance tracking (F10.2 `pftui performance` CLI, F10.3 TUI panel). Also provides real daily portfolio value data to fix the 3M chart "Waiting for data" bug reported by testers.
- Files: new `src/db/snapshots.rs`, `src/db/mod.rs`, `src/db/schema.rs` (2 new tables), `src/commands/refresh.rs` (snapshot after price cache)
- Tests: 14 new tests (11 in db/snapshots, 3 in refresh integration). Total: 940 passing, clippy clean.
- TODO: F10.1 Automated daily portfolio snapshots (P1)

### 2026-03-04 — F6.2: `pftui alerts` CLI

- What: Full CLI for managing alerts: `alerts add "rule"`, `alerts list`, `alerts remove <id>`, `alerts check`, `alerts ack <id>`, `alerts rearm <id>`. Supports `--json` for agent output and `--status` filter for list. Check command shows distance-to-trigger for armed alerts, groups results by status (newly triggered, armed, acknowledged).
- Why: Enables headless alert management for agents and scripts. Completes the CLI layer of F6 unified alert system.
- Files: new `src/commands/alerts.rs`, `src/commands/mod.rs`, `src/cli.rs` (Alerts subcommand), `src/main.rs` (dispatch + removed dead_code allow on alerts mod)
- Tests: 11 new tests (928 total), clippy clean

### 2026-03-04 — F6.1: Unified alert engine + DB schema

- What: Alert rules engine supporting three alert types: price (`"GC=F above 5500"`), allocation (`"gold allocation above 30%"`), and indicator (`"GC=F RSI below 30"`). Natural language rule parser, SQLite storage with status lifecycle (armed → triggered → acknowledged), check engine that evaluates alerts against cached prices with distance-to-trigger calculation.
- Why: Foundation for the entire F6 unified alert system. All subsequent alert features (CLI, TUI badge, refresh integration) build on this data layer.
- Files: new `src/alerts/{mod,rules,engine}.rs`, new `src/db/alerts.rs`, `src/db/schema.rs` (alerts table migration), `src/db/mod.rs`, `src/main.rs`
- Tests: 39 new tests (16 parser, 12 DB CRUD, 11 engine). Total: 916 passing, clippy clean.

### 2026-03-04 — F3.4: `pftui macro` CLI command

- What: New `pftui macro` command — terminal-friendly macro dashboard. Displays yields (2Y/5Y/10Y/30Y), currencies (DXY, EUR, GBP, JPY, CNY), commodities (gold, silver, oil, copper, nat gas), VIX with regime context, FRED economic data (FFR, CPI, PPI, unemployment), and derived metrics (Au/Ag ratio, Au/Oil ratio, Cu/Au ratio, yield curve status). Key indicators strip at top for quick scanning. 1-day change arrows from price history. `--json` flag for structured agent output.
- Why: Most-requested feature across 3 of 4 testers. Eliminates dependency on external `fetch_prices.py` for macro data. Completes F3 (Macro Dashboard) feature set.
- Files: new `src/commands/macro_cmd.rs`, `src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`
- Tests: 7 new tests (empty DB terminal, empty DB JSON, seeded data terminal, seeded data JSON, fmt_commas, derived metrics, zero-denominator safety). Total: 879 passing.
- TODO: F3.4 `pftui macro` CLI command (P1)

### 2026-03-04 — F3.3: Economy tab enhancement — macro dashboard layout

- What: transformed Economy tab [4] from a flat table into a 3-panel macro intelligence dashboard. Added Key Numbers top strip (DXY, VIX, 10Y, Gold, Oil, Silver with day change at a glance). Added braille yield curve chart showing 2Y/5Y/10Y/30Y with linear interpolation and color-coded state. Added Derived Metrics panel with gold/silver ratio, 10Y-2Y spread with regime context, gold/oil ratio, copper/gold ratio, and VIX sentiment context. Added Silver Futures (SI=F) to economy symbols for cross-asset ratio calculations.
- Why: F3.3 from TODO.md — Economy tab needs to be a full macro intelligence dashboard, not just a flat indicator table. Top strip provides at-a-glance key numbers, yield curve chart visualizes the term structure, derived metrics surface cross-asset regime signals.
- Files: `src/tui/views/economy.rs` (new `render_top_strip`, `render_yield_curve_chart`, `render_derived_metrics`, `render_macro_table` functions; `yield_curve_label` helper; silver added to `economy_symbols`)
- Tests: 871 passing (was 866), 5 new tests (silver inclusion, 4 yield curve label states), clippy clean
- TODO: F3.3 Economy tab enhancement (P1)

### 2026-03-04 — Watchlist daily change % column (P1 feedback)

- What: added 1D change % column to `pftui watchlist` CLI output. Computes daily change from price history (last two records) per symbol, with proper Yahoo symbol mapping for crypto. Output now shows: Symbol, Name, Category, Price, 1D Chg %, Updated.
- Files: `src/commands/watchlist_cli.rs` (added `yahoo_symbol_for`, `compute_change_pct` helpers, 6-column row layout, 11 new tests)
- Tests: 866 passing (was 855), clippy clean

### 2026-03-04 — Bulk watchlist add (P1 feedback)

- What: added `--bulk` flag to `pftui watch` command. `pftui watch --bulk GOOG,META,AMZN,TSLA` adds all symbols in one command instead of requiring 20 separate calls. Categories auto-detected per symbol. Optional `--category` override applies to all.
- Files: `src/cli.rs` (Watch variant gains `bulk` field, `symbol` becomes Optional), `src/main.rs` (Watch handler parses comma-separated bulk input)
- Tests: 856 passing, clippy clean
- TODO: [Feedback] Bulk watchlist add (P1)

### 2026-03-04 — Fix history cash inclusion (P0 feedback)

- What: `history --date` now includes cash positions regardless of transaction date. Previously, cash set via `set-cash` (which stamps today's date) was filtered out when querying historical dates, showing misleading totals (e.g. $184k instead of $362k).
- Files: `src/commands/history.rs`
- Tests: added `history_cash_included_regardless_of_date` regression test. Total: 856 passing.

### 2026-03-04 — Macro symbols in `refresh` cycle (F3.2)

- What: `pftui refresh` now fetches and caches all economy dashboard symbols (DXY, VIX, oil, copper, yields, FX pairs) alongside portfolio and watchlist prices. Macro symbols deduplicate against portfolio positions (e.g. GC=F). Output shows macro symbol count.
- Files: `src/commands/refresh.rs`
- Tests: 4 updated tests (collect_symbols now accounts for macro symbols). Total: 855 passing.

### 2026-03-04 — FRED API integration + economic_cache DB (F3.1)

- What: added FRED API client (`src/data/fred.rs`) and SQLite economic indicator cache (`src/db/economic_cache.rs`). Supports 6 macro series: DGS10 (10Y yield), FEDFUNDS, CPIAUCSL (CPI), PPIACO (PPI), UNRATE, T10Y2Y (yield curve spread). New `economic_cache` DB table with (series_id, date) primary key. Added `fred_api_key` optional config field. Aggressive caching with staleness detection per frequency (3 days for daily, 45 days for monthly series).
- Files: new `src/data/fred.rs`, new `src/data/mod.rs`, new `src/db/economic_cache.rs`, `src/db/mod.rs`, `src/db/schema.rs`, `src/config.rs`, `src/main.rs`, `src/app.rs`
- Tests: 17 new tests (6 fred metadata/staleness, 11 economic_cache CRUD). Total: 855 passing.
- TODO: F3.1 FRED API integration

### 2026-03-03 — Add `--technicals` flag to `brief` and `summary` CLI commands (F1.4)

- What: added `--technicals` flag to both `pftui brief` and `pftui summary`. When passed, appends a technicals table showing RSI(14) with signal label (overbought/neutral/oversold), MACD line + histogram with signal label (bullish/bearish), SMA(50), and SMA(200) for each non-cash position. Uses existing indicators engine with cached price history (up to 250 days). Cash positions are skipped. Missing data gracefully shows "—" or "N/A".
- Files: `cli.rs` (flag definitions), `main.rs` (dispatch), `commands/brief.rs` (technicals computation + markdown table), `commands/summary.rs` (technicals computation + plain text table)
- Tests: 5 new tests — rsi_label_categories, macd_label_categories, technicals_section_skips_cash, technicals_section_empty_data, brief_with_technicals_flag. Total: 839 passing.
- TODO: F1.4 `--technicals` flag for `brief` and `summary`

### 2026-03-03 — Add compact RSI(14) indicator column to Positions and Watchlist tabs (F1.3)

- What: Added RSI column to Positions tab (full and privacy views) and Watchlist tab. Shows RSI(14) value with color-coded zones: red >70 (overbought), green <30 (oversold), neutral otherwise. Direction arrows (▲/▼) show RSI momentum vs previous bar. Uses the existing `indicators::compute_rsi()` engine.
- Why: F1.3 — at-a-glance RSI per position without opening the detail popup. Helps spot overbought/oversold conditions across the whole portfolio.
- Files: `src/tui/views/positions.rs` (added `build_rsi_spans()`, RSI column in full/privacy tables), `src/tui/views/watchlist.rs` (RSI column)
- Tests: 834 passing (+6 new: empty history, insufficient data, all-rising overbought, all-falling oversold, neutral range, rising arrow)
- TODO: F1.3 — Compact indicator strip on position rows

### 2026-03-03 — Wire indicators into asset detail popup, add MACD + RSI gauge + SMA(200) (F1.2)

- What: Replaced local SMA/BB/RSI implementations in asset detail popup with the `indicators/` module. Added MACD(12,26,9) display with histogram bars, RSI visual gauge bar (color-zoned), and SMA(200). Removed dead_code suppressions from indicators module.
- Why: F1.2 — first consumer of the indicators engine in the TUI. Makes technical analysis visible per-asset in the detail popup.
- Files: `src/indicators/mod.rs`, `src/indicators/bollinger.rs`, `src/tui/views/asset_detail_popup.rs`
- Tests: 828 passing (replaced 5 old local-function tests with 4 new gauge/MACD/integration tests)
- TODO: F1.2 — Technicals in asset detail popup

### 2026-03-03 — Add technical indicators math module (F1.1)

- What: New `src/indicators/` module with pure math functions: RSI (Wilder's smoothing, period 14), MACD (12/26/9 with EMA), SMA (configurable period), and Bollinger Bands (20,2 with band width). All operate on `&[f64]` slices — no I/O, no side effects.
- Why: Foundation for F1.2–F1.4 (technicals in asset detail popup, position rows, CLI output). Replaces future need for external `fetch_prices.py` dependency.
- Files: new `src/indicators/{mod,rsi,macd,sma,bollinger}.rs`, `src/main.rs` (module registration)
- Tests: 26 new tests (RSI: 7, MACD: 6, SMA: 6, Bollinger: 6, EMA: 1). Total: 829 passing.
- TODO: F1.1 Indicators math module (P1)

### 2026-03-03 — Fix U.UN (Sprott Uranium) price accuracy via FX conversion

- What: Yahoo Finance returns prices in the security's native currency (CAD for TSX-listed U-UN.TO). The code hardcoded `currency: "USD"`, causing a ~40% price inflation for Canadian securities. Now `fetch_price()` and `fetch_history()` extract the currency from Yahoo's metadata and, for non-USD securities, automatically fetch the live FX rate (e.g., CADUSD=X) and convert to USD. Historical prices use date-matched FX history with spot rate fallback.
- Why: P0 — `brief` reported U.UN at +31.7% gain when actual was ~-4%. Root cause: CAD price stored as USD.
- Files: `src/price/yahoo.rs` (added `fetch_fx_rate()`, `fetch_fx_history()`, currency detection in `fetch_price()` and `fetch_history()`)
- Tests: all 803 existing tests pass, no regressions. FX conversion is transparent to all consumers (TUI, CLI, price service).

### 2026-03-03 — Add daily P&L to `brief` and `summary` CLI commands

- What: Added 1D P&L (daily change in $ and %) to both CLI commands. `brief` now shows portfolio-level "**1D:** +$X (Y%)" line under the total value, plus a per-position "1D" column in the positions table showing each asset's daily price change %. `summary` now prints a "1D P&L" header line with portfolio-level daily dollar and percent change. Both modes (full and percentage) supported in `brief`; full mode in `summary`.
- Why: P0 — most requested feature across all 3 testers. TUI header showed daily P&L but CLI commands didn't.
- Files: `src/commands/brief.rs` (daily P&L header, 1D column in both full and percentage tables), `src/commands/summary.rs` (hist_1d fetch, `print_daily_pnl_header()`, threaded through run_full/run_percentage)
- Tests: all 803 tests pass, no new tests needed (existing brief integration tests cover the code paths)

### 2026-03-03 — Fix 2 clippy warnings (vec_init_then_push, int_plus_one)

- What: resolved final 2 clippy warnings. Added `#[allow(clippy::vec_init_then_push)]` to `build_help_lines()` in help.rs (100+ sequential pushes make `vec![]` macro impractical). Replaced `char_count + sep_chars + 1 <= max_chars` with `char_count + sep_chars < max_chars` in regime_assets.rs.
- Why: P0 — blocking release. `cargo clippy` now passes with zero warnings.
- Files: `src/tui/views/help.rs`, `src/tui/widgets/regime_assets.rs`
- Tests: all 803 tests pass, no changes needed

### 2026-03-03 — Fix chart ratio labels and add /BTC to all assets

- What: Fixed USD chart ratio labels from misleading "USD/Gold", "USD/BTC" to honest "DXY/Gold", "DXY/SPX", "DXY/BTC" (since DXY is the actual proxy used, not literal USD). Added DXY/SPX ratio variant for USD cash positions. Extended /BTC ratio to all equities and funds (previously only commodities had it), so SLV, VTI, AAPL etc. now show /BTC comparison charts.
- Why: P0 — ratio labels should honestly reflect the underlying data. Commodities-only /BTC restriction was arbitrary; comparing any asset to BTC is useful context.
- Files: `src/app.rs` (chart_variants_for_position USD/cash branches, generic equity/fund/commodity branch, 4 updated tests)
- Tests: 803 passing, 4 updated (test_usd_cash_variants, test_regular_equity_has_ratio_variants, test_fund_has_ratio_variants, test_equity_has_btc_ratio)
- TODO: Fix chart ratios (P0), Fix commodities missing /BTC ratio (P0)

### 2026-03-03 — Click column headers to sort positions table

- What: added mouse click-to-sort on column headers in the positions table. Clicking the Asset column sorts by name, Gain% sorts by gain percentage, and Alloc% sorts by allocation. Clicking an already-active sort column toggles between ascending and descending. Works in both full (8-column) and privacy (6-column) table layouts. Column hit detection computes boundaries from the same width constraints used by the render code (accounting for table borders, column spacing, and the 57%/43% left/right panel split in wide mode). Sort flash animation triggers on column header clicks just like keyboard sort changes. Non-sortable columns (Qty, Price, Day%, 52W, Trend) are ignored on click.
- Why: P2 Mouse Enhancements — click sort column headers. Natural, discoverable interaction — users expect clicking column headers to sort. Complements the existing keyboard sort shortcuts (a, %, $, n, c, Tab).
- Files: `src/app.rs` (new `handle_column_header_click` method, header row detection in `handle_content_click`, 5 new tests), `src/tui/views/help.rs` (added "Click header" to mouse section)
- Tests: 749 passing (5 new: click_column_header_sorts_by_asset_name, click_column_header_toggles_direction_on_same_field, click_column_header_alloc_column, click_column_header_updates_sort_flash_tick, click_column_header_ignored_in_non_positions_view). Zero new clippy warnings.

### 2026-03-03 — Move watchlist from separate page to main screen sub-tab

- What: watchlist is now a sub-tab on the main Positions screen instead of a separate view. Press `w` to toggle between Positions and Watchlist on the main screen. The section header dynamically switches between "POSITIONS" and "WATCHLIST". The right pane (ASSET OVERVIEW) remains visible alongside the watchlist. Removed the `ViewMode::Watchlist` variant entirely, removed the `[5]Watch` tab from the header bar, and updated all navigation functions (move_down/up, jump_to_top/bottom, scroll half-page) to route through the new `MainTab` enum. Position-only keys (A for add transaction, X for delete) are guarded behind `MainTab::Positions`. Key `1` resets both `view_mode` and `main_tab` to Positions. Help overlay updated: `5 Watchlist` → `w Toggle Watchlist`.
- Why: P0 Owner Request — watchlist shouldn't require leaving the main screen. Having it as a sub-tab (`w` toggle) keeps the user in the same layout context with the chart pane still visible, making it easy to quickly check watched assets without losing position context. Reduces view count from 5 to 4 for cleaner navigation.
- Files: `src/app.rs` (new `MainTab` enum, `main_tab` field, `w` keybinding, updated all navigation match arms, removed `ViewMode::Watchlist`, 6 new tests), `src/tui/ui.rs` (dynamic section label, watchlist rendering in left pane), `src/tui/views/help.rs` (updated key hint), `src/tui/views/watchlist.rs` (removed title from block), `src/tui/widgets/header.rs` (removed `[5]Watch` tab)
- Tests: 6 new tests (default tab, w toggles to watchlist, w toggles back, w only in positions view, key 1 resets, tab persists across view switch). Total: 610 tests passing.
- TODO: Move watchlist from separate page to main screen tab (P0)

### 2026-03-03 — Add POSITIONS and ASSET OVERVIEW section headers

- What: added section header bars above the positions table (left pane) and asset overview (right pane) in the standard two-column layout. Headers render as a styled rule line: `── LABEL ────────` with `text_accent` for the label and `border_subtle` for decorative rules, on a `surface_2` background for visual separation between layout sections. Gracefully omitted when terminal is too short.
- Why: clear visual hierarchy between layout sections. Positions and asset overview now have distinct labeled regions, improving scannability of the two-column layout.
- Files: `src/tui/theme.rs` (new `SECTION_HEADER_HEIGHT` constant, `render_section_header()` function), `src/tui/ui.rs` (updated `render_positions_layout()` with section headers in left and right panes)
- Tests: 6 new — section header height constant, label rendering, surface_2 background, zero-height skip, narrow-width skip, full-width fill. Total: 578 tests passing.
- TODO: Add "POSITIONS" section header (P1), Add "ASSET OVERVIEW" header to right pane (P1)

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

### 2026-03-04 — Add client-side rate limiting to price fetching

- What: added inter-request delays to prevent Yahoo Finance and CoinGecko rate limiting when fetching prices for large portfolios (40+ symbols). Yahoo requests get ~100ms delay between sequential calls. CoinGecko history fetches get ~200ms delay. History batch fetching changed from fully concurrent (JoinSet) to sequential with delays. Applied to both TUI price service (`price/mod.rs`) and CLI `refresh` command.
- Why: demo mode and fresh installs fire 40+ requests with no delay, triggering 429 rate limits from Yahoo and CoinGecko free tier.
- Files: `src/price/mod.rs` (fetch_all, fetch_history_batch + new constants), `src/commands/refresh.rs` (fetch_all_prices)
- Tests: all 855 tests pass, no changes needed (rate limiting is timing-only, no logic changes)
- TODO: Add client-side rate limiting to price fetching (P0)

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

_Older entries archived in CHANGELOG-archive.md_
