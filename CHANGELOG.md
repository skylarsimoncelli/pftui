# Changelog

> Reverse chronological. Each entry: date, summary, files changed, tests.
> Automated runs append here after completing TODO items.

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
