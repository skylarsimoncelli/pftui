# Analytics Engine — Architecture & Roadmap

## Overview

pftui's Analytics Engine is a multi-timeframe intelligence system that processes market data across four distinct time horizons. Each layer operates on different data sources, different update frequencies, and produces different kinds of actionable signals.

As of the F53-F58 architecture work, the engine also exposes canonical server-owned analytics products that sit above the raw timeframe layers. These are the shared contracts consumed by CLI, web, mobile, and later the AI layer:

- `pftui analytics situation --json` — canonical "what matters now" payload
- `pftui analytics deltas --json` — server-owned change radar across monitoring windows
- `pftui analytics catalysts --json` — ranked upcoming event pressure and countdowns
- `pftui analytics impact --json` — portfolio-aware exposure ranking
- `pftui analytics opportunities --json` — high-alignment non-held ideas
- `pftui analytics synthesis --json` — cross-timeframe alignment, divergence, and constraints
- `pftui analytics narrative --json` — machine-readable recap and analytical memory

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

## Canonical Analytics Products

These contracts are where pftui now turns raw layer state into reusable intelligence. The design goal is simple: the hard ranking, delta detection, cross-timeframe reasoning, and portfolio-aware interpretation should live in Rust/Postgres, not be recomputed independently in every client or prompt.

### Situation

`pftui analytics situation --json`

Answers:
- what matters now
- why it matters
- how severe it is
- which assets and portfolio exposures are affected

Core output:
- headline / subtitle
- summary stats
- `watch_now[]`
- `portfolio_impacts[]`
- `risk_matrix[]`

### Deltas

`pftui analytics deltas --json [--since last-refresh|close|24h|7d]`

Answers:
- what changed
- when it changed
- how material it is

Core output:
- persisted `situation_snapshots`
- ranked `change_radar[]`
- change windows for last refresh, prior close, 24h, and 7d

### Catalysts

`pftui analytics catalysts --json [--window today|tomorrow|week]`

Answers:
- what is coming next
- how soon it lands
- why it matters to scenarios and the portfolio

Core output:
- `CatalystEvent`
- countdown bucket
- significance
- affected assets
- scenario / prediction linkage

### Impact And Opportunities

`pftui analytics impact --json`
`pftui analytics opportunities --json`

Answers:
- why current developments matter to the existing book
- what strong opportunities exist outside it

Core output:
- evidence chains from scenarios, trends, signals, catalysts, and convictions
- held/watchlist exposure ranking
- non-held opportunity ranking

### Synthesis

`pftui analytics synthesis --json`

Answers:
- where timeframes agree
- where they disagree
- which higher-layer constraints dominate
- what deserves watching tomorrow

Core output:
- strongest alignment
- highest-confidence divergence
- constraint flows
- unresolved tensions
- watch-tomorrow candidates

### Narrative

`pftui analytics narrative --json`

Answers:
- what the system believes now versus recently
- which scenario / conviction / trend shifts matter
- what lessons and surprises should persist as analytical memory

Core output:
- recap events
- scenario shifts
- conviction shifts
- trend changes
- prediction scorecard summary
- surprises
- lessons
- catalyst outcomes

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

### Phase 6: Rust-First Intelligence Layer (shipped in F53-F58)

This phase moved a major part of the "intelligence" surface out of prompts and client-side heuristics into native analytics products:

- Situation engine
- Delta engine
- Catalyst engine
- Portfolio impact and opportunities engine
- Cross-timeframe synthesis engine
- Narrative state and structured recap layer

The remaining AI-layer work is now primarily:
- judgment
- escalation
- prose synthesis
- deep external research

Not:
- ranking raw priorities from scratch
- recomputing what changed
- rebuilding the same cross-timeframe synthesis in every prompt

---

# User Guide — Analytics Engine

## Overview

pftui's Analytics Engine is a multi-timeframe intelligence system that gives you a complete picture of market forces from intraday noise to decade-long empire cycles. Unlike traditional tools that focus on a single timeframe, the Analytics Engine tracks four distinct layers simultaneously, each providing different types of market signals that inform your investment decisions.

The system automatically detects when all timeframes align (strong conviction signals) or diverge (investigation opportunities), giving you the same kind of multi-dimensional market analysis that institutional traders use, but in a simple, actionable format.

## The Four Timeframes

