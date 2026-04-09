# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical Deploy Bug

### [Feedback] Fix deploy.sh: /usr/local/bin/pftui not updated on deploy
**Source:** Discovered during Apr 9 daily review. Corroborates all agent feedback since v0.27.0 (PRs #663+).
**Why:** `cargo install --path .` (and `deploy.sh`) installs to `/root/.cargo/bin/pftui`, but `/usr/local/bin/pftui` is earlier in PATH. Systemd services and all agent invocations resolve `/usr/local/bin/pftui` — which was still v0.26.0 after v0.27.0 was supposedly deployed. Every feature shipped since v0.26.0 (digest --agent-filter, portfolio status, scenario mappings, situation log fixes, etc.) was inaccessible to agents. Manually patched Apr 9 03:xx UTC by copying v0.27.0 binary to `/usr/local/bin/pftui`.
**Scope:** Update `scripts/deploy.sh` to also atomically install to `/usr/local/bin/pftui` (or add a symlink from `/usr/local/bin/pftui` → `/root/.cargo/bin/pftui`). Update release process docs and Step 0 smoke test to confirm `which pftui` resolves to the correct version. Files: `scripts/deploy.sh`, `docs/RELEASING.md`, agent routine docs.
**Effort:** < 1 hour.

---

## P1 - Data Quality & Agent Reliability

### [Feedback] Fix data news feed NEWS_UNAVAIL
**Source:** evening-analyst (Apr 9, 80/78).
**Why:** `pftui data news` returned `NEWS_UNAVAIL` — primary signal source was unavailable for 24h. News is a core analytical input; unavailability forces all agents to fall back to web_search and degrades every routine that depends on news feeds.
**Scope:** (1) Investigate news fetcher for broken source, expired API key, or rate-limit condition. (2) Add structured `{status: "unavailable", reason: "..."}` JSON error output instead of bare NEWS_UNAVAIL string. (3) Consider secondary news source fallback (RSS feeds). Files: `src/data/news.rs`, `src/commands/news.rs`.
**Effort:** 2–3 hours.

### [Feedback] Fix calendar garbled event names (recurring)
**Source:** low-agent (Apr 8, 70/72) + evening-analysis (Apr 9, 82/79). 2nd occurrence.
**Why:** `pftui data calendar` returns corrupted numeric strings (percentages/numbers) instead of event names. Recurring across two distinct agents on different days — not a one-off.
**Scope:** Trace calendar event name parsing/rendering in `src/commands/calendar.rs` and `src/data/calendar.rs`. Likely a parsing/serialization bug where event names are being replaced with associated numeric values (forecast/actual). Add regression test for event name integrity. Files: `src/commands/calendar.rs`, `src/data/calendar.rs`.
**Effort:** 1–2 hours.

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

### [Feedback] Fix data news --hours returning empty descriptions
**Source:** low-timeframe-analyst (Apr 8, 72/74).
**Why:** `pftui data news --hours 4` returns headline titles but empty `description` fields. Agents must follow up with `web_fetch` per article to get context, which defeats the purpose of the integrated news command.
**Scope:** Ensure RSS snippet/summary is captured and stored in the news DB table. Include description/snippet field in `data news --json` output. Files: `src/data/news.rs`, `src/commands/news.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix analytics situation indicators not re-evaluating on refresh
**Source:** low-timeframe-analyst (Apr 8, 72/74).
**Why:** `analytics situation indicator list` shows `last_checked` timestamps from March 22 — indicators are not being re-evaluated when `pftui data refresh` runs. Stale indicator status makes situation tracking unreliable.
**Scope:** Wire situation indicator evaluation into the `data refresh` pipeline so indicators are checked on each refresh cycle. Files: `src/commands/refresh.rs`, `src/analytics/situation.rs`.
**Effort:** 2–3 hours.

---

## P2 - Coverage And Agent Consumption

### [Feedback] Document/validate analytics macro regime set valid labels
**Source:** medium-timeframe-analyst (Apr 9, 68/76).
**Why:** `pftui analytics macro regime set` help text only lists `risk-on`, `risk-off`, `crisis` as valid values, but `transitioning` was accepted without error. Agents must discover undocumented labels by trial and either get silent success or silent failure. Should enumerate all valid regime labels or validate with an enum and surface accepted values in help/error messages.
**Scope:** Audit all regime label handling in `src/analytics/macro_regime.rs`. If labels are freeform strings, document the canonical set. If they should be typed, add an enum with clap validation. Files: `src/cli.rs`, `src/analytics/macro_regime.rs`.
**Effort:** 1–2 hours.

### [Feedback] Support null/empty --symbol in prediction add for non-asset predictions
**Source:** medium-timeframe-analyst (Apr 9, 68/76).
**Why:** `pftui journal prediction add --symbol ...` requires a ticker symbol, making macro predictions (CPI print, NFP, ISM) awkward to file — agents must use a proxy ticker or leave the field blank via workarounds.
**Scope:** Make `--symbol` optional in `journal prediction add`. When absent, store as NULL. Update display and scoring to handle symbol-less predictions gracefully. Files: `src/cli.rs`, `src/commands/predict.rs`, `src/db/predictions.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix analytics situation update log argument parsing for long strings
**Source:** medium-timeframe-analyst (Apr 9, 68/76).
**Why:** `analytics situation update log --situation <long-text>` exits 1 on longer detail strings. Appears to be an argument parsing or shell-escaping issue rather than a DB error. Short strings work; longer ones fail.
**Scope:** Investigate clap argument parsing for `--situation` and `--detail` flags in the situation log path. Check for length limits, quote handling, or special character issues. Files: `src/cli.rs`, `src/commands/situation.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix calibration returning empty / auto-suggest Polymarket scenario mappings
**Source:** evening-analysis (Apr 9, 82/79). 5+ sessions flagging this.
**Why:** `analytics calibration` returns empty because no scenario-to-contract mappings have been configured, despite 1699 Polymarket contracts flowing through the system. The feature is effectively dead without mappings. A CLI command to auto-suggest mappings (keyword matching scenarios to contracts) would unblock this entirely.
**Scope:** Add `data predictions map suggest` (or `analytics calibration suggest`) command that keyword-matches active scenario names against Polymarket contract titles and outputs candidate mappings for confirmation. Files: `src/commands/predictions.rs`, `src/data/predictions.rs`.
**Effort:** 2–4 hours.

### [Feedback] Fix debate tool returning empty debate_id
**Source:** evening-analysis (Apr 9, 82/79).
**Why:** `journal agent debate start` returns an empty `debate_id` field, making it impossible to programmatically add rounds or resolve debates. Debates cannot be persisted across agent steps.
**Scope:** Investigate `debate start` return path in `src/commands/debates.rs`. Ensure the inserted debate ID is returned in both JSON and terminal output. Add CLI test for non-empty debate_id on start. Files: `src/commands/debates.rs`.
**Effort:** < 1 hour.

### [Feedback] Add auto-lesson-extraction for wrong predictions > 7 days old
**Source:** evening-analyst (Apr 9, 80/78). Lesson coverage at 8% (8/62 wrong predictions).
**Why:** Lesson coverage has been below 10% for multiple sessions. Manual lesson extraction doesn't scale. A command to bulk-generate skeleton lessons for wrong predictions older than N days (with auto-filled miss-type from prediction data) would dramatically improve coverage.
**Scope:** New `journal prediction lessons auto-extract [--days 7] [--dry-run]` command that scans wrong predictions without lessons and creates skeleton lesson entries (pre-filling symbol, prediction text, miss-type=unknown, root-cause=needs-review). Files: `src/commands/lessons.rs` (or `predict.rs`), `src/cli.rs`.
**Effort:** 2–3 hours.

### [Feedback] Fix GDPNow still stale despite PR #651 fallback
**Source:** evening-analyst (Apr 9, 80/78). 98 days stale for a weekly series.
**Why:** PR #651 added Atlanta Fed web fallback for GDPNow, but agents are still reporting 98-day staleness on Apr 9. Either the fallback isn't being triggered, the stored value isn't being surfaced, or the data refresh hasn't run with the new code since the v0.26.0→v0.27.0 deploy bug masked the fix.
**Scope:** (1) Verify fallback triggers correctly by running `pftui data refresh --only gdp` after the deploy fix. (2) If still stale, debug `src/data/fred.rs` GDPNOW_WEB fallback logic. (3) Add stale-GDPNow warning to `pftui system doctor`. Files: `src/data/fred.rs`, `src/commands/economy.rs`.
**Effort:** 1–2 hours.

### [Feedback] Add intraday regime refresh / event-triggered override
**Source:** evening-analysis (Apr 9, 82/79).
**Why:** During fast-reversing events (ceasefire day), regime showed `risk-off` despite risk-on price action in real time. The macro regime classifier runs on the standard refresh cycle and lags intraday events.
**Scope:** Add a lightweight `analytics macro regime evaluate` command that re-scores regime from current cached prices/VIX without a full refresh cycle. Agents can call this after major news to update regime context mid-session. Files: `src/analytics/macro_regime.rs`, `src/cli.rs`.
**Effort:** 2–4 hours.

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

### [Feedback] Add pftui data news --breaking/--today flag with full descriptions
**Source:** medium-timeframe-analyst (Apr 8, 72/78) + medium-timeframe-analyst (Apr 9, 68/76).
**Why:** Agents fall back to web_search for breaking news because `pftui data news` returns cached results that may be hours old. A `--breaking` or `--today` flag that triggers a live fetch (not daemon-cached) with full descriptions would reduce web_search dependence significantly.
**Scope:** Add `--breaking` flag to `data news` (or `data news --today`) that bypasses cache and fetches fresh headlines with full RSS snippets. Files: `src/commands/news.rs`, `src/data/news.rs`, `src/cli.rs`.
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
| Medium-Timeframe Analyst | 68% | 76% | Apr 9 | ↓ (LOWEST. FRED 67+ days stale on CPI/PPI/GDP/PCE; situation log arg parsing; regime label docs.) |
| Low-Agent | 70% | 72% | Apr 8 | ↓ (calendar garbled names; daily_change null still; digest --agent-filter was deploy-bug, now fixed.) |
| Low-Timeframe Analyst | 72% | 74% | Apr 8 | → (news descriptions empty; situation indicators stale.) |
| Evening Analyst | 80% | 78% | Apr 9 | ↑ (from 72/75 Apr 7 — news unavailable Apr 9 blocked run.) |
| Evening Analysis | 82% | 79% | Apr 9 | ↑ (from 78/75 Apr 6 — debate empty ID; calibration still empty; calendar garbled.) |
| Morning Brief | 82% | 78% | Apr 5 | → (stable.) |
| High-Agent | 72% | 78% | Apr 6 | → (fear-greed, COMEX 403, views --layer, docs syntax gaps remain.) |
| Medium-Agent | 72% | 78% | Apr 7 | → (FRED staleness persistent.) |
| Macro-Timeframe Analyst | 55% | 62% | Apr 5 | ↑ (many items shipped Apr 6–7; expect score recovery.) |

**Score balance:** Medium-Timeframe Analyst at 68/76 is the lowest — their pain points (FRED staleness, situation log, regime docs) should get P1/P0 priority.

**Top 3 priorities based on feedback:**
1. **P0: Fix deploy.sh /usr/local/bin/pftui gap** — all v0.27.0 features were inaccessible to agents; manually patched Apr 9. Make permanent.
2. **P1: Fix data news NEWS_UNAVAIL** — primary signal source down for evening-analyst; needs immediate investigation + fallback.
3. **P1: Fix calendar garbled event names (recurring)** — 2nd occurrence across two agents; corrupted numeric strings instead of event names.

**Shipped since last review (Apr 8 — previous run):**
- v0.27.0 deployed (PR #663) — 84-commit release: situation severity docs, digest filters, agent ack help, lesson-coverage scorecard, guidance data health, analytics medium snapshot, COT schedule, GDPNow fallback, CPI/PPI BLS fallback, DGS10 Yahoo fallback, silver price status, clippy fixes
- Deploy bug discovered and patched Apr 9: `/usr/local/bin/pftui` was v0.26.0; copied v0.27.0 binary manually
- analytics digest --agent-filter confirmed working after deploy fix (was not a code regression)

**Release status:** v0.27.0 (Apr 9, 84 commits). **Tests:** 2606 passed / 0 failed / 2 ignored. **Clippy:** ✅ Clean. **Release eligibility:** ❌ No new feature commits since v0.27.0 — only feedback/report PRs (#664–#668). No release needed until P0/P1 fixes land.

**GitHub stars:** 9 — Homebrew Core requires 50+.
