# thinkorswim UX Research for pftui

**Research Date:** 2026-03-06  
**Target:** pftui terminal portfolio tracker  
**Subject:** thinkorswim (ToS) platform — professional trading terminal UX patterns

---

## Executive Summary

**Key Takeaways for pftui:**

1. **Linked Views are Critical** — ToS uses color-coded "symbol links" (clipboard icons with color+number codes) to synchronize multiple views to the same symbol. When you click a symbol in a watchlist, all linked charts/panels update instantly. This is the foundation of their workflow efficiency. **pftui should implement symbol linking** — selecting a position in the main table should instantly update detail panels, charts, news, etc.

2. **Information Density Through Customization** — ToS packs massive amounts of data without overwhelming by making **everything** customizable: columns, panels, colors, layouts, studies. Users start with sensible defaults but can drill down. **pftui should embrace aggressive customization**: let users toggle columns, choose chart overlays, hide/show widgets via config or runtime hotkeys.

3. **Keyboard-First Power User Workflows** — ToS has comprehensive hotkey support (Ctrl/Alt/Shift combos, customizable per command category: General, Chart, Watchlist, Active Trader, thinkScript Editor). Power users rely on hotkeys for: switching tabs, adding filters, entering orders, jumping between timeframes. **pftui already has excellent vim-style nav** — extend this to all new features (scanner filters, alert creation, column customization).

4. **Scanners are Filters, Not Searches** — Stock Hacker (scanner) uses **up to 25 stacked filters** with boolean logic groups (All/None/Any). Pre-scan histograms show result distribution before you run. Users save scan queries, create alerts when results change. **pftui should implement a filter-based screener** with composable predicates (price > X, volume > Y, allocation drift > Z), saveable queries, alert integration.

5. **Contextual Panels Beat Tabs** — ToS doesn't force you into a single "mode". The main window has 8 tabs (Monitor, Trade, Analyze, Scan, MarketWatch, Charts, Tools, thinkManual), but each tab can host **grids of sub-panels** (up to 6+ windows per tab). Users create custom workspaces. **pftui should move beyond a single main view** — add split-pane support, detachable widgets, saveable workspace layouts.

---

## Layout Patterns: Workspace & Organization

### How ToS Organizes the Interface

**Two-Zone Architecture:**
- **Left Sidebar (Gadgets):** Persistent access to watchlists, news, dashboard, FX map, trade flash, phase scores. Gadgets are small, focused widgets that stay visible while you navigate tabs.
- **Main Window (Tabs):** 8 primary tabs, each with sub-tabs. Example: Charts tab has customizable grid layouts (1x1, 2x2, 3x2, etc.), each cell an independent chart.

**Six Workspaces (Templates):**
1. **Default:** General-purpose, balanced layout. Left sidebar + main window with all 8 tabs.
2. **Futures:** Futures Trader interface, Trade Flash gadget, 5d/5m charts.
3. **Active Trader:** Short-term trading. Active Trader Ladder (DOM-style price ladder), multiple timeframe charts (1d, 1w, 1y), Analyze risk profile.
4. **Long Term:** Multi-year charts (1y/1d, Max:Yearly), company news, Phase Scores (fundamental rankings).
5. **Forex:** Forex Trader interface, FX Currency Map gadget, dual timeframes (5d/5m, 5y/1d).
6. **Options:** Options Chain, volatility charts, Analyze → Probability Analysis, Greeks dashboard.

**What Translates to TUI:**
- **Persistent sidebar with compact info widgets** ✅ pftui already has this (allocation bars, sparkline).
- **Multiple named workspaces** — Terminal UI can support this with saved config profiles (e.g., `pftui --workspace=trading` vs `--workspace=analysis`).
- **Tab bar for primary views** ✅ pftui has this (1–7 for Positions/Transactions/Markets/etc.)
- **Linked views** — Symbol selection propagates across panels via color-coded links. **Critical for terminals** — clicking a position should update chart, news, detail stats.

