# Keybindings

Full keybinding reference for pftui. Press `?` in the TUI for an interactive version.

## Navigation

| Key | Action |
|---|---|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `gg` | Jump to top |
| `G` | Jump to bottom (non-Positions views) |
| `End` | Jump to bottom |
| `Ctrl+d` | Scroll down half page |
| `Ctrl+u` | Scroll up half page |
| `Enter` | Open position detail popup (press again for chart) |
| `Esc` | Close popup / chart / help overlay |
| `/` | Search / filter by symbol or name |
| `q` / `Ctrl+C` | Quit |

## Views

| Key | View |
|---|---|
| `1` | Positions — your portfolio holdings |
| `2` | Transactions — buy/sell history (full mode only) |
| `3` | Markets — major indices, commodities, crypto, forex |
| `4` | Economy — treasury yields, currencies, macro indicators |
| `5` | Watchlist — tracked assets you don't hold |
| `W` then `1/2/3` | Switch watchlist group |
| `8` | Chart Grid — mini charts for held positions |
| `9` | Journal |

## Charts

| Key | Action |
|---|---|
| `J` / `K` | Cycle chart variant (price, ratio, all) |
| `h` / `l` | Cycle chart timeframe: 1W, 1M, 3M, 6M, 1Y, 5Y |

Charts display SMA(20) and SMA(50) moving averages on single-symbol views, and volume bars below the price chart. Ratio charts show relative performance (e.g., BTC/SPX, AAPL/QQQ).

## Sorting

| Key | Sort By |
|---|---|
| `a` | Allocation % (descending) |
| `A` | Positions sub-mode: allocation sort (descending) |
| `%` | Gain % (descending) |
| `P` | Positions sub-mode: performance sort (descending) |
| `$` | Total gain (descending) |
| `n` | Name (ascending) |
| `c` | Category (ascending) |
| `G` | Positions sub-mode: group by category |
| `d` | Date (descending, transactions view) |
| `Tab` | Toggle sort direction |

## Actions

| Key | Action |
|---|---|
| `f` | Cycle category filter (All → Equity → Crypto → …) |
| `:`, then `scan` | Open interactive scan builder modal |
| `r` | Force price refresh |
| `Watchlist: a` | Add alert for selected watchlist symbol |
| `Watchlist: c` | Open chart popup for selected watchlist symbol |
| `Watchlist: r` | Remove selected watchlist symbol |
| `i` | Add transaction for selected position (full mode) |
| `p` | Toggle privacy view (full mode only) |
| `t` | Cycle color theme |
| `?` | Toggle help overlay |

## Mouse

| Action | Effect |
|---|---|
| Scroll wheel ↑↓ | Navigate up/down in current view |
| Click tab label | Switch to that view (Pos/Tx/Mkt/Econ/Watch) |
| Click row | Select that position/item |
| Click anywhere | Dismiss help overlay, search overlay, or detail popup |
