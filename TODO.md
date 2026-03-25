# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

- [x] ~~**Alert summary for investigation continuity** — Added `--recent` and `--recent-hours` flags to `analytics alerts list`. Merged in PR #309 (Mar 25).~~
- [x] ~~**Weekend/after-hours CLI mode** — Added `system market-hours [--json]` command for session-aware agent routines. Merged in PR #318 (Mar 25).~~
- [x] ~~**Regime transition alerts on scenario probability shifts** — New `scenario` AlertKind auto-fires when probability shifts ≥10pp. Merged in PR #314 (Mar 25).~~

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 78% | 75% | Mar 25 | ↑ (up from 72/74 on Mar 24. Situation/narrative/deltas commands praised. Still lowest scorer — `analytics predictions` and `analytics situation list` returned empty.) |
| Medium-Timeframe Analyst | 85% | 80% | Mar 25 | → (stable at 85/80. COT extreme detection praised. Wants regime transition alerts on probability shifts >10%.) |
| Low-Timeframe Analyst | 85% | 80% | Mar 24 | ↑ (recovered from 75/85 crash on Mar 23 — deltas fix helped. Wants integrated news sentiment scoring.) |
| High-Timeframe Analyst | 85% | 75% | Mar 23 | → (no new review since Mar 23. Economy data quality fixes shipped.) |
| Low-Timeframe Midday | 85% | 88% | Mar 23 | → (stable, no new review.) |
| Morning Intelligence | 85% | 90% | Mar 23 | → (stable, no new review.) |
| Alert Investigator | 85% | 80-82% | Mar 25 | → (stable, consistent routine monitoring. Multiple reviews today — system working well. Wants alert summary for continuity.) |
| Dev Agent | 92% | 94% | Mar 25 | → (stable high. Shipped correlation breaks #291 cleanly. Codebase praised as well-structured.) |

**Key changes since last review (Mar 24):**
- v0.16.0 released Mar 24. 47 new commits since tag.
- Shipped: correlation breaks in situation room (#291), correlations --json + list subcommand (#283), portfolio unrealized (#277), portfolio daily-pnl (#270), analytics situation --json fix (#263), economy data format fix (#257), mobile TLS cert fix
- Tests: 1628 passing (up from 1626), clippy clean
- Evening Analyst improved 72→78 usefulness, 74→75 overall — situation/narrative/deltas praised but predictions/situation-list discoverability hurt
- Medium-Timeframe stable at 85/80 — COT extreme detection was critical for repositioning calls
- Low-Timeframe Analyst recovered to 85/80 from 75/85 crash
- Alert Investigator stable and consistent across multiple daily reviews

**Top 3 priorities based on feedback:**
1. ~~**P1: `analytics predictions` discoverability**~~ — SHIPPED #300. `analytics predictions` now aliases `data predictions`.
2. ~~**P1: `analytics situation list` empty with no guidance**~~ — SHIPPED #300. JSON output now returns structured object with hint.
3. ~~**P2: Regime transition alerts**~~ — SHIPPED #314. Scenario alerts auto-fire on ≥10pp probability shifts.

**Release eligibility:** v0.17.0 released. All P1 items shipped (#300 — analytics predictions alias + situation list guidance). Tests: 1659 passing (+8 new from #327 power-flow), clippy clean, no P0 bugs. No P1 remaining. All P2 and P3 items shipped.

**GitHub stars:** 5 — Homebrew Core requires 50+.
