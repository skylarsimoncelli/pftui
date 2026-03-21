# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 — Critical

_(none)_

## P1 — Always-On Analytics Engine

_(none)_

## P2 — Coverage And Agent Consumption

- [Feedback] **Oil physical vs futures premium tracking** — Medium-Timeframe Analyst (85/90) needs physical oil spot vs front-month futures premium data for war-time indicator analysis. Add `data oil-premium` or extend existing `data oil-inventory` to include spot/futures spread. Critical for geopolitical regime analysis. (`src/commands/oil.rs` or new)

- [Feedback] **Correlation break alerts** — Low-Timeframe Analyst wants alerts when historically correlated pairs diverge beyond N sigma. Could build on existing `analytics correlations` infrastructure. Add configurable thresholds and integrate with alert system. (`src/commands/correlations.rs`, `src/commands/alerts.rs`)

- [Feedback] **Scenario probability tracking in data sources** — Low-Timeframe Analyst wants scenario probabilities surfaced in data source commands for faster narrative shift detection. Could auto-inject active scenario probabilities into `analytics summary` and `analytics low` output. (`src/commands/scenario.rs`, `src/commands/summary.rs`)

## P3 — Long Term

- **Integrate native narrative/situation analytics into `agents/routines/` prompts** — Update the multi-timeframe analyst and delivery routines so they consume the new Rust/Postgres-native analytics products instead of re-deriving the same logic in prompt text. Scope:
  1. Review `low-timeframe-analyst.md`, `medium-timeframe-analyst.md`, `high-timeframe-analyst.md`, `macro-timeframe-analyst.md`, `morning-brief.md`, and `evening-analysis.md`
  2. Replace prompt-internal recap / “what changed” / cross-timeframe synthesis / portfolio-impact / catalyst-ranking steps with calls to `pftui analytics situation --json`, `pftui analytics deltas --json`, `pftui analytics catalysts --json`, `pftui analytics impact --json`, `pftui analytics opportunities --json`, `pftui analytics synthesis --json`, and `pftui analytics narrative --json` where appropriate
  3. Keep the routines focused on judgment, escalation, and prose synthesis, not recomputing facts already owned by the analytics layer
  4. Update routine examples and handoff contracts so analysts reference canonical payload fields rather than vague prompt lore
  5. Verify the revised routines still preserve the multi-timeframe operating model while reducing AI-side duplicated reasoning

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
2. **P2: Oil physical/futures premium** — Medium-Timeframe Analyst needs this for war-regime analysis (currently fastest-improving tester, would cement gains).
3. **P2: Correlation break alerts** — Low-Timeframe Analyst wants sigma-based divergence alerts on historically linked pairs.

**Release status:** v0.14.0 is current release. 34 post-release commits (feedback entries, 3 code fixes). Next release (v0.14.1 or v0.15.0) gated on resolving remaining P1/P2 items. No P0 bugs.

**GitHub stars:** 4 — Homebrew Core requires 50+.
