# pftui Analytics & Intelligence — Feature Specification

> This document defines the analytics and intelligence features for pftui, covering financial engineering, TUI UX design, CLI output format, and agent-optimized output. Each feature is scoped as a distinct TODO item.

## Design Principles

1. **No command sprawl.** Group related analytics under existing commands/tabs. New CLI commands only when the domain is genuinely distinct.
2. **TUI-first.** Every analytic should have a home in the TUI. CLI commands are supplementary — for agents and scripting.
3. **Financial expert UX.** Think Bloomberg Terminal, thinkorswim, Koyfin. Data-dense, scannable, color-coded by signal strength. No dashboards-for-dashboards-sake.
4. **Agent-optimized output.** Every CLI command supports `--json` for structured agent consumption. The `brief` command becomes the single agent entry point with expandable sections.
5. **Progressive disclosure.** Summary view shows the signal. Detail view shows the math. Popup shows the full analysis.

---

## Current TUI Architecture

```
[1] Positions  — Portfolio holdings, allocation bars, price chart, regime bar
[2] Transactions — Transaction history
[3] Markets   — Broad market data, heatmap tinting, sparklines
[4] Economy   — Economic indicators, yield curve status
[5] Watchlist  — Tracked non-held assets
```

## Proposed TUI Architecture (after analytics)

```
[1] Positions   — Holdings + per-position technicals + risk overlay
[2] Transactions — History (unchanged)
[3] Markets     — Broad market + correlations + sector performance
[4] Economy     — Macro indicators + FRED data + central bank tracker
[5] Watchlist   — Tracked assets + entry level proximity
[6] Analytics   — Portfolio-level analysis: risk, scenarios, signals, alerts
```

Key change: **one new tab [6] Analytics** absorbs portfolio-level analysis. Everything else integrates into existing tabs via sub-views or enhanced panels.

---

## Feature Specifications

### F1: Technical Indicators Engine

**What:** RSI(14), MACD (12/26/9), SMA 50/200, Bollinger Bands(20,2) computed from cached price history.

**Financial engineering:**
- RSI: Wilder's smoothed RS, 14-period. Display as 0-100 with zones: <30 oversold (green), >70 overbought (red), 30-70 neutral.
- MACD: 12-period EMA minus 26-period EMA. Signal line = 9-period EMA of MACD. Histogram = MACD - Signal. Bullish crossover = MACD crosses above signal.
- SMA: Simple moving average. 50-day (medium trend) and 200-day (long trend). Golden cross (50 > 200) vs death cross (50 < 200).
- Bollinger Bands: 20-day SMA ± 2 standard deviations. Width indicates volatility. Price at upper band = overbought, lower = oversold.

**TUI integration:**
- **Positions tab [1]:** New column or indicator strip on each position row. Compact: `RSI 45 ▲` or emoji-coded: 🟢/🟡/🔴. Click/Enter on a position → detail popup already exists → add "Technicals" section showing full RSI/MACD/SMA/BB with mini sparkline.
- **Watchlist tab [5]:** Same indicator strip per watched asset. Helps identify entry setups.
- **Asset detail popup:** Full technicals panel with: RSI gauge, MACD histogram (braille), price vs SMA50/200 relationship, BB width.

**CLI integration:**
- `pftui brief` always includes technicals section with RSI/MACD/SMA for each position (no flag needed).
- `pftui summary` does not include technicals (use brief for that).
- No new standalone command — technicals are a property of assets, not a separate domain.

**Agent output (`--json`):**
```json
{
  "symbol": "GC=F",
  "technicals": {
    "rsi_14": 54.8,
    "rsi_signal": "neutral",
    "macd": 12.5,
    "macd_signal": 10.2,
    "macd_histogram": 2.3,
    "macd_crossover": "bullish",
    "sma_50": 4798.00,
    "sma_200": 4200.00,
    "price_vs_sma50": "above",
    "golden_cross": true,
    "bb_upper": 5300.00,
    "bb_lower": 4900.00,
    "bb_width_pct": 8.2
  }
}
```

**Rust implementation:** New `src/indicators/` module with `rsi.rs`, `macd.rs`, `sma.rs`, `bollinger.rs`. Pure math on `Vec<f64>` price series from `price_history` DB table. No external dependencies needed.

**Files:** new `src/indicators/{mod,rsi,macd,sma,bollinger}.rs`, modify `src/tui/views/positions.rs`, `src/tui/views/watchlist.rs`, `src/tui/views/asset_detail_popup.rs`, `src/commands/brief.rs`

