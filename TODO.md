# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 - Data Quality & Agent Reliability

### [Done] Add prediction lesson bulk command — lesson coverage at 8% is critical
**Source:** evening-analyst (Apr 9, 80/78 — explicit P1 flag: "Lesson coverage at 8% (8 of 62 wrong predictions) - system cannot learn at this rate. Recommend auto-lesson-extraction for wrong predictions >7 days old"). evening-analysis (Apr 9: "8% CRITICAL — target 80%. Only 5 of ~62 wrong predictions have structured lessons"). Corroborates Apr 5 evening-analysis feedback ("prediction lessons backlog (63 unresolved) needs a bulk-lesson workflow. Suggest: pftui prediction lesson bulk command").
**Why:** Lesson coverage is 8% (8 of 62 wrong predictions with structured lessons). The system's self-improvement loop is functionally non-functional — agents cannot identify systematic biases because 92% of wrong predictions have no post-mortem. The `prediction scorecard --lesson-coverage` flag (PR #656) surfaces unlessoned predictions, but there is no efficient command for processing them in bulk. At 3 lessons per evening session, reaching the 80% target would take 15+ days of manual processing.
**Scope:** (1) Add `pftui prediction lesson bulk` subcommand that lists all wrong predictions without lessons, sorted by age (oldest first). (2) Add `--auto-stub` flag that generates a template lesson from the prediction claim + outcome, requiring only the agent to fill `root_cause` and `going_forward` fields. (3) Surface lesson coverage % prominently in `prediction scorecard` output alongside the `--lesson-coverage` list. Files: `src/commands/prediction.rs`, `src/db/prediction.rs`.
**Effort:** 3–5 hours.

---

## P2 - Coverage And Agent Consumption

### [Done] Add data fear-greed subcommand
**Source:** high-agent (Apr 6, 72/78).
**Why:** `pftui data fear-greed` subcommand is missing. Fear & Greed Index is a key sentiment indicator for high-timeframe structural analysis and is commonly referenced alongside VIX. Alternative.me API is free and reliable.
**Scope:** New `data fear-greed` command pulling from Alternative.me Crypto Fear & Greed API (or CNN F&G for traditional markets). Store in DB table with history. JSON + terminal output. Integrate into `data refresh` and surface in `analytics market-snapshot`. Files: `src/commands/fear_greed.rs` (new), `src/cli.rs`, `src/main.rs`, `src/data/refresh.rs`.
**Effort:** 3–5 hours.

### [Done] Fix sovereign data returning empty (COMEX 403 on gold/silver)
**Source:** high-agent (Apr 6, 72/78).
**Why:** `pftui data supply` returns empty data due to COMEX returning 403 Forbidden. The stale-cache fallback added in PR #636 should handle this, but may not be reaching cached rows or the COMEX URL needs updating.
**Scope:** (1) Confirm stale-cache fallback in `src/commands/supply.rs` is working for COMEX 403 cases. (2) If not, trace the fallback path and fix. (3) Add alternative source (Kitco public API or WGC) if COMEX 403 is persistent. Files: `src/commands/supply.rs`.
**Effort:** 2–4 hours.

### [Done] Add --layer filter to analytics views divergence
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
**Source:** medium-timeframe-analyst (Apr 8, 72/78; Apr 9, 68/76 — repeated across two sessions).
**Why:** Agents fall back to `web_search` for breaking news because `pftui data news` returns daemon-cached results that may be hours old. A `--breaking` or `--today` flag that triggers a live fetch (bypassing cache) would reduce `web_search` dependence and keep news within the pftui ecosystem. Note: the `NEWS_UNAVAIL` root cause is a separate P1 bug — this feature request applies once the core feed is stable.
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

### [Feedback] Fix analytics debate tool returning empty debate_id
**Source:** evening-analysis (Apr 9, 82/79 — "Debate tool returned empty debate_id despite topic being valid — could not persist debate programmatically").
**Why:** The `analytics debate` command returns an empty `debate_id` in its JSON output even when the topic is valid and the debate runs successfully. Without a valid ID, agents cannot reference, retrieve, or persist the debate for later scoring or cross-session review. The Apr 9 evening analysis ran 3 adversarial debate rounds but could not store them programmatically.
**Scope:** Check the debate ID generation and return path. The ID is likely generated internally but not serialised into the JSON response, or the DB insert is failing silently and returning a zero/empty ID. Files: `src/commands/debate.rs` (or equivalent), `src/db/debate.rs`.
**Effort:** < 1 hour.

