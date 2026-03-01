# Changelog

> Reverse chronological. Each entry: date, summary, files changed, tests.
> Automated runs append here after completing TODO items.

## Format

```
### YYYY-MM-DD — Summary of change
- What: brief description of what was done
- Why: what problem it solves or what value it adds
- Files: list of files modified
- Tests: tests added or modified
- TODO: which TODO item was completed
```

---


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