**Effort:** Medium (2-3 sessions). Math is straightforward. UI integration is the bulk.

---

### F2: Correlation Matrix

**What:** Rolling 30-day Pearson correlation between all held + watched assets. Identifies diversification, crowded trades, and correlation breaks.

**Financial engineering:**
- Pearson correlation on daily returns (not prices). Returns = (P_today - P_yesterday) / P_yesterday.
- Rolling 30-day window. Optionally 7-day and 90-day for short/long comparison.
- Correlation ranges: -1 (perfect inverse) to +1 (perfect correlation). |r| > 0.7 = strong, 0.3-0.7 = moderate, <0.3 = weak.
- **Correlation break detection:** Compare 30-day correlation to 90-day. If |Δ| > 0.3, flag as "correlation break" — these are regime change signals.

**TUI integration:**
- **Markets tab [3]:** New sub-panel or toggleable view. Grid/matrix layout. Cells color-coded: deep green = strong positive, deep red = strong negative, yellow = near zero. Diagonal = 1.0 (self). Only show held assets + key macro (DXY, VIX) to keep matrix readable (max ~8-10 assets).
- **Analytics tab [6]:** Full expanded correlation matrix with all assets. Toggle between 7d/30d/90d windows.

**CLI integration:**
- `pftui brief --correlations` adds a correlation summary section.
- No standalone command — correlations are a market-level analytic, lives in `brief`.

**Agent output:**
```json
{
  "correlations": {
    "window_days": 30,
    "pairs": [
      {"a": "GC=F", "b": "SI=F", "r": 0.85, "signal": "strong_positive"},
      {"a": "GC=F", "b": "BTC", "r": -0.12, "signal": "uncorrelated"},
      {"a": "BTC", "b": "^GSPC", "r": 0.67, "signal": "moderate_positive"}
    ],
    "breaks": [
      {"a": "GC=F", "b": "BTC", "r_30d": -0.12, "r_90d": 0.45, "delta": -0.57, "signal": "BREAK"}
    ]
  }
}
```

**Files:** new `src/indicators/correlation.rs`, modify `src/tui/views/markets.rs`, new sub-view `src/tui/views/correlation_grid.rs`, `src/commands/brief.rs`

**Effort:** Medium (1-2 sessions). Math is simple. Grid rendering is the main work.

---

### F3: Macro Dashboard (Economy Tab Enhancement)

**What:** Expand Economy tab [4] into a full macro intelligence dashboard. Add DXY, VIX, 10Y yield, fed funds rate, CPI, PPI, gold/silver ratio, oil. Cache via FRED API (free, no key needed for basic series) and Yahoo Finance.

**Financial engineering:**
- **FRED series:** DGS10 (10Y yield), FEDFUNDS (fed funds rate), CPIAUCSL (CPI), PPIFIS (PPI Final Demand), UNRATE (unemployment), T10Y2Y (yield curve spread), VIXCLS (VIX — also available from Yahoo)
- **Yahoo series:** DX-Y.NYB (DXY), ^VIX, CL=F (oil WTI), HG=F (copper)
- **Derived:** Gold/silver ratio = GC=F / SI=F. Real rate = 10Y yield - CPI YoY. Yield curve = 10Y - 2Y.
- **Update frequency:** FRED data is daily/monthly depending on series. Cache aggressively — most macro data doesn't change intraday. `pftui refresh` fetches these alongside asset prices.

**TUI integration:**
- **Economy tab [4]:** Currently has basic indicators. Enhance with:
  - **Top strip:** Key numbers in a scannable row: `DXY 99.27 ↑ | VIX 25.49 ⚠️ | 10Y 4.07% ↑ | FFR 3.50-3.75 | CPI 3.0% | PPI ATH`
  - **Yield curve visualization:** Braille/block chart showing 2Y/5Y/10Y/30Y curve shape. Color: green = normal, yellow = flat, red = inverted.
  - **Macro trends panel:** Each indicator with 30-day sparkline + direction arrow + context. E.g., "VIX 25.49 ↑18.9% — above fear threshold (25)"
  - **Central bank section** (see F5)
  - **Gold/silver ratio:** Current vs historical average. "62:1 (hist avg 60:1) — near fair value" vs "93:1 — silver historically cheap"

**CLI integration:**
- `pftui macro` — single new command that outputs the macro dashboard in terminal-friendly format. This is the ONE new command that's justified — "macro" is a genuinely distinct domain from portfolio tracking.
- Also feeds into `pftui brief` automatically.

