# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 — Critical

### Fix clippy `-D warnings` failure

`cargo clippy --all-targets -- -D warnings` fails with `unnecessary_first_then_check` in `src/tui/views/analytics.rs:321` and `field_reassign_with_default` in `src/app.rs:8127`. Must be clean before any release.

Files: `src/tui/views/analytics.rs`, `src/app.rs`.

---

## P1 — Feature Requests

### F44: Smart Alerts — Technical & Macro Pattern Detection

The current alerts system is price-threshold only (`BTC below 55000`). Agents are doing pattern recognition work that pftui should compute natively. When BTC loses its 200-day SMA, pftui should detect that and raise an alert, not wait for an agent to pull the data and figure it out.

This is the single biggest leverage point for moving from 55% pftui to 70%+ pftui in the agent workflow.

#### F44.1: Technical Alerts

Add `kind = "technical"` alerts that fire on computed indicator conditions:

```
pftui analytics alerts add --kind technical --symbol BTC \
  --condition "price_below_sma200" --label "BTC lost 200-day SMA"

pftui analytics alerts add --kind technical --symbol GC=F \
  --condition "rsi_above_70" --label "Gold overbought RSI"

pftui analytics alerts add --kind technical --symbol SI=F \
  --condition "price_below_sma50" --label "Silver below 50-day SMA"
```

Supported conditions (use pre-computed data from `data refresh`):
- `price_above_sma50`, `price_below_sma50` (and sma200)
- `rsi_above_70`, `rsi_below_30` (overbought/oversold)
- `macd_cross_bullish`, `macd_cross_bearish`
- `bollinger_break_upper`, `bollinger_break_lower`
- `price_change_pct_above_X`, `price_change_pct_below_X` (daily % move)
- `correlation_break` (30-day correlation diverges >2 std from 90-day)

These evaluate against data already in the DB from `data refresh`. No new data fetching needed.

Files: `src/commands/alerts.rs`, `src/db/alerts.rs`. Schema: add `condition` column to `alerts` table.

#### F44.2: Macro/Economic Alerts

Add `kind = "macro"` alerts that fire on economic data changes:

```
pftui analytics alerts add --kind macro \
  --condition "vix_regime_shift" --label "VIX crossed regime threshold"

pftui analytics alerts add --kind macro \
  --condition "yield_curve_inversion_change" --label "Yield curve inverted/uninverted"

pftui analytics alerts add --kind macro \
  --condition "regime_change" --label "Market regime shifted"

pftui analytics alerts add --kind macro \
  --condition "fear_greed_extreme" --label "F&G hit extreme zone"
```

Supported conditions:
- `regime_change` (risk-off → risk-on, or vice versa)
- `vix_regime_shift` (crossed 20, 25, 30, 35 thresholds)
- `fear_greed_extreme` (below 15 or above 85)
- `yield_curve_inversion_change` (10Y-2Y spread sign change)
- `dxy_century_cross` (DXY crossed 100 in either direction)
- `correlation_regime_break` (major correlation pair diverges from historical)

Files: `src/commands/alerts.rs`, `src/db/alerts.rs`, `src/data/macro_alerts.rs`.

#### F44.3: Alert Evaluation Engine

`data refresh` currently updates prices and computes technicals. Extend it to evaluate all active smart alerts after each refresh:

```
pftui data refresh
  ... existing refresh ...
  ✓ Smart alerts evaluated (3 triggered, 22 armed)
```

Triggered alerts write to a `triggered_alerts` log table with timestamp, condition, and the data that triggered them. Alerts can be `one-shot` (trigger once, go to triggered) or `recurring` (re-arm after trigger, with cooldown period).

Schema: `triggered_alerts` table: id, alert_id, triggered_at, trigger_data (JSON), acknowledged.

Files: `src/data/refresh.rs`, `src/db/triggered_alerts.rs`.

#### F44.4: Alert Watchdog Cron

Lightweight Haiku-level agent cron that runs every hour:

1. Runs `pftui data refresh` (re-evaluates all smart alerts)
2. Runs `pftui analytics alerts list --triggered --since 1h --json`
3. If any new triggers: sends a concise alert message to Telegram
4. If nothing: replies `NO_REPLY`

This replaces the current pattern where LOW/MEDIUM/HIGH agents discover technical breaks during their scheduled runs (which could be hours after the break happened).

Cron config:
- Schedule: every hour, 24/7
- Model: Haiku (cheapest, fastest)
- Delivery: message tool to Telegram on triggers, NO_REPLY otherwise
- Timeout: 60 seconds

