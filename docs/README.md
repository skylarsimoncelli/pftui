# pftui

**A full-featured terminal portfolio tracker and market dashboard built in Rust.**

Bloomberg Terminal aesthetics. btop-level polish. Live market data, braille charts, ratio analysis, 6 color themes, and privacy mode — all in your terminal.

## Features

### Portfolio Tracking
- Full portfolio mode with buy/sell transactions and FIFO cost basis
- Percentage-only mode for privacy-first users (no monetary values stored)
- Positions table with quantity, price, gain %, allocation %, 52-week range indicator, and inline sparkline trends
- 52-week high/low range bar showing current price position within the annual range, with distance-from-high percentage
- Category-grouped allocation bar chart (horizontal, fractional Unicode bars)
- 90-day portfolio value sparkline (braille dot-matrix)
- CSV and JSON export

### Markets Overview
- Dedicated Markets tab (`3`) with major indices, commodities, crypto, forex, and bonds
- 18 market symbols: SPX, NDX, DJI, RUT, VIX, Gold, Silver, Oil, NatGas, BTC, ETH, SOL, DXY, EUR, GBP, JPY, 10Y, 2Y
- Live prices and daily change % with gain-aware color coding
- Full j/k/gg/G/Ctrl+d/Ctrl+u navigation
- Auto-fetches data on tab activation and at startup

### Economy Dashboard
- Dedicated Economy tab (`4`) with macro economic indicators
- 14 economic symbols across 4 groups: Yields (2Y, 5Y, 10Y, 30Y), Currency (DXY, EUR, GBP, JPY, CNY), Commodities (Gold, Oil, Copper, NatGas), Volatility (VIX)
- Treasury yields formatted as percentages (e.g. 4.325%)
- Visual group separators between sections
- Live values and daily change % with gain-aware color coding
- Full j/k/gg/G/Ctrl+d/Ctrl+u navigation
- Auto-fetches data on tab activation and at startup

### Watchlist
- Dedicated Watchlist tab (`5`) to track assets without holding them
- Add/remove symbols via CLI: `pftui watch <SYMBOL>`, `pftui unwatch <SYMBOL>`
- Auto-detects asset category (equity, crypto, commodity, etc.) or specify with `--category`
- Live prices and daily change % with gain-aware color coding
- Full j/k/gg/G/Ctrl+d/Ctrl+u navigation
- Empty state shows usage instructions
- Symbols stored in SQLite `watchlist` table, persisted across sessions
- Case-insensitive symbol handling (stored uppercase)

### Market Data
- Live spot prices via Yahoo Finance and CoinGecko
- Auto-refresh on configurable interval (default: 60s)
- ~130 built-in asset names (equities, crypto, ETFs, forex, commodities)
- 62 CoinGecko coin ID mappings with Yahoo Finance fallback
- Price flash animations on updates
- SQLite price cache for instant startup before first live fetch

### Charts
- Unicode braille dot-matrix rendering (2x4 dots per cell = high resolution)
- Per-position price charts with selectable timeframe (1W, 1M, 3M, 6M, 1Y, 5Y)
- Ratio charts: BTC/SPX, BTC/Gold, Gold/SPX, and more
- Multi-panel "All" view stacking multiple charts vertically
- Gain-aware gradient coloring (green gradient for gains, red for losses)
- `J`/`K` cycling through chart variants per asset
- Volume bars below price charts using block characters (▁▂▃▄▅▆▇█), theme-aware muted coloring
- SMA(20) and SMA(50) moving average overlays on single-symbol price charts, using theme accent colors

### Themes
- 6 built-in themes: Midnight (default), Catppuccin Mocha, Nord, Dracula, Solarized Dark, Gruvbox
- 28 named color slots per theme covering every UI element
- Gain intensity scaling (color saturation proportional to gain magnitude)
- Pulse animations (live indicator, price flash)
- Theme persists to config on change

### Position Detail
- Press Enter on any position to open a full-screen detail popup
- Shows: symbol, name, category, current price, quantity, avg cost, cost basis, value, gain, gain%, allocation%
- Displays up to 10 most recent buy/sell transactions for the position
- Respects privacy mode (hides quantity, cost, gain, and transactions)
- Press Enter again from popup to open the chart in the sidebar
- Esc closes the popup
- Theme-aware with gain-colored performance metrics and category badges