**What Doesn't Translate:**
- Drag-and-drop panel resizing (mouse-first). TUI equivalent: fixed split ratios or configurable layouts via config.toml.
- Multi-monitor detached windows (ToS users drag tabs to separate screens). TUI equivalent: tmux panes or terminal tabs.

**Recommendations:**
- Add **workspace presets** to config.toml: `layout = "compact"` (single-pane), `"split"` (chart + positions), `"analyst"` (chart + news + detail).
- Implement **symbol linking**: when user selects a position in the main table (j/k nav), update:
  - Price chart to that symbol
  - Detail panel (bottom or side) with fundamentals, news, recent transactions
  - Watchlist highlight if symbol is watched
- Support **split-pane view** (vertical or horizontal): main table on left, detail/chart on right. Toggle with `:split` command or `S` key.

---

## Chart Innovation: What pftui Should Steal

### ToS Chart Features

**Core Capabilities:**
- **Multi-Timeframe Overlays:** Display higher-timeframe candles on lower-timeframe charts. E.g., 30min candles overlaid on a 5min chart. Helps identify larger trends while trading intraday.
- **Study/Indicator Library:** 100+ built-in studies (moving averages, RSI, Bollinger Bands, volume profiles, Ichimoku, etc.). Users can stack studies as overlays (main chart) or sub-charts (below price).
- **Custom Studies (thinkScript):** Users write their own indicators. Studies can be shared, imported, and added to charts or watchlist columns.
- **Comparison Overlays:** Plot multiple symbols on the same chart (normalized % change or absolute price). E.g., compare SPY, QQQ, BTC on one chart.
- **Drawing Tools:** Trendlines, Fibonacci retracements, horizontal levels, channels. Synced across charts of the same symbol (unless disabled).
- **Flexible Grids:** Charts tab supports 1×1 to 6+ grid layouts. Each cell is an independent chart with its own symbol, timeframe, studies. Users save layouts as "chart grids".
- **Crosshair Synchronization:** Hover on one chart, crosshairs sync across all linked charts (same timeframe or different symbols).

**What Translates to TUI (Braille Charts):**
- **Multi-Timeframe Support** ✅ pftui already has ChartTimeframe enum (1W, 1M, 3M, 6M, 1Y, 5Y). Extend with `T` key to cycle timeframes.
- **Comparison Overlays** — Plot portfolio value vs SPY on the same braille chart. **High value for benchmark comparison.**
- **Simple Indicators** — Add moving average (SMA) overlay to price charts. Could render as a second braille line in a different color.
- **Chart Variants** ✅ pftui already has Single/Ratio/All chart modes (J/K keys). Keep this — it's excellent.

**What Doesn't Translate:**
- Complex studies (Ichimoku, volume profiles) — not enough resolution in braille.
- Drawing tools (trendlines, Fib retracements) — mouse-first, fragile in TUI.
- Candlestick patterns — braille line charts only.

**Recommendations:**
- **Add benchmark comparison chart:** `:chart vs SPY` or `C` key when viewing a position. Plot position % change vs benchmark % change over selected timeframe (1M, 3M, 1Y). Use different colors/styles for each line.
- **Add SMA overlay to price charts:** Config option `chart_sma = [20, 50, 200]` renders moving averages as faint braille lines on top of price. Power users want this.
- **Add volume sub-chart:** Below the main price chart, render a braille bar chart of daily volume. Small (3 rows max), toggleable with `V` key.
- **Persist chart timeframe per position:** When you change timeframe on one position, remember it. Next time you view that position, restore the timeframe. Store in a `chart_state` table in SQLite.
- **Add "Chart All Positions" view:** Grid of mini braille charts (one per position), 6–9 per screen. Let user see all holdings at a glance. Inspired by ToS flexible grids.

---

## Scanner/Screener UX: Composable Filters

### How ToS Stock Hacker Works

**Filter-Based Approach:**
- Users build scans by stacking **up to 25 filters** in three boolean groups:
  - **All of the following** (AND logic)
  - **None of the following** (NOT logic)
  - **Any of the following** (OR logic)
