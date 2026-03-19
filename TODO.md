# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Feedback-Driven Fixes

### [Feedback] Fix price snapshot command exit code 2

> Evening Analyst (Mar 19) reports the price snapshot command returning exit code 2. Need a reliable price-only snapshot path.
>
> Files to check: `src/commands/snapshot.rs` or `system snapshot`

## P1 — Always-On Analytics Engine

### F47: Dedicated Background Daemon

> Vision fit: pftui should be always running even when the TUI/web UI is closed.
>
> Current gap:
> - Background refresh exists inside TUI/web sessions
> - There is no first-class long-running daemon/service mode for ingestion + analytics
> - "Always-on" currently depends on a UI process or external cron
>
> Actionable scope:
> 1. Add `pftui system daemon` with refresh scheduler, lock coordination, structured logs,
>    and health heartbeat
> 2. Support per-source cadence config instead of one global interval
> 3. Run refresh, technical snapshot generation, level generation, alert evaluation, and cleanup in one loop
> 4. Expose daemon status via `pftui data status --json`
> 5. Add systemd launch docs for the daemon path as the recommended always-on deployment

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

### F49: Precomputed Signal Engine

> Vision fit: AI should receive mechanical signal state, not derive it from raw indicator values.
>
> Completed:
> 1. ✅ `technical_signals` table (SQLite + PostgreSQL) for per-symbol, per-timeframe signal events
> 2. ✅ Signal generation during refresh from stored technical snapshots (RSI overbought/oversold, MACD cross, SMA 200 reclaim/break, BB squeeze, volume expansion, 52W extremes)
> 3. ✅ Each signal includes severity, direction, trigger price, and explanation
> 4. ✅ `pftui analytics signals --source technical [--symbol SYM] [--json]` — also supports `--source all` (default, shows both) and `--source timeframe` (cross-layer only)
>
> Remaining:
> 5. Reuse the same store for alerts, movers context, and agent brief generation

## P2 — Coverage And Agent Consumption

### [Feedback] Add oil inventory/SPR data command

> Medium-Timeframe Analyst (Mar 19, 75/82) suggests adding `pftui data oil-inventory` or similar for EIA oil inventory and SPR data. Would enhance energy analysis without web searches.
>
> Files to check: `src/data/` for new data source module

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
- P0 TIMESTAMPTZ bug (PR #37) fixed Mar 19 — this was blocking data refresh for multiple agents for 24+ hours. Binary deployment also verified.
- F49 (Precomputed Technical Signal Engine, steps 1-4) shipped Mar 19 (PR #38). 49 signals from 80 symbols on first production refresh.
- F51 (Asset Intelligence Blob) shipped Mar 18 (PR #35).
- F46 (Market Structure Levels) surfaced in brief/web/TUI/alerts Mar 18 (PR #34).
- Evening Analyst improved 55→65 usefulness after `analytics scenario` CRUD shipped (PR #30), but still lowest due to `data sovereign` failures and empty `analytics summary`/`divergence` output.

**Top 2 priorities based on feedback:**

1. **P1: Fix `data sovereign` failures** — Both Evening Analyst and Medium-Timeframe Analyst report failures. Blocking two testers.
2. **P1: Fix `analytics summary`/`divergence` empty JSON** — Evening Analyst reports empty objects from core analytics consumption surfaces.

**Release status:** v0.12.1 shipped Mar 16. 108 commits since then including F45, F46, F49 (steps 1-4), F51, P0 TIMESTAMPTZ fix, batch prediction scoring, alert flapping cooldown, scenario CRUD. Build green: 1352 tests pass, clippy clean. **3 new P1 feedback bugs added this review.** Release should wait until the P1 feedback fixes land, then cut v0.13.0.

**GitHub stars:** 2 — Homebrew Core requires 50+.