**Agent output:**
```json
{
  "macro": {
    "dxy": {"value": 99.27, "change_1d": 0.9, "trend": "rising"},
    "vix": {"value": 25.49, "change_1d": 18.9, "alert": true, "threshold": 25},
    "yield_10y": {"value": 4.07, "change_1d": 0.02},
    "fed_funds": {"value": 3.625, "range": "3.50-3.75"},
    "cpi_yoy": {"value": 3.0},
    "ppi": {"value": 152.17, "note": "ATH"},
    "gold_silver_ratio": {"value": 61.5, "hist_avg": 60},
    "real_rate": {"value": 1.07},
    "yield_curve_2s10s": {"value": 0.15, "status": "flat"}
  }
}
```

**Files:** new `src/data/fred.rs`, new `src/db/economic_cache.rs`, modify `src/tui/views/economy.rs`, new `src/commands/macro_cmd.rs`, `src/commands/brief.rs`, `src/price/mod.rs` (add macro symbols to refresh)

**Effort:** Large (3-4 sessions). FRED API integration, new DB schema, rich TUI rendering.

---

### F4: Portfolio Risk & Scenario Engine

**What:** Portfolio-level risk metrics and "what-if" scenario modeling with cascading asset impacts.

**Financial engineering:**
- **Volatility:** Annualized standard deviation of daily portfolio returns. Per-asset and portfolio-level.
- **Max drawdown:** Largest peak-to-trough decline in portfolio value from cached history.
- **Sharpe ratio:** (Portfolio return - risk-free rate) / portfolio volatility. Use fed funds as risk-free proxy.
- **Value at Risk (VaR):** "In a 95th percentile event, you could lose $X in a day." Historical VaR from return distribution.
- **Concentration risk:** Herfindahl index on allocation. Flag if any single asset >30% or top 3 >70%.
- **Scenario engine:** Define macro scenarios with per-asset impact multipliers. E.g., "Oil $100" → gold +5%, silver +3%, BTC -8%, equities -5%. Apply to current portfolio to show projected value. Extends existing `--what-if` (which does single-asset price change) to multi-asset macro scenarios.
- **Stress test presets:** "2008 GFC", "2020 COVID", "1973 Oil Crisis", "Iran Escalation" — historical drawdowns applied to current portfolio.

**TUI integration:**
- **Analytics tab [6] (new):** This is the home for portfolio-level analysis.
  - **Risk panel:** Volatility gauge, max drawdown, Sharpe, VaR — color-coded (green/yellow/red).
  - **Concentration chart:** Visual bar showing top holdings as % of portfolio with threshold lines.
  - **Scenario selector:** List of preset + custom scenarios. Select one → shows projected portfolio value, per-asset impact, total P&L. Interactive: tweak parameters.
- **Positions tab [1]:** Add volatility column (small σ number per position).

**CLI integration:**
- Extend existing `--what-if` flag to accept scenario names: `pftui summary --what-if "oil-100"` or `pftui summary --what-if "gold:-10%,btc:-20%"`.
- Risk metrics added to `pftui brief` by default (1-line risk summary).

**Agent output:**
```json
{
  "risk": {
    "portfolio_volatility_ann": 12.5,
    "max_drawdown_pct": -8.2,
    "sharpe_ratio": 0.85,
    "var_95_daily_usd": 4200,
    "concentration_hhi": 0.18,
    "concentration_flag": "moderate — gold at 25%"
  },
  "scenario": {
    "name": "Oil $100",
    "projected_value": 358000,
    "change_usd": -4700,
    "change_pct": -1.3,
    "per_asset": [
      {"symbol": "GC=F", "impact_pct": 5.0, "projected": 95800},
      {"symbol": "BTC", "impact_pct": -8.0, "projected": 62700}
    ]
  }
}
```

**Files:** new `src/analytics/{mod,risk,scenarios,stress_test}.rs`, new `src/tui/views/analytics.rs`, modify `src/commands/summary.rs` (what-if extension), `src/commands/brief.rs`

**Effort:** Large (3-4 sessions). Scenario engine is the complex part — need curated impact matrices.

---

### F5: Central Bank & Sovereign Holdings Tracker

**What:** Track central bank gold purchases, sovereign BTC holdings, and institutional flows. The intelligence layer that makes pftui unique.