- Filter types:
  - **Stock filters:** price, volume, net change, % change, market cap, etc.
  - **Option filters:** Greeks (delta, theta, gamma, vega), implied volatility, open interest, probability of expiring ITM.
  - **Fundamental filters:** P/E ratio, EPS, dividend yield, debt/equity.
  - **Study filters:** Custom thinkScript indicators (e.g., RSI > 70, MACD crossover).
  - **Pattern filters:** Classic chart patterns (head & shoulders, triangles, flags). Max 1 pattern filter per scan.

**Pre-Scan Histograms:**
- As you adjust filter parameters, ToS shows a **live histogram** of how many results fall into each micro-range. Example: for "Price between $10–$50", the histogram shows distribution (more results at $15–20, fewer at $45–50).
- Total match count updates in real-time.

**Saved Queries & Alerts:**
- Users save scan configurations as named queries (public or private).
- Users create **alerts when scan results change**: "Notify me hourly when new symbols match this scan" or "Alert me when symbols drop out of results."

**Results Display:**
- Results appear in a watchlist-like table. Fully sortable by any column.
- Users can add columns, create alerts on individual symbols, send to charts.
- For pattern scans: results include a "Patterns" column. Click to see a chart with patterns highlighted.

**What Translates to TUI:**
- ✅ **Everything.** Scanners are inherently text-based.
- **Filter composition** — pftui can build filters using a DSL or interactive prompt: `:scan add price > 100`, `:scan add volume > 1M`, `:scan add allocation_drift > 5%`.
- **Result table** — Already fits pftui's table-driven UI.
- **Saved queries** — Store in SQLite `scan_queries` table. Load with `:scan load momentum`, save with `:scan save my_scan`.

**What Doesn't Translate:**
- Pre-scan histograms (require live market data streaming). pftui works with cached prices.
- Pattern recognition (chart patterns). Would need ML or complex heuristics. Skip for now.

**Recommendations:**
- **Add `pftui scan` subcommand** with filter DSL:
  ```
  pftui scan --filter "allocation_pct > 10" --filter "day_change_pct > 2" --limit 20
  pftui scan --filter "category = Crypto" --filter "cost_basis > 1000" --sort-by gain_pct
  ```
- **Interactive scan builder in TUI:** `:scan` opens a modal:
  ```
  Scan Builder
  ──────────────────────────────────────
  Filters (All):
    • price > 100
    • volume > 1M
    • allocation_drift > 5%
  
  [A]dd filter  [R]emove  [S]ave  [L]oad  [Enter] Run
  ```
- **Saveable scan queries:** Store in `scan_queries` table (id, name, filters_json, created_at). Show saved scans in `:scan list`.
- **Integrate with alerts:** `:alert create scan momentum` — when scan results change, trigger alert.
- **Show results in a filterable table:** Same UX as positions table. Columns: symbol, price, % change, allocation, gain/loss. Sortable (j/k nav, Enter to view detail).

---

## Watchlist UX: Customization & Sorting

### How ToS Watchlists Work

**Core Features:**
- **Multiple watchlists:** Personal (user-created) and public (Top 10, Lovers & Losers, Industry groups, Portfolio).
- **Column customization:** Right-click header → Customize. Add/remove columns from a huge library (price, volume, Greeks, fundamentals, custom studies).
- **Custom columns via thinkScript:** Users write code to compute watchlist column values. E.g., "% change from 20-day high", "RSI color-coded by threshold".
- **Sorting:** Click any column header to sort (ascending/descending/none). Click multiple times to cycle.
- **Linked to other views:** Watchlist has a **symbol link icon** (color-coded). Clicking a symbol propagates to linked charts, trade panels, news.
- **Inline actions:** Right-click a symbol → Trade, Add chart, Create alert, Remove from watchlist.

**Watchlist Gadget (Left Sidebar):**
- Compact view with up/down arrows to cycle through watchlists.
- Show actions menu for print, screenshot, save as watchlist, detach.

