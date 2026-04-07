# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

### [Feedback] Fix clippy errors blocking release
**Source:** Daily build — Apr 7, 2026.
**Why:** `cargo clippy --all-targets -- -D warnings` fails with errors introduced by recent PRs. Blocks release eligibility.
- `src/commands/power_signals.rs:77` — `casting the result of i64::abs() to u32`; fix: use `.unsigned_abs()` instead.
- `src/commands/supply.rs` — two `items_after_test_module` lint errors; fix: move the `format_with_commas` helper function (and any other non-test items) to before the `mod tests` block.
**Files:** `src/commands/power_signals.rs:77`, `src/commands/supply.rs`.
**Effort:** < 30 minutes.

---

## P1 - Data Quality & Agent Reliability

### [x] [Feedback] Fix `analytics technicals --symbols` filter silently ignored
**Source:** medium-agent (Apr 6, 65/72 — new low). Corroborated by Low-Timeframe Analyst (Apr 5, 80/78).
**Why:** `analytics technicals --symbols BTC,GC=F` is accepted by the CLI but dumps the full symbol set — the filter has no effect. Agents are forced to manually grep the JSON output. Similar silent-ignore bug was previously fixed on `analytics trends list` (PR #566).
**Scope:** Investigate `analytics technicals` backend — apply the `--symbols` filter at the DB level, same pattern as the list_trends_filtered_backend fix. Files: `src/analytics/technicals.rs`, `src/db/technicals.rs`.
**Effort:** 1–2 hours.

### [x] [Feedback] Fix analytics situation indicator list returning empty despite indicators existing
**Source:** Low-Timeframe Analyst (Apr 5, 80/78).
**Why:** Agent confirmed 3 indicators existed for the Iran situation but `analytics situation` indicator list returned empty — data routing or filter bug in the situation room's indicator fetch.
**Scope:** Trace the indicator query path in `analytics situation`. Likely a join or filter mismatch between situation rows and their linked indicators. Files: `src/analytics/situation.rs`, `src/db/situation.rs`.
**Effort:** 1–2 hours.

### [x] [Feedback] COT data 13 days stale — investigate refresh path
**Source:** Evening Analysis (Apr 6, 78/75).
**Why:** COT (Commitment of Traders) data was 13 days stale, forcing fallback to web search for positioning data that pftui should own. Staleness this extreme suggests the refresh source failed silently or the COT source was not included in recent refresh runs.
**Scope:** Check COT refresh source for silent failures; add staleness warning to `data economy` / `data refresh` output when COT is >7 days old; ensure COT is included in all full refresh plans. Files: `src/data/cot.rs` (or equivalent), `src/data/refresh.rs`.
**Effort:** 1–2 hours.

### [x] [Feedback] Fix journal scenario update triggered_at timestamp type mismatch
**Source:** medium-agent (Apr 5, 78/82).
**Why:** `journal scenario update` for "Iran-US War Escalation" hit a DB error on first attempt (column `triggered_at` timestamp type mismatch) but succeeded on retry — intermittent type coercion bug that could silently corrupt scenario state.
**Scope:** Investigate the `triggered_at` column handling in `journal scenario update` SQLite and Postgres paths. Likely a bind-type inconsistency (string vs NaiveDateTime). Files: `src/journal/scenario.rs`, `src/db/scenarios.rs`.
**Effort:** < 1 hour.

### [x] [Feedback] Add `--agent` alias for `--source-agent` on prediction add
**Source:** medium-agent (Apr 6, 65/72).
**Why:** Agent used `prediction add --agent` which silently failed (clap rejected unknown flag). The correct flag is `--source-agent`. Adding a clap alias eliminates the discoverability gap, consistent with the `data quotes` / `portfolio snapshot` alias pattern.
**Scope:** Add `#[arg(alias = "agent")]` on the `--source-agent` flag in the prediction add CLI definition. Also add cross-reference in `after_help`. Files: `src/cli.rs`.
**Effort:** < 30 minutes.

---

## P2 - Coverage And Agent Consumption

### [x] [Feedback] journal scenario update: support partial name match or ID-based lookup
**Source:** medium-agent (Apr 6, 65/72).
**Why:** `journal scenario update` requires an exact full-string name match. When the name doesn't match precisely (case, whitespace, abbreviation), the update fails with no suggestions. Agents waste cycles on trial-and-error or fall back to the timestamp-prone retry path.
**Scope:** Add fuzzy/case-insensitive name matching (LIKE or LOWER()) as a fallback when exact match returns 0 rows, or support `--id <N>` as an alternative. Display candidate matches when multiple fuzzy results exist. Files: `src/journal/scenario.rs`, `src/db/scenarios.rs`.
**Effort:** 1–2 hours.

### [x] [Feedback] analytics macro outcomes: cross-reference to scenario update for probability edits
**Source:** Macro-Timeframe Analyst (Apr 5, 55/62).
**Why:** Analyst concluded macro outcomes was read-only with "no way to update scenario probabilities via CLI." The actual path is `journal scenario update --probability X` but there is no cross-reference from `analytics macro` help text. Add `after_help` guidance and optionally a thin `analytics macro outcomes update` alias.
**Scope:** Add `after_help` on `analytics macro` commands pointing to `journal scenario update`. Optionally add a thin alias `analytics macro outcomes update` that delegates to the journal path. Files: `src/cli.rs`, `src/analytics/macro_cmd.rs`.
**Effort:** 30 minutes.

### [x] [Feedback] Power composite signal dashboard (`analytics power-signals`)
**Source:** Low-Timeframe Analyst (Apr 5, 80/78).
**Why:** Analyst manually checks gold/oil/defense/VIX as a "power structure checklist" each run. A first-class `analytics power-signals` command would standardize this across agents and save time per session.
**Scope:** New `analytics power-signals` command aggregating regime-flows, power-flow assess, and FIC/MIC conflict output into a single ranked signal table. JSON + terminal output. Reuse existing `analytics regime-flows` and `analytics power-flow conflicts` backends. Files: `src/analytics/power_signals.rs` (new), `src/cli.rs`, `src/main.rs`.
**Effort:** 3–5 hours.

### [x] [Feedback] Scenario-to-prediction-market mapping: surface unmapped contracts
**Source:** Evening Analysis (Apr 6, 78/75).
**Why:** 1699 Polymarket contracts are flowing but zero are mapped to active scenarios. The `data predictions map` command exists (PR #422) but agents have no visibility into which contracts are good candidates for mapping. Guidance or `analytics calibration` should surface unmapped high-relevance contracts.
**Scope:** Add a `data predictions suggest-mappings` or enrich `analytics guidance` with a "unmapped high-relevance contracts" section (top N contracts by liquidity that match active scenario keywords). Files: `src/data/predictions.rs`, `src/commands/guidance.rs`.
**Effort:** 2–4 hours.

### [x] [Feedback] Automatic event detection for scenario creation
**Source:** Evening Analysis (Apr 3, 82/78).
**Why:** When major macro events occur (e.g. Warsh nomination, tariff announcements, FOMC surprises), agents should automatically create or suggest new scenarios. Currently agents must manually identify events and run `journal scenario add`. Auto-detection from news/calendar/catalyst feeds would close this gap.
**Scope:** Detect high-impact events from news sentiment spikes + catalyst scoring, auto-suggest `journal scenario add` with pre-filled parameters. Could integrate into `analytics guidance` or as a standalone `analytics scenario detect`.
**Effort:** 1-2 weeks.

### [x] [Feedback] prediction lesson bulk command for batch processing wrong predictions
**Source:** Evening Analysis (Apr 5, 72/68 — lowest overall scorer). 45–63 unresolved lessons in backlog.
**Why:** `journal prediction lessons add` is one-at-a-time. With 45+ wrong predictions needing lessons, agents need a bulk workflow to catch up and maintain the improvement loop.
**Scope:** New `journal prediction lessons bulk` command — either interactive (prompt per unresolved prediction) or file-based (read lessons from JSON/CSV). Include `--unresolved` flag on `journal prediction lessons list` to surface the backlog. Files: `src/journal/lessons.rs` (or equivalent).
**Effort:** 2–4 hours.

### [x] [Feedback] Support --tags as comma-separated list for journal entry add
**Source:** medium-agent (Apr 5, 78/82).
**Why:** `journal entry add` only accepts a single `--tag` flag. Agents had to use a single tag when multiple apply. Comma-separated `--tags` (e.g. `--tags iran,oil,geopolitical`) would match the multi-value pattern used elsewhere.
**Scope:** Change `--tag` to accept either multiple `--tag` flags or a comma-separated `--tags` alias. Parse and split on comma. Files: `src/cli.rs`, `src/journal/entries.rs`.
**Effort:** < 1 hour.

### [x] [Feedback] pftui data refresh --stale flag to selectively refresh only degraded feeds
**Source:** medium-agent (Apr 5, 78/82). Analytics situation showed 3 stale data sources.
**Why:** Agents want to refresh only stale/degraded feeds without triggering a full refresh (which is slow). `--only` and `--skip` flags exist but require knowing which sources are stale. A `--stale` flag that auto-detects and refreshes only degraded sources would be more ergonomic.
**Scope:** Add `--stale` flag to `data refresh`. Query each source's `fetched_at` and include only those beyond their freshness threshold in the RefreshPlan. Mutually exclusive with `--only`/`--skip`. Files: `src/data/refresh.rs`, `src/cli.rs`.
**Effort:** 2–4 hours.

---

### [x] [P1] Silver price stale — fallback to web_search when pftui returns stale value
**Source:** Evening analysis data integrity audit (Apr 7). Silver showing $70.28 from portfolio vs ~$72 from web.
**Why:** pftui `data prices` for silver returns portfolio cost basis when live price is unavailable, misleading agents. Should either return live XAG/USD or explicit STALE error so agents fall back to web_search.
**Scope:** Check `data prices` silver fetch path. If last refresh > 4h, mark stale in output (`"status": "stale"`) or return error. Agents already have logic to fall back to web_search on stale. Files: `src/data/fx.rs` or price fetch module, `src/cli/data.rs`.
**Effort:** 2–3 hours.

### [x] [Feedback] 10Y Yield FRED series DGS10 stale — add fallback data source
**Source:** Evening analysis data integrity audit (Apr 7). DGS10 FRED series 4 days stale.
**Why:** FRED DGS10 is updated with 1-day lag at most. 4-day staleness suggests fetch failure. Treasury.gov Direct API or Yahoo Finance `^TNX` are reliable fallbacks.
**Scope:** Add fallback to `src/data/fred.rs` for DGS10: if FRED fetch fails or returns stale, try Yahoo Finance `^TNX` symbol. Log fallback source in output. Also add staleness threshold check — flag if > 2 days old.
**Effort:** 3–4 hours.

### [x] [Feedback] CPI/PPI FRED series degraded — fix FRED fetch status + add staleness warning
**Source:** Evening analysis data integrity audit (Apr 7). CPIAUCSL last fetched Feb 1, 2026 — 65 days stale. Corroborated by evening-analyst (Apr 7, 72/75): "CPI 65 days stale degrades macro data quality."
**Why:** FRED CPIAUCSL and PPIFIS should auto-refresh on schedule. 65-day staleness means the fetch is silently failing. Agents are hallucinating or using stale values for the most important macro indicator.
**Scope:** (1) Diagnose why FRED CPI/PPI fetch is failing silently — check error handling in `src/data/fred.rs`. (2) Add explicit staleness status to `pftui data economy --json` output with `last_updated` and `stale: true/false`. (3) Add BLS.gov direct fallback for CPI/PPI when FRED fails. (4) If data > 45 days old, return explicit error string rather than stale value so agents know to web_search.
**Effort:** 4–6 hours.

### [x] [Feedback] GDP series stale 188 days — add GDPNow as primary source, BEA as fallback
**Source:** Evening analysis data integrity audit (Apr 7). FRED GDP stale since Oct 2025, GDPNow also stale. evening-analyst (Apr 7): "GDP 188 days stale."
**Why:** GDP is a quarterly series; quarterly = expected staleness between prints. But the GDPNow nowcast updates daily and should always have a current estimate. Both being stale means the fetch is broken.
**Scope:** (1) Fix GDPNow fetch in economy module — Atlanta Fed endpoint may have changed. (2) Add staleness context to output: "GDP last print: Q3 2025. Next print: Apr 30. GDPNow nowcast: X%" — so agents understand the data rhythm rather than flagging it as broken. (3) If GDPNow unreachable, note that in output explicitly.
**Effort:** 3–4 hours.

### [x] [Feedback] COT data — add explicit next-report-date field and auto-retry on Friday
**Source:** Evening analysis data integrity audit (Apr 7). COT last report Mar 31, weekly cadence, Apr 4 report may exist.
**Why:** COT is published every Friday at 3:30 PM ET by CFTC for the prior Tuesday's positions. Agents can't tell if the latest report has been fetched or if a newer one is available. Stale COT during active markets (oil at 1.9th pct) is a significant intelligence gap.
**Scope:** (1) Add `next_report_date` and `report_date` fields to `pftui data economy --json` COT section. (2) Add Friday auto-retry logic — if current time is Friday after 3:30 PM ET and last COT report is > 5 days old, trigger re-fetch. (3) Add CFTC API endpoint validation — confirm current URL is still valid.
**Effort:** 3–4 hours.

---

## P2 - Coverage And Agent Consumption

### [x] [Feedback] Fix analytics medium command returning no useful output
**Source:** medium-agent (Apr 7, 72/78 — continued downtrend from 78/82 on Apr 5).
**Why:** `pftui analytics medium` returned empty or unexpected data — no useful medium-timeframe synthesized view. Without this, agents manually cross-reference `analytics synthesis` + `journal conviction list`, which is time-consuming and error-prone.
**Scope:** Investigate `analytics medium` backend — ensure it returns synthesized medium-TF data (analyst views, scenario state, conviction scores). Add diagnostics when data is missing rather than silently returning empty. If the command requires prior `analytics views set` data to be meaningful, document this clearly in `--help` with an example. Files: `src/commands/analytics.rs` (or medium-specific handler), `src/cli.rs`.
**Effort:** 1–2 hours.

### [x] [Feedback] Show stale data warning at session start / on first analytics call
**Source:** medium-agent (Apr 7, 72/78). medium-agent (Apr 5). medium-agent (Apr 5): "12/17 stale FRED series — a stale data warning at session start would save inference time."
**Why:** When FRED/COT/other feeds are degraded, agents discover this mid-analysis rather than at the start of a session. Proactive surfacing saves inference time and prevents agents from building analysis on stale inputs.
**Scope:** Enrich `analytics guidance --json` with a `data_health` summary showing stale/degraded sources. Alternatively, add a brief staleness banner to `portfolio brief --json` when >30% of tracked sources are stale. Both approaches reuse existing `data status` staleness checks. Files: `src/commands/guidance.rs`, `src/data/refresh.rs`.
**Effort:** 2–3 hours.

### [Feedback] Add --lesson-coverage flag to prediction scorecard
**Source:** evening-analyst (Apr 7, 72/75).
**Why:** Agents want to surface unlessoned wrong predictions for remediation without running a separate `journal prediction lessons list --unresolved` query. A `--lesson-coverage` flag on the scorecard would annotate wrong predictions with their lesson status and a ready-to-run `lessons add` command.
**Scope:** Add `--lesson-coverage` flag to `journal prediction scorecard --json`. Join against `prediction_lessons` table to annotate each wrong prediction with lesson status (`has_lesson: bool`, `lesson_type` if present). Terminal output: append `[no lesson]` tag on unlessoned wrong predictions. Files: `src/commands/predict.rs`, `src/cli.rs`.
**Effort:** 2–3 hours.

### [Feedback] Fix analytics situation update log severity validation and docs
**Source:** low-timeframe-analyst (Apr 6, 55/72 — tied lowest usefulness). low-timeframe-analyst (Apr 6, 75/78).
**Why:** `analytics situation update log --severity high` was rejected as an invalid value with no documentation of valid values. Agents were forced to discover valid severity values by trial and error, burning time in the lowest-scoring tester workflow.
**Scope:** (1) Identify valid severity values accepted by `analytics situation update log`. (2) Add them to CLI help text via `value_parser` or `possible_values` so they appear in `--help`. (3) Improve the error message to list valid options. Files: `src/cli.rs`, `src/analytics/situation.rs`.
**Effort:** < 1 hour.

### [Feedback] Add --from date filter and --agent-filter to analytics digest
**Source:** low-timeframe-analyst (Apr 6, 55/72). low-timeframe-analyst (Apr 6, 75/78).
**Why:** `analytics digest --from <date>` flag is missing entirely. `analytics digest --agent-filter <agent>` was requested to allow per-agent digest output without post-processing the full JSON. Both flags improve agent workflow speed significantly.
**Scope:** Add `--from <date>` and `--agent-filter <agent-name>` flags to `analytics digest`. Apply as WHERE clauses in the backend query. Files: `src/commands/digest.rs` (or equivalent), `src/cli.rs`.
**Effort:** 1–2 hours.

### [Feedback] Add data fear-greed subcommand
**Source:** high-agent (Apr 6, 72/78 — new reviewer).
**Why:** `pftui data fear-greed` subcommand is missing. Fear & Greed Index is a key sentiment indicator for high-timeframe structural analysis and is commonly referenced alongside VIX. Alternative.me API is free and reliable.
**Scope:** New `data fear-greed` command pulling from Alternative.me Crypto Fear & Greed API (or CNN F&G for traditional markets). Store in DB table with history. JSON + terminal output. Integrate into `data refresh` and surface in `analytics market-snapshot`. Files: `src/commands/fear_greed.rs` (new), `src/cli.rs`, `src/main.rs`, `src/data/refresh.rs`.
**Effort:** 3–5 hours.

### [Feedback] Fix sovereign data returning empty (COMEX 403 on gold/silver)
**Source:** high-agent (Apr 6, 72/78).
**Why:** `pftui data supply` (or sovereign gold/silver physical supply endpoint) returns empty data due to COMEX returning 403 Forbidden. The stale-cache fallback added in PR #636 should now handle this, but the fallback may not be reaching cached rows or the COMEX URL needs updating.
**Scope:** (1) Confirm stale-cache fallback in `src/commands/supply.rs` is working for COMEX 403 cases. (2) If not, trace the fallback path and fix. (3) Add alternative source (Kitco public API or WGC) if COMEX 403 is persistent. Files: `src/commands/supply.rs`.
**Effort:** 2–4 hours.

### [Feedback] Add --layer filter to analytics views divergence
**Source:** high-agent (Apr 6, 72/78).
**Why:** `analytics views divergence` returns all cross-timeframe divergences. HIGH vs LOW conflicts are the most actionable, but agents must post-process JSON to extract them. A `--layer high` or `--layer low` filter would surface only relevant conflicts for a given timeframe perspective.
**Scope:** Add `--layer <timeframe>` flag to `analytics views divergence`. Filter divergences where at least one side is the specified timeframe. Files: `src/commands/views.rs` (or analytics/views.rs), `src/cli.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix CLAUDE.md syntax for analytics trends evidence add command
**Source:** high-agent (Apr 6, 72/78).
**Why:** Routine docs reference `trends evidence-add` (hyphenated subcommand) but the correct CLI path is `analytics trends evidence add` (multi-level subcommand chain). Agents using docs hit command-not-found errors and have to discover the right path by trial and error.
**Scope:** Update CLAUDE.md and any agent routine docs that reference `trends evidence-add` to use the correct `analytics trends evidence add --id <N>` syntax. Files: `CLAUDE.md`, relevant agent routine markdown files.
**Effort:** < 30 minutes.

### [Feedback] Clarify agent message ack --to flag help text
**Source:** low-timeframe-analyst (Apr 6, 55/72).
**Why:** `pftui agent message ack --to` help text is ambiguous — unclear whether `--to` expects an agent name, message ID, or conversation thread. Agents waste time on trial and error.
**Scope:** Improve `--to` help text with a concrete example (e.g. `--to morning-brief`). Add `after_help` with usage examples. Files: `src/cli.rs`.
**Effort:** < 30 minutes.

---

## P3 - Long Term

### F59: Capital Flow Tracking
**Source:** Competitive research (NOFX institutional flow data).
**Why:** Institutional fund flows, ETF creation/redemption, and open interest changes reveal positioning that price alone doesn't show. pftui has BTC ETF flows and COT but lacks broader equity/commodity fund flow data.
**Scope:** New `data flows` source pulling ETF flow data (ETF.com or similar), institutional 13F filings, and crypto exchange flow data. New table `capital_flows`. Integration into agent routines.
**Effort:** 3-4 weeks. **Priority:** P3 — enhances analysis but agents can web_search for this.

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Low-Timeframe Analyst | 55% | 72% | Apr 6 | ↓↓ (**Tied lowest usefulness.** Severity docs broken, digest filters missing, agent ack ambiguous.) |
| Macro-Timeframe Analyst | 55% | 62% | Apr 5 | ↓↓ (**Tied lowest usefulness.** Many items shipped Apr 6 — likely improving next run.) |
| Evening Analyst | 72% | 75% | Apr 7 | ↓ (FRED stale CPI/GDP degrading macro quality. --lesson-coverage requested.) |
| Medium-Agent | 72% | 78% | Apr 7 | ↓ (78→72 use. analytics medium empty, stale FRED warning needed.) |
| Medium-Timeframe Analyst | 72% | 78% | Apr 7 | ↓ (85→72 use. analytics medium command same gap.) |
| High-Agent | 72% | 78% | Apr 6 | new reviewer. fears-greed missing, COMEX 403, views divergence --layer, docs syntax wrong.) |
| Evening Analysis | 78% | 75% | Apr 6 | ↑ (72→78 use, 68→75 overall. COT+technicals+scorecard all fixed Apr 6.) |
| Morning Brief | 82% | 78% | Apr 5 | → (stable.) |
| High-Timeframe Analyst | 85% | 90% | Mar 30 | → (stable — not seen recently.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable — not seen recently.) |
| Morning Brief Cron | 85% | 80% | Apr 2 | → (stable.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable — not seen recently.) |

**Top 3 priorities based on feedback:**
1. **Clippy P0 — fix immediately.** Blocks release. Two files: `power_signals.rs` (abs cast) and `supply.rs` (items after test module). < 30 min fix.
2. **Low-Timeframe + Macro both at 55% usefulness.** Low-TF: fix situation severity validation, digest filters, agent ack docs. Macro: all P0 items shipped Apr 6 — expect scores to recover next run.
3. **Stale FRED/GDP/COT data degrading all agents.** CPI 65 days stale, GDP 188 days stale — P1 items. Fix FRED fetch failures and add fallback sources.

**Shipped since last review (Apr 6 — previous run):**
- analytics macro log add (P0) — `analytics macro log add` subcommand fully wired
- technicals / regime / supply fallback fixes (P0) — all three now return diagnostics/fallbacks instead of empty
- prediction scorecard date-bucketing fix (P0) — UTC/local timezone bug fixed
- analytics technicals --symbols filter (P1) — comma-separated filter now applied at DB level
- analytics situation indicator list (P1) — linked indicators now surface in snapshot
- COT staleness logic (P1) — report-date-keyed staleness, retry on refetch
- journal scenario update timestamp (P1) — UTC RFC3339 bind normalized
- --agent alias for prediction add (P1) — clap alias added
- scenario update partial name match (P2) — --id flag + case-insensitive fuzzy lookup
- macro outcomes cross-reference docs (P2) — after_help pointing to journal scenario update
- analytics power-signals (P2) — new consolidated power-structure dashboard
- data predictions suggest-mappings (P2) — unmapped high-liquidity contract suggestions
- analytics scenario detect (P2) — auto event detection from news/catalysts
- prediction lesson bulk workflow (P2) — --unresolved flag + bulk --input JSON path
- journal entry multi-tag support (P2) — --tag repeatable + --tags comma-separated alias
- data refresh --stale flag (P2) — refreshes only degraded feeds
- silver price stale exposure (P1) — stale status flag added to data prices output (PR #646)

**Release status:** v0.26.0 (Apr 4). **Tests:** 2571 passed / 0 failed / 2 ignored. **Clippy:** FAILING (4 errors in power_signals.rs + supply.rs — introduced by Apr 6 PRs). **Release blocked** until clippy is green. Once fixed, all conditions are met to cut v0.27.0 (17 features/fixes shipped since v0.26.0).

**GitHub stars:** 9 — Homebrew Core requires 50+.
