---
name: pftui-intelligence-report
description: Produce branded PFTUI Intelligence Reports. Spawns an Opus-level sub-agent that ingests all multi-timeframe agent outputs from the pftui database, pulls live market data, runs deep research, constructs scenario-probability analysis, and generates a professional PDF report with pftui.com branding. Supports two modes: (1) single-asset deep analysis on a specific question, and (2) multi-topic newsletter covering broad macro outlook. Use when the user wants a deep analysis on an asset, a market question, or a periodic intelligence newsletter.
---

# PFTUI Intelligence Report

Produce professional, branded intelligence reports powered by pftui's multi-timeframe analytics engine.

## Modes

### Mode 1: Single-Asset Deep Analysis
Focused analysis on one asset or market question (e.g. "BTC cycle analysis", "gold vs BTC divergence", "is the dollar peaking").

### Mode 2: Newsletter / Multi-Topic Report
Broad macro outlook covering multiple assets and themes. Used for periodic (monthly/quarterly) intelligence newsletters.

## Arguments

- **topic**: The asset, question, or newsletter theme (e.g. "BTC", "gold vs BTC divergence", "Current outlook for remainder of 2026")
- **mode**: "analysis" (single-asset) or "newsletter" (multi-topic). Default: "analysis"
- **sections** (newsletter mode): List of topics to cover as separate sections
- **title**: Report title for the PDF header
- **subtitle** (optional): Subtitle for the PDF header
- **date**: Report date
- **context** (optional): Any additional framing the user provided

---

## Step 1: Ingest Full System State

Query the pftui database for everything. For single-asset mode, filter to the relevant asset. For newsletter mode, pull everything.

```bash
export PGPASSWORD=<from local config>

# All scenario probabilities
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT name, probability, updated_at FROM scenarios ORDER BY probability DESC;"

# All convictions (latest per symbol)
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT DISTINCT ON (symbol) symbol, score, LEFT(notes, 300), recorded_at FROM convictions ORDER BY symbol, recorded_at DESC;"

# All open predictions
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT id, claim, timeframe, confidence, source_agent FROM user_predictions WHERE outcome IS NULL ORDER BY created_at DESC;"

# Recent scored predictions (accuracy calibration)
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT claim, outcome, timeframe, lesson FROM user_predictions WHERE outcome IS NOT NULL ORDER BY scored_at DESC LIMIT 20;"

# Structural cycles
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT name, stage, evidence FROM structural_cycles;"

# Power metrics (latest per country)
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT country, metric, score, trend FROM power_metrics ORDER BY country, metric;"

# Historical parallels
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT period, event, similarity, outcome FROM historical_parallels ORDER BY similarity DESC;"

# Structural outcomes
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT name, probability FROM structural_outcomes ORDER BY probability DESC;"

# Recent analytical notes
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT section, LEFT(content, 400), date FROM daily_notes WHERE section='analysis' ORDER BY date DESC LIMIT 15;"

# Recent agent messages (cross-timeframe intelligence)
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT from_agent, layer, LEFT(content, 300), created_at FROM agent_messages ORDER BY created_at DESC LIMIT 20;"

# Scenario signals
psql -h 127.0.0.1 -U pftui -d pftui -t -c \
  "SELECT scenario_id, signal_name, status, evidence FROM scenario_signals ORDER BY scenario_id;"
```

Also read any prior analysis reports if they exist and are relevant:
```bash
ls /root/.openclaw/workspace-finance/reports/*.md
cat /root/.openclaw/workspace-finance/reports/<relevant-reports>.md
```

And the structural framework:
```bash
cat /root/.openclaw/workspace-finance/STRUCTURAL.md
```

## Step 2: Live Research

Run deep, targeted web searches. The number depends on the mode:

- **Single-asset mode:** 5-8 searches focused on the specific asset
- **Newsletter mode:** 8-12 searches covering all sections

Focus on:
1. **Current prices and recent action** for all relevant assets
2. **Flow data**: ETF inflows/outflows, COMEX positioning, fund allocations, exchange balances
3. **Correlation shifts**: Decorrelation patterns and what they mean
4. **Institutional positioning**: COT data, options skew, whale activity
5. **Macro context**: DXY, rates, yield curve, Fed expectations, geopolitical developments
6. **Sentiment**: Fear and Greed indices, analyst consensus, prediction markets
7. **Historical parallels**: Has this pattern played out before? What happened?
8. **Counter-narrative research**: Actively search for the BEAR case if leaning bull, and vice versa
9. **Sector-specific data**: Job displacement data (AI), military situation (Iran), leading indicators (recession), central bank buying (gold)

