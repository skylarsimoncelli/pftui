# Architecture — pftui

Quick-reference for automated agents. Read this FIRST to find the right files.

## Core State

| File | Lines | Purpose |
|------|-------|---------|
| `src/app.rs` | ~6000 | God file: App struct, keybindings, tick loop, portfolio computation, chart variants. If splitting, key sections: state fields (top), `handle_key()` (~line 950), `compute_*()` methods, chart variant logic |
| `src/config.rs` | ~120 | Config struct, load/save `~/.config/pftui/config.toml` |
| `src/cli.rs` | ~150 | Clap CLI definitions, all subcommands |
| `src/main.rs` | ~200 | CLI dispatch, entry point |

## Data Layer

| File | Lines | Purpose |
|------|-------|---------|
| `src/db/schema.rs` | ~100 | SQLite migrations (4 tables: transactions, price_cache, price_history, allocations) |
| `src/db/transactions.rs` | ~100 | Transaction CRUD |
| `src/db/price_cache.rs` | ~80 | Spot price cache CRUD |
| `src/db/price_history.rs` | ~150 | Daily history CRUD, `merge_history()` |
| `src/db/allocations.rs` | ~60 | Percentage-mode allocation CRUD |
| `src/db/watchlist.rs` | ~80 | Watchlist CRUD |

## Models

| File | Lines | Purpose |
|------|-------|---------|
| `src/models/position.rs` | ~200 | Position struct, `compute_positions()`, `compute_positions_from_allocations()` |
| `src/models/transaction.rs` | ~50 | Transaction, NewTransaction, TxType |
| `src/models/asset.rs` | ~80 | AssetCategory, PriceProvider enums |
| `src/models/asset_names.rs` | ~460 | 130+ symbol→name map, `infer_category()`, `search_names()` |
| `src/models/price.rs` | ~40 | PriceQuote, HistoryRecord structs |

## Price Service

| File | Lines | Purpose |
|------|-------|---------|
| `src/price/mod.rs` | ~350 | PriceService (dedicated thread + Tokio), PriceCommand/PriceUpdate channels |
| `src/price/yahoo.rs` | ~250 | Yahoo Finance API (spot + history), `normalize_yahoo_symbol()` for TSX tickers |
| `src/price/coingecko.rs` | ~300 | CoinGecko API (spot + history), 62-coin ID map, Yahoo fallback |

## TUI Views (each takes `(&mut Frame, Rect, &App)`)

| File | Lines | Purpose |
|------|-------|---------|
| `src/tui/ui.rs` | ~200 | Root layout compositor, panel splits, section headers |
| `src/tui/views/positions.rs` | ~1450 | Positions table (full + privacy), watchlist tab, row rendering, sort logic |
| `src/tui/views/markets.rs` | ~520 | Markets tab with sparklines, momentum, heatmap |
| `src/tui/views/economy.rs` | ~710 | Economy tab with indicators, yield curve |
| `src/tui/views/transactions.rs` | ~300 | Transaction list view |
| `src/tui/views/help.rs` | ~350 | Help overlay popup |
| `src/tui/views/position_detail.rs` | ~570 | Position detail popup |
| `src/tui/views/search_overlay.rs` | ~450 | Global asset search (`/` key) |
| `src/tui/views/asset_detail_popup.rs` | ~850 | Full asset detail popup (from search) |
| `src/tui/views/context_menu.rs` | ~200 | Right-click context menu |

## TUI Widgets

| File | Lines | Purpose |
|------|-------|---------|
| `src/tui/theme.rs` | ~1300 | Theme struct (28 color slots), 11 themes, gradients, animation constants, `render_popup_shadow()` |
| `src/tui/widgets/price_chart.rs` | ~1970 | Braille price/ratio charts, SMA, Bollinger, crosshair, area fill |
| `src/tui/widgets/header.rs` | ~740 | Top bar: logo, tabs, portfolio value, ticker tape, clock |
| `src/tui/widgets/status_bar.rs` | ~400 | Bottom bar: key hints, search mode, theme toast |
| `src/tui/widgets/sidebar.rs` | ~200 | Sidebar compositor |
| `src/tui/widgets/allocation_bars.rs` | ~350 | Category allocation bars with change indicators |
| `src/tui/widgets/portfolio_sparkline.rs` | ~550 | Portfolio braille sparkline with timeframe gains |
| `src/tui/widgets/portfolio_stats.rs` | ~150 | Top/worst performer stats |
| `src/tui/widgets/asset_header.rs` | ~200 | Asset detail header above chart |
| `src/tui/widgets/top_movers.rs` | ~250 | Top movers by category |
| `src/tui/widgets/skeleton.rs` | ~150 | Loading shimmer placeholders |
| `src/tui/widgets/regime_bar.rs` | ~200 | Regime intelligence health bar |

## Regime Intelligence

| File | Lines | Purpose |
|------|-------|---------|
| `src/regime/mod.rs` | ~690 | 9-signal regime scorer (VIX, yields, DXY, Cu/Au, etc.) |
| `src/regime/suggestions.rs` | ~420 | Regime-based portfolio suggestions |

## CLI Commands

| File | Lines | Purpose |
|------|-------|---------|
| `src/commands/setup.rs` | ~685 | Interactive setup wizard with fuzzy search |
| `src/commands/summary.rs` | ~1090 | `pftui summary` with --group-by, --period, --what-if |
| `src/commands/export.rs` | ~420 | JSON/CSV export with --output flag |
| `src/commands/import.rs` | ~720 | JSON import with replace/merge modes |
| `src/commands/history.rs` | ~600 | `pftui history --date` time travel |
| `src/commands/brief.rs` | ~665 | `pftui brief` markdown summary |
| `src/commands/demo.rs` | ~300 | `pftui demo` with mock portfolio |
| `src/commands/snapshot.rs` | ~250 | `pftui snapshot` ANSI/plain render |
| `src/commands/watchlist_cli.rs` | ~200 | `pftui watchlist` display |
| `src/commands/set_cash.rs` | ~180 | `pftui set-cash` shortcut |
| `src/commands/refresh.rs` | ~150 | `pftui refresh` headless price fetch |
| `src/commands/value.rs` | ~100 | `pftui value` quick check |

## Key Patterns

- **Adding a keybinding**: `app.rs` → `handle_key()` → match on `KeyCode`
- **Adding a view/tab**: `ViewMode` enum in `app.rs` + `ui.rs` compositor + help.rs + header tab
- **Adding a CLI command**: `cli.rs` (clap) + `commands/new.rs` + `main.rs` dispatch
- **Adding a widget**: `tui/widgets/new.rs` + wire into parent view + `mod.rs`
- **Chart changes**: `price_chart.rs` for rendering, `app.rs` for variant logic
- **Theme changes**: `theme.rs` — all 11 themes must be updated for new color slots
