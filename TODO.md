# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 - Data Quality & Agent Reliability

### [Feedback] Fix analytics situation update log --driver DB error (triggered_at)
**Source:** medium-timeframe-analyst (Apr 8, 72/78). Corroborates medium-agent (Apr 5, 78/82).
**Why:** `analytics situation update log --driver <text>` throws a `triggered_at` timestamp type mismatch DB error. The `journal scenario update` timestamp bug was fixed (PR prev), but the same bind inconsistency persists on the `analytics situation update log` path — different code path, same root cause.
**Scope:** Trace the `analytics situation update log` write path. Normalize `triggered_at` binds to UTC RFC3339 string on both SQLite and Postgres. Files: `src/analytics/situation.rs`, `src/db/situation.rs`.
**Effort:** < 1 hour.

### [Feedback] Fix daily_change null for commodity positions in portfolio brief
**Source:** low-agent (Apr 7, 72/74).
**Why:** `portfolio brief --json` returns `null` for `change_1d` on commodity positions (SI=F, GC=F) while `analytics movers` returns correct daily % for the same symbols. Agents building on portfolio brief miss the commodity move signal.
**Scope:** Trace `change_1d` population for commodity positions in `portfolio brief`. Likely the daily change fetch doesn't cover futures symbols. Files: `src/commands/brief.rs`, `src/data/prices.rs`.
**Effort:** 1–2 hours.

---

## P2 - Coverage And Agent Consumption

### [Feedback] Add data fear-greed subcommand
**Source:** high-agent (Apr 6, 72/78).
**Why:** `pftui data fear-greed` subcommand is missing. Fear & Greed Index is a key sentiment indicator for high-timeframe structural analysis and is commonly referenced alongside VIX. Alternative.me API is free and reliable.
**Scope:** New `data fear-greed` command pulling from Alternative.me Crypto Fear & Greed API (or CNN F&G for traditional markets). Store in DB table with history. JSON + terminal output. Integrate into `data refresh` and surface in `analytics market-snapshot`. Files: `src/commands/fear_greed.rs` (new), `src/cli.rs`, `src/main.rs`, `src/data/refresh.rs`.
**Effort:** 3–5 hours.

### [Feedback] Fix sovereign data returning empty (COMEX 403 on gold/silver)
**Source:** high-agent (Apr 6, 72/78).
**Why:** `pftui data supply` returns empty data due to COMEX returning 403 Forbidden. The stale-cache fallback added in PR #636 should handle this, but may not be reaching cached rows or the COMEX URL needs updating.
**Scope:** (1) Confirm stale-cache fallback in `src/commands/supply.rs` is working for COMEX 403 cases. (2) If not, trace the fallback path and fix. (3) Add alternative source (Kitco public API or WGC) if COMEX 403 is persistent. Files: `src/commands/supply.rs`.
**Effort:** 2–4 hours.

