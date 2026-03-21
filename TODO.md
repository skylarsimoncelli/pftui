# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Always-On Analytics Engine

_(none)_

## P2 - Coverage And Agent Consumption

- [Feedback] **Scenario probability tracking in data sources** - Low-Timeframe Analyst wants scenario probabilities surfaced in data source commands for faster narrative shift detection. Could auto-inject active scenario probabilities into `analytics summary` and `analytics low` output. (`src/commands/scenario.rs`, `src/commands/summary.rs`)

## P3 - Long Term

- ~~**Integrate native analytics into routines**~~ — DONE (commit `109fd67`). All 6 routines now consume situation, deltas, catalysts, impact, opportunities, synthesis, narrative where relevant.

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
- Medium-Timeframe Analyst big jump: 70/75 → 85/90 - data coverage dramatically improved
- BTC 224K% anomaly fixed via plausibility guard (±500% cap, PR #99)
- `journal notes --section alert` added (PR #107) - alert-investigator feedback resolved
- `score-batch` already exists but Evening Analyst didn't find it - discoverability issue, not missing feature

**Top 3 priorities based on feedback:**
1. **P1: CLI discoverability** - `analytics conviction list` fails, `score-batch` undiscoverable. Evening Analyst (lowest scorer) directly impacted.
2. **P2: Scenario probability tracking** - Low-Timeframe Analyst wants scenario probabilities surfaced in data source commands.
3. **P3: Integrate native analytics into agent routines** - Replace prompt-internal recap/synthesis with canonical analytics CLI calls.

**Release status:** v0.14.0 is current release. 34 post-release commits (feedback entries, 3 code fixes). Next release (v0.14.1 or v0.15.0) gated on resolving remaining P1/P2 items. No P0 bugs.

**GitHub stars:** 4 - Homebrew Core requires 50+.
