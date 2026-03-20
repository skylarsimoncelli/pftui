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

**Key changes since last review (Mar 20):**
- v0.14.0 released (84 commits since v0.13.0, PR #84, tag pushed)
- All P0/P1 bugs fixed: movers stale-close (#65), movers empty history (#75), economy reconciliation (#74), news --json (#71), prediction UX (#73)
- Bulk acknowledge for alerts and agent messages (#79)
- Economy plausibility validation (#67), CI Postgres parity fix (#72)

**Top priorities based on feedback:**

_(All tracked issues resolved. TODO backlog empty. Accepting new feature requests.)_

**Release status:** **v0.14.0 released** (Mar 20, 2026). 1495 tests pass, clippy clean. Release pipeline triggered — builds for Linux, macOS, Windows, .deb, .rpm, Docker, iOS, crates.io, Homebrew.

**GitHub stars:** 2 — Homebrew Core requires 50+.