## Step 3: Construct Scenarios

For each topic/section, build 3-5 competing scenarios. Each scenario must include:

1. **Probability** (scenarios within a topic must sum to 100%)
2. **Evidence FOR** (specific data points, not narratives)
3. **Evidence AGAINST** (specific data points)
4. **Confirmation signal** (what would prove this scenario correct in the next 1-4 weeks)
5. **Invalidation signal** (what would prove it wrong)
6. **Timeframe** for resolution

Do NOT anchor on the system's existing view. Challenge it. If all timeframe agents are bullish, construct a serious bear case and evaluate it honestly.

## Step 4: Predictions

Make falsifiable, time-bound predictions with confidence levels (0.0-1.0).

- **Single-asset mode:** 3-5 predictions
- **Newsletter mode:** 2-3 predictions per section

Each prediction must:
- State a specific, measurable outcome
- Have a clear target date
- Include reasoning tied to the scenario analysis
- State what would falsify it

## Step 5: Portfolio Allocation Framework

For **single-asset mode**: Specific implications for the asset in question. What levels matter. What signals to watch.

For **newsletter mode**: Present allocation frameworks for different investor profiles. NOT personal advice. NOT specific to any individual portfolio. Present 3 profiles:

- **Conservative**: Risk-averse, capital preservation priority
- **Balanced**: Growth-seeking with hedging
- **Conviction-Driven**: High-conviction macro investors willing to concentrate

For each profile: suggested allocation ranges across cash, BTC, gold/silver, equities, commodities based on the scenario probabilities.

---

## Output Formats

### Single-Asset Deep Analysis

```markdown
# DEEP ANALYSIS: [Asset/Question]

## Data Snapshot (date)
[Key prices, flows, positioning data in table format]

## System State
[What the 4 timeframe agents currently think, summarized]

## Scenario Analysis
### Scenario A: [name]
Probability: X%
[Evidence for/against, confirmation/invalidation signals]

## Probability Summary
[Table: scenario, probability, key variable]

## Predictions
[Numbered, with confidence, reasoning, falsification criteria]

## Portfolio Implications
[Honest assessment]

## What to Watch
[Specific data points and dates that will resolve the analysis]
```

### Newsletter / Multi-Topic Report

```markdown
# [Title]

> PFTUI Intelligence Report | [Subtitle] | [Date]

## Executive Summary
[3-4 paragraphs. The macro picture in plain language. What regime are we in? 
What are the dominant forces? What is the central tension?]

## Market Regime
[Current regime classification from pftui analytics. Transition state. 
Historical context.]

---

## I. [Topic 1]
### Current State
[Price, key metrics, recent developments]

### Multi-Timeframe View
[What each timeframe layer sees. Where they agree and disagree.]

### Scenarios for [timeframe]
[3-4 scenarios with probabilities, evidence, confirmation/invalidation signals]

### Key Levels
[Critical price levels, thresholds, dates]

---

## II. [Topic 2]
[Same structure]

---

[Continue for each section]

---

## Cross-Asset Correlation Map
[How all the topics interact. If one scenario plays out in one section, 
what does it mean for the others? Map the interdependencies.]

---

## Portfolio Allocation Framework
### Conservative Allocation
[For risk-averse investors]

### Balanced Allocation
[For growth-seeking investors with hedging]

### Conviction-Driven Allocation
[For high-conviction macro investors]

---

## Methodology
[Brief section explaining pftui's multi-timeframe analytics engine: 
4 specialist timeframe agents (LOW/MEDIUM/HIGH/MACRO) feeding into 
cross-timeframe synthesis. PostgreSQL-backed data aggregation. 
Evidence-weighted scenario probabilities updated daily.]

---

*PFTUI Intelligence Report | pftui.com | Generated by Sentinel Multi-Timeframe Analytics Engine*
*This report is for informational purposes only and does not constitute financial advice.*
```

## Step 6: Generate PDF Report