**What Translates to TUI:**
- ✅ **All of it.** Watchlists are perfect for TUIs.
- **Column customization** — pftui already has this for positions table (via config.toml). Extend to watchlist.
- **Sorting** — Click (or keystroke) to sort by column.
- **Multiple watchlists** — Store in `watchlist_groups` table. Switch with `:watch <name>` or `W` key + number.

**What Doesn't Translate:**
- Right-click context menus (mouse). TUI equivalent: `?` key opens action menu when hovering a row.

**Recommendations:**
- **Extend watchlist column customization:** Add `watchlist_columns` to config.toml:
  ```toml
  [watchlist]
  columns = ["symbol", "price", "change_pct", "volume", "market_cap"]
  ```
- **Add custom columns for watchlist:** Let users define computed columns in config:
  ```toml
  [[watchlist.custom_columns]]
  name = "Momentum"
  formula = "change_7d / volatility_30d"
  ```
- **Add watchlist groups:** Store multiple watchlists (`watchlist_groups` table: id, name, symbols_json). Switch with `1`/`2`/`3` keys or `:watch crypto`, `:watch tech`.
- **Link watchlist to main view:** When you select a watchlist symbol (Enter), it opens position detail or adds to a "focused symbols" bar (top of screen).
- **Inline actions in watchlist:** Press `a` to add alert, `c` to view chart, `r` to remove from watchlist.

---

## Information Architecture: Hierarchical Data Organization

### How ToS Organizes Data

**Hierarchy:**
1. **Tabs (Top-Level):** Monitor, Trade, Analyze, Scan, MarketWatch, Charts, Tools, thinkManual.
2. **Sub-Tabs (Second-Level):** E.g., Analyze tab has: Add Simulated Trades, Risk Profile, Probability Analysis, Economic Data, thinkBack, Fundamentals, Earnings.
3. **Panels (Third-Level):** Within a sub-tab, users can split into grids. E.g., Charts tab can have a 2×2 grid with 4 independent charts.
4. **Gadgets (Persistent Sidebar):** Watchlists, news, dashboard, FX map. Always visible, independent of tabs.

**Data Layering:**
- **Primary:** Symbol price, position size, P&L (always visible in Position Summary).
- **Secondary:** Fundamentals (P/E, EPS), news, corporate actions (visible in detail panels).
- **Tertiary:** Study values, Greeks, probability cones (visible in specialized sub-tabs).

**What Translates to TUI:**
- **Tab bar for primary views** ✅ pftui already has this (1–7 keys).
- **Sub-tabs or modes within views** — E.g., Positions view could have sub-modes: "By Allocation", "By Gain/Loss", "By Category".
- **Detail panels at bottom/side** — When you select a position, bottom 30% of screen shows detail (transactions, news, chart).

**What Doesn't Translate:**
- Arbitrary grid layouts (requires mouse or complex tiling). TUI equivalent: fixed split ratios.

**Recommendations:**
- **Add sub-modes to existing views:**
  - Positions: Press `G` to group by category, `A` to sort by allocation, `P` to sort by performance.
  - Markets: Press `C` to view crypto only, `E` to view equities only.
- **Add detail panel (bottom 30%):** When you select a position (Enter or `d`), bottom of screen shows:
  - Recent transactions (last 5)
  - Braille price chart (90d)
  - Latest news headline (1 line)
  - Allocation drift vs target
- **Add breadcrumb nav at top:** Show current view hierarchy:
  ```
  Positions → AAPL → Detail
  ```

---

## Keyboard-First Patterns: Power User Shortcuts

### ToS Keyboard Shortcuts

**Categories of Hotkeys:**
1. **General:** Navigate tabs, open setup, toggle gadgets, search symbols.
2. **Active Trader:** Buy at bid/ask/market, flatten position, cancel orders, adjust quantity.
3. **Watchlist:** Add/remove symbols, sort columns, create alerts.
4. **Chart:** Add studies, change timeframe, toggle drawings, zoom in/out.
5. **thinkScript Editor:** Code completion, save scripts, run backtests.

**Customization:**
- Every command can be assigned a custom hotkey (Ctrl, Alt, Shift + key).
- Tooltips appear when you press Ctrl/Alt/Shift (showing available shortcuts).

