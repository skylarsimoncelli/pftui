# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Freshness & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

- [Feedback] **Portfolio impact estimate command** — Evening Analyst (Mar 23) wants `analytics impact-estimate` or similar that shows projected P&L under each scenario probability shift, rather than requiring manual calculation. Would move Evening Analyst scores significantly.
- [Feedback] **`macro cycles current` command** — Macro-Timeframe Analyst (Mar 22) requests a command to get 2026 power metrics directly rather than only historical data. Would streamline weekly structural analysis workflow.
- [Feedback] **Alert count in situation summary** — Low-Timeframe Analyst (Mar 22) suggests adding alert count/status to situation summary output for quicker operational awareness.
- [Feedback] **Agent message ack syntax clarity** — Evening Analyst (Mar 23) reports confusion between `ack-all` vs `ack --all` syntax. Consider adding both forms or better help text to reduce friction.
- [Feedback] **Weekend/after-hours CLI mode** — Low-Timeframe Analyst (Mar 21) suggests streamlining commands for non-market hours to skip stale intraday data and focus on positioning/prep.

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 72% | 70% | Mar 23 | ↓ (down from 72/78 on Mar 22 — overall dropped, wants impact-estimate command and clearer ack syntax) |
| Medium-Timeframe Analyst | 75% | 85% | Mar 23 | ↓ (down from 85/90 on Mar 22 — empty COT/calendar data hurt usefulness significantly) |
| Low-Timeframe Analyst | 85% | 88% | Mar 22 | → (stable high, wants alert count in situation summary) |
| Macro-Timeframe Analyst | 85% | 90% | Mar 22 | → (first data point, wants `macro cycles current` command) |
| Morning Brief | 85% | 90% | Mar 22 | → (first data point, analytics commands working well) |
| Morning Intelligence | 85% | 80% | Mar 21 | → (stable, no new data since Mar 21) |
| Alert Investigator | 85% | 90% | Mar 23 | → (stable, consistent routine monitoring) |
| Dev Agent | 90% | 92% | Mar 23 | → (shipping consistently, portfolio allocation #204 shipped) |

**Key changes since last review (Mar 22):**
- v0.15.0 released Mar 22 — 46 post-release commits: F53 Situation Engine (Phases 1-4), analytics weekly-review (#169), portfolio allocation (#204), website logo, plus feedback PRs
- Tests: 1590 passing (up from 1585), clippy clean
- Two testers regressed: Evening Analyst overall 78→70, Medium-Timeframe usefulness 85→75
- Evening Analyst: recovery stalled — wants portfolio-impact-estimate, ack syntax confusing
- Medium-Timeframe Analyst: significant drop due to empty data sources (COT/calendar)
- Portfolio allocation command shipped (#204) — closes previous top priority
- F53 Situation Engine fully shipped (all 4 phases) — closes previous top priority

**Top 3 priorities based on feedback:**
1. **P1: Data source refresh gaps** — COT/calendar empty for Medium-Timeframe Analyst. Biggest score regression this cycle.
2. **P2: Portfolio impact estimate** — Evening Analyst (lowest overall at 70%) needs scenario-aware P&L projections.
3. **P2: `macro cycles current`** — Quick access to current power metrics for Macro-Timeframe Analyst.

**Release eligibility:** v0.15.0 released yesterday. 46 new commits since tag but mostly feedback PRs and F53 feature work already in v0.15.0. No P0 bugs. Could release when the P1 data refresh issue is resolved — meaningful new features include portfolio allocation and situation engine integration.

**GitHub stars:** 4 — Homebrew Core requires 50+.
