# Analytics Engine — Architecture & Roadmap

## Overview

pftui's Analytics Engine is a multi-timeframe intelligence system that processes market data across four distinct time horizons. Each layer operates on different data sources, different update frequencies, and produces different kinds of actionable signals.

This is pftui's core differentiator: no other retail tool offers structured multi-timeframe analysis where each layer feeds into the layers above and below it.

```
┌─────────────────────────────────────────────────────────────────┐
│                    ANALYTICS ENGINE                             │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  MACRO (years → decades)                                  │  │
│  │  Empire cycles, reserve currency, power transitions       │  │
│  │  Updated: weekly │ Signal: structural allocation bias     │  │
│  ├───────────────────────────────────────────────────────────┤  │
│  │  HIGH (months → years)                                    │  │
│  │  AI disruption, political regime, tech adoption curves    │  │
│  │  Updated: weekly │ Signal: sector rotation, theme shifts  │  │
│  ├───────────────────────────────────────────────────────────┤  │
│  │  MEDIUM (weeks → months)                                  │  │
│  │  Economic data, war, Fed policy, earnings, macro events   │  │
│  │  Updated: daily │ Signal: scenario probabilities, thesis  │  │
│  ├───────────────────────────────────────────────────────────┤  │
│  │  LOW (hours → days)                                       │  │
│  │  TA, sentiment, calendar, prediction markets, flows       │  │
│  │  Updated: per-refresh │ Signal: entry/exit, volatility    │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                 │
│  Each layer CONSTRAINS the layer below it:                      │
│  Macro bias → High themes → Medium scenarios → Low signals      │
└─────────────────────────────────────────────────────────────────┘
```

## The Four Timeframes

### 1. Low-Timeframe Analytics (hours → days)

**Purpose:** Predict intra-day and intra-week volatility and market moves.

**Data sources:**
- Technical analysis: RSI, MACD, SMA, Bollinger Bands, volume, OHLCV patterns
- Prediction markets: Polymarket, Manifold odds on near-term events
- Economic calendar: upcoming releases with impact ratings and countdown
- Sentiment: Fear & Greed indices (crypto + traditional), social sentiment
- Flows: ETF inflows/outflows, on-chain data, COMEX delivery notices
- Options: put/call ratio, implied volatility, unusual activity
- Price action: movers, gaps, breakouts, sector rotation intraday

**Update frequency:** Every `pftui refresh` (~2x daily for agents, on-demand for humans)

**Signal type:** Entry/exit timing, volatility alerts, event risk warnings

**Existing pftui commands that serve this layer:**
`refresh`, `macro`, `movers`, `sentiment`, `predictions`, `calendar`, `etf-flows`, `correlations`, `alerts`, `sector`, `heatmap`, `scan`, `options`, `oil`, `crisis`

**Database tables:**
`price_cache`, `price_history`, `prediction_cache`, `predictions_history`, `calendar_events`, `sentiment_cache`, `sentiment_history`, `onchain_cache`, `fx_cache`, `news_cache`, `alerts`, `correlation_snapshots` (F31.9), `regime_snapshots` (F31.10)

---

### 2. Medium-Timeframe Analytics (weeks → months)

**Purpose:** Predict bigger swing shifts based on the economy, war, Fed policy, macro events.

**Data sources:**
- Economic data: CPI, NFP, GDP, PPI, JOLTS, PMI, unemployment (BLS, FRED)
- Central bank policy: FOMC decisions, dot plots, Fed speakers, CME FedWatch
- Geopolitical events: wars, sanctions, trade agreements, elections
- Commodity fundamentals: COMEX inventory, CFTC COT positioning, supply/demand
- Corporate: earnings seasons, sector earnings trends, guidance shifts
- News: macro headlines, Reuters, Bloomberg, Brave Search intelligence
- Scenario analysis: probability-weighted outcomes with signal tracking

**Update frequency:** Daily (agents), weekly review (comprehensive)

**Signal type:** Scenario probabilities, thesis updates, conviction changes, allocation recommendations

**Existing pftui commands:**
`economy`, `supply`, `sentiment` (COT), `fedwatch`, `global`, `brief`, `eod`, `news`, `research`, `sovereign`

**Database tables:**
`bls_cache`, `economic_cache`, `economic_data`, `worldbank_cache`, `comex_cache`, `cot_cache`, `scenarios` + `scenario_signals` + `scenario_history` (F31.1), `thesis` + `thesis_history` (F31.2), `convictions` (F31.3), `research_questions` (F31.4), `user_predictions` (F31.5), `daily_notes` (F31.7), `opportunity_cost` (F31.8)

---

### 3. High-Timeframe Analytics (months → years)

**Purpose:** Track trend shifts in AI, technology, politics, demographics, and industry that reshape markets over quarters to years.

