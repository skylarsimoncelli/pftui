# Architecture

## Component Diagram

```
┌─────────────────────────────────────────────────┐
│                     main.rs                      │
│            CLI dispatch / startup flow            │
└──────────┬──────────────────┬────────────────────┘
           │                  │
     ┌─────▼─────┐    ┌──────▼──────┐
     │ commands/  │    │   tui/      │
     │ setup      │    │   mod.rs    │──── event.rs (Key/Tick/Resize)
     │ add_tx     │    │   ui.rs     │──── theme.rs (6 themes, 28 slots)
     │ remove_tx  │    │   views/    │──── views/ (positions, transactions, help)
     │ list_tx    │    │   widgets/  │──── widgets/ (header, status, sidebar, charts)
     │ export     │    └──────┬──────┘
     │ summary    │           │
     └─────┬──────┘    ┌──────▼──────┐
           │           │   app.rs    │
           │           │ App state   │
           │           │ keybindings │
           │           │ tick loop   │
           └─────┬─────┴──────┬──────┘
                 │            │
          ┌──────▼──┐  ┌──────▼──────┐
          │  db/    │  │  price/     │
          │ SQLite  │  │ PriceService│ ← dedicated thread + Tokio runtime
          │ 5 tables│  │ Yahoo + CG  │ ← channel-based communication
          └─────────┘  └─────────────┘
                 │            │
          ┌──────▼────────────▼──────┐
          │       models/            │
          │ Position, Transaction,   │
          │ Allocation, AssetCategory│
          │ PriceQuote, HistoryRecord│
          └──────────────────────────┘
```

## Data Flow

1. **Startup**: load config → open DB + migrate → load cached prices → launch TUI
2. **Price service**: spawns OS thread with Tokio runtime, sends `FetchAll` on startup
3. **Channel loop**: price thread sends `PriceUpdate` messages → `app.tick()` drains non-blocking on every 16ms frame
4. **Recompute**: on new quotes → recompute positions, gains, allocations, display sort/filter
5. **Render**: stateless widget functions read `&App` and draw to terminal via ratatui

## Price Routing

| Asset Type | Primary Source | Fallback |
|---|---|---|
| Crypto | CoinGecko (spot batch + daily history) | Yahoo Finance (`{SYM}-USD`) |
| Equity, Fund, Index | Yahoo Finance | — |
| Commodity (futures) | Yahoo Finance | — |
| Forex | Yahoo Finance | — |
| Cash | Hardcoded 1.0 | — |

## Layout

The layout adapts to terminal width for usability on different screen sizes.

**Standard layout (≥100 columns):**

```
┌──────────────────────────────────────┐
│ Header (logo, tabs, value, clock)    │  2 rows
├───────────────────────┬──────────────┤
│                       │ Sidebar      │
│ Positions/Txns/Mkts   │  alloc bars  │  57% / 43%
│                       │  sparkline   │
│                       │   — or —     │
│                       │ Price chart  │
├───────────────────────┴──────────────┤
│ Status bar (hints, live indicator)   │  2 rows
└──────────────────────────────────────┘
```

**Compact layout (<100 columns):**

```
┌──────────────────────────────────────┐
│ Header (logo, tabs, value)           │  2 rows
├──────────────────────────────────────┤
│                                      │
│ Positions/Txns/Mkts (full width)     │
│                                      │
├──────────────────────────────────────┤
│ Status bar (essential hints only)    │  2 rows
└──────────────────────────────────────┘
```

In compact mode, the sidebar (allocation bars, sparkline, price chart) is hidden and positions use the full width. Header abbreviates tab names and hides the clock/theme indicator. Status bar shows only essential hints.

## Chart System

### Chart Types

- **Single**: one asset's price history (timeframe selectable with `h`/`l`)
- **Ratio**: numerator ÷ denominator (e.g., BTC/SPX, BTC/Gold)
- **All**: multi-panel stacked view of all variants for the selected asset

### Variants by Asset

