# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

### [x] [Feedback] Add `analytics macro log add` subcommand
**Source:** Macro-Timeframe Analyst (Apr 5, 55/62 — lowest usefulness score of any tester).
**Why:** `analytics macro log` is read-only; there is no `add`/`write` subcommand. Analyst had to use `journal entry add` as an awkward workaround, which severs the macro-specific workflow and loses structured fields. This is the primary pain point for the lowest-scoring tester.
**Scope:** Add `analytics macro log add` subcommand with flags matching analyst needs: `--development <text>`, `--cycle-impact <text>`, `--outcome-shift <text>`, `--date <YYYY-MM-DD>` (default today). Store in existing macro log table. Mirror the `journal entry add --content` pattern established in PR #375. Files: `src/analytics/macro_cmd.rs` (or equivalent), `src/cli.rs`, `src/main.rs`.
**Effort:** 2–3 hours.

### [x] [Feedback] Fix technicals / regime / supply commands returning empty for evening-analysis agent
**Source:** Evening Analysis (Apr 5, 72/68 — lowest overall scorer).
**Why:** Three commands that evening-analysis relies on for core analysis returned empty output: `analytics technicals`, `analytics macro regime`, and `analytics supply`. This forces fallback to web_search and significantly reduces pftui's analytical value for the lowest-scoring tester.
**Scope:** Investigate each command's data source — likely stale/missing cache rows or a backend dispatch bug. Add diagnostic output when result set is empty (e.g. "No technical signals found — run `data refresh` first"). Files: `src/analytics/technicals.rs`, `src/analytics/regime.rs`, possibly `src/data/supply.rs`.
**Effort:** 1–2 hours investigation + fix.

### [x] [Feedback] Fix prediction scorecard per-date returning zeros despite new predictions
**Source:** Evening Analysis (Apr 5, 72/68 — lowest overall scorer).
**Why:** Agent added 5 new predictions on Apr 5, then ran prediction scorecard and got zero counts per-date — a clear data pipeline bug that breaks the accountability/feedback loop.
**Scope:** Investigate `journal prediction scorecard` date-bucketing logic. Likely a timezone mismatch (UTC insert vs local date grouping) or a query filter cutting off same-day predictions. Files: `src/journal/predictions.rs`, related `_backend` DB functions.
**Effort:** 1–2 hours.

---

## P1 - Data Quality & Agent Reliability