**Data sources:**
- AI/technology adoption: research paper output, patent filings, semiconductor capacity, AI spending, productivity data
- Political regime: regulatory direction, fiscal policy trajectory, election cycles
- Demographic trends: workforce composition, immigration, urbanisation
- Industry disruption: EV adoption curves, nuclear/solar buildout, robotics, space commercialisation
- Capital flows: FDI trends, venture funding cycles, IPO markets
- Sector leadership rotation: which sectors are gaining/losing relative strength on quarterly basis

**Update frequency:** Weekly

**Signal type:** Sector rotation themes, multi-quarter positioning, which industries are gaining structural tailwinds

**New pftui commands needed:**
`pftui trends` — show tracked high-timeframe trends with direction and evidence

**Database tables (NEW):**
```
trend_tracker        — named trends with direction, timeframe, conviction
trend_evidence       — dated evidence entries for each trend
trend_asset_impact   — which assets/sectors each trend favours/hurts
```

---

### 4. Macro-Timeframe Analytics (years → decades)

**Purpose:** Monitor shifts in global powers, reserve currency status, empire cycles, and structural regime changes.

**Data sources:**
- Dalio's 8 Power Metrics: education, innovation, competitiveness, economic output, trade share, military, financial centre, reserve currency
- Reserve currency data: IMF COFER, SWIFT payments share, bilateral trade agreements
- Sovereign debt: debt/GDP ratios, foreign holdings of treasuries, fiscal trajectory
- Central bank reserves: gold buying, reserve composition changes
- Geopolitical structure: BRICS expansion, NATO evolution, trade bloc formation
- Historical parallels: pattern matching to previous empire transitions

**Update frequency:** Weekly (structural data moves slowly)

**Signal type:** Structural allocation bias (which asset classes are on the right side of history), regime transition indicators

**Existing pftui commands:**
`global`, `sovereign`

**New pftui commands:**
`pftui structural dashboard/metric-set/metric-list/cycle-set/outcome-add/parallel-add/log-add` (F31.11)

**Database tables:**
`power_metrics`, `structural_cycles`, `structural_outcomes`, `structural_outcome_history`, `historical_parallels`, `structural_log` (all F31.11)

---

## How The Layers Interact

**Constraint flows downward:**
- Macro says "we're in late-stage empire, gold is structurally bullish" → this constrains High and Medium layers to weight gold-positive scenarios higher
- High says "AI disruption is accelerating white-collar displacement" → this constrains Medium layer to weight stagflation/recession scenarios higher
- Medium says "FOMC likely to cut 75bp, war inflation persisting" → this constrains Low layer to look for gold/oil entries, not tech longs

**Signal flows upward:**
- Low detects correlation break (BTC-SPX decorrelating) → flags to Medium as potential regime shift
- Medium sees NFP -92K confirming recession → flags to High as AI disruption thesis gaining evidence
- High sees BRICS payment system going live → flags to Macro as reserve currency transition accelerating

**The Analytics Engine's job is to keep all four layers coherent.** When they're aligned (Macro bullish gold + High bullish commodities + Medium bullish on war scenario + Low showing gold breakout), that's a high-conviction signal. When they diverge (Macro bullish gold but Low showing gold breakdown), that's a signal to investigate — either the low-timeframe is noise, or the higher-timeframe thesis is wrong.

---

## Implementation Roadmap

### Phase 1: Foundation (F31 — in progress)
All F31 tables provide the database backbone for Medium and Macro timeframes.
- Scenarios, thesis, convictions, predictions → Medium
- Structural tables → Macro
- Agent messages, daily notes → Cross-timeframe coordination
- Regime classification, correlations → Low/Medium boundary

### Phase 2: Analytics Engine CLI (F33 — new)
New top-level command: `pftui analytics`
- `pftui analytics summary` — combined view across all 4 timeframes
- `pftui analytics low` — Low-timeframe dashboard (existing data, new presentation)
- `pftui analytics medium` — Medium-timeframe dashboard
- `pftui analytics high` — High-timeframe dashboard (requires F33 trend tables)
- `pftui analytics macro` — Macro-timeframe dashboard (requires F31.11)
- `pftui analytics alignment` — show where timeframes agree/diverge
- All `--json` for agent consumption

### Phase 3: High-Timeframe Tables (F33 — new)
The only missing data layer. Low, Medium, and Macro are covered by existing + F31 tables.
- `trend_tracker` — named trends with direction and conviction
- `trend_evidence` — dated evidence for each trend
- `trend_asset_impact` — asset/sector implications per trend
- `pftui trends` CLI

### Phase 4: Cross-Timeframe Signals (F34 — future)
Automated detection of alignment and divergence across timeframes.
- `timeframe_signals` table — logged when layers agree or conflict
- Computed during `pftui refresh` by comparing regime + scenarios + trends + structural outcomes
- `pftui analytics signals` shows active cross-timeframe signals

### Phase 5: Documentation & Product (F35)
- README rewrite: Analytics Engine as core value prop
- Website section: multi-timeframe diagram, explanation
- AGENTS.md: how agents use each timeframe layer
- PRODUCT-VISION.md update
