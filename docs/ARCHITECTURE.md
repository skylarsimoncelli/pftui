# Architecture — pftui

Quick-reference for automated agents. Read this FIRST, then only open the files your task needs.
Use `read --offset N --limit M` to read specific line ranges instead of full files.

## app.rs Line Map (6000 lines — DO NOT read in full)

| Lines | Section | When to read |
|-------|---------|-------------|
| 1-285 | Imports, enums (ViewMode, SortField, ChartVariant, MainTab) | Adding views/sort modes/chart types |
| 286-492 | `struct App` fields | Adding new state |
| 493-847 | `App::new()`, `init()`, `init_offline()`, price update handlers | Startup/init changes |
| 848-980 | `recompute_regime()` | Regime signal changes |
| 980-1040 | `compute_portfolio_value_history()` (LOCF forward-fill) | Portfolio chart bugs |
| 1041-1138 | `compute_daily_change()`, `compute_timeframe_gains()` | Gain/loss calculations |
| 1139-1314 | `chart_variants_for_position()` | Chart ratio/variant bugs |
| 1315-1397 | `tick()` — 60fps loop, animation counters | Animation/tick changes |
| 1398-1733 | `handle_key()` — ALL keybindings | Adding/changing keys |
| 1734-2820 | `handle_mouse()`, helpers, sorting, filtering | Mouse/sort/filter changes |
| 2821+ | `#[cfg(test)]` blocks (~3100 lines) | NEVER read unless writing tests |

## price_chart.rs Line Map (1970 lines)

| Lines | Section | When to read |
|-------|---------|-------------|
| 1-40 | Imports, `slice_history()` | — |
| 41-125 | `render()` — main dispatch (single/ratio/multi) | Chart layout changes |
| 126-440 | `render_multi_panel`, `render_single_chart`, `render_ratio_chart`, minis | Specific chart type changes |
| 440-540 | `compute_ratio()`, `compute_sma()`, `compute_bollinger()` | Technical indicator changes |
| 540-870 | `render_braille_chart()` — core braille renderer, SMA/BB overlays, crosshair, area fill | Chart rendering changes |
| 871-990 | `render_braille_mini()`, `area_fill_bg()` | Mini chart / fill changes |
| 991+ | `render_braille_lines()` (embeddable), tests | Embedding charts elsewhere |

## asset_detail_popup.rs Line Map (850 lines)

| Lines | Section | When to read |
|-------|---------|-------------|
| 1-25 | AssetDetailState struct, `render()` dispatch | Adding popup state |
| 25-117 | `render()` — popup frame, shadow, scroll | Popup layout changes |
| 117-708 | `build_lines()` — info, price, chart, technicals, portfolio context | Adding/changing popup sections |
| 709+ | Tests | NEVER read unless writing tests |

## positions.rs Line Map (1450 lines)

| Lines | Section | When to read |
|-------|---------|-------------|
| 1-220 | Helpers: 52W range, change%, row_background, category dividers | Row rendering helpers |
| 218-393 | `render()` dispatch, `render_full_table()` | Full table layout |
| 394-770 | `render_privacy_table()`, watchlist rendering | Privacy/watchlist changes |
| 771+ | Tests | NEVER read unless writing tests |

## Quick Reference

| Task | Read these files (with line ranges) |
|------|-------------------------------------|
| Fix keybinding | `app.rs:1398-1733` |
| Fix chart ratio | `app.rs:1139-1314` |
| Fix portfolio chart | `app.rs:980-1040` |
| Add new widget | `tui/widgets/new.rs` + parent view + `widgets/mod.rs` |
| Add CLI command | `cli.rs` + `commands/new.rs` + `main.rs` |
| Add view/tab | `app.rs:1-285` (ViewMode enum) + `ui.rs` + `help.rs` + `header.rs` |
| Theme changes | `theme.rs` (all 11 themes) |
| Price fetching | `price/yahoo.rs` or `price/coingecko.rs` |
| Mouse handling | `app.rs:1734-2820` |
| Add state field | `app.rs:286-492` (struct) + `app.rs:493-847` (init) |
| Technical indicators | `indicators/mod.rs` re-exports ONLY (never read individual files) |
| Asset detail popup | `asset_detail_popup.rs:117-708` (build_lines sections) |
| FX / currency work | `price/yahoo.rs` (YMetaData.currency, FX rate fetch) |

