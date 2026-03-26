# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

_(none)_

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 65% | 68% | Mar 26 | ↓ (dropped from 78/75 on Mar 25. Scenario discoverability, predictions stats/unanswered errors, economy indicator confidence, wants auto-scored predictions. overnight-futures ✅ shipped. **Lowest scorer — critical priority.**) |
| Medium-Timeframe Analyst | 85% | 80% | Mar 25 | → (stable at 85/80. COT extreme detection praised. Regime transition alerts shipped #314.) |
| Low-Timeframe Analyst | 75% | 80% | Mar 26 | ↓ (from 95/90. Wants prediction accuracy feedback loop per-timeframe. Shipped prediction stats filters #356.) |
| High-Timeframe Analyst | 85% | 90% | Mar 26 | ↑ (85/90 Mar 26. Requested automated scenario probability updates → shipped #366 `analytics scenario suggest`.) |
| Low-Timeframe Midday | 85% | 88% | Mar 23 | → (no new review since Mar 23.) |
| Morning Intelligence | 85% | 90% | Mar 23 | → (no new review since Mar 23.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent. System healthy.) |
| Dev Agent | 92% | 94% | Mar 26 | → (stable high. Shipped ratio alerts #332, predictions subcommands #334.) |

**Key changes since last review (Mar 25):**
- v0.17.0 released Mar 25. 46 new commits since tag.
- Shipped: ratio-based alerts (#332), predictions stats/scorecard/unanswered (#334), Dixon Power Flow Tracker (#327), market-hours command (#318), scenario probability alerts (#314)
- Tests: 1701 passing (up from 1628), clippy clean
- Evening Analyst **dropped** 78→65 usefulness, 75→68 overall — scenario discoverability, economy confidence, missing auto-scoring
- Low-Timeframe Analyst **surged** 85→95 usefulness, 80→90 overall — correlation breaks and ratio alerts praised
- Predictions stats/unanswered fix (#334) shipped in response to Evening Analyst's 65/68 — score impact TBD next review

**Shipped since last review:**
1. ~~**P2: Auto-scored prediction lifecycle**~~ — ✅ Shipped #341. `journal prediction auto-score` command.
2. ~~**P2: Correlation breaks × impact analysis cross-reference**~~ — ✅ Shipped #341. `--with-impact` flag on `analytics correlations latest --json`.
3. ~~**P2: Sector-wide theme detection**~~ — ✅ Shipped #351. `analytics movers themes` subcommand. Detects rotation patterns across sectors/categories.
4. ~~**P2: Prediction stats per-timeframe/agent filtering**~~ — ✅ Shipped #356. `--timeframe` and `--agent` flags on prediction stats.
5. ~~**P2: News sentiment scoring**~~ — ✅ Shipped #358. `analytics news-sentiment` command + `data news --with-sentiment` flag. Keyword-based scoring with category aggregation.
6. ~~**P1: Consolidated morning-brief command**~~ — ✅ Shipped #363. `analytics morning-brief --json` combines situation, deltas, synthesis, scenarios, correlation breaks, catalysts, impact, alerts, news sentiment in one call.
7. ~~**P2: Automated scenario probability suggestions**~~ — ✅ Shipped #366. `analytics scenario suggest --json` analyzes signal evidence + probability trends to suggest adjustments.
8. ~~**P2: Regime-asset flow correlation tracker**~~ — ✅ Shipped #369. `analytics regime-flows --json` cross-references regime with asset flows, detects 8 power structure patterns (geopolitical stress, inflationary pulse, etc.).

**Release eligibility:** ✅ READY — v0.19.0. 50+ commits since v0.18.0, 10 significant features shipped, 1769 tests passing, clippy clean, no P0 bugs.

**GitHub stars:** 5 — Homebrew Core requires 50+.
