# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

### [Feedback] Fix technicals / regime / supply commands returning empty for evening-analysis agent
**Source:** Evening Analysis (Apr 5, 72/68 — lowest overall scorer).
**Why:** Three commands that evening-analysis relies on for core analysis returned empty output: `analytics technicals`, `analytics macro regime`, and `analytics supply`. This forces fallback to web_search and significantly reduces pftui's analytical value for the lowest-scoring tester.
**Scope:** Investigate each command's data source — likely stale/missing cache rows or a backend dispatch bug. Add diagnostic output when result set is empty (e.g. "No technical signals found — run `data refresh` first"). Files: `src/analytics/technicals.rs`, `src/analytics/regime.rs`, possibly `src/data/supply.rs`.
**Effort:** 1–2 hours investigation + fix.

### [Feedback] Fix prediction scorecard per-date returning zeros despite new predictions
**Source:** Evening Analysis (Apr 5, 72/68 — lowest overall scorer).
**Why:** Agent added 5 new predictions on Apr 5, then ran prediction scorecard and got zero counts per-date — a clear data pipeline bug that breaks the accountability/feedback loop.
**Scope:** Investigate `journal prediction scorecard` date-bucketing logic. Likely a timezone mismatch (UTC insert vs local date grouping) or a query filter cutting off same-day predictions. Files: `src/journal/predictions.rs`, related `_backend` DB functions.
**Effort:** 1–2 hours.

---

## P1 - Data Quality & Agent Reliability

### [Feedback] Fix journal scenario update triggered_at timestamp type mismatch
**Source:** medium-agent (Apr 5, 78/82).
**Why:** `journal scenario update` for "Iran-US War Escalation" hit a DB error on first attempt (column `triggered_at` timestamp type mismatch) but succeeded on retry — intermittent type coercion bug that could silently corrupt scenario state.
**Scope:** Investigate the `triggered_at` column handling in `journal scenario update` SQLite and Postgres paths. Likely a bind-type inconsistency (string vs NaiveDateTime). Files: `src/journal/scenario.rs`, `src/db/scenarios.rs`.
**Effort:** < 1 hour.

---

## P2 - Coverage And Agent Consumption

### [Feedback] Automatic event detection for scenario creation
**Source:** Evening Analysis (Apr 3, 82/78).
**Why:** When major macro events occur (e.g. Warsh nomination, tariff announcements, FOMC surprises), agents should automatically create or suggest new scenarios. Currently agents must manually identify events and run `journal scenario add`. Auto-detection from news/calendar/catalyst feeds would close this gap.
**Scope:** Detect high-impact events from news sentiment spikes + catalyst scoring, auto-suggest `journal scenario add` with pre-filled parameters. Could integrate into `analytics guidance` or as a standalone `analytics scenario detect`.
**Effort:** 1-2 weeks.

### [Feedback] prediction lesson bulk command for batch processing wrong predictions
**Source:** Evening Analysis (Apr 5, 72/68 — lowest overall scorer). 63 unresolved lessons in backlog.
**Why:** `journal prediction lessons add` is one-at-a-time. With 63 wrong predictions needing lessons, agents need a bulk workflow to catch up and maintain the improvement loop.
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
| Evening Analysis | 72% | 68% | Apr 5 | ↓ (82→72 use, 78→68 overall. Technicals/regime/supply empty; scorecard zeros. **Lowest overall scorer.**) |
| Evening Analyst | 72% | 68% | Apr 1 | ↓ (78→72 use, 75→68 overall. Backtest 26.7% WR — routine/strategy issue, not tooling.) |
| medium-agent | 78% | 82% | Apr 5 | → (new entry. DB timestamp bug + tag UX friction.) |
| Medium-Timeframe Analyst | 85% | 88% | Apr 3 | ↑ (75→85 use, 80→88 overall. Alert thresholds shipped #572.) |
| Low-Timeframe Analyst | 85% | 88% | Apr 3 | ↑ (85→85 use, 80→88 overall. Break history shipped #588.) |
| Macro-Timeframe Analyst | 80% | 85% | Mar 29 | → (stable.) |
| High-Timeframe Analyst | 85% | 90% | Mar 30 | → (stable.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable.) |
| Morning Brief Cron | 85% | 80% | Apr 2 | → (stable.) |
| Morning Brief | 85% | 88% | Apr 3 | ↑ (85→85 use, 80→88 overall.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable.) |
| Dev Agent | 92% | 94% | Apr 4 | → (stable high.) |

**Top 3 priorities based on feedback:**
1. **Evening Analysis empty commands (P0)** — lowest overall at 68%. Technicals/regime/supply returning empty forces web_search fallback. Prediction scorecard zeros breaks accountability loop.
2. **Prediction lesson bulk workflow (P2)** — 63 unresolved lessons creating technical debt. Evening Analysis (lowest scorer) flagged this; improvement loop stalled.
3. **medium-agent timestamp bug (P1)** — intermittent DB error on scenario update could silently corrupt state. Low effort to fix.

**Shipped since last review (Apr 5):**
_(nothing shipped yet — this is today's review)_

**Release status:** v0.26.0 released Apr 4. 2527 tests passing, clippy clean. No P0 bugs in prior review — two new P0s identified today from evening-analysis lowest-scorer feedback.

**GitHub stars:** 9 — Homebrew Core requires 50+.
