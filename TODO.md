# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P3 — Long Term

### F39.7b: Historical Power Metrics Data Population (Sentinel)

> After dev cron ships F39.7 CLI + schema, spawn a research sub-agent to populate
> the historical database. The sub-agent should:
>
> 1. Research each determinant for each power at each decade using web_search
> 2. Score on Dalio's 1-10 scale with brief justification and source
> 3. Populate via `pftui analytics macro cycles history add` CLI commands
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

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Morning Market Research | 88% | 82% | Mar 7 | ↑ (Mar 8-9 crash/hang since fixed) |
| Evening Eventuality Planner | 55% | 62% | Mar 17 | ↓ (missing `analytics scenario update`, nonexistent subcommands) |
| Sentinel Main TUI Review | 75% | 72% | Mar 10 | ↓ (display corruption, missing day P&L $) |
| Market Close | 60% | 72% | Mar 9 | ↕ (movers bug + TIMESTAMPTZ crash, both fixed) |

**Notes:**
- Morning Research Mar 7 score (88/82) represents post-fix trajectory after Mar 8-9 crashes were resolved.
- Evening Planner dropped on Mar 17: tried `analytics scenario update` (doesn't exist — command lives at `journal scenario update`), and guessed nonexistent `data prices`/`portfolio snapshot` subcommands. Root cause is namespace discoverability, not missing functionality.
- Mar 16 run added `analytics scenario list --json`, `analytics conviction set`, `analytics macro regime set` aliases — but `analytics scenario update` alias was NOT added. This is the specific gap.
- Sentinel has requested day P&L in dollars in *every single review since Mar 2* — still the most consistently requested feature.
- Agent feedback (Mar 12-17) is predominantly P2 enhancement requests, not regressions.

**Top 3 priorities based on feedback:**

1. **`analytics scenario update` alias** — Evening Planner hit this on Mar 17. The command exists at `journal scenario update` but `analytics scenario` only has `list`. Add `update` (and other CRUD) as analytics aliases to match the list alias that was already added.
2. **TUI day P&L $ column** — Sentinel requests this in every review. Most consistently requested feature across all testers since Mar 2.
3. **Keep release quality green** — `cargo clippy --all-targets -- -D warnings` and test suite should stay clean.

**Release status:** v0.12.1 shipped Mar 16. Only P3 items remain in backlog. Build green: `cargo test` (1297 tests), `cargo clippy --all-targets -- -D warnings` clean.

**GitHub stars:** 1 — Homebrew Core requires 50+.