### [Feedback] Add --layer filter to analytics views divergence
**Source:** high-agent (Apr 6, 72/78).
**Why:** `analytics views divergence` returns all cross-timeframe divergences. HIGH vs LOW conflicts are the most actionable, but agents must post-process JSON to extract them.
**Scope:** Add `--layer <timeframe>` flag to `analytics views divergence`. Filter divergences where at least one side is the specified timeframe. Files: `src/commands/views.rs`, `src/cli.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix CLAUDE.md syntax for analytics trends evidence add command
**Source:** high-agent (Apr 6, 72/78).
**Why:** Routine docs reference `trends evidence-add` (hyphenated) but correct CLI path is `analytics trends evidence add` (multi-level). Agents hit command-not-found errors.
**Scope:** Update CLAUDE.md and agent routine docs that reference `trends evidence-add` to use correct `analytics trends evidence add --id <N>` syntax.
**Effort:** < 30 minutes.

### [Feedback] Add pftui data news --breaking/--today flag for higher-cadence news
**Source:** medium-timeframe-analyst (Apr 8, 72/78).
**Why:** Agents fall back to web_search for breaking news because `pftui data news` returns cached results that may be hours old. A `--breaking` or `--today` flag that triggers a live fetch (not daemon-cached) would reduce web_search dependence and keep news within the pftui ecosystem.
**Scope:** Add `--breaking` flag to `data news` (or `data news --today`) that bypasses cache and fetches fresh headlines. Apply higher-priority fetch cadence. Files: `src/commands/news.rs`, `src/data/news.rs`, `src/cli.rs`.
**Effort:** 2–4 hours.

### [Feedback] Fix data refresh hard timeout with no error output
**Source:** low-timeframe-analyst (Apr 7, 72/78).
**Why:** `pftui data refresh` was SIGTERMed at ~90s with no error message — agent had no way to know refresh failed, or which sources succeeded vs failed. Fallback to cached data worked, but silent failure is brittle.
**Scope:** (1) Add signal handler for SIGTERM in `data refresh` to print partial results before exit. (2) Consider `--quick` flag or `--timeout <secs>` to allow agent-controlled timeout. (3) On timeout, emit structured JSON with `{status: "partial", completed_sources: [...], failed_sources: [...]}` instead of hard kill. Files: `src/commands/refresh.rs`, `src/cli.rs`.
**Effort:** 2–4 hours.

### [Feedback] Fix prediction market --category filter for geopolitical/Iran contracts
**Source:** low-timeframe-analyst (Apr 7, 72/78).
**Why:** `data predictions markets --category geopolitics` returned only 1 result (an OpenAI hardware question misclassified). Iran/Fed contracts that should match returned 0. Category classification or keyword matching for prediction market contracts is unreliable.
**Scope:** Investigate category classification in `data/predictions.rs` market fetch. Likely the tag-to-category mapping is too narrow. Add Iran/geopolitical keywords. Files: `src/data/predictions.rs`.
**Effort:** 1–2 hours.

---

## P3 - Long Term

### F59: Capital Flow Tracking
**Source:** Competitive research (NOFX institutional flow data).
**Why:** Institutional fund flows, ETF creation/redemption, and open interest changes reveal positioning that price alone doesn't show.
**Scope:** New `data flows` source pulling ETF flow data (ETF.com or similar), institutional 13F filings, and crypto exchange flow data. New table `capital_flows`. Integration into agent routines.
**Effort:** 3–4 weeks.

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Medium-Timeframe Analyst | 72% | 78% | Apr 8 | → (--driver DB error recurring; stale FRED still present despite fallback PRs.) |
| Medium-Agent | 72% | 78% | Apr 7 | → (analytics medium improved; FRED staleness persistent.) |
| Low-Agent | 72% | 74% | Apr 7 | → (new reviewer; commodity daily_change null in brief.) |
| Evening Analyst | 72% | 75% | Apr 7 | → (scorecard lesson-coverage shipped; FRED staleness persists.) |
| Low-Timeframe Analyst | 72% | 78% | Apr 7 | ↑ (was 55/72 Apr 6 — severity docs, digest filters, ack help all shipped.) |
| High-Agent | 72% | 78% | Apr 6 | new reviewer. fear-greed, COMEX 403, views --layer, docs syntax gaps remain. |
| Evening Analysis | 78% | 75% | Apr 6 | ↑ (from 72/68 Apr 5.) |
| Morning Brief | 82% | 78% | Apr 5 | → (stable.) |
| Macro-Timeframe Analyst | 55% | 62% | Apr 5 | ↑ (many items shipped Apr 6–7; expect score recovery on next run.) |

**Top 3 priorities based on feedback:**
1. **commodity daily_change null in portfolio brief (P1)** — low-agent 72/74 affected. Quick fix: trace brief.rs change_1d path for futures symbols.
2. **analytics situation update log --driver DB error (P1)** — recurring across Apr 5 and Apr 8. Same root cause as previously fixed `journal scenario update`, different code path.
3. **FRED staleness still degrading macro quality** — fallback PRs merged but agents continue reporting degraded FRED. Re-audit fallback activation logic.

**Shipped since last review (Apr 7 — previous run):**
- Fix clippy unnecessary_cast in cot.rs test data — `week as i64` → `week` (this PR)
- analytics situation severity validation docs (PR #658) — `--severity` now shows valid values
- analytics digest --from/--agent-filter flags (PR #659) — date + agent filtering
- agent message ack --to clarified help text (PR #660) — concrete usage examples
- prediction scorecard --lesson-coverage (PR #656) — annotates unlessoned wrong predictions
- stale data health in analytics guidance (PR #654) — surfaces degraded sources at session start
- analytics medium snapshot improved (PR #653) — now returns useful medium-TF data
- COT schedule metadata + Friday retry (PR #652) — `next_report_date` field, auto-refetch
- GDPNow fallback + GDP cadence context (PR #651) — fixes 188-day staleness
- CPI/PPI FRED fallbacks (PR #650) — BLS fallback when FRED fails
- DGS10 Yahoo Finance fallback (PR #649) — ^TNX fallback for 4-day staleness
- silver stale price status (PR #646) — `stale: true` flag on data prices
- clippy errors in power_signals.rs + supply.rs (PR #648) — unblocked release eligibility

**Release status:** v0.26.0 (Apr 4). **Tests:** 2606 passed / 0 failed / 2 ignored. **Clippy:** ✅ Clean (cot.rs fix this PR). **Release eligibility:** ✅ All conditions met — cut v0.27.0 immediately after this PR merges (84 commits of features/fixes since v0.26.0).

**GitHub stars:** 9 — Homebrew Core requires 50+.