### Privacy
- **Percentage mode**: stores only allocation percentages, no monetary data exists in DB
- **Privacy view toggle** (`p`): hides quantities and gains in-session, shows only prices and allocations
- Header, positions table, sidebar, and sort keys all adapt to privacy state


### Responsive Layout
- Adapts to terminal width automatically
- Below 100 columns: sidebar hidden, positions use full width, header and status bar condensed
- At 100+ columns: standard two-panel layout with sidebar/chart panel
- Layout updates instantly on terminal resize
### CLI
- Interactive setup wizard with symbol autocomplete and multi-match disambiguation
- `add-tx` (interactive or flag-driven), `remove-tx`, `list-tx`
- `summary` for quick stdout portfolio overview
- `export csv` / `export json`

## Architecture

### Component Diagram

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
          │ 4 tables│  │ Yahoo + CG  │ ← channel-based communication
          └─────────┘  └─────────────┘
                 │            │
          ┌──────▼────────────▼──────┐
          │       models/            │
          │ Position, Transaction,   │
          │ Allocation, AssetCategory│
          │ PriceQuote, HistoryRecord│
          └──────────────────────────┘
```

### Data Flow

1. **Startup**: load config → open DB + migrate → load cached prices → launch TUI
2. **Price service**: spawns OS thread with Tokio runtime, sends `FetchAll` on startup
3. **Channel loop**: price thread sends `PriceUpdate` messages → `app.tick()` drains non-blocking on every 16ms frame
4. **Recompute**: on new quotes → recompute positions, gains, allocations, display sort/filter
5. **Render**: stateless widget functions read `&App` and draw to terminal via ratatui

### Price Routing

| Asset Type | Primary Source | Fallback |
|---|---|---|
| Crypto | CoinGecko (spot batch + daily history) | Yahoo Finance (`{SYM}-USD`) |
| Equity, Fund, Index | Yahoo Finance | — |
| Commodity (futures) | Yahoo Finance | — |
| Forex | Yahoo Finance | — |
| Cash | Hardcoded 1.0 | — |

### Layout

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

In compact mode:
- Sidebar (allocation bars, sparkline, price chart) is hidden; positions use full width
- Header abbreviates tab names and hides clock/theme indicator
- Status bar shows only essential hints (Help, Search)

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

## Inspirations

- **[btop](https://github.com/aristocratos/btop)** — aesthetic bar, visual density, theme system
- **[lazygit](https://github.com/jesseduffield/lazygit)** — keyboard-driven TUI UX patterns
- **[tickrs](https://github.com/tarkah/tickrs)** — terminal stock ticker, braille charting
- **Bloomberg Terminal** — information density, multi-panel layout, ratio analysis
- **[ticker](https://github.com/achannarasappa/ticker)** — simple terminal stock tracker

## Installation

```bash
# Clone and build
git clone https://github.com/skylarsimoncelli/pftui.git
cd pftui
cargo build --release

# Binary at target/release/pftui
# Optionally copy to PATH:
cp target/release/pftui ~/.local/bin/
```

### First Run

```bash
pftui
# → Launches interactive setup wizard
# → Choose Full mode (transactions) or Percentage mode (allocations only)
# → Enter your positions with symbol autocomplete
# → TUI launches automatically after setup
```

## Usage

```bash
pftui                          # Launch TUI (or setup wizard if first run)
pftui setup                    # Re-run setup wizard
pftui add-tx                   # Add transaction interactively
pftui add-tx --symbol AAPL --category equity --tx-type buy \
             --quantity 10 --price 150 --date 2025-01-15
