# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Always-On Analytics Engine

### F48: Rich OHLCV History And Data-Quality Layer

> Vision fit: technical analysis quality depends on data quality and richer candles than close-only series.
>
> Current gap:
> - `price_history` logic mostly relies on close and partial volume
> - open/high/low are optional in models but not treated as first-class stored inputs
> - this limits robust breakout, wick, range, ATR, and volatility analysis
>
> Actionable scope:
> 1. Upgrade historical storage so open/high/low/close/volume are fully persisted and queryable
> 2. Backfill OHLCV where providers support it
> 3. Add data-quality metadata per symbol: coverage, stale bars, source, gaps, split-adjust ambiguity
> 4. Add `pftui analytics gaps --symbol SYM` or equivalent asset-level data-quality output
> 5. Use OHLCV-aware calculations for Bollinger, ATR, range expansion, and breakout detection



## P2 — Coverage And Agent Consumption

### F50: Configurable Universe Expansion

> Vision fit: the system should analyze more than just current holdings and watchlist when running always-on.
>
> Current gap:
> - Refresh symbol discovery is driven by portfolio, watchlist, economy symbols, and sector ETFs
> - There is no first-class tracked-universe config for sectors, indices, macro proxies, or custom symbol packs
>
> Actionable scope:
> 1. Add `tracked_universe` config groups for indices, sectors, commodities, FX, rates, crypto majors, and custom symbols
> 2. Feed the universe into refresh, technical snapshots, levels, and signals
> 3. Add CLI commands to inspect and mutate tracked universes
> 4. Ensure per-source rate limits and refresh cadences remain safe

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

### F39.7b: Historical Power Metrics Data Population (Sentinel)

> After dev cron ships F39.7 CLI + schema, spawn a research sub-agent to populate
> the historical database. The sub-agent should:
>
> 1. Research each determinant for each power at each decade using web_search
> 2. Score on Dalio's 1-10 scale with brief justification and source
> 3. Populate via `pftui analytics macro cycles history add` CLI commands
> 4. Cross-reference Dalio's own charts from "Principles for Dealing with
>    the Changing World Order" as a baseline, then refine with primary sources
>
> Powers and spans:
> - US: 1900-2020 (13 decades)
> - China: 1900-2020 (13 decades)
> - Russia/USSR: 1900-2020 (13 decades, note regime transitions)
> - UK/British Empire: 1900-2020 (13 decades, the decline narrative)
> - Japan: 1900-2020 (13 decades, rise and plateau)
> - EU: 1950-2020 (8 decades, post-ECSC)
> - India: 1950-2020 (8 decades, post-independence)
> - Saudi: 1940-2020 (9 decades, post-oil discovery)
>
> Estimated: ~700 rows. Each needs a score, notes, and source.
> Break into multiple sub-agent runs by country if needed.

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
2. **P2: F50 Configurable Universe Expansion** — track more symbols beyond holdings/watchlist
3. **P2: F52 Refresh DAG / source policies** — move beyond the current sequential refresh pipeline

**Release status:** v0.13.0 shipped Mar 19. F47 daemon rollout is complete. CI green. Ready for the next feature cycle.

**GitHub stars:** 2 — Homebrew Core requires 50+.