| Asset | Variants |
|---|---|
| **BTC** | All, BTC/USD, BTC/SPX, BTC/Gold, BTC/QQQ |
| **Gold** | All, Gold/USD, Gold/BTC, Gold/SPX, Gold/QQQ |
| **USD Cash** | All, Dollar Index (DXY), USD/Gold, BTC/USD |
| **Other Cash** | All, {CCY}/USD, {CCY}/DXY, Gold, BTC |
| **Equity** | All, {SYM}/USD, {SYM}/SPX, {SYM}/QQQ |
| **Fund** | All, {SYM}/USD, {SYM}/SPX, {SYM}/QQQ |
| **Crypto (non-BTC)** | All, {SYM}/USD, {SYM}/BTC, {SYM}/SPX |
| **Commodity (non-Gold)** | All, {SYM}/USD, {SYM}/SPX, {SYM}/QQQ |
| **Forex** | Single price chart |

### Rendering

All charts use Unicode braille characters (U+2800–U+28FF). Each terminal cell encodes a 2×4 dot grid, giving effective resolution of `(width×2) × (height×4)` data points. Linear interpolation resamples data to fit the available terminal width.

Gradient direction is gain-aware: positive gains color bottom-to-top green, negative gains color bottom-to-top red.

**Volume bars** appear below single-symbol price charts as a row of Unicode block characters (▁▂▃▄▅▆▇█). Each character represents relative volume for that time slice, scaled to the maximum volume in the visible range. Volume coloring uses a muted blend of the theme's text and surface colors.

**SMA(20) and SMA(50)** moving average lines are overlaid on single-symbol price charts as thin braille dot lines. SMA(20) uses the theme's accent color and SMA(50) uses the border accent color. SMAs are not shown on ratio charts or multi-panel "All" views. A legend in the stats line identifies which line is which.

## Database

SQLite with WAL journal mode and foreign keys enabled. Database location:
- **Linux**: `~/.local/share/pftui/pftui.db`
- **macOS**: `~/Library/Application Support/pftui/pftui.db`

### Tables

**transactions** — buy/sell records (full mode)

| Column | Type | Notes |
|---|---|---|
| id | INTEGER PK | autoincrement |
| symbol | TEXT | ticker symbol |
| category | TEXT | equity, crypto, forex, cash, commodity, fund |
| tx_type | TEXT | buy, sell |
| quantity | TEXT | Decimal string |
| price_per | TEXT | Decimal string |
| currency | TEXT | default USD |
| date | TEXT | YYYY-MM-DD |
| notes | TEXT | nullable |
| created_at | TEXT | auto datetime |

**price_cache** — latest spot prices (PK: symbol + currency)

| Column | Type | Notes |
|---|---|---|
| symbol | TEXT | ticker |
| price | TEXT | Decimal string |
| currency | TEXT | default USD |
| fetched_at | TEXT | RFC 3339 |
| source | TEXT | yahoo, coingecko |

**price_history** — daily close prices (PK: symbol + date)

| Column | Type | Notes |
|---|---|---|
| symbol | TEXT | ticker |
| date | TEXT | YYYY-MM-DD |
| close | TEXT | Decimal string |
| volume | TEXT | nullable, trading volume |
| source | TEXT | yahoo, coingecko |

**portfolio_allocations** — percentage mode allocations (unique symbol)

| Column | Type | Notes |
|---|---|---|
| id | INTEGER PK | autoincrement |
| symbol | TEXT | unique |
| category | TEXT | asset category |
| allocation_pct | TEXT | Decimal string |
| created_at | TEXT | auto datetime |

**watchlist** — tracked symbols not in portfolio (PK: symbol unique)

| Column | Type | Notes |
|---|---|---|
| id | INTEGER PK | autoincrement |
| symbol | TEXT | unique, stored uppercase |
| category | TEXT | asset category |
| added_at | TEXT | auto datetime |

## Configuration

Config file: `~/.config/pftui/config.toml` (Linux) or `~/Library/Application Support/pftui/config.toml` (macOS)

```toml
base_currency = "USD"        # Portfolio valuation currency
refresh_interval = 60        # Price refresh interval in seconds
portfolio_mode = "full"      # "full" (transactions) or "percentage" (allocations only)
theme = "midnight"           # Active theme name
```

