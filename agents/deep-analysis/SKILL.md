---
name: deep-analysis
description: Deep cross-timeframe analysis on a specific asset or market question. Spawns an Opus-level sub-agent that ingests all timeframe agent outputs, pulls live market data, assigns scenario probabilities, and makes falsifiable predictions. Use when the user asks a structural question about an asset, a divergence, a regime shift, or any "what is really happening with X" question.
---

# Deep Analysis

Run a deep, Opus-level cross-timeframe analysis on a specific asset or market question.

## Arguments

- **asset**: The asset or question to analyze (e.g. "BTC", "gold vs BTC divergence", "oil after Kharg", "is the dollar peaking")
- **context** (optional): Any additional framing the user provided

## What This Skill Does

1. Pulls ALL recent timeframe agent output from the database (convictions, predictions, scenarios, agent messages, notes, trend evidence)
2. Runs live web research on the asset (price, flows, positioning, sentiment, correlations, macro context)
3. Constructs competing scenarios for what is happening
4. Assigns probabilities to each scenario with specific evidence for and against
5. Makes falsifiable, time-bound predictions with confidence levels
6. Assesses portfolio implications

## Workflow

### Step 1: Ingest System State

Query the pftui database for everything relevant to the asset:

```bash
export PGPASSWORD=<from local config>

# Convictions for the asset and related assets
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT symbol, score, LEFT(notes, 200), recorded_at FROM convictions WHERE symbol ILIKE '%<asset>%' OR symbol IN (<related symbols>) ORDER BY recorded_at DESC LIMIT 20;"

# Pending predictions involving the asset
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT id, claim, timeframe, confidence, source_agent FROM user_predictions WHERE outcome IS NULL AND (claim ILIKE '%<asset>%' OR symbol ILIKE '%<asset>%') ORDER BY created_at DESC;"

# Scenario probabilities
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT name, probability, updated_at FROM scenarios ORDER BY probability DESC;"

# Recent agent messages mentioning the asset
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT from_agent, category, layer, LEFT(content, 200), created_at FROM agent_messages WHERE content ILIKE '%<asset>%' ORDER BY created_at DESC LIMIT 10;"

# Recent analytical notes
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT section, LEFT(content, 200), date FROM daily_notes WHERE content ILIKE '%<asset>%' ORDER BY date DESC LIMIT 10;"

# Scored predictions (for pattern recognition)
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT claim, outcome, lesson, scored_at FROM user_predictions WHERE outcome IS NOT NULL AND (claim ILIKE '%<asset>%' OR symbol ILIKE '%<asset>%') ORDER BY scored_at DESC LIMIT 10;"
```

Also pull broad system context:
- All scenario probabilities
- Full conviction table (to understand relative positioning across assets)
- Recent prediction accuracy (to calibrate confidence levels)

### Step 2: Live Research

Run 5-8 targeted web searches. Focus on:

1. **Current price and recent action** for the asset and correlated assets
2. **Flow data**: ETF inflows/outflows, COMEX positioning, fund allocations, exchange balances
3. **Correlation shifts**: Is the asset decorrelating from its usual pairs? What does that mean?
4. **Institutional positioning**: COT data, options skew, dark pool prints, whale activity
5. **Macro context**: DXY, rates, yield curve, Fed expectations, geopolitical developments
6. **Sentiment**: Fear and Greed, social sentiment, analyst consensus, prediction markets
7. **Historical parallels**: Has this pattern played out before? What happened?
8. **Counter-narrative research**: Actively search for the BEAR case if you're leaning bull, and vice versa

### Step 3: Construct Scenarios

Build 3-5 competing scenarios for what is happening with the asset. Each scenario must include:

1. **Probability** (all scenarios must sum to 100%)
2. **Evidence FOR** (specific data points, not narratives)
3. **Evidence AGAINST** (specific data points)
4. **Confirmation signal** (what would prove this scenario correct in the next 1-4 weeks)
5. **Invalidation signal** (what would prove it wrong)
6. **Timeframe** for resolution

Do NOT anchor on the system's existing view. Challenge it. If all timeframe agents are bullish, construct a serious bear case and evaluate it honestly.

### Step 4: Predictions

Make 3-5 falsifiable, time-bound predictions with confidence levels (0.0-1.0).

Each prediction must:
- State a specific, measurable outcome
- Have a clear target date
- Include reasoning tied to the scenario analysis
- State what would falsify it

### Step 5: Portfolio Implications

Given the scenario analysis:
- Does the current allocation make sense?
- Is any action warranted, or should the user hold current positioning?
- What would need to change for a strong allocation recommendation?
- What is the risk of inaction vs. the risk of action?

Be honest about uncertainty. If the picture is unclear, say so. "Maintain current positioning and watch for X" is a valid conclusion.

## Output Format

```
# DEEP ANALYSIS: [Asset/Question]
## Data Snapshot (date)
[Key prices, flows, positioning data in table format]

## System State
[What the 4 timeframe agents currently think, summarized]

## Scenario Analysis
### Scenario A: [name]
Probability: X%
[Evidence for/against, confirmation/invalidation signals]

### Scenario B: [name]
...

## Probability Summary
[Table: scenario, probability, key variable]

## Predictions
[Numbered, with confidence, reasoning, falsification criteria]

## Portfolio Implications
[Honest assessment of positioning]

## What to Watch
[Specific data points and dates that will resolve the analysis]
```

## Rules

- This is DEEP analysis. Go beyond headlines. Historical parallels, structural forces, data patterns.
- Be honest about uncertainty. Assign realistic probabilities, not theatrical ones.
- Always construct at least one serious counter-narrative to your primary thesis.
- Predictions must be falsifiable and time-bound. No "could go either way" hedging.
- Challenge the system's existing view. If all agents agree, that's suspicious, not comforting.
- Do NOT include portfolio values or financial details in any output that could be shared externally.
- Source verification: confirm key data points from multiple sources before building scenarios on them.
- Calibrate confidence using the system's recent prediction accuracy. If the system has been 40% accurate, don't assign 80% confidence.
