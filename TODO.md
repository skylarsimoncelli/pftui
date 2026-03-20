# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 — Critical

_(none)_

## P1 — Always-On Analytics Engine

_(none)_

## P2 — Coverage And Agent Consumption

_(none)_

## P3 — Long Term

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 45% | 55% | Mar 20 | ↓↓ (movers scanner returning 0 during extreme moves, news --json broken, prediction UX) |
| Medium-Timeframe Analyst | 70% | 75% | Mar 20 | ↓ (economy data inconsistencies, web research still needed for context) |
| Alert Investigator | 85% | 80% | Mar 20 | → (stable, routine functioning, no false positives) |
| Morning Brief Agent | 85% | 82% | Mar 18 | → (alert-watchdog cron errors noted) |
| Low-Timeframe Analyst | 85% | 90% | Mar 19 | ↑ (up from 75/80, all commands working, good JSON output) |
| High-Timeframe Analyst | 85% | 82% | Mar 19 | → (excellent for trend tracking, wants automated correlation detection) |
| Dev Agent | 90% | 90% | Mar 20 | → (shipping consistently, F48 step 2 clean) |

**Key changes since last review (Mar 19):**
- F48 step 2 complete: ATR, range expansion, breakout detection (PR #57)
- F48 OHLCV backfill command shipped (PR #59)
- F50 configurable universe expansion shipped (PR #60)
- CONTRIBUTING.md and branch protection docs added (PR #58)
- F39.7b historical data population completed (810 rows)

**Top priorities based on feedback:**

_(P1 economy data inconsistencies resolved in PR #74 — cross-source reconciliation, source/confidence metadata)_

**Release status:** v0.13.0 is current. 53 commits since tag. P0 movers stale-close bug fixed (PR #65). Build green, 1440 tests pass, clippy clean. **Release v0.14.0 is eligible** — no P0 blockers remaining.

**GitHub stars:** 2 — Homebrew Core requires 50+.
