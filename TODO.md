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

**Latest scores per tester (most recent review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Morning Market Research | 88% | 82% | Mar 7 | ↑ (Mar 8-9 crash/hang since fixed) |
| Evening Eventuality Planner | 55% | 68% | Mar 16 | ↓ (missing conviction/regime CLI paths) |
| Sentinel Main TUI Review | 75% | 72% | Mar 10 | ↓ (display corruption noted) |

**Notes:** Morning Research hit 0/15 on Mar 8 (DB crash) and 15/30 on Mar 9 (API hang) — both root causes fixed in v0.7.0+. The Mar 7 score of 88/82 reflects post-fix trajectory. Sentinel dropped from 85/88 (Mar 7) to 75/72 (Mar 10) citing TUI display corruption and missing day P&L dollar column.

**Top 3 priorities based on feedback:**

1. **TUI display reliability + day P&L $ column** — Sentinel has requested daily P&L in dollars in every single review since Mar 2. This is the most consistently requested feature across all testers.
2. **Historical macro cycles CLI + data population** — The remaining open work is now concentrated in the long-cycle analytics path under P3.
3. **Keep release quality green** — `cargo clippy --all-targets -- -D warnings` and the feature-feedback regression tests should stay clean before the next release.

**Release status:** Remaining open work is now P3 only. Current branch validation passes with `cargo test` (1283 tests) and `cargo clippy --all-targets -- -D warnings`.

**Homebrew Core:** 0 stars — not eligible (requires 50+).
