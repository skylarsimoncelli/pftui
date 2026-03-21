# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 — Critical

_(none)_

## P1 — Always-On Analytics Engine

- [Feedback] **`analytics recap --date today` returns empty when no events yet** — Evening Analyst reported empty results. If the date is today and no recap events exist yet, fall back to showing yesterday's recap with a note, or display a "no events recorded yet today" message instead of empty output. (`src/commands/recap.rs`)

### F53: Situation Engine — Canonical “What Matters Now” Layer

> Status: shipped on `feat/situation-engine` / PR #121. The canonical situation contract now exists in Rust and is reused by CLI, mobile, and web.

> Vision fit: the system should not depend on prompt text or client-side heuristics to decide what matters. One canonical market situation model should be computed in Rust/Postgres and reused everywhere.
>
> Product principle:
> - move analysis out of the AI layer and into the analytics layer
> - make the Situation Room a first-class analytics product
> - give mobile, web, CLI, and future agent surfaces the same ranked answer to: what matters now, why, how severe, and which assets are affected
>
> Actionable scope:
> 1. Add `pftui analytics situation --json`
> 2. Define native `SituationSnapshot`, `SituationInsight`, `PortfolioImpact`, `RiskState`, and `CrossTimeframeState`
> 3. Centralize ranking/severity logic in Rust instead of UI code
> 4. Reuse the same contract through mobile and web APIs
> 5. Add deterministic ranking and empty-data tests

### F54: Delta Engine — “What Changed” As A Native Product

> Status: shipped on `feat/situation-engine` / PR #121 follow-up. Server-owned `situation_snapshots`, `analytics deltas`, and shared `change_radar` API payloads are in place; future work can deepen coverage and persistence cadence.

> Vision fit: monitoring is about state transitions, not just snapshots. The analytics layer should explicitly report what changed since the last refresh, prior close, 24h, and 7d.
>
> Actionable scope:
> 1. Add `pftui analytics deltas --json [--since last-refresh|close|24h|7d]`
> 2. Detect changes in timeframe scores, alerts, freshness, sentiment, market pulse, scenarios, convictions, and correlations
> 3. Persist snapshots/deltas where needed so history is server-owned instead of client-owned
> 4. Feed `change_radar[]` into mobile and web
> 5. Add stable/no-change and major-shift tests

### F55: Catalyst Engine — Event Relevance And Countdown

> Status: shipped on `feat/situation-engine` / PR #121 follow-up. Rust-native `analytics catalysts` now ranks upcoming events by countdown, macro significance, portfolio relevance, and scenario/prediction linkage, and the same feed is exposed to mobile and web.

> Vision fit: pftui should identify what is coming next and why it matters to the portfolio and current regime, not just report what already happened.
>
> Actionable scope:
> 1. Add `pftui analytics catalysts --json [--window today|tomorrow|week]`
> 2. Define `CatalystEvent` with time, source, category, significance, affected assets, linked scenarios/predictions, and countdown bucket
> 3. Rank catalysts by portfolio relevance and macro significance
> 4. Expose catalyst feeds in Situation Room and web
> 5. Add windowing, ranking, and linkage tests

### F56: Portfolio Impact And Opportunity Engine

> Status: shipped on `feat/situation-engine` / PR #121 follow-up. Native `analytics impact` and `analytics opportunities` now rank held/watchlist exposure and non-held ideas from shared evidence chains built from convictions, trends, scenarios, technical signals, and catalysts, and those outputs are reused by the mobile Situation Room and web API.

> Vision fit: pftui should understand the user’s book and discover important opportunities outside it. That is what turns it into an intelligence system instead of a passive tracker.
>
> Actionable scope:
> 1. Add `pftui analytics impact --json` for holdings/watchlist exposure
> 2. Add `pftui analytics opportunities --json` for high-alignment non-held assets
> 3. Build explicit evidence chains from signals, scenarios, trends, regime, and catalysts
> 4. Reuse outputs in mobile and web Situation Room
> 5. Add exposure ranking and held-vs-non-held tests

### F57: Cross-Timeframe Synthesis Engine

> Status: shipped on `feat/situation-engine` / PR #121 follow-up. Native `analytics synthesis` now computes strongest alignment, highest-confidence divergences, constraint flows, unresolved tensions, and watch-tomorrow candidates, with shared reuse through mobile and web.

