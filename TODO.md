# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Always-On Analytics Engine

### F48: Rich OHLCV History And Data-Quality Layer (remaining)

> Steps 1, 3, 4 complete (PR #56): schema upgraded, OHLCV persisted from Yahoo, `analytics gaps --symbol` shipped.
> Step 2 (OHLCV-aware calculations) complete (PR pending): ATR-14, ATR ratio, range expansion, day range ratio added to technical snapshots.
>
> Remaining scope:
> 1. Backfill OHLCV where providers support it (re-fetch history with OHLCV for existing symbols)



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

## P3 — Long Term

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Alert Investigator | 85% | 90% | Mar 19 | → (consistently high, stable) |
| Morning Brief Agent | 85% | 82% | Mar 18 | → (alert-watchdog cron errors noted) |
| Low-Timeframe Analyst | 75% | 80% | Mar 18 | ↓ (data refresh timestamp bug — now fixed by PR #37) |
| Medium-Timeframe Analyst | 75% | 82% | Mar 19 | ↓ (data sovereign failures, wants oil inventory data) |
| Evening Analyst | 65% | 72% | Mar 19 | ↑ (improved from 55/62; still lowest — empty analytics, price snapshot fails, sovereign broken) |
| Dev Agent | 90% | 88% | Mar 19 | → (shipping features and fixes consistently) |

**Key changes since last review:**
- F49 complete: technical signals wired into brief + movers JSON (PR #52)
- Broker integration shipped (PR #41): Trading212, IBKR, Binance, Kraken, Coinbase, Crypto.com
- `data prices` consolidated endpoint shipped (PR #47)
- `data oil-inventory` EIA command shipped (PR #44)
- `data sovereign` COMEX failures fixed (PR #45)
- `analytics summary`/`divergence` empty JSON fixed (PR #46)
- Price snapshot exit code 2 fixed (PR #47 — `data prices` replaces missing command)
- Postgres parity CI fixed (PR #50)
- CHANGELOG.md conflict markers resolved (PR #52 batch)
- All 3 P1 feedback bugs from Mar 19 review resolved

**Top priorities:**

1. **P1: F48 Rich OHLCV History** — upgrade from close-only to full candle data
2. **P2: F52 Refresh DAG / source policies** — move beyond the current sequential refresh pipeline

**Release status:** v0.13.0 shipped Mar 19. F47 daemon rollout is complete. CI green. Ready for the next feature cycle.

**GitHub stars:** 2 — Homebrew Core requires 50+.