**Common Power User Patterns (from Reddit/YouTube):**
- **Buy/Sell Hotkeys:** Alt+B (buy at ask), Alt+S (sell at bid), Ctrl+F (flatten position).
- **Chart Timeframe:** Ctrl+1 (1min), Ctrl+2 (5min), Ctrl+D (daily), Ctrl+W (weekly).
- **Tab Switching:** Ctrl+M (Monitor), Ctrl+T (Trade), Ctrl+C (Charts), Ctrl+A (Analyze).
- **Symbol Search:** Ctrl+Shift+S opens symbol lookup modal.
- **Add Study:** Ctrl+E opens study picker on current chart.

**What Translates to TUI:**
- ✅ **Everything.** pftui already has excellent vim-style keybindings.
- **Extend hotkey coverage to new features:** scanners (`:scan`), alerts (`:alert`), targets (`:target`), journal (`:journal`).

**What Doesn't Translate:**
- Mouse-required actions (drag trendlines, resize panels).

**Recommendations:**
- **Document all hotkeys in `:help`:** Already done! pftui help overlay is excellent.
- **Add hotkeys for every new feature:**
  - Scanners: `S` opens scan builder.
  - Alerts: `A` creates alert on selected position.
  - Targets: `T` sets allocation target.
  - Journal: `J` opens journal entry modal.
  - Chart timeframe: `<` / `>` cycle timeframes (or `t` / `T`).
  - Benchmark comparison: `B` toggles SPY overlay on chart.
- **Add command palette (vim-style):** `:` opens command mode. Type `:scan add volume > 1M`, `:alert create BTC`, etc. Autocomplete available commands.
- **Add hotkey hints at bottom of screen:** Context-sensitive. E.g., when hovering a position: `[Enter] Detail | [c] Chart | [a] Alert | [t] Target`.

---

## What Doesn't Translate: Mouse-First Features

### Features That Need Graphics or Mouse

**1. Active Trader Ladder (DOM-style order entry):**
- Real-time bid/ask price ladder with click-to-trade. Futures/day traders use this for sub-second order entry.
- **Why it doesn't translate:** Requires live Level 2 market data + instant order execution. pftui is not a trading platform.
- **pftui equivalent:** Not needed. pftui is for portfolio tracking, not active trading.

**2. Drawing Tools (trendlines, Fibonacci retracements):**
- Users draw lines, channels, levels on charts with mouse.
- **Why it doesn't translate:** Mouse-first. TUI charts are braille, low resolution.
- **pftui equivalent:** Not needed. Focus on data analysis, not technical charting.

**3. Drag-and-Drop Panel Layouts:**
- Users resize panels by dragging borders, reorder tabs by dragging headers.
- **Why it doesn't translate:** Mouse-first. TUI equivalent is fixed layouts or config-driven ratios.
- **pftui equivalent:** Use config.toml for layout presets (`layout = "compact" | "split" | "analyst"`).

**4. Backtesting with ThinkBack (Interactive Replay):**
- ThinkBack lets users "replay" historical market data day by day, test trades in hindsight.
- **Why it doesn't translate:** Requires streaming historical data + complex state management. pftui uses cached prices.
- **pftui equivalent:** `pftui history --date 2024-01-01` shows portfolio as of past date (already implemented!). Keep this — it's excellent for "what if" analysis.

**5. Options Greeks Dashboard (Heat Maps, 3D Surfaces):**
- Visual heatmaps of delta/gamma/theta across strike prices.
- **Why it doesn't translate:** Requires 2D color gradients or 3D rendering. TUI can't do this well.
- **pftui equivalent:** Not needed unless pftui adds options support. If added, show Greeks in a table (strike, delta, theta, IV).

**6. Real-Time Streaming Data:**
- ToS gets sub-second price updates, Level 2 quotes, time & sales.
- **Why it doesn't translate:** pftui uses cached prices (refresh via `pftui refresh`). Not a real-time platform.
- **pftui equivalent:** Keep the cached price model. Add auto-refresh timer (optional): every 5 min, fetch new prices in background, flash price changes.