> Vision fit: “constraints flow downward, signals flow upward” should become a native analytics concept rather than prompt lore.
>
> Actionable scope:
> 1. Add `pftui analytics synthesis --json`
> 2. Compute strongest alignment, highest-confidence disagreement, constraint flows, unresolved tensions, and watch-tomorrow candidates
> 3. Define `AlignmentState`, `DivergenceState`, and `ConstraintState`
> 4. Reuse synthesis output in situation, briefs, mobile, and web
> 5. Add agreement/disagreement classification tests

### F58: Narrative State And Structured Recap Layer

> Vision fit: pftui should accumulate machine-readable analytical memory so recap and synthesis can be rendered without depending on an LLM.
>
> Actionable scope:
> 1. Add `pftui analytics narrative --json`
> 2. Include scenario shifts, conviction changes, trend changes, prediction scorecard summary, surprises, lessons, and catalyst outcomes
> 3. Persist recap/narrative state where useful
> 4. Expose structured recap views in mobile and web
> 5. Add ordering and empty-state tests

## P2 — Coverage And Agent Consumption

- [Feedback] **Oil physical vs futures premium tracking** — Medium-Timeframe Analyst (85/90) needs physical oil spot vs front-month futures premium data for war-time indicator analysis. Add `data oil-premium` or extend existing `data oil-inventory` to include spot/futures spread. Critical for geopolitical regime analysis. (`src/commands/oil.rs` or new)

- [Feedback] **Correlation break alerts** — Low-Timeframe Analyst wants alerts when historically correlated pairs diverge beyond N sigma. Could build on existing `analytics correlations` infrastructure. Add configurable thresholds and integrate with alert system. (`src/commands/correlations.rs`, `src/commands/alerts.rs`)

- [Feedback] **Scan threshold tuning** — Alert Investigator noted BIG-GAINERS scan triggered on minor gains during broad selloff (noise not signal). Add configurable minimum thresholds for scan alerts (e.g., minimum absolute % move, relative-to-market filter) to reduce false positives. (`src/commands/scan.rs`)

- [Feedback] **Scenario probability tracking in data sources** — Low-Timeframe Analyst wants scenario probabilities surfaced in data source commands for faster narrative shift detection. Could auto-inject active scenario probabilities into `analytics summary` and `analytics low` output. (`src/commands/scenario.rs`, `src/commands/summary.rs`)

## P3 — Long Term

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 65% | 72% | Mar 21 | ↑ (up from 45/55, movers fixed, journal alert section added, but CLI discoverability issues remain) |
| Medium-Timeframe Analyst | 85% | 90% | Mar 21 | ↑↑ (up from 70/75, data coverage now 90%, only 5 web searches needed vs 15+) |
| Alert Investigator | 75-85% | 80-82% | Mar 21 | → (stable, consistent routine monitoring, system functioning well) |
| Morning Intelligence | 75% | 80% | Mar 20 | → (portfolio sync concern noted) |
| Low-Timeframe Analyst | 85-90% | 85-88% | Mar 20 | → (stable high, BTC anomaly fixed via plausibility guard PR #99) |
| Dev Agent | 90% | 90-92% | Mar 21 | → (shipping consistently, all feedback items resolved, codebase clean) |

**Key changes since last review (Mar 20):**
- v0.14.0 released on Mar 20 (84 commits since v0.13.0)
- 34 commits since v0.14.0 tag: journal alert section (#107), mobile API runtime fix (#112), plausibility guard (#99), partial_cmp fix (#92)
- Tests: 1505 passing (up from 1495 at v0.14.0 release), clippy clean
- Evening Analyst recovering: 45/55 → 65/72 after P0 fixes landed
- Medium-Timeframe Analyst big jump: 70/75 → 85/90 — data coverage dramatically improved
- BTC 224K% anomaly fixed via plausibility guard (±500% cap, PR #99)
- `journal notes --section alert` added (PR #107) — alert-investigator feedback resolved
- `score-batch` already exists but Evening Analyst didn't find it — discoverability issue, not missing feature

**Top 3 priorities based on feedback:**
1. **P1: CLI discoverability** — `analytics conviction list` fails, `score-batch` undiscoverable. Evening Analyst (lowest scorer) directly impacted.
2. **P1: `analytics recap --date today` empty handling** — Evening Analyst couldn't use recap for analysis.
3. **P2: Oil physical/futures premium** — Medium-Timeframe Analyst needs this for war-regime analysis (currently fastest-improving tester, would cement gains).

**Release status:** v0.14.0 is current release. 34 post-release commits (feedback entries, 3 code fixes). Next release (v0.14.1 or v0.15.0) gated on resolving P1 items. No P0 bugs.

**GitHub stars:** 4 — Homebrew Core requires 50+.