### [Feedback] Fix `analytics technicals --symbols` filter silently ignored
**Source:** medium-agent (Apr 6, 65/72 — new low). Corroborated by Low-Timeframe Analyst (Apr 5, 80/78).
**Why:** `analytics technicals --symbols BTC,GC=F` is accepted by the CLI but dumps the full symbol set — the filter has no effect. Agents are forced to manually grep the JSON output. Similar silent-ignore bug was previously fixed on `analytics trends list` (PR #566).
**Scope:** Investigate `analytics technicals` backend — apply the `--symbols` filter at the DB level, same pattern as the list_trends_filtered_backend fix. Files: `src/analytics/technicals.rs`, `src/db/technicals.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix analytics situation indicator list returning empty despite indicators existing
**Source:** Low-Timeframe Analyst (Apr 5, 80/78).
**Why:** Agent confirmed 3 indicators existed for the Iran situation but `analytics situation` indicator list returned empty — data routing or filter bug in the situation room's indicator fetch.
**Scope:** Trace the indicator query path in `analytics situation`. Likely a join or filter mismatch between situation rows and their linked indicators. Files: `src/analytics/situation.rs`, `src/db/situation.rs`.
**Effort:** 1–2 hours.

### [Feedback] COT data 13 days stale — investigate refresh path
**Source:** Evening Analysis (Apr 6, 78/75).
**Why:** COT (Commitment of Traders) data was 13 days stale, forcing fallback to web search for positioning data that pftui should own. Staleness this extreme suggests the refresh source failed silently or the COT source was not included in recent refresh runs.
**Scope:** Check COT refresh source for silent failures; add staleness warning to `data economy` / `data refresh` output when COT is >7 days old; ensure COT is included in all full refresh plans. Files: `src/data/cot.rs` (or equivalent), `src/data/refresh.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix journal scenario update triggered_at timestamp type mismatch
**Source:** medium-agent (Apr 5, 78/82).
**Why:** `journal scenario update` for "Iran-US War Escalation" hit a DB error on first attempt (column `triggered_at` timestamp type mismatch) but succeeded on retry — intermittent type coercion bug that could silently corrupt scenario state.
**Scope:** Investigate the `triggered_at` column handling in `journal scenario update` SQLite and Postgres paths. Likely a bind-type inconsistency (string vs NaiveDateTime). Files: `src/journal/scenario.rs`, `src/db/scenarios.rs`.
**Effort:** < 1 hour.

### [Feedback] Add `--agent` alias for `--source-agent` on prediction add
**Source:** medium-agent (Apr 6, 65/72).
**Why:** Agent used `prediction add --agent` which silently failed (clap rejected unknown flag). The correct flag is `--source-agent`. Adding a clap alias eliminates the discoverability gap, consistent with the `data quotes` / `portfolio snapshot` alias pattern.
**Scope:** Add `#[arg(alias = "agent")]` on the `--source-agent` flag in the prediction add CLI definition. Also add cross-reference in `after_help`. Files: `src/cli.rs`.
**Effort:** < 30 minutes.

---

## P2 - Coverage And Agent Consumption

### [Feedback] journal scenario update: support partial name match or ID-based lookup
**Source:** medium-agent (Apr 6, 65/72).
**Why:** `journal scenario update` requires an exact full-string name match. When the name doesn't match precisely (case, whitespace, abbreviation), the update fails with no suggestions. Agents waste cycles on trial-and-error or fall back to the timestamp-prone retry path.
**Scope:** Add fuzzy/case-insensitive name matching (LIKE or LOWER()) as a fallback when exact match returns 0 rows, or support `--id <N>` as an alternative. Display candidate matches when multiple fuzzy results exist. Files: `src/journal/scenario.rs`, `src/db/scenarios.rs`.
**Effort:** 1–2 hours.

### [Feedback] analytics macro outcomes: cross-reference to scenario update for probability edits
**Source:** Macro-Timeframe Analyst (Apr 5, 55/62).
**Why:** Analyst concluded macro outcomes was read-only with "no way to update scenario probabilities via CLI." The actual path is `journal scenario update --probability X` but there is no cross-reference from `analytics macro` help text. Add `after_help` guidance and optionally a thin `analytics macro outcomes update` alias.
**Scope:** Add `after_help` on `analytics macro` commands pointing to `journal scenario update`. Optionally add a thin alias `analytics macro outcomes update` that delegates to the journal path. Files: `src/cli.rs`, `src/analytics/macro_cmd.rs`.
**Effort:** 30 minutes.

### [Feedback] Power composite signal dashboard (`analytics power-signals`)
**Source:** Low-Timeframe Analyst (Apr 5, 80/78).
**Why:** Analyst manually checks gold/oil/defense/VIX as a "power structure checklist" each run. A first-class `analytics power-signals` command would standardize this across agents and save time per session.
**Scope:** New `analytics power-signals` command aggregating regime-flows, power-flow assess, and FIC/MIC conflict output into a single ranked signal table. JSON + terminal output. Reuse existing `analytics regime-flows` and `analytics power-flow conflicts` backends. Files: `src/analytics/power_signals.rs` (new), `src/cli.rs`, `src/main.rs`.
**Effort:** 3–5 hours.

### [Feedback] Scenario-to-prediction-market mapping: surface unmapped contracts
**Source:** Evening Analysis (Apr 6, 78/75).
**Why:** 1699 Polymarket contracts are flowing but zero are mapped to active scenarios. The `data predictions map` command exists (PR #422) but agents have no visibility into which contracts are good candidates for mapping. Guidance or `analytics calibration` should surface unmapped high-relevance contracts.
**Scope:** Add a `data predictions suggest-mappings` or enrich `analytics guidance` with a "unmapped high-relevance contracts" section (top N contracts by liquidity that match active scenario keywords). Files: `src/data/predictions.rs`, `src/commands/guidance.rs`.
**Effort:** 2–4 hours.

### [Feedback] Automatic event detection for scenario creation
**Source:** Evening Analysis (Apr 3, 82/78).
**Why:** When major macro events occur (e.g. Warsh nomination, tariff announcements, FOMC surprises), agents should automatically create or suggest new scenarios. Currently agents must manually identify events and run `journal scenario add`. Auto-detection from news/calendar/catalyst feeds would close this gap.
**Scope:** Detect high-impact events from news sentiment spikes + catalyst scoring, auto-suggest `journal scenario add` with pre-filled parameters. Could integrate into `analytics guidance` or as a standalone `analytics scenario detect`.
**Effort:** 1-2 weeks.

### [Feedback] prediction lesson bulk command for batch processing wrong predictions
**Source:** Evening Analysis (Apr 5, 72/68 — lowest overall scorer). 45–63 unresolved lessons in backlog.
**Why:** `journal prediction lessons add` is one-at-a-time. With 45+ wrong predictions needing lessons, agents need a bulk workflow to catch up and maintain the improvement loop.
**Scope:** New `journal prediction lessons bulk` command — either interactive (prompt per unresolved prediction) or file-based (read lessons from JSON/CSV). Include `--unresolved` flag on `journal prediction lessons list` to surface the backlog. Files: `src/journal/lessons.rs` (or equivalent).
**Effort:** 2–4 hours.

### [Feedback] Support --tags as comma-separated list for journal entry add
**Source:** medium-agent (Apr 5, 78/82).
**Why:** `journal entry add` only accepts a single `--tag` flag. Agents had to use a single tag when multiple apply. Comma-separated `--tags` (e.g. `--tags iran,oil,geopolitical`) would match the multi-value pattern used elsewhere.
**Scope:** Change `--tag` to accept either multiple `--tag` flags or a comma-separated `--tags` alias. Parse and split on comma. Files: `src/cli.rs`, `src/journal/entries.rs`.
**Effort:** < 1 hour.

### [Feedback] pftui data refresh --stale flag to selectively refresh only degraded feeds
**Source:** medium-agent (Apr 5, 78/82). Analytics situation showed 3 stale data sources.
**Why:** Agents want to refresh only stale/degraded feeds without triggering a full refresh (which is slow). `--only` and `--skip` flags exist but require knowing which sources are stale. A `--stale` flag that auto-detects and refreshes only degraded sources would be more ergonomic.
**Scope:** Add `--stale` flag to `data refresh`. Query each source's `fetched_at` and include only those beyond their freshness threshold in the RefreshPlan. Mutually exclusive with `--only`/`--skip`. Files: `src/data/refresh.rs`, `src/cli.rs`.
**Effort:** 2–4 hours.

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
| Macro-Timeframe Analyst | 55% | 62% | Apr 5 | ↓↓ (**Lowest usefulness.** Macro log add missing, outcomes read-only.) |
| Evening Analysis | 78% | 75% | Apr 6 | ↑ (72→78 use, 68→75 overall. COT stale, lesson backlog 45/64, FRED degraded.) |
| Evening Analyst | 72% | 68% | Apr 1 | ↓ (stale — last seen Apr 1. Backtest WR 26.7%.) |
| medium-agent | 65% | 72% | Apr 6 | ↓↓ (78→65 use, 82→72 overall. technicals --symbols broken, --agent alias missing, scenario name UX.) |
| Low-Timeframe Analyst | 80% | 78% | Apr 5 | ↓ (85→80 use, 88→78 overall. Indicator list empty, power-signals missing.) |
| Morning Brief | 82% | 78% | Apr 5 | → (stable.) |
| Medium-Timeframe Analyst | 85% | 88% | Apr 3 | ↑ (75→85 use, 80→88 overall.) |
| High-Timeframe Analyst | 85% | 90% | Mar 30 | → (stable.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable.) |
| Morning Brief Cron | 85% | 80% | Apr 2 | → (stable.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable.) |
| Dev Agent | 92% | 94% | Apr 4 | → (stable high.) |

**Top 3 priorities based on feedback:**
1. **Macro-Timeframe Analyst missing commands (P0)** — lowest usefulness at 55%. `analytics macro log add` is the primary gap; analyst cannot log macro analysis in the correct namespace.
2. **Evening Analysis empty commands (P0)** — overall 75% (improved from 68%). Technicals/regime/supply still empty; prediction scorecard zeros still broken.
3. **medium-agent tooling gaps (P1)** — dropped to 65/72. `analytics technicals --symbols` silently ignored; `--agent` flag not recognized (should be `--source-agent`); scenario name matching friction.

**Shipped since last review (Apr 5):**
_(nothing shipped yet — this is today's review)_

**Release status:** v0.26.0 (Apr 4). 2527 tests passing, 0 failed, 2 ignored. Clippy clean. **Release blocked** by P0 bugs (macro log add, technicals/regime/supply empty, prediction scorecard zeros).

**GitHub stars:** 9 — Homebrew Core requires 50+.
