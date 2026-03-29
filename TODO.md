# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

### F55: Prediction Market Probability Feeds
**Source:** Competitive research (prediction-market-analysis, pmxt). Biggest intelligence gap.
**Why:** Polymarket/Kalshi contracts represent real-money consensus on geopolitical and macro events. These are exactly the scenarios Sentinel tracks (Iran war, recession, Fed decisions). Currently agents estimate probabilities from vibes and news. Prediction market data gives them crowd-calibrated baselines backed by actual capital at risk.
**Scope:**
- [ ] F55.4: `data predictions map --scenario "<name>"` — link a prediction market contract to a pftui scenario. When refreshed, auto-log the market probability as a data point in scenario history.
- [ ] F55.5: `analytics calibration --json` — compare pftui scenario probabilities vs prediction market consensus. Flag divergences >15pp. "Your Iran War estimate: 38%. Polymarket: 22%. Divergence: +16pp."
- [ ] F55.6: Agent routine integration — morning-brief and evening-analysis include prediction market calibration section. Agents explain divergences between their estimates and market consensus.
**Completed:** F55.1 (#422), F55.2 (#422), F55.3 (#422).
**Effort:** 2-3 weeks. **Priority:** P1 — this is the single highest-value data source pftui doesn't have.

## P2 - Coverage And Agent Consumption

### [Feedback] Catalyst-Scenario Linkage
**Source:** Evening Analysis feedback (Mar 29, 78/75 — lowest scorer).
**Why:** `analytics synthesis` shows catalysts with 0 linked scenarios on every catalyst. Connecting upcoming catalysts (Core PCE, ISM Manufacturing, FOMC) to active scenarios would significantly improve decision support. The evening analyst explicitly called this out as "empty catalyst-scenario linkage."
**Scope:**
- [ ] Auto-link catalysts to scenarios by keyword/category matching in `build_catalysts_backend()`. Each catalyst should reference which active scenarios it could impact and in which direction.
- [ ] Enrich `CatalystJson` with `linked_scenarios: Vec<{name, direction, relevance}>` field.
- [ ] Update terminal output to show linked scenarios per catalyst.
**Files:** `src/commands/analytics.rs` (catalysts section), `src/analytics/situation.rs` (if catalysts built there).
**Effort:** 1 session. **Priority:** P2 — directly addresses lowest-scorer workflow friction.

### [Feedback] Prediction Lesson Extraction
**Source:** Evening Analysis feedback (Mar 29, 78/75 — lowest scorer).
**Why:** 43 wrong predictions exist with no structured lessons extracted. This is technical debt that degrades the model improvement loop. The evening analyst flagged this as a gap.
**Scope:**
- [ ] `journal prediction lessons --json` — for each scored-wrong prediction, extract a structured lesson: what was predicted, what happened, why it was wrong (directional miss, timing miss, magnitude miss), and what signal was misread.
- [ ] Store lessons in a `prediction_lessons` table or as metadata on existing predictions.
- [ ] Agent routine integration — evening-analysis reviews recent wrong predictions and generates lessons.
**Files:** `src/commands/predictions.rs`, `src/db/` (new table or field), `src/cli.rs`.
**Effort:** 1-2 sessions. **Priority:** P2 — closes the self-improvement feedback loop.

### F56: Adversarial Debate Mechanism
**Source:** Competitive research (TradingAgents bull/bear debate, ai-hedge-fund persona diversity).
**Why:** pftui's timeframe agents currently produce independent reports that the evening-analysis synthesises. There's no structured adversarial process. TradingAgents forces bull and bear researchers to debate with evidence before decisions. This catches contradictions, strengthens conviction signals, and produces better analysis. Cross-timeframe tension is already identified as "the intelligence product" in AGENTS.md. This formalises it.
**Scope:**
- [ ] F56.1: New `agent debate` CLI domain — `agent debate start --topic "<asset or scenario>" --rounds 3`, `agent debate history --json`, `agent debate summary --json`.
- [ ] F56.2: New table `debates` — debate_id, topic, status (active/resolved), created_at, resolved_at. New table `debate_rounds` — debate_id, round_num, position (bull/bear), agent_source, argument_text, evidence_refs, created_at.
- [ ] F56.3: Evening-analysis routine update — before writing the final analysis, the agent runs `agent debate start` on the 1-2 most contentious topics of the day (identified from timeframe divergence). It plays both bull and bear, citing specific data from each timeframe agent. The debate output feeds into the final synthesis.
- [ ] F56.4: `analytics debate-score --json` — track which side (bull/bear) was right historically for each debated topic. Feeds into system accuracy tracking.
**Not in scope:** Multi-agent real-time debate (requires concurrent sessions). V1 is single-agent playing both sides with structured format.
**Effort:** 1-2 weeks. **Priority:** P2 — improves analysis quality but the current system works.

### F57: Timeframe Analyst Self-Awareness
**Source:** Competitive research insight: the best multi-agent systems make their reasoning transparent and trackable.
**Why:** The 4 timeframe analysts (LOW/MEDIUM/HIGH/MACRO) are the architecture. But right now their outputs are opaque text blobs in `agent_messages`. The evening synthesis reads them but there's no structured way to see where each analyst stands on each asset, track how their views evolve over time, or measure which analyst is most accurate at which task. Making the analysts' reasoning structured and queryable makes the whole system smarter.
**Scope:**
- [ ] F57.1: New table `analyst_views` — analyst (low/medium/high/macro), asset, direction (bull/bear/neutral), conviction (-5 to +5), reasoning_summary, key_evidence, blind_spots, updated_at. Each analyst writes a structured view per asset on every run.
- [ ] F57.2: `analytics views --json` — show current view from each analyst for all held/watched assets. Matrix format: rows = assets, columns = analysts, cells = direction + conviction.
- [ ] F57.3: `analytics views history --asset <SYM> --json` — how each analyst's view on an asset has evolved over time. Track conviction drift and flip points.
- [ ] F57.4: `analytics views divergence --json` — surface assets where analysts strongly disagree. LOW says bear -3 but HIGH says bull +4 = the interesting signal. Ranked by divergence magnitude.
- [ ] F57.5: `analytics views accuracy --json` — per-analyst accuracy. Which timeframe is best at short-term calls? Which catches structural turns? Feed this back into the synthesis layer so evening-analysis knows which analyst to weight more.
- [ ] F57.6: Agent routine integration — each timeframe analyst writes structured views via `analytics views set` after every run. Evening-analysis reads the view matrix before synthesis. Morning-brief includes a one-line divergence summary.
**Effort:** 2 weeks. **Priority:** P2 — makes the existing architecture observable and self-improving.

### F58: Prediction Accuracy Backtesting
**Source:** Competitive research (ai-hedge-fund backtester, TradingAgents paper results).
**Why:** pftui tracks prediction accuracy forward but can't replay decisions against historical data. The system has 231 predictions and growing. Backtesting would answer: "If I had followed the system's high-conviction calls, what would my returns be?" This closes the self-improvement feedback loop and validates (or invalidates) the entire agent architecture.
**Scope:**
- [ ] F58.1: `analytics backtest predictions --json` — replay all scored predictions. For each: entry price at prediction date, exit price at target date, theoretical P&L if acted on at stated conviction level.
- [ ] F58.2: `analytics backtest report --json` — aggregate backtest results. Win rate by conviction level, by timeframe, by asset class, by source agent. Sharpe ratio equivalent for prediction-based strategy.
- [ ] F58.3: `analytics backtest agent --agent <name> --json` — per-agent accuracy breakdown. Which timeframe analyst produces the best predictions?
- [ ] F58.4: Agent routine integration — weekly self-review (macro-timeframe-analyst) includes backtest summary. Surface which conviction levels and which agents are most reliable.
**Not in scope:** Full portfolio simulation, position sizing, transaction costs. V1 is prediction accuracy analysis only.
**Effort:** 2-3 weeks. **Priority:** P2 — valuable but not blocking daily operations.

## P3 - Long Term

### F59: Capital Flow Tracking
**Source:** Competitive research (NOFX institutional flow data).
**Why:** Institutional fund flows, ETF creation/redemption, and open interest changes reveal positioning that price alone doesn't show. pftui has BTC ETF flows and COT but lacks broader equity/commodity fund flow data.
**Scope:** New `data flows` source pulling ETF flow data (ETF.com or similar), institutional 13F filings, and crypto exchange flow data. New table `capital_flows`. Integration into agent routines.
**Effort:** 3-4 weeks. **Priority:** P3 — enhances analysis but agents can web_search for this.



---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analysis | 78% | 75% | Mar 29 | → (stable at 78/75. Catalyst-scenario linkage empty + 43 wrong predictions without lessons flagged. **Lowest scorer — priority.**) |
| Medium-Timeframe Analyst | 75% | 85% | Mar 29 | ↓ (85→75 usefulness. `data quotes` alias #419 shipped. Macro regime detection praised.) |
| Low-Timeframe Analyst | 85% | 90% | Mar 28 | → (stable. Alert triage #405 + regime transitions #407 + cross-timeframe resolve #410 shipped.) |
| High-Timeframe Analyst | 85% | 90% | Mar 26 | → (stable. Scenario suggest #366 shipped.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable. Correlation break interpretation #412 shipped.) |
| Morning Brief | 85% | 80% | Mar 28 | → (stable. Morning-brief #363 shipped.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable. Commodity coverage shipped.) |
| Dev Agent | 92% | 94% | Mar 29 | → (stable high. F55.1-F55.3 prediction market contracts shipped #422.) |

**Top 3 priorities based on feedback:**
1. **Catalyst-scenario linkage** (Evening Analysis 78/75 — lowest scorer) — synthesis catalysts show 0 linked scenarios, reducing decision support value.
2. **Prediction lesson extraction** (Evening Analysis 78/75) — 43 wrong predictions with no structured lessons. Technical debt degrading improvement loop.
3. **F55.4-F55.6 completion** (prediction market mapping + calibration) — enables market-consensus comparison, high value for all analyst agents.

**Shipped since last review (Mar 28 → Mar 29):**
1. ✅ `data quotes` alias (#419) — addresses medium-timeframe-analyst `data quotes fails`
2. ✅ F55.1-F55.3 prediction market contracts (#422) — Polymarket tag-based fetching, enriched schema, 24 new tests

**Release status:** v0.21.0 eligible — 37 commits since v0.20.0, no P0 bugs, 1935 tests passing, clippy clean.

**GitHub stars:** 8 (was 7) — Homebrew Core requires 50+.