**Financial engineering:**
- **Gold:** Monthly data from WGC/IMF. Top 10 holders by tonnes, % of reserves. Monthly net purchases. Track: China streak, Poland accumulation, repatriation news.
- **BTC:** Government holdings from bitcointreasuries.net. Corporate holdings (Strategy, MARA, Tesla). ETF AUM. Updated weekly or on-demand.
- **Silver:** COMEX registered/eligible inventory. LBMA vault totals. ETF holdings (SLV, PSLV). Supply deficit tracking.
- **The $5,790 crossover tracker:** Calculate current gold reserves value vs USD reserves value. Show countdown to crossover.

**Data sources:**
- WGC gold data: scrape quarterly from gold.org or use cached static data updated monthly
- bitcointreasuries.net: JSON API (public)
- COMEX: CME Group data, scrapeable
- Manual/curated updates for slower-moving data (CB purchases are monthly)

**TUI integration:**
- **Economy tab [4]:** New "Sovereign Holdings" sub-section.
  - **Gold:** "Central Banks: 36,521t total. 2026 YTD: +Xt. China: 15th month. Top buyer: Poland (+102t)." Mini bar chart of top holders.
  - **BTC:** "Governments: 646,681 BTC (3.08%). Strategy: 720,737 BTC. ETFs: 1.1M BTC." Mini bar chart.
  - **Silver:** "COMEX registered: 92.9M oz. Coverage: 13.9%. Deficit: ~230M oz/yr (year 6)."
  - **Crossover tracker:** Progress bar → "$5,790/oz for gold to surpass USD reserves. Currently $5,090 (88%)"

**CLI integration:**
- `pftui macro --sovereign` or included in default `pftui macro` output.
- Feeds into `pftui brief` as a "Structural Flows" section.

**Agent output:**
```json
{
  "sovereign": {
    "gold_cb_total_tonnes": 36521,
    "gold_ytd_purchases_tonnes": 120,
    "china_streak_months": 15,
    "gold_crossover_price": 5790,
    "gold_crossover_pct": 88,
    "btc_government_total": 646681,
    "btc_corporate_total": 920000,
    "btc_etf_total": 1100000,
    "silver_comex_registered_moz": 92.9,
    "silver_comex_coverage_pct": 13.9,
    "silver_deficit_moz": 230
  }
}
```

**Files:** new `src/data/{sovereign,comex,wgc}.rs`, new `src/db/sovereign_cache.rs`, modify `src/tui/views/economy.rs`, `src/commands/macro_cmd.rs`

**Effort:** Large (3-4 sessions). Data sourcing and caching is the main complexity. Some data will need manual/curated updates.

---

### F6: Alert & Threshold Engine

**What:** Define price/indicator thresholds. Check on every refresh. Surface breaches in TUI + CLI + agent output.

**Financial engineering:**
- **Price alerts:** `GC=F above 5500`, `BTC below 55000`, `GBPUSD below 1.30`
- **Indicator alerts:** `VIX above 25`, `DXY above 100`, `RSI(GC=F) below 30`
- **Portfolio alerts:** `gold_allocation above 30%`, `total_value below 350000`
- **Compound alerts:** `VIX above 25 AND DXY above 100` (risk-off confirmation)
- **Relative alerts:** `gold_silver_ratio above 80` (silver cheap)

**TUI integration:**
- **Status bar (bottom):** Alert count badge: `⚠️ 2 alerts`. Click/hotkey to expand.
- **Alerts popup:** Overlay showing all active alerts with status (armed/triggered/acknowledged).
- **Per-asset indicators:** In Positions/Watchlist tabs, assets with triggered alerts get a ⚠️ icon.
- **Analytics tab [6]:** Full alerts management panel. Add/remove/edit thresholds.

**CLI integration:**
- `pftui alerts` — shows all thresholds and current status (new command, justified as distinct domain).
- `pftui alerts add "VIX above 25"` — add threshold.
- `pftui brief` includes triggered alerts section by default.
- `pftui refresh` checks thresholds after price update and reports any newly triggered.

**Agent output:**
```json
{
  "alerts": {
    "triggered": [
      {"rule": "VIX above 25", "current": 25.49, "threshold": 25, "triggered_at": "2026-03-03"},
      {"rule": "GBP/USD below 1.33", "current": 1.330, "threshold": 1.33, "triggered_at": "2026-03-03"}
    ],
    "armed": [
      {"rule": "BTC below 55000", "current": 68938, "distance_pct": 20.2},
      {"rule": "DXY above 100", "current": 99.27, "distance_pct": 0.7}
    ]
  }
}
```

