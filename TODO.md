# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Feature Requests

### Move `journal` to top-level command

`journal` is currently nested under `pftui agent journal`. It should be top-level: `pftui journal`.

Journal is the shared knowledge layer (predictions, convictions, notes, scenarios, entries).
It is used by both humans and agents. Nesting it under `agent` implies it's agent-only,
but a human tracks predictions and convictions too. It belongs alongside `portfolio`,
`analytics`, `data`, `agent`, and `system` as a peer.

```
# Current (wrong):
pftui agent journal prediction add ...
pftui agent journal conviction set ...

# Target:
pftui journal prediction add ...
pftui journal conviction set ...
```

Move `journal` out of `agent` to top-level. Keep `agent message` under `agent`.
Update `docs/CLI-TREE.md` and `docs/CLI-MIGRATION.md`.

Source: `src/cli.rs` (move Journal enum from Agent to top-level Commands), `src/main.rs`.

### [Feedback] Weekend-Aware Movers Command

`pftui analytics movers` shows 0 movers on weekends because it compares to Friday close. Should compare Friday close to weekend crypto/futures prices (Hyperliquid, Binance perpetuals) so agents running Saturday/Sunday routines still see meaningful movements.

Source: evening-analysis feedback (Mar 15). Files: `src/commands/movers.rs`.

### [Feedback] `analytics scenario list --json`

`pftui analytics scenario list` should support `--json` output for programmatic consumption. Currently agents must cross-reference scenario names manually. Most other analytics commands already support `--json`.

Source: evening-analysis feedback (Mar 15). Files: `src/commands/scenario.rs`, `src/cli.rs`.

### ~~F42: CLI Domain Consolidation~~ ✅ Complete (shipped v0.10.0)

All 10 subtasks completed. Five-domain hierarchy finalized: `agent`, `analytics`, `data`, `portfolio`, `system`.

---

## P2 — Nice to Have

---

## P3 — Long Term

### ~~F40: CLI Hierarchy Restructure~~ ✅ Complete (shipped v0.10.0)

Full namespace restructure shipped. All legacy aliases removed. All agent routines and docs updated to canonical v0.10.0 paths.

### F36: Investor Perspectives Panel

Multi-lens macro analysis via sub-agents. 15 named legends + 10 archetypes + custom.
Full spec in git history (commit `5e34607`). Depends on F31 `--json` completeness
and OpenClaw sub-agent spawning.

### F39.7 Data Population (Sentinel, post-dev-cron)

> After dev cron ships F39.7 CLI + schema, spawn a research sub-agent to populate
> the historical database. The sub-agent should:
>
> 1. Research each determinant for each power at each decade using web_search
> 2. Score on Dalio's 1-10 scale with brief justification and source
> 3. Populate via `pftui analytics macro history add` CLI commands
> 4. Cross-reference Dalio's own charts from "Principles for Dealing with
>    the Changing World Order" as a baseline, then refine with primary sources
>
> Powers and spans:
> - US: 1900-2020 (13 decades)
> - China: 1900-2020 (13 decades)
> - Russia/USSR: 1900-2020 (13 decades, note regime transitions)
> - UK/British Empire: 1900-2020 (13 decades, the decline narrative)
> - Japan: 1900-2020 (13 decades, rise and plateau)
> - EU: 1950-2020 (8 decades, post-ECSC)
> - India: 1950-2020 (8 decades, post-independence)
> - Saudi: 1940-2020 (9 decades, post-oil discovery)
>
> Estimated: ~700 rows. Each needs a score, notes, and source.
> Break into multiple sub-agent runs by country if needed.

---

## Feedback Summary

**Latest Scores Per Tester (most recent review):**

| Tester | Latest Score | Date | Trend |
|--------|-------------|------|-------|
| Morning Market Research | 88% | Mar 7 | ↑ (recovered from 15% DB crash; last working review strong) |
| Evening Eventuality Planner | 82% | Mar 8 | ↑ (recovered from hang; stable 75-88% when working) |
| Sentinel Main (TUI) | 72% | Mar 10 | → (stable 72-88% range; Day$ P&L shipped, needs next review) |
| Market Close | 92% | Mar 6 | ↑ (peaked when data pipeline stable; movers fix shipped) |
| UX Analyst | 75% | Mar 8 | → (stable 68-78% range) |
| Integration Optimiser | 70% | Mar 11 | — (single review) |
| Medium-Timeframe Analyst | 88% | Mar 15 | ↑ (strong analytical workflow, 85% of routine handled by pftui) |
| Low-Timeframe Analyst | 80% | Mar 14 | → (solid analytics platform) |
| Morning Brief Agent | 75% | Mar 14 | — (first scored review) |
| Evening Analysis | 75% | Mar 15 | → (weekend-aware movers gap noted) |

**Score Trend Notes:**
- Agent scores have broadly stabilized in the 72-88% range since the DB crash/hang bugs were fixed (Mar 9).
- Medium-timeframe analyst hit 88% — highest new-tester score — reflecting mature analytics engine.
- Sentinel TUI plateau (72-88%) should break upward: Day$ P&L was shipped (CHANGELOG Mar 14 `ba86400`).
- Most recent feedback is from the agent pipeline testers (low/medium/evening/morning), not the original four (Morning Research, Evening Planner, Sentinel, Market Close). The original testers haven't reviewed since the Mar 14 batch of fixes.
- F42 CLI domain consolidation shipped and all agent routines updated to v0.10.0 paths.

**Top 3 Priorities Based on Feedback:**

1. **Move `journal` to top-level command** — Currently under `agent journal`. Journal is the shared knowledge layer used by humans and agents alike. In-progress P1.
2. **Weekend-aware movers** — Agents running Saturday/Sunday routines see 0 movers. Need crypto/futures comparison for weekend context. New P1 from Mar 15.
3. **`analytics scenario list --json`** — Most analytics commands support `--json` but scenarios list doesn't. Breaks agent consistency expectations. New P1 from Mar 15.

**Resolved Since Last Summary (Mar 11):**
- ✅ Data source conflict detection (shipped Mar 14)
- ✅ `predict score` positional args (shipped Mar 14)
- ✅ `correlations latest` command (shipped Mar 14)
- ✅ Today-only alert filtering (shipped Mar 14)
- ✅ Data source reliability hardening (shipped Mar 13-14)
- ✅ `trends evidence-add` help clarity (shipped Mar 13)
- ✅ `psql` connection docs (shipped Mar 13)
- ✅ Agent-msg batch mode (shipped Mar 13)
- ✅ Brief `--json` external market movers (shipped Mar 13)
- ✅ Scenario update `--notes` (shipped Mar 12)
- ✅ Predict add timeframe/confidence flags (shipped Mar 12)
- ✅ Agent-msg reply/flag workflow (shipped Mar 12)
- ✅ Scan trackline breach detection (shipped Mar 14)
- ✅ Conviction negative-score syntax (shipped Mar 12)
- ✅ F42 CLI domain consolidation — all 10 subtasks (shipped Mar 14)
- ✅ Day$ P&L in TUI positions (shipped Mar 14)

**Build Status:**
- `cargo test`: 1225 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: 1 warning (`field_reassign_with_default` in `app.rs:7258`). Minor fix needed.
- v0.10.0 tagged and released.
- GitHub stars: 0 — Homebrew Core submission not yet eligible (requires 50+).