## Module Index

### Data Layer
`db/schema.rs` (migrations) · `db/transactions.rs` (CRUD) · `db/price_cache.rs` (spot cache) · `db/price_history.rs` (daily history, merge) · `db/allocations.rs` (% mode) · `db/watchlist.rs`

### Models
`models/position.rs` (Position, compute_positions) · `models/transaction.rs` (Transaction, TxType) · `models/asset.rs` (AssetCategory, PriceProvider) · `models/asset_names.rs` (130+ symbols, infer_category, search) · `models/price.rs` (PriceQuote, HistoryRecord)

### Price Service
`price/mod.rs` (PriceService thread + Tokio channels) · `price/yahoo.rs` (Yahoo spot+history, TSX normalization, FX conversion via YMetaData.currency) · `price/coingecko.rs` (CoinGecko 62-coin map, Yahoo fallback)

### TUI Views (signature: `(&mut Frame, Rect, &App)`)
`tui/ui.rs` (root layout) · `views/positions.rs` (positions+watchlist table) · `views/markets.rs` (markets tab) · `views/economy.rs` (economy tab) · `views/transactions.rs` · `views/help.rs` (help popup) · `views/position_detail.rs` · `views/search_overlay.rs` (/ search) · `views/asset_detail_popup.rs` · `views/context_menu.rs` (right-click)

### TUI Widgets
`theme.rs` (28 color slots, 11 themes, animations, shadows) · `widgets/price_chart.rs` (braille charts, SMA, BB, crosshair) · `widgets/header.rs` (top bar) · `widgets/status_bar.rs` (bottom bar) · `widgets/sidebar.rs` (compositor) · `widgets/allocation_bars.rs` · `widgets/portfolio_sparkline.rs` · `widgets/portfolio_stats.rs` · `widgets/asset_header.rs` · `widgets/top_movers.rs` · `widgets/skeleton.rs` · `widgets/regime_bar.rs`

### Indicators (DO NOT read individual files — use mod.rs re-exports)
```
indicators/mod.rs re-exports:
  compute_rsi(&[f64], period) -> Vec<Option<f64>>           // RSI. period=14 standard
  compute_sma(&[f64], period) -> Vec<Option<f64>>           // Simple moving average
  compute_macd(&[f64]) -> MacdResult { macd, signal, histogram: Vec<f64> }
  compute_bollinger(&[f64], period, multiplier) -> BollingerBands { upper, lower, middle, width: Vec<f64> }
```
Color conventions: RSI >70 = red (overbought), <30 = green (oversold), 30-70 = neutral

### Regime
`regime/mod.rs` (9-signal scorer) · `regime/suggestions.rs` (portfolio suggestions)

### CLI Commands
`commands/setup.rs` (wizard) · `commands/summary.rs` (--group-by, --period, --what-if) · `commands/export.rs` (JSON/CSV) · `commands/import.rs` (replace/merge) · `commands/history.rs` (--date) · `commands/brief.rs` · `commands/demo.rs` · `commands/snapshot.rs` · `commands/watchlist_cli.rs` · `commands/set_cash.rs` · `commands/refresh.rs` · `commands/value.rs`

## Key Patterns

- **Keybinding**: `app.rs` → `handle_key()` L1398 → match `KeyCode`
- **View/tab**: `ViewMode` enum L1-285 + `ui.rs` + `help.rs` + `header.rs`
- **CLI command**: `cli.rs` (clap) + `commands/new.rs` + `main.rs`
- **Widget**: `widgets/new.rs` + wire into parent view + `mod.rs`
- **Chart**: `price_chart.rs` render, `app.rs` L1139 variant logic
- **Theme**: `theme.rs` — update ALL 11 themes for new color slots