**Files:** new `src/alerts/{mod,engine,rules}.rs`, new `src/db/alerts.rs`, modify `src/tui/widgets/status_bar.rs`, new `src/tui/views/alerts_popup.rs`, new `src/commands/alerts.rs`, modify `src/commands/refresh.rs`

**Effort:** Medium (2-3 sessions). Rule parser + DB + checking engine. TUI integration is incremental.

---

### F7: Enhanced Agent Output (`brief --agent`)

**What:** Single CLI entry point for all agent-consumable data. Token-efficient, structured, comprehensive.

**Financial engineering:** N/A — this is output formatting.

**Design:**
- `pftui brief --agent` outputs a single JSON blob containing ALL available intelligence: positions, prices, technicals, macro, correlations, risk, alerts, sovereign holdings, regime status.
- Structured for LLM consumption: flat where possible, pre-computed signals, human-readable labels alongside numeric values.
- Replaces the need for agents to run multiple commands.
- Optional sections: `pftui brief --agent --sections positions,macro,alerts` for targeted, smaller output.

**This replaces `fetch_prices.py` entirely once macro indicators are in pftui.**

**Files:** modify `src/commands/brief.rs`

**Effort:** Small (1 session) once F1-F6 data layers exist. This is the aggregation layer.

---

## CLI Command Summary (Final)

**Existing (no changes):**
- `pftui` (TUI launch)
- `pftui summary` / `pftui brief` / `pftui value`
- `pftui refresh` / `pftui watchlist` / `pftui history`
- `pftui add-tx` / `pftui remove-tx` / `pftui list-tx` / `pftui set-cash`
- `pftui export` / `pftui import` / `pftui snapshot` / `pftui demo`
- `pftui watch` / `pftui unwatch` / `pftui setup`

**Enhanced (flags added):**
- `pftui brief --correlations --agent` (technicals always included)
- `pftui summary --what-if "scenario-name"`
- `pftui refresh` (now also fetches macro + checks alerts)

**New (3 commands, each for a genuinely distinct domain):**
- `pftui macro` — macro dashboard (DXY, VIX, yields, CB data, sovereign holdings)
- `pftui alerts` / `pftui alerts add "..."` — threshold management
- `pftui risk` — portfolio risk metrics (optional, could live under `summary --risk`)

That's 3 new commands max. Could be 2 if `risk` merges into `summary`.

---

## Implementation Priority

| Priority | Feature | Impact on Crons | Impact on Human UX | Effort | Dependencies |
|----------|---------|----------------|-------------------|--------|--------------|
| **P1** | **F1: Technical Indicators** | HIGH — replaces fetch_prices.py | HIGH — per-asset signals | Medium | Price history cache (exists) |
| **P1** | **F3: Macro Dashboard** | HIGH — replaces fetch_prices.py entirely | HIGH — full macro view | Large | FRED API integration |
| **P1** | **F6: Alert Engine** | HIGH — automates threshold checking | HIGH — proactive notifications | Medium | F1, F3 for indicator alerts |
| **P2** | **F7: Agent Brief** | CRITICAL — single agent entry point | Low | Small | F1, F3, F6 |
| **P2** | **F2: Correlation Matrix** | Medium — new analytical capability | HIGH — portfolio insight | Medium | Price history cache |
| **P2** | **F4: Risk & Scenarios** | Medium — scenario modeling | HIGH — stress testing | Large | F1, price history |
| **P3** | **F5: Sovereign Holdings** | Medium — structural intelligence | HIGH — unique differentiator | Large | Data sourcing |

**Phase 1 (immediate):** F1 + F3 + F6 → pftui becomes the sole data source for all crons, replacing `fetch_prices.py` entirely. Human gets technicals, macro dashboard, and alerts in TUI.

**Phase 2 (next week):** F7 + F2 + F4 → Agent integration is complete. Human gets correlation analysis and risk modeling.

**Phase 3 (following week):** F5 → Intelligence layer. The moat. No other TUI does this.

---

### F8: Journal & Decision Log

**What:** Structured trade journal stored in SQLite. Hotkey-triggered overlay in TUI. Full CLI command suite for agents to seed, query, and search entries.

**Design philosophy:** The TUI keeps it minimal — a popup overlay, not a main tab. The CLI is the power interface for agents and scripting.

