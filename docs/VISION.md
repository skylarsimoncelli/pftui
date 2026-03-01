# Vision

## Mission

The best terminal experience for viewing your investment portfolio and market data. Period.

Bloomberg Terminal information density + btop visual polish + vim keyboard UX = pftui.

## Core Identity

**What pftui IS:**
- A maximalist terminal dashboard — more data, more charts, more views, more value per pixel
- Vim-native — j/k/gg/G/Esc/Enter/? are muscle memory, not learned behavior
- Beautiful by default — every theme is hand-tuned, every animation is intentional
- Financially precise — rust_decimal everywhere, zero floating point for money
- Privacy-conscious — percentage mode stores no monetary data; privacy toggle hides values instantly

**What pftui is NOT:**
- Not a web app — terminal-native rendering, keybindings, performance
- Not minimal — density is the goal, not simplicity
- Not ugly — aesthetics are a hard requirement, not optional
- Not a trading platform — read-only data and tracking, no order execution
- Not approximate — exact decimal arithmetic or nothing

## Design Principles

Use these to make implementation decisions:

### 1. Information Density
Every terminal cell earns its place. If a panel has empty space, it should show more data. Combine multiple data points in single rows (sparkline trends in table cells, gain-colored text, category badges). No "loading" states that persist — show cached data immediately, update when live.

### 2. Visual Hierarchy
The eye should flow naturally: header → main content → sidebar → status bar. Use color intensity to signal importance (bright = actionable/changing, muted = static/labels). Gain-aware coloring everywhere — green/red gradients scaled by magnitude, not binary.

### 3. Vim-Native UX
Follow vim conventions faithfully:
- `j`/`k` — down/up (already done)
- `gg` — jump to top, `G` — jump to bottom
- `/` — search/filter, `Esc` — cancel/close
- `Ctrl+d`/`Ctrl+u` — half-page scroll
- Number keys for view tabs (1, 2, 3...)
- Single-key actions for common operations
- No mouse required for any feature

### 4. Graceful Degradation
Missing data never crashes or garbles the UI. Show the best available data: cached prices on startup, "---" for missing fields, "Loading..." only when nothing is cached. Stale indicators (yellow dot) tell the user what they're looking at.

### 5. Theme Coherence
Every new widget, view, or visual element must look correct in all 6 themes. Use the 28 named color slots — never hardcode RGB values. Test visually in at least midnight (dark) and solarized (medium contrast) before shipping.

## Feature Quality Bar

A feature is "done" when:
- Works with all 6 asset categories (equity, crypto, forex, cash, commodity, fund)
- Renders correctly in all 6 themes
- Respects privacy mode (both percentage mode and `p` toggle)
- Handles missing/stale data gracefully
- Uses rust_decimal for any monetary value
- Uses `app.theme.*` for all colors
- Keybindings documented in help overlay (`?`)
- Status bar hints updated
- Has tests for any non-trivial logic
- Vim conventions followed for any new keybindings

## Target Feature Set

The complete vision for pftui — what "done" looks like:

### Views (number-key tabs)
1. **Positions** — portfolio holdings with live prices, gains, allocation, sparkline trends *(done)*
2. **Transactions** — buy/sell history table *(done)*
3. **Markets** — broad market overview: major indices, sectors, commodities, crypto leaders
4. **Economy** — macro dashboard: treasury yields, Fed rate, CPI, unemployment, DXY, fear/greed
5. **News** — financial news feed, per-asset and market-wide, headline + source + timestamp
6. **Watchlist** — track assets without holding them, same chart/data access as positions

### Chart Enhancements
- Multiple timeframes: 1D, 1W, 1M, 3M, 6M, 1Y, 5Y, Max
- Candlestick variant (OHLC with braille encoding)
- Volume bars overlay below price chart
- Moving average overlays (SMA 20/50/200, EMA)
- Technical indicators as sub-panels (RSI, MACD, Bollinger Bands)
- Equity ratio charts (vs SPX, vs sector ETF)

### Portfolio Analytics
- Benchmark comparison (portfolio vs SPX, vs BTC, custom)
- Sharpe ratio, max drawdown, volatility
- Correlation matrix between positions
- Dividend tracking and yield display
- Tax lot tracking and realized gain/loss
- Multi-portfolio support

### Data Sources
- Financial news (RSS feeds, free APIs)
- FRED economic data (treasury yields, CPI, unemployment, GDP)
- Fear & Greed index
- Sector/industry heatmap data
- Earnings calendar
- Options chains (if free source exists)

### UX
- Symbol search with `/` (vim-style)
- Half-page scroll with Ctrl+d/Ctrl+u
- gg/G for jump to top/bottom
- Configurable layout (panel sizes, sidebar position)
- Terminal notifications on significant price moves
- Responsive layout adapting to terminal size
