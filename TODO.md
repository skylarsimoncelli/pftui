# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Always-On Analytics Engine

- **F53: Situation Engine — Phase 1 + Phase 2 + Phase 3 complete, remaining:** Phase 4 = agent routine updates (agents use `situation update log`, `situation indicator list`, `situation exposure` in their workflows).

## P2 - Coverage And Agent Consumption

- [Feedback] **`portfolio allocation` shortcut** — Evening Analyst (Mar 22) wants a quick allocation view without running full `portfolio summary`. Could be as simple as `portfolio drift` with a more intuitive alias or a dedicated slim output mode.
- [Feedback] **Weekend/after-hours CLI mode** — Low-Timeframe Analyst (Mar 21) suggests streamlining commands for non-market hours to skip stale intraday data and focus on positioning/prep.

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 72% | 78% | Mar 22 | ↑ (up from 65/72 on Mar 21, continuing recovery — analytics working better, but performance bug and missing weekly-review) |
| Medium-Timeframe Analyst | 85% | 90% | Mar 22 | → (stable high, minor syntax learning curve, batch scoring exists but wasn't discovered) |
| Alert Investigator | 85% | 82% | Mar 22 | → (stable, consistent routine monitoring, wants correlation detection and regime notifications) |
| Morning Intelligence | 75% | 80% | Mar 20 | → (no new data since Mar 20, noted non-existent analytics commands in routine) |
| Low-Timeframe Analyst | 85% | 90% | Mar 21 | → (stable high, wants weekend/after-hours mode) |
| Dev Agent | 92% | 93% | Mar 22 | → (shipping consistently, F53 Phase 2 shipped PR #182, indicator evaluation in refresh pipeline) |

**Key changes since last review (Mar 21):**
- v0.14.1 released Mar 21 (3 code fixes: plausibility guard #99, journal alert section #107, mobile API runtime #112)
- 74 commits since v0.14.1: `system search` (#155), scenario probabilities in analytics (#148), correlation breaks (#141), oil-premium (#134), change_1d scan field (#127), narrative layer, synthesis engine, impact/opportunities engine, catalysts engine, deltas engine, situation room enhancements, website fixes
- Tests: 1585 passing (up from 1578), clippy clean
- Evening Analyst continuing recovery: 65/72 → 72/78 — still lowest but trending up
- All previous top 3 priorities RESOLVED: CLI discoverability (#155), scenario probabilities (#148), routine integration (109fd67)
- `system search` command now helps agents discover existing features like `score-batch`
- New P0: `portfolio performance` TIMESTAMPTZ bug found by Evening Analyst

**Top 3 priorities based on feedback:**
1. **P1: F53 Situation Engine** — Major feature to evolve static scenarios into living, data-connected monitoring.
2. **P2: `portfolio allocation` shortcut** — Quick allocation view without full `portfolio summary`.
3. **P2: Weekend/after-hours CLI mode** — Streamline commands for non-market hours.

**Release status:** v0.15.0 released Mar 22. 76 post-release commits from v0.14.1 including: system search (#155), scenario probabilities (#148), correlation breaks (#141), oil-premium (#134), narrative/synthesis/catalysts/deltas/impact/opportunities engines, situation room enhancements, portfolio performance TIMESTAMPTZ fix (#166). No P0 bugs.

**GitHub stars:** 4 — Homebrew Core requires 50+.