**SQLite schema:**
```sql
CREATE TABLE journal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,           -- ISO 8601 (default: now)
    content TEXT NOT NULL,             -- free-form entry text
    tag TEXT,                          -- category: trade, thesis, prediction, reflection, alert, lesson, call
    symbol TEXT,                       -- optional asset link (GC=F, BTC, etc.)
    conviction TEXT,                   -- high, medium, low (optional)
    status TEXT DEFAULT 'open',        -- open, validated, invalidated, closed
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE INDEX idx_journal_timestamp ON journal(timestamp);
CREATE INDEX idx_journal_tag ON journal(tag);
CREATE INDEX idx_journal_symbol ON journal(symbol);
CREATE INDEX idx_journal_status ON journal(status);
```

**TUI integration:**
- **Tab [7] Journal** — full tab in the numbered menu: `[1] Positions [2] Transactions [3] Markets [4] Economy [5] Watchlist [6] Analytics [7] Journal`
- **Journal view layout:**
  ```
  ┌─ [7] JOURNAL ──────────────────────────────────────────────────┐
  │ Date              │ Content                           │ Tag    │
  │───────────────────│───────────────────────────────────│────────│
  │ 2026-03-03 11:30  │ Added gold — BRICS thesis, CB...  │ trade  │
  │ 2026-03-03 08:00  │ VIX breached 25 — fear confirmed  │ alert  │
  │ 2026-03-01 04:46  │ Iran war — 10 predictions logged  │ call   │
  │ 2026-02-27 15:00  │ BTC conviction resolved +3 long   │ thesis │
  │ ...               │                                   │        │
  └─────────────────────────────────────────────────────────────────┘
  [a] Add entry  [/] Search  [↑↓] Scroll  [Enter] Expand
  ```
- **`a` key (in Journal tab)** — inline prompt: date (pre-filled with now, editable), content (free text), tag (optional, tab-complete from existing tags). Simple and fast.
- **Scrollable.** Most recent first. Date and tag columns are compact. Content column takes remaining width, truncated with `...` — Enter on a row to see full text in a detail popup.
- **Search within Journal tab:** `/` opens search overlay filtered to journal entries.

**CLI command suite — `pftui journal`:**

```
pftui journal                          # List recent entries (last 20)
pftui journal add "content text"       # Add entry (timestamp = now)
pftui journal add "content" --date "2026-03-01 04:46" --tag prediction --symbol BTC --conviction high
pftui journal list                     # All entries (paginated)
pftui journal list --since 7d          # Last 7 days
pftui journal list --since 2026-02-24  # Since specific date
pftui journal list --tag call          # Filter by tag
pftui journal list --tag call,prediction  # Multiple tags
pftui journal list --symbol GC=F       # Filter by asset
pftui journal list --status open       # Filter by status
pftui journal list --limit 50          # Control count
pftui journal search "gold thesis"     # Full-text search across all content
pftui journal search "BRICS" --since 30d  # Search with time filter
pftui journal update <id> --status validated  # Update status
pftui journal update <id> --content "revised text"  # Edit content
pftui journal remove <id>              # Delete entry
pftui journal tags                     # List all tags with counts
pftui journal stats                    # Summary: total entries, entries by tag, entries by month
```

**All commands support `--json` for agent consumption.**

**Agent output example (`--json`):**
```json
{
  "entries": [
    {
      "id": 1,
      "timestamp": "2026-03-03T11:30:00Z",
      "content": "Added gold — BRICS thesis, central bank buying, $5,150 entry",
      "tag": "trade",
      "symbol": "GC=F",
      "conviction": "high",
      "status": "open"
    }
  ],
  "total": 1,
  "query": {"since": "7d", "tag": "trade"}
}
```

**Agent integration — replaces JOURNAL.md sections:**
| JOURNAL.md Section | Replacement |
|---|---|
| Open calls | `pftui journal list --tag call --status open --json` |
| Trade tracker notes | Transaction notes + `journal list --tag trade --json` |
| Big moves log | `pftui journal list --tag move --since 1d --json` |
| Predictions | `pftui journal list --tag prediction --status open --json` |
| Lessons learned | `pftui journal list --tag lesson --json` |
| Reflections | `pftui journal list --tag reflection --since 7d --json` |
| Hypotheses | `pftui journal search "hypothesis" --status open --json` |

**Seeding from existing JOURNAL.md:**
A one-time migration script parses JOURNAL.md and creates entries with correct timestamps, tags, and statuses. This populates the DB with 2 weeks of history immediately.

**Files:** new `src/db/journal.rs`, new `src/commands/journal.rs`, new `src/tui/views/journal_popup.rs`, `src/app.rs` (add `j` hotkey), `cli.rs`

**Effort:** Medium (2 sessions). DB schema + CLI is session 1. TUI popup is session 2.

---

### F9/F11: Absorbed into F6 (Unified Alert Engine)