### [Feedback] Fix analytics macro regime set — valid labels not documented
**Source:** medium-timeframe-analyst (Apr 9, 68/76 — "analytics macro regime set accepted 'transitioning' regime label successfully but help docs only list risk-on/risk-off/crisis — document valid labels or validate with enum").
**Why:** `analytics macro regime set` accepts `transitioning` as a valid input but `--help` output only lists `risk-on`, `risk-off`, and `crisis`. Agents waste time guessing undocumented labels by trial and error. Should enumerate all valid labels in help text or validate input against an explicit enum with a clear error message listing valid options.
**Scope:** Update `analytics macro regime set` help text to enumerate all valid regime labels (including `transitioning`, `stagflation`, and any others accepted by the DB). Add enum validation: if an invalid label is passed, print a helpful error listing valid options. Files: `src/commands/regime.rs` (or equivalent), `src/cli.rs`.
**Effort:** < 30 minutes.

### [Feedback] Add prediction add --symbol null/empty support for non-asset predictions
**Source:** medium-timeframe-analyst (Apr 9, 68/76 — "prediction add --symbol field should support null/empty without requiring a ticker — non-asset predictions (CPI, NFP) are awkward to file").
**Why:** Economic data predictions (CPI, NFP, PMI, GDP, Core PCE) do not map to a single asset symbol. Requiring `--symbol` forces agents to invent placeholder tickers (e.g., `CPI`, `MACRO`, `NFP`) which pollute the symbol namespace and make filtered queries unreliable. Making `--symbol` optional (defaulting to NULL) would make macro data predictions first-class citizens.
**Scope:** Allow `--symbol` to be optional in `prediction add`. If omitted, store as NULL in DB. Update `prediction list`, `prediction scorecard`, and `prediction score` to handle null symbol gracefully (display as `—` or `[macro]`). Files: `src/commands/prediction.rs`, `src/db/prediction.rs`, `src/cli.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix VIX/regime signal lag during fast-reversing events
**Source:** evening-analysis (Apr 9, 82/79 — "VIX/regime signal lag during fast-reversing events (ceasefire day) caused regime to show risk-off despite risk-on price action — needs intraday refresh or event-triggered override").
**Why:** During the Apr 8 ceasefire, `analytics macro regime` showed `risk-off` for hours after markets had clearly shifted to risk-on (VIX -12.9%, S&P futures +2.5-3%, BTC +6.89%). Agents applying the stale regime classification applied wrong correlation assumptions. The system needs either faster intraday regime re-evaluation or a manual override command for fast-moving event days.
**Scope:** (1) Add `analytics macro regime override --regime <label> --reason <text> --expires <duration>` for manual intraday override that auto-expires (e.g., after 4 hours). (2) Consider triggering a regime re-evaluation automatically when VIX moves >15% intraday (the alert threshold already exists and could hook into regime re-check). Files: `src/commands/regime.rs`, `src/analytics/regime.rs`.
**Effort:** 2–3 hours.

### [Feedback] Fix analytics situation indicator list — stale last_checked timestamps
**Source:** low-timeframe-analyst (Apr 8, 72/74 — "analytics situation indicator list shows stale last_checked timestamps (March 22) — indicator pipeline not re-evaluating on each refresh cycle; should auto-update on pftui data refresh").
**Why:** Situation indicators show `last_checked: 2026-03-22` — over two weeks stale — even after `pftui data refresh` has run. The indicator evaluation pipeline is not wired into the `data refresh` cycle, so situation monitoring is running on signal evaluations that predate the Iran war. Agents relying on indicator status for situational awareness are reading outdated signals.
**Scope:** Wire situation indicator re-evaluation into the `data refresh` pipeline so `last_checked` is updated on each refresh cycle. Files: `src/data/refresh.rs`, `src/analytics/situation.rs`.
**Effort:** 1–2 hours.

### [Feedback] Fix data news --hours JSON output missing description field
**Source:** low-timeframe-analyst (Apr 8, 72/74 — "pftui data news --hours 4 titles lack descriptions (empty description field). Would benefit from including RSS snippet/summary in JSON output so news can be assessed without a follow-up web_fetch").
**Why:** `pftui data news --hours 4` returns headlines with an empty `description` field in JSON. Agents cannot assess news relevance from the title alone and must issue a `web_fetch` for each item to determine relevance — defeating the purpose of the aggregated news feed. Including the RSS snippet/summary in the JSON output would eliminate this round-trip.
**Scope:** Populate the `description` field from the RSS/source snippet at ingest time and ensure it is stored in the news DB table. Surface it in the JSON output from `data news`. Files: `src/commands/news.rs`, `src/data/news.rs`, `src/db/news.rs`.
**Effort:** 1–2 hours.

### [Feedback] Add auto-suggest CLI command for scenario-to-prediction-market mappings
**Source:** evening-analysis (Apr 9, 82/79 — "Calibration command still returning empty (no scenario-to-contract mappings after 5+ sessions flagging this). Suggestion: add automated Polymarket mapping for top 4 scenarios on first run, or CLI command to auto-suggest mappings"). Corroborates Apr 6 evening-analysis and multiple prior sessions flagging the same gap.
**Why:** `pftui data predictions calibration` has returned empty for 5+ consecutive sessions because no scenario-to-contract mappings have been created. The calibration system is entirely non-functional without mappings, and agents repeatedly flag this as a gap. Adding auto-suggest would unblock the calibration workflow in a single command.
**Scope:** (1) Add `pftui data predictions map --auto-suggest` that searches tracked Polymarket contracts for keywords matching each active scenario name and outputs the top 3 mapping candidates per scenario. (2) Add `pftui data predictions map --scenario <name> --contract-id <id>` for explicit manual mapping. (3) If calibration is empty and scenarios exist, surface a one-time prompt suggesting the user run `--auto-suggest`. Files: `src/commands/predictions.rs`, `src/data/predictions.rs`.
**Effort:** 2–4 hours.

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
| Evening Analysis | 82% | 79% | Apr 9 | ↑ (debate tool returns empty debate_id; calendar garbled 2nd occurrence; calibration empty 5+ sessions; regime lag on ceasefire day.) |
| Evening Analyst | 80% | 78% | Apr 9 | ↑ (news NEWS_UNAVAIL P1; lesson coverage 8% critical; COT 9d stale pre-war.) |
| Medium-Timeframe Analyst | 68% | 76% | Apr 9 | ↓ (FRED 67d+ stale on CPI/PPI/GDP/PCE despite fallback PRs; situation update log --driver recurring + --situation arg exit 1.) |
| Medium-Agent | 72% | 78% | Apr 7 | → (analytics medium improved; FRED staleness persistent.) |
| Low-Timeframe Analyst | 72% | 74% | Apr 8 | → (news descriptions empty; indicator list last_checked stale March 22.) |
| Low-Agent | 70% | 72% | Apr 8 | ↓ (was 72/74 Apr 7 — digest --agent-filter regression post-PR #659; calendar garbled.) |
| High-Agent | 72% | 78% | Apr 6 | new reviewer. fear-greed, COMEX 403, views --layer, docs syntax gaps remain. |
| Morning Brief | 82% | 78% | Apr 5 | → (stable.) |
| Macro-Timeframe Analyst | 55% | 62% | Apr 5 | ↑ (many items shipped Apr 6–7; expect score recovery on next run.) |

**Top 3 priorities based on feedback:**
1. **pftui data news NEWS_UNAVAIL root cause (P1)** — primary signal source completely down for the Apr 9 session; entire news workflow fell back to `web_search`. Fix before next cron run.
2. **FRED fallback activation re-audit (P1)** — CPI/PPI/GDP/GDPNow/DGS10 all stale despite PRs #649–651 being shipped. Fallback logic not activating; macro data degraded across all agents for 5+ sessions.
3. **Prediction lesson bulk command — lesson coverage 8% (P1)** — 92% of wrong predictions have no structured lesson. System self-improvement loop non-functional. Evening-analyst rated this P1; target is 80% coverage.

**Shipped since last review (Apr 7 — previous run):**
- Fix clippy unnecessary_cast in cot.rs test data — `week as i64` → `week` (this PR)
- analytics situation severity validation docs (PR #658) — `--severity` now shows valid values
- analytics digest --from/--agent-filter flags (PR #659) — date + agent filtering *(regression: --agent-filter still throwing unexpected argument error post-merge — see P1)*
- agent message ack --to clarified help text (PR #660) — concrete usage examples
- prediction scorecard --lesson-coverage (PR #656) — annotates unlessoned wrong predictions
- stale data health in analytics guidance (PR #654) — surfaces degraded sources at session start
- analytics medium snapshot improved (PR #653) — now returns useful medium-TF data
- COT schedule metadata + Friday retry (PR #652) — `next_report_date` field, auto-refetch *(COT still 9d stale Apr 9 — Friday retry not firing — see P1)*
- GDPNow fallback + GDP cadence context (PR #651) — fixes 188-day staleness *(GDPNow still 98d stale Apr 9 — fallback not activating — see P1 FRED re-audit)*
- CPI/PPI FRED fallbacks (PR #650) — BLS fallback when FRED fails *(CPI/PPI still 67d stale Apr 9 — fallback not activating — see P1 FRED re-audit)*
- DGS10 Yahoo Finance fallback (PR #649) — ^TNX fallback for 4-day staleness *(DGS10 still stale Apr 7 — fallback not activating — see P1 FRED re-audit)*
- silver stale price status (PR #646) — `stale: true` flag on data prices
- clippy errors in power_signals.rs + supply.rs (PR #648) — unblocked release eligibility

**Release status:** v0.26.0 (Apr 4). **Tests:** 2606 passed / 0 failed / 2 ignored. **Clippy:** ✅ Clean (cot.rs fix this PR). **Release eligibility:** ✅ All conditions met — cut v0.27.0 immediately after this PR merges (84 commits of features/fixes since v0.26.0).

**GitHub stars:** 9 — Homebrew Core requires 50+.
