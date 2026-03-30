# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

- [Feedback] **Situation engine auto-population from crons** — Evening Analyst (88/85 Mar 30) reports `analytics situation/recap/synthesis` returning empty despite regime data existing in `analytics summary`. The situation engine requires manual `journal scenario promote` + indicator/update setup. Agent crons (evening-analysis, morning-brief) should auto-populate situation data from existing regime, scenario, and signal sources so agents get non-empty results without manual setup. See `src/commands/situation.rs`, `src/analytics/situation.rs`.

## P2 - Coverage And Agent Consumption

- [Feedback] **Fresher PMI/GDP data sources** — Medium-Timeframe Analyst (85/75 Mar 30) reports some indicators are stale. PMI and GDP sourced from Brave scraping have low confidence and may lag. Investigate adding ISM PMI direct source or increasing FRED refresh frequency for GDP/GDI. See `src/data/economic.rs`, `src/commands/economy.rs`.

- [Feedback] **`portfolio stress-test --list-scenarios`** — Low-Timeframe Analyst (85/88 Mar 29) wants a way to discover available scenario names for stress testing without trial/error. Add `--list-scenarios` flag to `portfolio stress-test` that shows active scenario names. See `src/commands/stress_test.rs`.

- [Feedback] **Historical regime transition data** — Macro-Timeframe Analyst (80/85 Mar 29) wants historical regime transition data to track past crisis periods. `analytics macro regime transitions` exists but may need richer historical context. Verify current coverage and add date-range filtering or summary stats if missing. See `src/commands/regime.rs`, `src/commands/regime_transitions.rs`.

### F57: Timeframe Analyst Self-Awareness
**Source:** Competitive research insight: the best multi-agent systems make their reasoning transparent and trackable.
**Why:** The 4 timeframe analysts (LOW/MEDIUM/HIGH/MACRO) are the architecture. But right now their outputs are opaque text blobs in `agent_messages`. The evening synthesis reads them but there's no structured way to see where each analyst stands on each asset, track how their views evolve over time, or measure which analyst is most accurate at which task. Making the analysts' reasoning structured and queryable makes the whole system smarter.
**Scope:**
- [x] F57.1: New table `analyst_views` — analyst (low/medium/high/macro), asset, direction (bull/bear/neutral), conviction (-5 to +5), reasoning_summary, key_evidence, blind_spots, updated_at. Each analyst writes a structured view per asset on every run. *(done: PR #446)*
- [x] F57.2: `analytics views portfolio-matrix --json` — portfolio-aware view matrix: all held + watched + viewed assets. Coverage stats in JSON. *(done: PR #450)*
- [x] F57.3: `analytics views history --asset <SYM> --json` — how each analyst's view on an asset has evolved over time. Track conviction drift and flip points. *(done: PR #453)*
- [x] F57.4: `analytics views divergence --json` — surface assets where analysts strongly disagree. LOW says bear -3 but HIGH says bull +4 = the interesting signal. Ranked by divergence magnitude. *(done: PR #457)*
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
| Evening Analyst | 88% | 85% | Mar 30 | ↑ (78→88 use, 75→85 overall. Major improvement! Consolidated evening-brief #416, debate mechanism, lesson extraction all landed. Main friction: situation engine empty without manual setup.) |
| Medium-Timeframe Analyst | 85% | 75% | Mar 30 | ↓ overall (85→75). Stale indicator data. `data quotes` alias helped. **Lowest overall scorer — priority.** |
| Low-Timeframe Analyst | 85% | 88% | Mar 29 | ↓ overall (90→88). Stable usefulness. Wants stress-test scenario discoverability. |
| Macro-Timeframe Analyst | 80% | 85% | Mar 29 | → (new scorer. Wants scenarios + historical regime transitions.) |
| High-Timeframe Analyst | 85% | 90% | Mar 26 | → (stable. Scenario suggest #366 shipped.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable. Correlation break interpretation #412 shipped.) |
| Morning Brief | 85% | 80% | Mar 29 | → (stable. Scenario tracking on promotion requested.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable. Commodity coverage shipped.) |
| Dev Agent | 92% | 94% | Mar 30 | → (stable high. F57.2-F57.3 shipped.) |

**Top 3 priorities based on feedback:**
1. **Situation engine auto-population (P1)** — Evening Analyst's main friction. Situation/recap/synthesis empty without manual setup. Auto-populating from crons would also benefit Medium-Timeframe and Morning Brief workflows.
2. **F57.4-F57.6 completion** — Analyst view divergence + accuracy + routine integration. Makes the existing architecture self-improving.
3. **Fresher indicator data** — Medium-Timeframe Analyst (lowest overall at 75%) needs less stale PMI/GDP. Improving FRED refresh or adding direct ISM source.

**Shipped since last review (Mar 30):**
1. ✅ F57.2 portfolio-matrix (#450) — `analytics views portfolio-matrix` with coverage stats
2. ✅ F57.3 analyst view history (#453) — `analytics views history` with drift tracking

**Release status:** v0.22.0 eligible — 47 commits since v0.21.0, no P0 bugs, 2043 tests passing, clippy clean. Features shipped: F55 complete, F56 complete, F57.1-F57.3, prediction lessons, catalyst linkage, adversarial debates, analyst views.

**GitHub stars:** 8 — Homebrew Core requires 50+.