pftui remove-tx <id>           # Remove transaction by ID
pftui list-tx                  # List all transactions
pftui summary                  # Print portfolio summary
pftui export csv               # Export positions as CSV
pftui export json              # Export positions as JSON
pftui watch AAPL               # Add AAPL to watchlist
pftui watch BTC --category crypto  # Add with explicit category
pftui unwatch AAPL             # Remove from watchlist
```

## Keybindings

### Navigation

| Key | Action |
|---|---|
| `j` / `Down` | Move down |
| `k` / `Up` | Move up |
| `gg` | Jump to top |
| `G` | Jump to bottom |
| `Ctrl+d` | Scroll down half page |
| `Ctrl+u` | Scroll up half page |
| `1` | Positions view |
| `2` | Transactions view (full mode only) |
| `3` | Markets overview |
| `4` | Economy dashboard |
| `5` | Watchlist |
| `Enter` | Open position detail popup (press again for chart) |
| `Esc` | Close chart / help overlay |
| `/` | Search / filter by name |
| `q` / `Ctrl+C` | Quit |

### Sorting

| Key | Sort By |
|---|---|
| `a` | Allocation % (descending) |
| `%` | Gain % (descending) |
| `$` | Total gain (descending) |
| `n` | Name (ascending) |
| `c` | Category (ascending) |
| `d` | Date (descending, transactions view) |
| `Tab` | Toggle sort direction |

### Other

| Key | Action |
|---|---|
| `f` | Cycle category filter (All → Equity → Crypto → ...) |
| `r` | Force price refresh |
| `p` | Toggle privacy view (full mode only) |
| `t` | Cycle color theme |
| `J` / `K` | Cycle chart variant (when chart open) |
| `h` / `l` | Cycle chart timeframe: 1W, 1M, 3M, 6M, 1Y, 5Y (when chart open) |
| `?` | Toggle help overlay |

## Themes

Six built-in themes, cycled with `t`. Persisted to config automatically.

| Theme | Description |
|---|---|
| **Midnight** | Deep navy/charcoal with jewel-tone accents (default) |
| **Catppuccin** | Catppuccin Mocha — warm browns/purples with pastels |
| **Nord** | Cool arctic blue-gray palette |
| **Dracula** | Purple/dark with vivid accents |
| **Solarized** | Solarized Dark — teal-tinted with warm/cool accents |
| **Gruvbox** | Warm retro brown/orange palette |

Each theme defines 28 color slots: surfaces (4 levels), borders (4 types), text (4 levels), gain/loss/neutral, live/stale indicators, chart gradient (3 stops), and 6 category colors.

## Chart System

### Chart Types

- **Single**: one asset's price history (timeframe selectable with h/l)
- **Ratio**: numerator ÷ denominator (e.g., BTC/SPX, Gold/BTC)
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

All charts use Unicode braille characters (U+2800–U+28FF). Each terminal cell encodes a 2x4 dot grid, giving effective resolution of `(width×2) × (height×4)` data points. Linear interpolation resamples data to fit the available terminal width.

Gradient direction is gain-aware: positive gains color bottom-to-top green, negative gains color bottom-to-top red.

Volume bars appear below single-symbol price charts as a row of Unicode block characters (▁▂▃▄▅▆▇█). Each character represents relative volume for that time slice, scaled to the maximum volume in the visible range. Volume coloring uses a muted blend of the theme's text and surface colors.

SMA(20) and SMA(50) moving average lines are overlaid on single-symbol price charts as thin braille dot lines. SMA(20) uses the theme's accent color and SMA(50) uses the border accent color, making them visually distinct from the price area fill. SMAs are not shown on ratio charts or multi-panel "All" views. A legend in the stats line identifies which line is which.

## Database

SQLite database at `~/Library/Application Support/pftui/pftui.db` (macOS). WAL journal mode, foreign keys enabled.

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

Config file: `~/Library/Application Support/pftui/config.toml`

```toml
base_currency = "USD"        # Portfolio valuation currency
refresh_interval = 60        # Price refresh interval in seconds
portfolio_mode = "full"      # "full" (transactions) or "percentage" (allocations only)
theme = "midnight"           # Active theme name
```

All fields have defaults. Missing keys are handled gracefully. Theme changes persist immediately on `t` keypress.

## Project Structure

```
pftui/
├── Cargo.toml               # Dependencies and project metadata
├── Cargo.lock
├── CLAUDE.md                 # Claude Code project instructions
├── CHANGELOG.md              # What changed and when
├── TODO.md                   # Prioritized backlog
├── docs/
│   ├── README.md             # This file
│   └── VISION.md             # Project vision and design guide
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
    │   └── watchlist.rs       # Watchlist CRUD
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
