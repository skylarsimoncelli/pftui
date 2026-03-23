# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Freshness & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

- [Feedback] **Alert count in situation summary** — Low-Timeframe Analyst (Mar 22) suggests adding alert count/status to situation summary output for quicker operational awareness.
- [Feedback] **Weekend/after-hours CLI mode** — Low-Timeframe Analyst (Mar 21) suggests streamlining commands for non-market hours to skip stale intraday data and focus on positioning/prep.

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 72% | 70% | Mar 23 | ↓ (down from 72/78 on Mar 22 — ack syntax clarity shipped #226, impact-estimate shipped #218) |
| Medium-Timeframe Analyst | 75% | 85% | Mar 23 | ↓ (down from 85/90 on Mar 22 — empty COT/calendar data hurt usefulness significantly) |
| Low-Timeframe Analyst | 85% | 88% | Mar 22 | → (stable high, wants alert count in situation summary) |
| Macro-Timeframe Analyst | 85% | 90% | Mar 22 | → (first data point, wants `macro cycles current` command) |
| Morning Brief | 85% | 90% | Mar 22 | → (first data point, analytics commands working well) |
| Morning Intelligence | 85% | 80% | Mar 21 | → (stable, no new data since Mar 21) |
| Alert Investigator | 85% | 90% | Mar 23 | → (stable, consistent routine monitoring) |
| Dev Agent | 90% | 92% | Mar 23 | → (shipping consistently, impact-estimate #218 shipped) |

**Key changes since last review (Mar 22):**
- v0.15.0 released Mar 22 — 48 post-release commits: F53 Situation Engine (Phases 1-4), analytics weekly-review (#169), portfolio allocation (#204), impact-estimate (#218), COT fix, plus feedback PRs
- Tests: 1601 passing (up from 1598), clippy clean
- Two testers regressed: Evening Analyst overall 78→70, Medium-Timeframe usefulness 85→75
- Evening Analyst: impact-estimate command shipped (#218) — should help recovery. Still wants ack syntax clarity.
- Medium-Timeframe Analyst: COT freshness fix shipped — should restore data availability
- Portfolio allocation command shipped (#204), impact-estimate shipped (#218)
- F53 Situation Engine fully shipped (all 4 phases)

**Top 3 priorities based on feedback:**
1. **P2: `macro cycles current`** — Quick access to current power metrics for Macro-Timeframe Analyst.
2. **P2: Alert count in situation summary** — Low-Timeframe Analyst wants alert count in summary for quicker awareness.
3. **P2: Weekend/after-hours CLI mode** — Low-Timeframe Analyst wants streamlined commands for non-market hours.

**Release eligibility:** v0.15.0 released Mar 22. 48 new commits since tag including portfolio allocation (#204), impact-estimate (#218), COT fix, and feedback PRs. No P0 bugs. Ready to release — meaningful new features include portfolio allocation, impact-estimate, and COT freshness fix.

**GitHub stars:** 4 — Homebrew Core requires 50+.