All fields have defaults. Missing keys are handled gracefully. Theme changes persist immediately on `t` keypress.

## Technology

| Choice | Rationale |
|---|---|
| **Rust** | Performance, safety, single static binary, no runtime dependencies |
| **ratatui + crossterm** | Mature TUI framework, cross-platform terminal backend |
| **rust_decimal** | Exact decimal arithmetic for financial data — no floating point errors |
| **SQLite (bundled)** | Zero-config embedded database, statically linked via rusqlite |
| **Yahoo Finance API** | Broad market coverage — equities, ETFs, futures, forex, indices |
| **CoinGecko API** | Free, no API key, batched spot prices, daily history for crypto |
| **Tokio** | Async runtime for non-blocking HTTP in the price service thread |
| **anyhow** | Ergonomic error handling with context propagation |
| **clap (derive)** | Type-safe CLI argument parsing |

## File Map

```
pftui/
├── Cargo.toml
├── Cargo.lock
├── CLAUDE.md                 # Claude Code project instructions
├── CHANGELOG.md              # What changed and when
├── TODO.md                   # Prioritized backlog
├── docs/
│   ├── README.md             # Project README
│   ├── VISION.md             # Project vision and design guide
│   ├── KEYBINDINGS.md        # Full keybinding reference
│   └── ARCHITECTURE.md       # This file
└── src/
    ├── main.rs               # Entry point, CLI dispatch
    ├── app.rs                # App state, keybindings, tick loop, chart logic
    ├── cli.rs                # clap CLI argument definitions
    ├── config.rs             # Config struct, TOML load/save
    ├── commands/
    │   ├── mod.rs
    │   ├── setup.rs          # Interactive setup wizard
    │   ├── add_tx.rs         # Add transaction command
    │   ├── remove_tx.rs      # Remove transaction command
    │   ├── list_tx.rs        # List transactions command
    │   ├── export.rs         # CSV/JSON export
    │   └── summary.rs        # Portfolio summary to stdout
    ├── db/
    │   ├── mod.rs            # open_db, default_db_path
    │   ├── schema.rs         # SQLite migrations
    │   ├── transactions.rs   # Transaction CRUD
    │   ├── price_cache.rs    # Spot price cache CRUD
    │   ├── price_history.rs  # Daily history CRUD
    │   ├── allocations.rs    # Percentage mode CRUD
    │   └── watchlist.rs      # Watchlist CRUD
    ├── models/
    │   ├── mod.rs
    │   ├── position.rs       # Position struct, compute functions
    │   ├── transaction.rs    # Transaction structs, TxType enum
    │   ├── allocation.rs     # Allocation struct
    │   ├── asset.rs          # AssetCategory, PriceProvider enums
    │   ├── asset_names.rs    # Symbol name map, category inference
    │   ├── price.rs          # PriceQuote, HistoryRecord
    │   └── portfolio.rs      # PortfolioSummary struct
    ├── price/
    │   ├── mod.rs            # PriceService, channels, commands
    │   ├── yahoo.rs          # Yahoo Finance integration
    │   └── coingecko.rs      # CoinGecko integration + fallback
    └── tui/
        ├── mod.rs            # Terminal setup, main run loop
        ├── event.rs          # Event handler thread
        ├── theme.rs          # Theme system (6 themes, 28 color slots)
        ├── ui.rs             # Root layout compositor
        ├── views/
        │   ├── mod.rs
        │   ├── positions.rs  # Positions table (full + privacy)
        │   ├── position_detail.rs # Position detail popup
        │   ├── transactions.rs # Transactions table
        │   ├── markets.rs    # Markets overview tab
        │   ├── economy.rs    # Economy dashboard tab
        │   ├── watchlist.rs  # Watchlist tab
        │   └── help.rs       # Help overlay popup
        └── widgets/
            ├── mod.rs
            ├── header.rs           # Top bar
            ├── status_bar.rs       # Bottom bar
            ├── sidebar.rs          # Sidebar compositor
            ├── allocation_bars.rs  # Category allocation bars
            ├── portfolio_sparkline.rs # 90d portfolio sparkline
            └── price_chart.rs      # Price/ratio braille charts
```
