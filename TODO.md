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

### [Feedback] 10Y Yield FRED series DGS10 stale — add fallback data source
**Source:** Evening analysis data integrity audit (Apr 7). DGS10 FRED series 4 days stale.
**Why:** FRED DGS10 is updated with 1-day lag at most. 4-day staleness suggests fetch failure. Treasury.gov Direct API or Yahoo Finance `^TNX` are reliable fallbacks.
**Scope:** Add fallback to `src/data/fred.rs` for DGS10: if FRED fetch fails or returns stale, try Yahoo Finance `^TNX` symbol. Log fallback source in output. Also add staleness threshold check — flag if > 2 days old.
**Effort:** 3–4 hours.

### [Feedback] CPI/PPI FRED series degraded — fix FRED fetch status + add staleness warning
**Source:** Evening analysis data integrity audit (Apr 7). CPIAUCSL last fetched Feb 1, 2026 — 65 days stale. Corroborated by evening-analyst (Apr 7, 72/75): "CPI 65 days stale degrades macro data quality."
**Why:** FRED CPIAUCSL and PPIFIS should auto-refresh on schedule. 65-day staleness means the fetch is silently failing. Agents are hallucinating or using stale values for the most important macro indicator.
**Scope:** (1) Diagnose why FRED CPI/PPI fetch is failing silently — check error handling in `src/data/fred.rs`. (2) Add explicit staleness status to `pftui data economy --json` output with `last_updated` and `stale: true/false`. (3) Add BLS.gov direct fallback for CPI/PPI when FRED fails. (4) If data > 45 days old, return explicit error string rather than stale value so agents know to web_search.
**Effort:** 4–6 hours.

### [Feedback] GDP series stale 188 days — add GDPNow as primary source, BEA as fallback
**Source:** Evening analysis data integrity audit (Apr 7). FRED GDP stale since Oct 2025, GDPNow also stale. evening-analyst (Apr 7): "GDP 188 days stale."
**Why:** GDP is a quarterly series; quarterly = expected staleness between prints. But the GDPNow nowcast updates daily and should always have a current estimate. Both being stale means the fetch is broken.
**Scope:** (1) Fix GDPNow fetch in economy module — Atlanta Fed endpoint may have changed. (2) Add staleness context to output: "GDP last print: Q3 2025. Next print: Apr 30. GDPNow nowcast: X%" — so agents understand the data rhythm rather than flagging it as broken. (3) If GDPNow unreachable, note that in output explicitly.
**Effort:** 3–4 hours.

### [Feedback] COT data — add explicit next-report-date field and auto-retry on Friday
**Source:** Evening analysis data integrity audit (Apr 7). COT last report Mar 31, weekly cadence, Apr 4 report may exist.
**Why:** COT is published every Friday at 3:30 PM ET by CFTC for the prior Tuesday's positions. Agents can't tell if the latest report has been fetched or if a newer one is available. Stale COT during active markets (oil at 1.9th pct) is a significant intelligence gap.
**Scope:** (1) Add `next_report_date` and `report_date` fields to `pftui data economy --json` COT section. (2) Add Friday auto-retry logic — if current time is Friday after 3:30 PM ET and last COT report is > 5 days old, trigger re-fetch. (3) Add CFTC API endpoint validation — confirm current URL is still valid.
**Effort:** 3–4 hours.

---

## P2 - Coverage And Agent Consumption

### [Feedback] Fix analytics medium command returning no useful output
**Source:** medium-agent (Apr 7, 72/78 — continued downtrend from 78/82 on Apr 5).
**Why:** `pftui analytics medium` returned empty or unexpected data — no useful medium-timeframe synthesized view. Without this, agents manually cross-reference `analytics synthesis` + `journal conviction list`, which is time-consuming and error-prone.
**Scope:** Investigate `analytics medium` backend — ensure it returns synthesized medium-TF data (analyst views, scenario state, conviction scores). Add diagnostics when data is missing rather than silently returning empty. If the command requires prior `analytics views set` data to be meaningful, document this clearly in `--help` with an example. Files: `src/commands/analytics.rs` (or medium-specific handler), `src/cli.rs`.
**Effort:** 1–2 hours.

### [Feedback] Show stale data warning at session start / on first analytics call
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