### **LOW — Hours to Days** ("What's happening right now?")

**What it tracks:** Real-time market conditions, sentiment, and tactical signals for immediate market movements.

- **Prices (84 symbols):** Real-time quotes across equities, crypto, commodities, currencies, and indices
- **VIX:** Market volatility and fear gauge with historical context
- **Fear & Greed:** Both crypto and traditional market sentiment indices  
- **Technical indicators:** RSI, MACD, SMA overlays for momentum and trend strength
- **Prediction markets:** Polymarket odds on near-term events and outcomes
- **Correlation snapshots:** How different assets are moving relative to each other
- **Regime classification:** Current market state (risk-on/risk-off/transition) with confidence scores
- **Calendar events:** Upcoming economic releases, earnings, FOMC meetings
- **Triggered alerts:** Price breakouts, volatility spikes, unusual moves

**Update frequency:** Every refresh cycle (typically 2x daily for automated systems)

**Signal type:** Tactical — "Gold is overbought on RSI." "BTC-SPX correlation just broke." "VIX spiked above 28."

**Commands:**
```bash
pftui refresh           # Update all LOW layer data
pftui movers            # Today's significant price moves  
pftui sentiment         # Fear & Greed indices + context
pftui predictions       # Prediction market odds
pftui calendar          # Economic calendar with impact ratings
pftui regime current    # Current market regime classification
pftui correlations      # Asset correlation matrix
pftui alerts            # Active price and volume alerts
```

### **MEDIUM — Weeks to Months** ("What scenarios are playing out?")

**What it tracks:** Macro scenarios and directional thesis development based on economic data and geopolitical events.

- **Macro scenarios with probabilities:** Recession, stagflation, soft landing, war escalation scenarios with assigned probabilities
- **Versioned thesis by section:** Your evolving macro view organized by theme (monetary policy, geopolitics, technology)
- **Conviction scores per asset (-5 to +5):** How bullish or bearish you are on each holding with numerical precision
- **Research questions with evidence:** Open questions you're tracking with accumulating evidence for each side
- **Economic data (BLS 101 series, COT, COMEX):** Employment, inflation, positioning data that drives scenario probabilities
- **User predictions with accuracy scoring:** Your own predictions with tracked accuracy over time
- **Opportunity cost tracking:** What you're NOT buying and why, with regular review prompts

**Update frequency:** Daily updates by agents or manual analysis

**Signal type:** Directional — "Stagflation scenario gaining evidence." "Gold conviction +4." "Fed 97% pricing 75bp cut."

**Commands:**
```bash
pftui scenario          # View and update scenario probabilities
pftui thesis            # Read and edit your evolving macro thesis  
pftui conviction        # Set/view conviction scores for assets
pftui predict           # Log predictions and track accuracy
pftui opportunity       # Track opportunity cost decisions
pftui economy           # Economic data dashboard
pftui supply            # Commodity supply/demand data
pftui fedwatch          # Federal Reserve policy tracking
pftui question          # Research questions with evidence logs
```

### **HIGH — Months to Years** ("What structural trends are reshaping markets?")

**What it tracks:** Multi-quarter structural trends that reshape entire industries and asset classes over years.

- **Multi-quarter structural trends:** Named trends with direction tracking (accelerating/stable/decelerating/reversing)
- **Evidence logs:** Dated evidence entries that support or contradict each trend
- **Conviction levels:** How confident you are in each trend's direction and timeline
- **Per-asset impact mappings:** Which specific assets/sectors each trend favors or hurts

**Example trends:**
- **AI disruption:** Workplace automation, productivity gains, job displacement effects
- **Nuclear renaissance:** New reactor builds, uranium demand, energy mix shifts  
- **BRICS de-dollarisation:** Alternative payment systems, reserve currency diversification
- **Commodity supercycles:** Infrastructure build-out driving metal and energy demand

**Update frequency:** Weekly review or when significant evidence emerges

**Signal type:** Thematic — "AI displacement accelerating — bullish defense contractors, bearish consumer discretionary." "Commodity supercycle: oil, copper, uranium all structurally tight."

**Commands:**
```bash
pftui trends add        # Add a new structural trend to track
pftui trends list       # View all tracked trends with status
pftui trends update     # Update trend direction and conviction  
pftui trends evidence-add  # Log new evidence for a trend
pftui trends impact-add    # Map trend impact to specific assets
pftui trends dashboard     # Overview of all HIGH layer trends
```