---

## Recommendations for pftui: Actionable Items

**Priority levels:** (P0 = critical, P1 = high value, P2 = nice-to-have, P3 = speculative)

### Layout & Workflow (P1)

1. **Symbol linking:** When user selects a position (j/k nav), update linked views:
   - Price chart (auto-switch symbol)
   - Detail panel (show recent transactions, news, fundamentals)
   - Watchlist highlight (if symbol is in watchlist)
   - **Implementation:** Add `selected_symbol: Option<String>` to App state. Update all views to read it.

2. **Split-pane view:** Add a "detail pane" (bottom or right 30% of screen) that shows chart + transactions + news for selected position.
   - **Hotkey:** `S` toggles split mode.
   - **Config:** `layout = "split"` in config.toml.
   - **Implementation:** Modify `ui.rs` to split Rect into main + detail. Pass selected position to detail renderer.

3. **Workspace presets:** Add named layout configs in config.toml:
   ```toml
   layout = "compact"  # single-pane, no sidebar
   layout = "default"  # sidebar + main view
   layout = "split"    # sidebar + main + detail pane
   layout = "analyst"  # sidebar + chart + news
   ```
   **Implementation:** Load layout in Config, apply in `ui.rs`.

### Charts (P1)

4. **Benchmark comparison chart:** Plot position % change vs SPY (or custom benchmark).
   - **Hotkey:** `B` toggles benchmark overlay.
   - **UI:** Render two braille lines on same chart (different colors: position=green, benchmark=gray).
   - **Implementation:** Fetch SPY history, compute % change series, plot with existing braille renderer.

5. **Persist chart timeframe per position:** When user changes timeframe (`T` key), store in `chart_state` table (symbol, timeframe).
   - **Implementation:** Add `chart_state` table to schema, save/load in App::handle_key.

6. **Add SMA overlay to price charts:** Config option `chart_sma = [20, 50, 200]` renders moving averages.
   - **UI:** Faint braille lines on top of price line.
   - **Implementation:** Compute SMA in price_chart.rs, plot as additional series.

7. **Add volume sub-chart:** Below price chart (3 rows max), render braille bar chart of daily volume.
   - **Hotkey:** `V` toggles volume chart.
   - **Implementation:** Fetch volume from history, render as braille bars below price.

8. **"Chart All Positions" grid view:** Mini braille charts (one per position), 6–9 per screen.
   - **Hotkey:** `8` (new view mode).
   - **Implementation:** New view in `views/chart_grid.rs`. Loop through positions, render mini charts in grid layout.

### Scanner/Screener (P2)

9. **Add `pftui scan` subcommand** with filter DSL:
   ```bash
   pftui scan --filter "allocation_pct > 10" --filter "day_change_pct > 2"
   pftui scan --filter "category = Crypto" --sort-by gain_pct --limit 20
   ```
   **Implementation:** New `commands/scan.rs`. Parse filters, iterate positions, apply predicates.

10. **Interactive scan builder in TUI:** `:scan` opens modal with filter UI.
    - **UI:** Modal with filter list, [A]dd/[R]emove/[S]ave/[L]oad actions.
    - **Implementation:** New modal in `views/scan_builder.rs`. Store active filters in App state.

11. **Saveable scan queries:** Store in `scan_queries` table (id, name, filters_json, created_at).
    - **Commands:** `:scan save my_scan`, `:scan load my_scan`, `:scan list`.
    - **Implementation:** Add `scan_queries` table to schema, CRUD functions in `db/scan_queries.rs`.

12. **Integrate scans with alerts:** `:alert create scan momentum` triggers when scan results change.
    - **Implementation:** Add `alert_type = "scan"` to alerts table. Periodically run scan, compare results, fire alert.

### Watchlist (P2)

13. **Extend watchlist column customization:** Add `watchlist_columns` to config.toml.
    - **Config:** `watchlist.columns = ["symbol", "price", "change_pct", "volume"]`.
    - **Implementation:** Load in Config, pass to watchlist_view.rs.