After completing the analysis, generate a branded PFTUI Intelligence Report PDF.

1. Write the full analysis as markdown:
```bash
# Path: /root/.openclaw/workspace-finance/reports/YYYY-MM-<slug>.md
```

2. Generate the PDF using the bundled generator:
```bash
python3 /root/pftui/agents/deep-analysis/gen-report.py \
  /root/.openclaw/workspace-finance/reports/YYYY-MM-<slug>.md \
  /root/.openclaw/workspace-finance/reports/YYYY-MM-<slug>.pdf \
  "<Report Title>" \
  "<Month Day, Year>" \
  "<Optional Subtitle>"
```

3. **DO NOT send the PDF or upload to the repo automatically.** The report requires manual review and fact-checking before publication. After generating the PDF, notify the user that the draft is ready for review at the file path. The user will review, request corrections if needed, and explicitly approve publication.

4. Only after the user approves, upload the PDF to the repo:
```bash
cp /root/.openclaw/workspace-finance/reports/YYYY-MM-<slug>.pdf \
  /root/pftui/newsletter/DD-Month-YYYY.pdf
cd /root/pftui
git add newsletter/
git -c user.name="skylarsimoncelli" -c user.email="skylar.simoncelli@icloud.com" \
  commit -m "Newsletter: <title> — <date>"
git push
```

And send to the user:
```bash
message action=send target=<user_id> message="📊 PFTUI Intelligence Report: <title>" filePath=/root/.openclaw/workspace-finance/reports/YYYY-MM-<slug>.pdf
```

File naming: `DD-Month-YYYY.pdf` (e.g. `15-March-2026.pdf`, `12-April-2026.pdf`). All newsletters live in `newsletter/` at the repo root.

The generator produces dark-themed PDFs matching pftui.com branding:
- Fonts: Inter (body) + JetBrains Mono (code, headers, metadata)
- Colors: #0d1117 background, #c9d1d9 text, #89dceb cyan accent, #a6e3a1 green accent, #89b4fa blue accent, #f38ba8 red accent
- Header: "PFTUI Intelligence Report" with title, date, subtitle, confidential classification
- Footer: page numbers, "Generated by Sentinel Intelligence System | pftui.com"
- Dependencies: `weasyprint`, `markdown` (pip)

---

## Rules

- **Professional tone.** These reports may be shared externally. Write for a sophisticated audience.
- **Data-backed everything.** Every probability needs evidence. Every claim needs a source. Reference pftui data explicitly.
- **Balanced.** Present bull AND bear cases for every asset. No cheerleading.
- **Emphasize pftui as the intelligence engine.** Reference "pftui analytics engine", "multi-timeframe analysis", "scenario probability tracking", "cross-timeframe synthesis". These reports demonstrate the product's capability.
- **No personal portfolio details.** No specific holdings, no dollar amounts from anyone's portfolio. Allocation sections are frameworks, not personal advice.
- **No em dashes or double hyphens in prose.** CLI flags are fine.
- **Be honest about uncertainty.** Assign realistic probabilities, not theatrical ones.
- **Always construct counter-narratives.** If the system leans one direction, build the opposing case with equal rigor.
- **Predictions must be falsifiable and time-bound.** No "could go either way" hedging.
- **Challenge the system's existing view.** If all timeframe layers agree, walk the reader through why that matters: distinct lenses converging is a strong signal, but it also means no layer is providing a natural contrarian check. Use this to motivate the bear case section, not to undermine the bull case. Frame it as rigorous methodology ("the bull case needs to survive the bear case before it earns conviction"), not self-doubt. Do not fabricate pftui features or claim the system "flags" something it doesn't.
- **Source verification is critical.** Every specific number (holdings, flows, prices, percentages) MUST be verified against a primary or authoritative source via web search before inclusion. Do not rely on training data for figures that change over time (e.g. corporate BTC holdings, ETF AUM, reserve levels). A single wrong number destroys the credibility of the entire report.
- **Calibrate confidence** using the system's recent prediction accuracy.
- **Include the disclaimer** at the bottom of every report.
- **Historical parallels** add depth. When a pattern has played out before, cite it with specific dates, outcomes, and similarity to the current situation.
- **Cross-asset interdependencies** are essential in newsletter mode. The value is in showing how themes connect, not just listing them.
