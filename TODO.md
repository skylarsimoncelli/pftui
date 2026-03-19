# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P3 — Long Term

## P1 — Always-On Analytics Engine

### F47: Dedicated Background Daemon

> Vision fit: pftui should be always running even when the TUI/web UI is closed.
>
> Current gap:
> - Background refresh exists inside TUI/web sessions
> - There is no first-class long-running daemon/service mode for ingestion + analytics
> - “Always-on” currently depends on a UI process or external cron
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
> Current gap:
> - ~~Cross-timeframe signals exist, but symbol-level technical signals are still mostly implicit~~ (Steps 1-4 shipped Mar 19)
> - ~~No normalized store for events like RSI overbought, MACD bull cross, MA reclaim, BB squeeze, volume expansion~~ (Shipped Mar 19)
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
| Alert Investigator | 95% | 90% | Mar 18 | → (consistently high, no issues) |
| Morning Brief Agent | 85% | 80% | Mar 17 | → (stable, minor threshold suggestion) |
| Low-Timeframe Analyst | 85% | 88% | Mar 17 | → (stable, batch scoring + cooldown requests) |
| Medium-Timeframe Analyst | 85% | 88% | Mar 18 | → (stable, wants conviction visualization) |
| Evening Analyst | 55% | 62% | Mar 18 | ↓ (missing `analytics scenario update`, confusing timeframe names) |

**Notes:**
- Evening Analyst is the clear outlier at 55/62. Root cause is unchanged from last review: `analytics scenario` only has `list`, not `update`. Agent had to use raw SQL. Also confused by prediction timeframe values (low/medium/high/macro vs short/medium/long).
- Alert Investigator is consistently 85-100% — no issues, system working as designed.
- Both low-timeframe and medium-timeframe analysts independently request prediction scoring improvements (batch scoring, pending-items interface).
- F45 (Persistent Technical Snapshots) shipped Mar 17, now removed from backlog.
- 50+ commits since v0.12.1 — F45, F46, F49 (steps 1-4), F51 are meaningful features.

**Top 3 priorities based on feedback:**

1. ~~**P1: Batch prediction scoring**~~ — Shipped Mar 18 (PR #31). `journal prediction score-batch` accepts multiple `id:outcome` pairs.
2. ~~**P2: Configurable overnight mover threshold**~~ — Already implemented: `analytics movers --threshold <pct>` exists with default 3%.
3. ~~**P2: Alert flapping cooldown logic**~~ — Shipped Mar 18. Added `alert_default_cooldown_minutes` config (default 30m) as floor for recurring alerts with cooldown_minutes=0.

**Release status:** v0.12.1 shipped Mar 16. F45 landed since then. `analytics scenario` CRUD alias shipped Mar 18 (PR #30). Build green: `cargo test` (1317 tests), `cargo clippy --all-targets -- -D warnings` clean. No P0 bugs remaining. Release eligible.

**GitHub stars:** 2 — Homebrew Core requires 50+.
