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

## Log

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
