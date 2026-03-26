# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

- [Feedback] **Auto-scored prediction lifecycle** — Evening Analyst (Mar 26) wants predictions to auto-score when market results become available, rather than requiring manual `journal prediction score`. Could use price data to auto-resolve price-target predictions. Files: `src/db/predictions.rs`, `src/commands/journal.rs`.
- [Feedback] **Correlation breaks × impact analysis cross-reference** — Low-Timeframe Analyst (95/90, Mar 25) wants correlation breaks cross-referenced with portfolio impact analysis for faster synthesis. Could add a `breaks_impact` field to situation room JSON showing which breaks affect held positions. Files: `src/analytics/situation.rs`.

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 65% | 68% | Mar 26 | ↓ (dropped from 78/75 on Mar 25. Scenario discoverability, predictions stats/unanswered errors, economy indicator confidence, wants auto-scored predictions. overnight-futures ✅ shipped. **Lowest scorer — critical priority.**) |
| Medium-Timeframe Analyst | 85% | 80% | Mar 25 | → (stable at 85/80. COT extreme detection praised. Regime transition alerts shipped #314.) |
| Low-Timeframe Analyst | 95% | 90% | Mar 25 | ↑ (surged from 85/80. Ratio-based alerts shipped #332. Wants correlation breaks × impact cross-reference.) |
| High-Timeframe Analyst | 85% | 75% | Mar 23 | → (no new review since Mar 23.) |
| Low-Timeframe Midday | 85% | 88% | Mar 23 | → (no new review since Mar 23.) |
| Morning Intelligence | 85% | 90% | Mar 23 | → (no new review since Mar 23.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent. System healthy.) |
| Dev Agent | 92% | 94% | Mar 26 | → (stable high. Shipped ratio alerts #332, predictions subcommands #334.) |

**Key changes since last review (Mar 25):**
- v0.17.0 released Mar 25. 46 new commits since tag.
- Shipped: ratio-based alerts (#332), predictions stats/scorecard/unanswered (#334), Dixon Power Flow Tracker (#327), market-hours command (#318), scenario probability alerts (#314)
- Tests: 1672 passing (up from 1628), clippy clean
- Evening Analyst **dropped** 78→65 usefulness, 75→68 overall — scenario discoverability, economy confidence, missing auto-scoring
- Low-Timeframe Analyst **surged** 85→95 usefulness, 80→90 overall — correlation breaks and ratio alerts praised
- Predictions stats/unanswered fix (#334) shipped in response to Evening Analyst's 65/68 — score impact TBD next review

**Top 2 priorities based on feedback:**
1. **P2: Auto-scored prediction lifecycle** — Evening Analyst wants predictions to resolve automatically from market data.
2. **P2: Correlation breaks × impact analysis cross-reference** — Low-Timeframe Analyst wants breaks cross-referenced with portfolio impact.

**Release eligibility:** ✅ READY — v0.18.0. 46 commits since v0.17.0, 5 significant features shipped, 1672 tests passing, clippy clean, no P0 bugs. New P1 items are discoverability/quality issues, not blockers.

**GitHub stars:** 5 — Homebrew Core requires 50+.