14. **Add watchlist groups:** Store multiple watchlists in `watchlist_groups` table.
    - **Hotkey:** `W` + number (1/2/3) switches watchlists.
    - **Commands:** `:watch crypto`, `:watch tech`, `:watch list`.
    - **Implementation:** Add `watchlist_groups` table, load/switch in App state.

15. **Link watchlist to main view:** Select a watchlist symbol (Enter) → opens position detail or sets focus.
    - **Implementation:** Add `selected_watchlist_symbol` to App, update main view to read it.

16. **Inline actions in watchlist:** Press `a` to add alert, `c` to view chart, `r` to remove.
    - **Hotkeys:** Add to watchlist key handler in App::handle_key.

### Information Architecture (P2)

17. **Add sub-modes to Positions view:**
    - `G` groups by category
    - `A` sorts by allocation
    - `P` sorts by performance
    - **Implementation:** Add `PositionsSortMode` enum to App, apply in positions view.

18. **Add detail panel (bottom 30%):** When position selected, show:
    - Recent transactions (last 5)
    - Braille price chart (90d)
    - Latest news headline
    - Allocation drift vs target
    - **Hotkey:** `d` toggles detail panel.
    - **Implementation:** Split Rect in ui.rs, render detail in new widget.

19. **Add breadcrumb nav at top:** Show view hierarchy (e.g., `Positions → AAPL → Detail`).
    - **Implementation:** Render in header.rs, read from App state.

### Keyboard Shortcuts (P1)

20. **Add hotkeys for all new features:**
    - `S` — open scan builder
    - `A` — create alert on selected position
    - `T` — set allocation target
    - `J` — open journal entry
    - `<` / `>` — cycle chart timeframes
    - `B` — toggle benchmark overlay
    - **Implementation:** Add cases to App::handle_key.

21. **Add command palette (vim-style):** `:` opens command mode.
    - **Commands:** `:scan`, `:alert`, `:target`, `:chart`, `:watch`.
    - **Implementation:** Add command parser, modal in `views/command_palette.rs`.

22. **Add hotkey hints at bottom:** Context-sensitive action hints.
    - **Example:** When hovering position: `[Enter] Detail | [c] Chart | [a] Alert`.
    - **Implementation:** Render in status_bar.rs, read from App context.

### Data & Caching (P2)

23. **Add auto-refresh timer (optional):** Every 5 min, fetch new prices in background, flash changes.
    - **Config:** `auto_refresh = true`, `refresh_interval_secs = 300`.
    - **Implementation:** Add timer in PriceService, send PriceCommand::RefreshAll periodically.

24. **Persist more state in SQLite:**
    - Chart timeframe per position (`chart_state` table)
    - Scan queries (`scan_queries` table)
    - Watchlist groups (`watchlist_groups` table)
    - Detail panel visibility (`ui_state` table)
    - **Implementation:** Add tables to schema, save/load in relevant modules.

### Discoverability (P3)

25. **Add onboarding tour (first run):** Show key hotkeys, explain views, demo navigation.
    - **Implementation:** Check `~/.config/pftui/onboarding_complete` flag. If missing, show tour modal on startup.

26. **Add tooltips on hover (if mouse enabled):** Show column descriptions, action hints.
    - **Implementation:** Detect mouse events in TUI event loop, render tooltip widget.

---

## Derived TODO Items

**Add these to `/root/pftui/TODO.md`:**

### High Priority (P1)

- [ ] **Symbol linking:** Selected position propagates to chart/detail/watchlist (30 min)
- [ ] **Split-pane view:** Add detail panel (bottom 30%) with chart + transactions + news (1 hr)
- [ ] **Benchmark comparison chart:** Plot position vs SPY with `B` key (45 min)
- [ ] **Persist chart timeframe per position:** Save in `chart_state` table (30 min)
- [ ] **Hotkeys for new features:** `S`=scan, `A`=alert, `T`=target, `J`=journal, `<`/`>`=timeframe (15 min)

### Medium Priority (P2)

