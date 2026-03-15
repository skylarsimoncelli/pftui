# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Feature Requests

### [Feedback] Weekend-Aware Movers Command

`pftui analytics movers` shows 0 movers on weekends because it compares to Friday close. Should compare Friday close to weekend crypto/futures prices (Hyperliquid, Binance perpetuals) so agents running Saturday/Sunday routines still see meaningful movements.

Source: evening-analysis feedback (Mar 15). Files: `src/commands/movers.rs`.

### [Feedback] `analytics scenario list --json`

`pftui analytics scenario list` should support `--json` output for programmatic consumption. Currently agents must cross-reference scenario names manually. Most other analytics commands already support `--json`.

Source: evening-analysis feedback (Mar 15). Files: `src/commands/scenario.rs`, `src/cli.rs`.

---

## P2 — Nice to Have

---

## P3 — Long Term

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