Watchlist entry levels (F9) and allocation drift detection (F11) are now part of F6. One alert system handles price alerts, allocation drift, and indicator thresholds. See F6 spec above.

**Unified alert types:**
- Price: `"GC=F above 5500"`, `"BTC below 55000"`
- Allocation: `"gold allocation above 30%"`, `"cash allocation below 30%"`
- Indicator: `"VIX above 25"`, `"GC=F RSI below 30"`
- Watchlist entry: `pftui watch TSLA --target 300 --direction below`

**Three-phase rollout:**
1. CLI check only (`pftui alerts check`) — no daemon, battle-tested first
2. Refresh hook (`pftui refresh` auto-checks) — still no daemon
3. Optional OS notifications (`pftui refresh --notify`) — `notify-send` / `osascript`, no daemon

**Crons should NOT rely on pftui alerts until battle-tested and bug-free.** Parallel operation with existing threshold checking, then cut over.

---

### F10: Portfolio Performance History

**What:** Automated daily portfolio snapshots → time-series returns → benchmark comparison.

**Financial engineering:**
- Daily snapshot: total value + per-position values, stored on every `refresh`
- Returns: simple return = (V_end - V_start) / V_start for any period
- Annualized return: (1 + total_return)^(365/days) - 1
- Benchmark comparison: fetch SPY (or custom) daily, compute same returns, show delta
- Time-weighted return (TWR) for accuracy when cash flows exist (adds/withdrawals)

**TUI integration:**
- **Positions tab [1]:** Compact return row in portfolio overview panel: `1D: -0.5% | 1W: +1.2% | 1M: +3.4% | YTD: +5.1%`
- **Existing 3M chart:** Fixed — now has actual data from daily snapshots

**CLI:**
```
pftui performance                     # MTD, QTD, YTD, inception
pftui performance --since 2026-02-24  # since rebalance
pftui performance --period weekly     # weekly return series
pftui performance --vs SPY            # benchmark comparison
pftui performance --json              # agent output
```

**Agent output:**
```json
{
  "performance": {
    "1d": -0.5, "1w": 1.2, "1m": 3.4, "ytd": 5.1,
    "since_inception": 8.3, "inception_date": "2026-02-20",
    "benchmark": {"symbol": "SPY", "1m": 2.1, "delta": 1.3}
  }
}
```

**Files:** new `src/db/snapshots.rs`, new `src/commands/performance.rs`, modify `commands/refresh.rs`, `tui/views/positions.rs`, `tui/widgets/portfolio_stats.rs`

**Effort:** Small-Medium (1-2 sessions). Snapshot storage is trivial. TWR calculation is the complexity.

---

### F12: Economic Calendar

**What:** Upcoming market-moving events integrated into Economy tab [4]. Replaces agent web searches for "what's happening this week."

**Data sources (free):**
- Finnhub free tier: economic calendar + earnings calendar API
- Trading Economics free: limited but covers major events
- Forex Factory: RSS feed, scrapeable
- Yahoo Finance: earnings dates per-symbol (already have Yahoo integration)

**Financial engineering:** N/A — this is data aggregation and display.

**TUI integration:**
- **Economy tab [4]:** Right-side panel (or toggleable sub-view if screen too narrow):
  ```
  ┌─ UPCOMING EVENTS ─────────────────────────────┐
  │ Mar 4  🔴 JOLTS Job Openings     prev: 7.6M   │
  │ Mar 5  🟡 ADP Employment         prev: 183K   │
  │ Mar 7  🔴 Non-Farm Payrolls      prev: 143K   │
  │ Mar 7  🔴 Unemployment Rate      prev: 4.0%   │
  │ Mar 12 🔴 CPI YoY               prev: 3.0%   │
  │ Mar 19 🔴 FOMC Rate Decision     prev: 3.50%  │
  │                                                │
  │ EARNINGS (watchlist)                           │
  │ Mar 6  COIN Q4 2025                            │
  │ Mar 12 HOOD Q4 2025                            │
  └────────────────────────────────────────────────┘
  ```
- Impact icons: 🔴 high, 🟡 medium, ⚪ low
- Countdown: events within 24h highlighted
- Earnings for watchlist stocks auto-included

**CLI:**
```
pftui calendar                    # next 7 days
pftui calendar --days 30          # next month
pftui calendar --impact high      # high-impact only
pftui calendar --json             # agent output
```

