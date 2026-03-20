# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 — Critical

_(none)_

## P1 — Always-On Analytics Engine

### [Feedback] Economy data inconsistencies and unclear units — PARTIALLY FIXED

> ~~Medium-Timeframe Analyst (Mar 20, 70/75): NFP=19, PMI=2025, claims=8000 — garbage values from Brave Search extraction.~~
> Fixed: Added plausibility validation (indicator-specific bounds) and unit labels in JSON output (PR #67). Remaining: fed_funds_rate discrepancy vs FedWatch is a different-source issue (Brave vs FRED), not validation. Consider cross-source reconciliation as follow-up.
>
> Files: `src/commands/economy.rs`, `src/data/economic.rs`

## P2 — Coverage And Agent Consumption


### [Feedback] Prediction CLI positional vs flag syntax confusion

> Evening Analyst (Mar 18, Mar 20): `prediction add` requires positional timeframe syntax but not `--timeframe` flag. Timeframe values (low/medium/high/macro) differ from intuitive names (short/medium/long). Consider accepting both positional and flag syntax, and adding aliases for common timeframe names.
>
> Files: `src/cli.rs`, `src/commands/predict.rs`

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

**Top 3 priorities based on feedback:**

1. **P1: Economy data inconsistencies** — Medium-Timeframe Analyst (70/75) confused by values without units/context.
2. **P2: Prediction CLI syntax confusion** — Evening Analyst wants `--timeframe` flag and intuitive aliases.

**Release status:** v0.13.0 is current. 53 commits since tag. P0 movers stale-close bug fixed (PR #65). Build green, 1440 tests pass, clippy clean. **Release v0.14.0 is eligible** — no P0 blockers remaining.

**GitHub stars:** 2 — Homebrew Core requires 50+.