Cost estimate: ~$0.01-0.02 per run (Haiku), ~$0.50/day for 24 runs. Negligible.

#### F44.5: Seed Default Alert Set

Ship with a sensible default set of smart alerts for common use cases:

```
pftui analytics alerts seed-defaults
```

Defaults:
- All portfolio symbols: SMA200 cross (both directions)
- All portfolio symbols: daily move >5%
- VIX: crossed 20, 25, 30, 35
- DXY: crossed 100
- Regime: any change
- Fear & Greed: extreme (<15 or >85)
- RSI: overbought/oversold on portfolio symbols

Users can remove or customize after seeding.

Files: `src/commands/alerts.rs`.

### F45: Reduce Agent web_search Dependency

Agents currently use web_search for ~45% of their data needs. These items target data that has structured API sources and can be computed/cached by `data refresh`, eliminating agent guesswork and improving data quality.

#### F45.5: Analyst Consensus Tracker

Agents web_search for "Goldman Sachs rate forecast" and "JP Morgan gold target" repeatedly. These change slowly (weekly/monthly). Add a `consensus` table where agents can log analyst calls via CLI:

```
pftui data consensus add --source "Goldman Sachs" --topic "rate_cuts" \
  --call "50bp cuts in Sep+Dec 2026" --date 2026-03-12
pftui data consensus add --source "JP Morgan" --topic "gold_target" \
  --call "$6,300 by year-end 2026" --date 2026-02-25
pftui data consensus list --topic rate_cuts --json
```

Agents log consensus calls when they find them, then all agents can read them from the DB instead of re-searching. MEDIUM agent maintains this during its research phase.

Files: `src/commands/data.rs`, `src/db/consensus.rs`. Schema: new `consensus_tracker` table.

#### F45.6: COT Positioning Interpretation

CFTC COT data is already fetched but agents still web_search to interpret it ("is gold COT positioning extreme?"). Add computed fields:

- Percentile rank of net positioning (vs 1yr and 3yr history)
- Z-score of current position vs rolling mean
- Extreme flag when positioning hits 90th+ or 10th- percentile

```
pftui data cot --json
{"gold": {"net_long": 142000, "percentile_1y": 85, "z_score": 1.4, "extreme": false}, ...}
```

Files: `src/data/cot.rs`, `src/commands/data.rs`.

### F46: Remote PostgreSQL Backend Support

The setup wizard currently offers SQLite only. Add full backend selection:

```
? Select database backend:
  ❯ Local SQLite (default, zero config)
    Local PostgreSQL (localhost)
    Remote PostgreSQL (custom host)
```

**Local SQLite:** Current default. No changes needed.

**Local PostgreSQL:** Prompt for database name, user, password. Host defaults to `127.0.0.1:5432`. Test connection before proceeding.

**Remote PostgreSQL:** Prompt for host, port, database name, user, password. Optionally accept a full connection string (`postgres://user:pass@host:port/db`). Test connection before proceeding. Support SSL/TLS option for cloud-hosted databases (Supabase, Neon, RDS, etc.).

Config output (`config.toml`):
```toml
database_backend = "postgres"
database_url = "postgres://user:pass@remote-host:5432/pftui?sslmode=require"
```

The Rust backend dispatch already supports Postgres fully. This is purely a setup wizard and config UX change.

Also update `pftui system setup` (if it exists) or the first-run wizard to offer the same options.

Files: `src/setup.rs` (or wherever the wizard lives), `src/config.rs`.

### [Feedback] Weekend-Aware Movers Command

`pftui analytics movers` shows 0 movers on weekends because it compares to Friday close. Should compare Friday close to weekend crypto/futures prices (Hyperliquid, Binance perpetuals) so agents running Saturday/Sunday routines still see meaningful movements.

Source: evening-analysis feedback (Mar 15). Files: `src/commands/movers.rs`.

### [Feedback] `analytics scenario list --json`

`pftui analytics scenario list` should support `--json` output for programmatic consumption. Currently agents must cross-reference scenario names manually. Most other analytics commands already support `--json`.

Source: evening-analysis feedback (Mar 15). Files: `src/commands/scenario.rs`, `src/cli.rs`.

### [Feedback] Missing `analytics conviction set` and `analytics macro regime set` CLI paths

Evening analyst (Mar 16) scored 55/68 because `analytics conviction set` and `analytics macro regime set` commands are missing or not routed. These are critical for agent routines that programmatically update convictions and regime classifications. Verify the CLI tree routes these correctly under the F42 five-domain hierarchy.