### **MACRO — Years to Decades** ("Where are we in the big cycle?")

**What it tracks:** Empire lifecycle analysis and structural regime changes that unfold over decades.

- **Empire lifecycle analysis:** Ray Dalio's framework tracking where major powers are in their rise/decline cycles
- **Power metrics across 8 dimensions:** Education, innovation, military strength, trade share, financial center status, governance quality, reserve currency usage
- **Structural cycles with stage tracking:** Big debt cycle, technology cycle, generational cycle with current stage identification
- **Structural outcomes with probabilities (10-30yr):** Long-term scenarios like reserve currency transition, demographic shifts, climate adaptation
- **Historical parallels with similarity scoring:** Pattern matching to previous empire transitions and crisis periods

**Update frequency:** Weekly review (structural data changes slowly)

**Signal type:** Structural — "US at Stage 5→6 transition in empire cycle." "1973-74 parallel: similarity score 8/10."

**Commands:**
```bash  
pftui structural metric-set     # Update power metrics for countries
pftui structural metric-list    # View current power metric scores
pftui structural cycle-set      # Set current stage in structural cycles
pftui structural outcome-add    # Add long-term outcome probability
pftui structural parallel-add   # Log historical parallel with similarity
pftui structural dashboard      # MACRO layer overview
```

## Cross-Timeframe Intelligence

The real power of the Analytics Engine comes from how the four timeframes interact and inform each other:

**Key Commands:**
- `pftui analytics summary` — All four layers in one comprehensive view
- `pftui analytics alignment` — Per-asset consensus across all timeframes

**Signal Flow Patterns:**

**Signals flow UPWARD (tactical → structural):**
- Correlation break in LOW layer → flags potential regime shift to MEDIUM
- MEDIUM scenario shift → provides evidence for HIGH layer trend  
- HIGH trend acceleration → confirms MACRO layer structural transition

**Context flows DOWNWARD (structural → tactical):**
- MACRO structural bias → weights MEDIUM scenario probabilities
- MEDIUM scenario dominance → influences HIGH trend interpretation  
- HIGH trend momentum → provides context for LOW layer signal interpretation

**Inter-layer Communication:**
Use `pftui agent-msg` with `--layer` and `--category escalation/feedback` to log when information moves between timeframes.

## Example: How Layers Interact

**Scenario:** Oil breaks $100

1. **LOW layer** detects the price alert and regime shift from risk-on to risk-off based on VIX spike and correlation changes
2. **MEDIUM layer** raises the probability of the "War Escalation" scenario from 20% to 45% based on the oil breakout
3. **HIGH layer** logs this as evidence strengthening the "Commodity Supercycle" trend, updating its direction from "stable" to "accelerating"  
4. **MACRO layer** notes this fits the Stage 6 conflict pattern in empire lifecycle analysis, where resource competition intensifies

**Result:** All four layers now agree that gold is bullish:
- LOW: Technical breakout above resistance
- MEDIUM: War scenario supports hard assets  
- HIGH: Commodity supercycle trend accelerating
- MACRO: Late-stage empire pattern favors gold

When `pftui analytics alignment --symbol GC=F` shows all four timeframes aligned, that's a ████ **STRONG** signal — maximum conviction for deployment.

## Quick Start

Get started with the Analytics Engine in 5 minutes:

```bash
# 1. Populate the LOW layer with current market data
pftui refresh

# 2. Set up a MEDIUM layer scenario 
pftui scenario add "Recession" --probability 30

# 3. Express conviction on an asset in MEDIUM layer
pftui conviction set GC=F --score 4

# 4. Add a HIGH layer structural trend
pftui trends add "AI Disruption" --direction accelerating

# 5. Set MACRO layer structural cycle stage  
pftui structural cycle-set "Big Debt Cycle" --stage 6

# 6. See all layers at once
pftui analytics summary

# 7. Check consensus on specific asset
pftui analytics alignment --symbol GC=F
```

The Analytics Engine transforms pftui from a portfolio tracker into a complete intelligence system that helps you understand market forces across every relevant timeframe. Whether you're making a quick tactical trade or a multi-year strategic allocation, you'll have the context you need to act with conviction.
