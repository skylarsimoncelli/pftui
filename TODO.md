# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P3 — Long Term

## P1 — Always-On Analytics Engine

### F46: Stored Market Structure And Key Levels

> Vision fit: pftui should map key levels mechanically so AI reasons on top of them.
>
> Current gap:
> - No persisted support/resistance/trigger levels engine
> - AI still has to infer structure from raw history and indicator output
> - Alerts can use indicators, but the system does not expose normalized market structure
>
> Actionable scope:
> 1. Add `technical_levels` table for support, resistance, breakout, breakdown, gap-fill,
>    20D/50D/200D MA levels, recent swing highs/lows, and 52W extremes
> 2. Compute levels from cached history during refresh for held + watchlist + configured universe symbols
> 3. Assign strength/confidence and source method to each level
> 4. Add `pftui analytics levels --symbol SYM --json`
> 5. Surface nearest actionable levels in `portfolio brief`, asset detail, and web asset endpoints
> 6. Allow alert creation directly from stored levels

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
> - Cross-timeframe signals exist, but symbol-level technical signals are still mostly implicit
> - No normalized store for events like RSI overbought, MACD bull cross, MA reclaim, BB squeeze, volume expansion
>
> Actionable scope:
> 1. Add `technical_signals` table for per-symbol, per-timeframe signal events
> 2. Generate signals during refresh from stored technical snapshots and levels
> 3. Include severity, direction, trigger price, expiry/staleness, and explanation
> 4. Add `pftui analytics signals technical [--symbol SYM] [--json]`
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

### F51: Asset Intelligence Blob

> Vision fit: the agent should be able to ask for one symbol and receive the full synthesized state.
>
> Current gap:
> - Analytics are available across several commands, but there is no canonical per-asset intelligence payload
> - Web/API handlers also compute and assemble slices independently
>
> Actionable scope:
> 1. Add `pftui analytics asset <SYMBOL> --json`
> 2. Return spot price, OHLCV stats, technical snapshot, key levels, technical signals,
>    correlations, regime context, scenario/trend/structural impacts, alerts, and freshness
> 3. Reuse the same view model in CLI, web, and future agent integrations
> 4. Treat this as the default AI consumption surface for market analysis

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
| Morning Market Research | 88% | 82% | Mar 7 | ↑ (Mar 8-9 crash/hang since fixed) |
| Evening Eventuality Planner | 55% | 62% | Mar 17 | ↓ (missing `analytics scenario update`, nonexistent subcommands) |
| Sentinel Main TUI Review | 75% | 72% | Mar 10 | ↓ (display corruption, missing day P&L $) |
| Market Close | 60% | 72% | Mar 9 | ↕ (movers bug + TIMESTAMPTZ crash, both fixed) |

**Notes:**
- Morning Research Mar 7 score (88/82) represents post-fix trajectory after Mar 8-9 crashes were resolved.
- Evening Planner dropped on Mar 17: tried `analytics scenario update` (doesn't exist — command lives at `journal scenario update`), and guessed nonexistent `data prices`/`portfolio snapshot` subcommands. Root cause is namespace discoverability, not missing functionality.
- Mar 16 run added `analytics scenario list --json`, `analytics conviction set`, `analytics macro regime set` aliases — but `analytics scenario update` alias was NOT added. This is the specific gap.
- Sentinel has requested day P&L in dollars in *every single review since Mar 2* — still the most consistently requested feature.
- Agent feedback (Mar 12-17) is predominantly P2 enhancement requests, not regressions.

**Top 3 priorities based on feedback:**

1. **`analytics scenario update` alias** — Evening Planner hit this on Mar 17. The command exists at `journal scenario update` but `analytics scenario` only has `list`. Add `update` (and other CRUD) as analytics aliases to match the list alias that was already added.
2. **TUI day P&L $ column** — Sentinel requests this in every review. Most consistently requested feature across all testers since Mar 2.
3. **Keep release quality green** — `cargo clippy --all-targets -- -D warnings` and test suite should stay clean.

**Release status:** v0.12.1 shipped Mar 16. Only P3 items remain in backlog. Build green: `cargo test` (1297 tests), `cargo clippy --all-targets -- -D warnings` clean.

**GitHub stars:** 1 — Homebrew Core requires 50+.