- [ ] **Add `pftui scan` subcommand:** Filter DSL (price > X, allocation > Y, etc.) (1 hr)
- [ ] **Interactive scan builder in TUI:** `:scan` modal with filters (1 hr)
- [ ] **Saveable scan queries:** Store in `scan_queries` table (45 min)
- [ ] **Watchlist column customization:** Add to config.toml (30 min)
- [ ] **Watchlist groups:** Multiple named watchlists, switch with `W` key (45 min)
- [ ] **Positions view sub-modes:** `G`=group by category, `A`=sort by allocation, `P`=sort by performance (30 min)
- [ ] **Detail panel widget:** Show recent txs + chart + news for selected position (1 hr)
- [ ] **Add SMA overlay to price charts:** Config option `chart_sma = [20, 50]` (45 min)
- [ ] **Add volume sub-chart:** Below price chart, toggle with `V` key (45 min)
- [ ] **Breadcrumb nav at top:** Show view hierarchy (Positions → AAPL → Detail) (20 min)
- [ ] **Command palette (vim-style):** `:` opens command mode with autocomplete (1 hr)
- [ ] **Context-sensitive hotkey hints at bottom:** Show available actions for current view (30 min)
- [ ] **Auto-refresh timer (optional):** Fetch new prices every 5 min in background (45 min)

### Low Priority (P3)

- [ ] **Workspace presets in config:** `layout = "compact" | "split" | "analyst"` (1 hr)
- [ ] **"Chart All Positions" grid view:** Mini braille charts, 6–9 per screen (1.5 hr)
- [ ] **Link watchlist to main view:** Select watchlist symbol → opens detail (30 min)
- [ ] **Inline watchlist actions:** `a`=alert, `c`=chart, `r`=remove (30 min)
- [ ] **Integrate scans with alerts:** Alert when scan results change (45 min)
- [ ] **Onboarding tour (first run):** Show key hotkeys and demo navigation (1 hr)

---

## Feasibility & Impact Matrix

| Feature | Impact | Feasibility | Priority | Effort |
|---------|--------|-------------|----------|--------|
| Symbol linking | High | High | P1 | 30 min |
| Split-pane view | High | Medium | P1 | 1 hr |
| Benchmark comparison | High | High | P1 | 45 min |
| Scan subcommand | High | High | P2 | 1 hr |
| Watchlist groups | Medium | High | P2 | 45 min |
| Detail panel widget | High | Medium | P2 | 1 hr |
| SMA overlay | Medium | Medium | P2 | 45 min |
| Command palette | Medium | Medium | P2 | 1 hr |
| Chart grid view | Low | Medium | P3 | 1.5 hr |
| Workspace presets | Low | High | P3 | 1 hr |

**Recommended first 5 tasks (quick wins with high impact):**
1. Symbol linking (30 min, P1)
2. Hotkeys for new features (15 min, P1)
3. Benchmark comparison chart (45 min, P1)
4. Persist chart timeframe (30 min, P1)
5. Scan subcommand (1 hr, P2)

**Total effort for P1 items:** ~4 hours  
**Total effort for P2 items:** ~8 hours  
**Total effort for P3 items:** ~5 hours

---

## Conclusion

thinkorswim's UX excellence comes from:
1. **Extreme customization** — users configure everything
2. **Linked views** — symbol selection propagates instantly
3. **Keyboard-first workflows** — power users never touch the mouse
4. **Filter-based discovery** — scanners use composable predicates, not keyword search
5. **Persistent state** — layouts, queries, alerts all saved

**pftui already nails #3 (vim-style nav).** The biggest wins will come from adding #1 (more customization), #2 (symbol linking), and #4 (scanners).

The key insight: **ToS doesn't try to be simple**. It's a power tool for professionals who invest time learning it. pftui should embrace this philosophy — prioritize **depth** over **simplicity**. Add features that unlock power-user workflows, even if they require reading docs.

The terminal is the perfect medium for this. Users who seek out a TUI portfolio tracker are already power users. Give them the tools to build their ideal workspace.
