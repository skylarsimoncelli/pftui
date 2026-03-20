# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 — Critical

### [Feedback] Movers scanner returning 0 results during extreme moves

> Evening Analyst (Mar 20, 45/55): `analytics movers` returned 0 results despite gold -10% and silver -14% — the biggest metals crash in 40 years. Suggests threshold, calculation, or data freshness issue in the movers pipeline. Investigate whether movers relies on stale daily close comparisons that break during multi-day crashes or if the threshold/filter logic is excluding extreme outliers.
>
> Files: `src/commands/movers.rs`, `src/analytics/technicals.rs`

## P1 — Always-On Analytics Engine

### F48: Rich OHLCV History And Data-Quality Layer (remaining)

> Steps 1-4 complete (PRs #56, #57, #59): schema upgraded, OHLCV persisted, ATR/range expansion shipped, backfill command shipped.
>
> Remaining scope:
> 1. Backfill OHLCV where providers support it (re-fetch history with OHLCV for existing symbols)

### [Feedback] `data news --json` fails with exit code 1 (text mode works)

> Evening Analyst (Mar 20): JSON output mode for news command exits with code 1 while text mode works fine. Agents consuming `--json` output get failures. Check JSON serialization path in news command for edge cases (empty results, special characters in article text, etc.).
>
> Files: `src/commands/news.rs` or equivalent news command handler

### [Feedback] Economy data inconsistencies and unclear units

> Medium-Timeframe Analyst (Mar 20, 70/75): `pftui data economy` shows fed_funds_rate=2% vs FedWatch showing 3.5-3.75% hold probability, NFP=19 with unclear units. Economy data values need source attribution, unit labels, and consistency checks against other pftui data sources.
>
> Files: `src/commands/economy.rs`, `src/data/fred.rs`

## P2 — Coverage And Agent Consumption

### F52: Refresh DAG, Parallelism, And Source Policies

> Vision fit: an always-on aggregator needs a scheduler and dependency graph, not just a long sequential refresh pass.
>
> Current gap:
> - `data refresh` is centralized, but much of it is still sequential and monolithic
> - freshness windows are hardcoded in command logic
> - source priorities and retry/backoff policies are not explicit
>
> Actionable scope:
> 1. Refactor refresh into a dependency-aware job graph
> 2. Parallelize safe source fetches with bounded concurrency and per-provider backoff
> 3. Move freshness thresholds and cadence policies into config/runtime policy structs
> 4. Emit structured refresh metrics: duration, failures, fallbacks, cached reuse, symbols updated
> 5. Add `pftui data refresh --json` summary output for agents and observability

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

1. **P0: Movers scanner returning 0 during extreme moves** — Evening Analyst's biggest pain (45/55). Missing the largest metals crash in 40 years is a critical gap for an analytics engine.
2. **P1: `data news --json` exit code 1** — JSON output broken while text works. Agents need reliable JSON.
3. **P1: Economy data inconsistencies** — Medium-Timeframe Analyst (70/75) confused by values without units/context.

**Release status:** v0.13.0 is current. 52 commits since tag. Major features landed (F48 steps 1-2, F50 universe, F47 daemon complete). Build green, 1420 tests pass, clippy clean. **Release v0.14.0 is eligible** once the P0 movers bug is resolved — or could ship now if the movers issue is deemed non-blocking since it's a data/threshold edge case rather than a crash.

**GitHub stars:** 2 — Homebrew Core requires 50+.
