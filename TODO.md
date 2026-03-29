# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(F55: Prediction Market Probability Feeds — COMPLETE. All sub-items shipped: F55.1-F55.3 #422, F55.4 #426, F55.5 #428, F55.6 #437.)_

## P2 - Coverage And Agent Consumption

_(Prediction Lesson Extraction — COMPLETE. CLI #432, agent routine integration #440.)_

### F56: Adversarial Debate Mechanism
**Source:** Competitive research (TradingAgents bull/bear debate, ai-hedge-fund persona diversity).
**Why:** pftui's timeframe agents currently produce independent reports that the evening-analysis synthesises. There's no structured adversarial process. TradingAgents forces bull and bear researchers to debate with evidence before decisions. This catches contradictions, strengthens conviction signals, and produces better analysis. Cross-timeframe tension is already identified as "the intelligence product" in AGENTS.md. This formalises it.
**Scope:**
- [x] F56.1: New `agent debate` CLI domain — `agent debate start --topic "<asset or scenario>" --rounds 3`, `agent debate history --json`, `agent debate summary --json`. *(done: PR #436)*
- [x] F56.2: New table `debates` — debate_id, topic, status (active/resolved), created_at, resolved_at. New table `debate_rounds` — debate_id, round_num, position (bull/bear), agent_source, argument_text, evidence_refs, created_at. *(done: PR #436)*
- [x] F56.3: Evening-analysis routine update — before writing the final analysis, the agent runs `agent debate start` on the 1-2 most contentious topics of the day (identified from timeframe divergence). It plays both bull and bear, citing specific data from each timeframe agent. The debate output feeds into the final synthesis. *(done: PR #442)*
- [ ] F56.4: `analytics debate-score --json` — track which side (bull/bear) was right historically for each debated topic. Feeds into system accuracy tracking.
**Not in scope:** Multi-agent real-time debate (requires concurrent sessions). V1 is single-agent playing both sides with structured format.
**Completed:** F56.1 (#436), F56.2 (#436), F56.3 (#442).
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
| Dev Agent | 92% | 94% | Mar 29 | → (stable high. F55.5 analytics calibration shipped #428.) |

**Top 3 priorities based on feedback:**
1. ~~**Prediction lesson extraction**~~ — COMPLETE (#432 CLI + #440 agent routine integration). Evening analysis now extracts structured lessons from wrong predictions every run.
2. ~~**F55.6 completion**~~ — COMPLETE (#437). Morning/evening briefs include calibration section.
3. **F56: Adversarial Debate Mechanism** — formalises bull/bear debate for contentious topics. F56.3 (evening-analysis integration) next.

**Shipped since last review (Mar 28 → Mar 29):**
1. ✅ `data quotes` alias (#419) — addresses medium-timeframe-analyst `data quotes fails`
2. ✅ F55.1-F55.3 prediction market contracts (#422) — Polymarket tag-based fetching, enriched schema, 24 new tests
3. ✅ F55.4 prediction market scenario mapping (#426) — link contracts to scenarios with auto-sync
4. ✅ F55.5 analytics calibration (#428) — compare scenario vs market probabilities, flag divergences
5. ✅ Catalyst-scenario linkage (#430) — category semantic matching with direction + relevance
6. ✅ Prediction lesson extraction (#432) — structured lessons from wrong predictions with DB storage
7. ✅ F56.1+F56.2 adversarial debate mechanism (#436) — `agent debate` CLI + `debates`/`debate_rounds` tables
8. ✅ Prediction lesson agent routine integration (#440) — evening-analysis now extracts lessons from wrong predictions every run
9. ✅ F56.3 adversarial debate evening-analysis integration (#442) — mandatory structured bull/bear debates on contentious topics before cross-timeframe synthesis

**Release status:** v0.21.0 eligible — 40 commits since v0.20.0, no P0 bugs, 1996 tests passing, clippy clean.

**GitHub stars:** 8 (was 7) — Homebrew Core requires 50+.