Source: evening-analyst feedback (Mar 16). Files: `src/cli.rs`, `src/main.rs`, `src/commands/analytics.rs`.

---

## P2 — Nice to Have

### [Feedback] `scenario update --notes` inline annotation

`pftui scenario update` should support `--notes` flag for inline annotation. Currently errors with unexpected argument when agents try to add context alongside probability updates. (Note: `--notes` was added as alias for `driver` in Mar 12 changelog — verify it works end-to-end or fix routing.)

Source: multiple agent feedback (Mar 10, 13, 16). Files: `src/commands/scenario.rs`, `src/cli.rs`.

### [Feedback] Prediction command ergonomics

`pftui predict add` timeframe param rejected but not documented in help. Add `--confidence` flag for prediction confidence scoring. Positional args for `predict score` should work alongside flag syntax.

Source: morning-intelligence, evening-analyst feedback (Mar 13-14). Files: `src/commands/predict.rs`, `src/cli.rs`.

### [Feedback] Agent message data quality flagging

No mechanism for agents to flag data quality issues in received messages. Add `agent-msg flag --quality` or similar so receiving agents can mark messages as containing errors and alert the sender.

Source: evening-analysis feedback (Mar 12). Files: `src/commands/agent_msg.rs`.

---

## P3 — Long Term

### F39.7a: `analytics macro cycles history` CLI

Add `history` subcommand under `analytics macro cycles` for reading and writing historical power metrics.

```
# Add a historical data point
pftui analytics macro cycles history add --country US --determinant education \
  --year 1950 --score 9 --notes "Post-GI Bill expansion, best university system globally"

# List history for a country
pftui analytics macro cycles history list --country US --json

# List history for a country + determinant
pftui analytics macro cycles history list --country US --determinant military --json

# List history for a specific decade across all countries
pftui analytics macro cycles history list --year 1940 --json
```

Flags:
- `--country` (required for add): country name
- `--determinant` (required for add): determinant name (education, innovation, competitiveness, military, trade, economic_output, financial, reserve_currency, governance, or any new ones added later)
- `--year` (required for add): year (integer, e.g. 1950, not decade)
- `--score` (required for add): 1-10 Dalio scale
- `--notes` (optional): free text for context, sources, justification
- `--json`: structured output for list

Table: `power_metrics_history` (already exists, verify schema matches).
Files: `src/commands/analytics.rs`, `src/cli.rs`, `src/db/structural.rs`.

### F39.7b: Historical Power Metrics Data Population (Sentinel)

> After dev cron ships F39.7 CLI + schema, spawn a research sub-agent to populate
> the historical database. The sub-agent should:
>
> 1. Research each determinant for each power at each decade using web_search
> 2. Score on Dalio's 1-10 scale with brief justification and source
> 3. Populate via `pftui analytics macro history add` CLI commands
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

**Latest scores per tester (most recent review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Morning Market Research | 88% | 82% | Mar 7 | ↑ (Mar 8-9 crash/hang since fixed) |
| Evening Eventuality Planner | 55% | 68% | Mar 16 | ↓ (missing conviction/regime CLI paths) |
| Sentinel Main TUI Review | 75% | 72% | Mar 10 | ↓ (display corruption noted) |

**Notes:** Morning Research hit 0/15 on Mar 8 (DB crash) and 15/30 on Mar 9 (API hang) — both root causes fixed in v0.7.0+. The Mar 7 score of 88/82 reflects post-fix trajectory. Evening Planner dropped from 82/80 (Mar 8) to 55/68 (Mar 16) due to missing `analytics conviction set` and `analytics macro regime set` CLI paths after the F42 CLI restructure. Sentinel dropped from 85/88 (Mar 7) to 75/72 (Mar 10) citing TUI display corruption and missing day P&L dollar column.

**Top 3 priorities based on feedback:**

1. **Fix clippy errors + route missing analytics CLI paths** (P0/P1) — Evening Planner dropped 27 points because conviction/regime commands aren't reachable under the new CLI tree. This is the single biggest score-recovery opportunity.
2. **TUI display reliability + day P&L $ column** — Sentinel has requested daily P&L in dollars in every single review since Mar 2. This is the most consistently requested feature across all testers.
3. **Weekend movers + scenario --json** — Agent routines running on weekends get zero movers data, and scenario list lacks --json for programmatic consumption.

**Release status:** 52 commits since v0.10.0. `cargo test` passes (1239 tests). `cargo clippy -D warnings` FAILS (2 errors). Fix clippy before releasing v0.11.0.

**Homebrew Core:** 0 stars — not eligible (requires 50+).
