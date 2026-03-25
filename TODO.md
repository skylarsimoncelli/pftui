# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

- [Feedback] **Alert summary for investigation continuity** — Alert Investigator (Mar 24) wants a command showing recent acknowledged alerts to maintain investigation context across cycles. May extend `analytics alerts list --acknowledged --recent`.
- [Feedback] **Weekend/after-hours CLI mode** — Low-Timeframe Analyst (Mar 21) suggests streamlining commands for non-market hours to skip stale intraday data and focus on positioning/prep.

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 72% | 74% | Mar 24 | → (stable low — overall ticked up 70→74, usefulness flat at 72. Still lowest scorer. Wants daily-pnl, correlations --json, performance-since-inception.) |
| Medium-Timeframe Analyst | 85% | 80% | Mar 24 | ↑ (recovered from 75/85 on Mar 23 — COT fix helped. Economy data format issues remain. Wants correlation breaks in situation room.) |
| Low-Timeframe Analyst | 75% | 85% | Mar 23 | ↓ (down from 85/90 earlier Mar 23 — deltas crash (#248 now fixed) hurt late session. Should recover next review.) |
| High-Timeframe Analyst | 85% | 75% | Mar 23 | ↓ (first scored review — economy data quality issues dragged overall down. Trend evidence system praised.) |
| Low-Timeframe Midday | 85% | 88% | Mar 23 | → (stable, minor --json gaps noted) |
| Morning Intelligence | 85% | 90% | Mar 23 | → (stable, regime confidence request noted) |
| Alert Investigator | 75-85% | 80% | Mar 24 | → (stable, consistent routine monitoring. Wants alert summary for continuity.) |
| Dev Agent | 92% | 94% | Mar 24 | ↑ (highest scorer, shipping consistently — deltas crash fix #248 deployed cleanly) |

**Key changes since last review (Mar 23):**
- 96 commits since v0.15.0 tag (Mar 22): F53 Situation Engine (all 4 phases), macro cycles current (#232), impact-estimate (#218), portfolio allocation (#204), ack --all (#226), alert summary in situation (#240), COT freshness fix (#212), deltas deserialize crash fix (#248), macOS desktop client
- Tests: 1604 passing (up from 1601), clippy clean
- Evening Analyst overall ticked up 70→74 but still lowest scorer — analytics praised but missing daily-pnl and correlations tooling
- Medium-Timeframe recovered 75→85 usefulness after COT fix, but economy data format issues flagged
- Low-Timeframe Analyst hit by deltas crash (now fixed) — expect recovery
- High-Timeframe Analyst first review: 85/75 — trend system praised, economy data quality issues hurt
- Dev Agent at all-time high 92/94

**Top 3 priorities based on feedback:**
1. **P2: Alert summary for investigation continuity** — Alert Investigator wants acknowledged alert history.
2. **P2: Weekend/after-hours CLI mode** — Low-Timeframe Analyst wants streamlined non-market-hours commands.

**Release eligibility:** v0.15.0 released Mar 22. 96 new commits with major features (F53 Situation Engine, impact-estimate, macro cycles current, portfolio allocation, alert summary, COT fix, deltas crash fix). All tests pass (1604), clippy clean, no P0 bugs. **Ready to release v0.16.0.**

**GitHub stars:** 5 — Homebrew Core requires 50+.