**Agent output:**
```json
{
  "events": [
    {"date": "2026-03-07", "name": "Non-Farm Payrolls", "impact": "high", "previous": "143K", "forecast": "160K"},
    {"date": "2026-03-19", "name": "FOMC Rate Decision", "impact": "high", "previous": "3.50%"}
  ],
  "earnings": [
    {"date": "2026-03-06", "symbol": "COIN", "name": "Coinbase Q4 2025"}
  ]
}
```

**Files:** new `src/data/calendar.rs`, new `src/db/calendar_cache.rs`, modify `tui/views/economy.rs`, new `src/commands/calendar.rs`, `cli.rs`

**Effort:** Medium (2 sessions). API integration + caching is session 1. TUI panel is session 2.

---

### F13: Position Annotations & Thesis Tracking

**What:** Structured per-position metadata: entry thesis, invalidation criteria, review dates, target levels. Agents query this instead of JOURNAL.md open calls.

**SQLite schema:**
```sql
CREATE TABLE annotations (
    symbol TEXT PRIMARY KEY,
    thesis TEXT,                    -- why you bought
    invalidation TEXT,             -- what would make you sell
    review_date TEXT,              -- when to re-evaluate
    target_add REAL,               -- price to add more
    target_sell REAL,              -- price to take profit
    conviction TEXT,               -- high/medium/low
    updated_at TEXT DEFAULT (datetime('now'))
);
```

**TUI integration:**
- **Position detail popup (exists):** New "Thesis" section below price chart:
  ```
  ─ THESIS ──────────────────────────────────────
  Entry: BRICS + CB buying + stagflation hedge
  Invalidate: Real rates go positive, gold <$4,500
  Review: 2026-03-20 (17 days) ⏰
  Add target: $4,800 (5.7% below)
  Sell target: $6,000 (17.8% above)
  Conviction: HIGH
  ```
- **Positions tab [1]:** ⏰ icon on positions with overdue review dates
- **Watchlist tab [5]:** Shows target_add with proximity (from F6 integration)

**CLI:**
```
pftui annotate GC=F --thesis "BRICS + CB buying"
pftui annotate GC=F --invalidate "Real rates positive, gold <4500"
pftui annotate GC=F --review-date 2026-03-20
pftui annotate GC=F --target-add 4800 --target-sell 6000
pftui annotate GC=F                   # show current annotation
pftui annotate GC=F --json            # agent output
pftui annotate --all --json           # all annotations for agents
pftui annotate --overdue              # positions past review date
```

**Agent output:**
```json
{
  "symbol": "GC=F",
  "thesis": "BRICS + CB buying + stagflation hedge",
  "invalidation": "Real rates go positive, gold <$4,500",
  "review_date": "2026-03-20",
  "review_overdue": false,
  "review_days_remaining": 17,
  "target_add": 4800,
  "target_add_distance_pct": -5.7,
  "target_sell": 6000,
  "target_sell_distance_pct": 17.8,
  "conviction": "high"
}
```

**Files:** new `src/db/annotations.rs`, new `src/commands/annotate.rs`, modify `tui/views/asset_detail_popup.rs`, `tui/views/positions.rs`, `cli.rs`

**Effort:** Small (1 session). Simple DB + CLI + popup section.

---

### F14: Tag-Based Asset Groups

**What:** Group assets by theme. Track combined allocation and performance per group.

**SQLite schema:**
```sql
CREATE TABLE groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,         -- "hard-assets", "inflation-hedges"
    symbols TEXT NOT NULL,             -- comma-separated: "GC=F,SI=F,BTC"
    created_at TEXT DEFAULT (datetime('now'))
);
```

**TUI integration:**
- **Positions tab [1]:** Filter dropdown or hotkey to filter by group. Allocation bars update to show group-level allocation.

**CLI:**
```
pftui group create "hard-assets" --symbols GC=F,SI=F,BTC
pftui group create "war-beneficiaries" --symbols GC=F,SI=F,XLE,CL=F
pftui group list
pftui group "hard-assets"              # combined allocation + performance
pftui group "hard-assets" --json       # agent output
pftui group remove "hard-assets"
```

**Agent output:**
```json
{
  "name": "hard-assets",
  "symbols": ["GC=F", "SI=F", "BTC"],
  "combined_allocation_pct": 50.2,
  "combined_value": 181500,
  "combined_1d_change_pct": -1.8
}
```

**Files:** new `src/db/groups.rs`, new `src/commands/group.rs`, modify `tui/views/positions.rs`, `tui/widgets/allocation_bars.rs`, `cli.rs`

**Effort:** Small (1 session). Simple CRUD + filter.
