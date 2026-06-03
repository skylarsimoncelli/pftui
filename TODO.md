# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P2 - Coverage And Agent Consumption


### `pftui report build daily` — umbrella tracker (do not pick directly)
**Source:** Skylar (May 28). Depends on both `pftui report` scaffold and the chart-helper-port items above.
**Why:** Once chart rendering is native, the next layer up is the report ASSEMBLY — pulling data, ordering sections, inlining charts, and writing the markdown that feeds the PDF renderer. Today that work lives in the Claude `/pftui-report` skill orchestration plus the ad-hoc Python build script generated per run. Making it a native `pftui report build daily` command means: (1) anyone (not just Claude) can build a daily report from a populated DB; (2) the assembly logic gets `cargo test` coverage; (3) the Claude skill becomes much thinner — it spawns analysts, then calls `pftui report build daily`, then PRs the output. (4) Removes the Python build script entirely from the steady-state pipeline.
**Implementation plan:** Section renderers, dry-run, and the `BuildContext::load` per-source data loaders are landed (loaders wired 2026-06-03 — the assembler previously rendered empty because `load` was a stub). Remaining work is the deferred-slot + data-quality follow-ups below, then the skill migration.
**Effort:** Core assembly complete; follow-ups below.

### Report assembler — deferred `BuildContext` slots (no backend yet)
**Source:** Surfaced during the 2026-06-03 `/pftui-report` validation run. The loaders landed but these slots still render their empty-state markers because no clean backend exists yet:
- `precious_metals_supply` (COMEX/COT positioning rows — backend exists but needs per-asset metric/interpretation mapping)
- `equity_breadth` + `equity_earnings` (advance/decline + earnings-revision aggregates are not materialized)
- `public_news_silence` / `private_news_silence` (`news_silence::list_baselines` doesn't expose the median/observed counts the `NewsVolumeSignal` renderer needs)
- `bitcoin_etf_flows`, `bitcoin_onchain`, `sovereign_gold_holdings`, `macro_news_volume`
- deeper private analytic slots: conviction trajectories, outlooks-by-horizon, risk-factor mappings, drift rows, private macro quadrant/scenarios/divergences/catalysts
**Why:** Each fills a currently-empty sub-block in an otherwise-substantive section. None block report generation. Wire incrementally, reusing existing `db::` query functions; never fabricate.

### Report — economy-indicator data quality (`economic_data` scrape errors)
**Source:** 2026-06-03 validation run. The Brave-extracted `economic_data` rows surface implausible values in the public Macro table: `nfp = 2024` (looks like a scraped year, not a payroll figure) and `ppi = 14`. The loader faithfully renders the table; the bug is upstream in the economy-indicator extraction.
**Why:** These values are publicly rendered in the daily newsletter. Either fix the extraction (preferred) or add a sanity-range filter that drops/flags out-of-band indicator values before they reach the report.

### Report — re-enable `report_build_daily_perf` test
**Source:** 2026-06-03. The perf test (`tests/report_build_daily_perf.rs`) is still `#[ignore]`d with the stale message "report build daily CLI not yet wired". The assembler is now wired with real loaders. Re-enable it and confirm the real loaders meet the <2s budget against `tests/fixtures/db/v0.27.0.sqlite` (raise the budget with justification only if a loader legitimately needs it).

### Migrate `/pftui-report` Claude skill — VALIDATION PENDING
**Source:** Skylar (May 28). Rewrite landed in this session (2026-06-03). `~/.claude/commands/pftui-report.md` shrunk from 1430 → 1025 lines: Step 4 now does only targeted web research; Step 5 is a single `pftui report build daily` invocation; the giant in-skill section template was retired. Privacy audit (Step 6), PDF render (Step 7a/b), website registry (Step 8), and PR/auto-merge (Step 9) unchanged. Assembler dry-run verified against the live DB: 11 public + 11 private sections fire in canonical order. `~/pftui-operator/charts.py` carries a DEPRECATED header (2026-06-03) noting the skill no longer imports it.
**Validation progress (2026-06-03):** First end-to-end `/pftui-report --mode both` run completed. It surfaced that `pftui report build daily` produced an entirely empty report — `BuildContext::load` was a documented stub wiring only 3 of ~30 data slots (the TODO above had incorrectly recorded the assembler as "landed"). Loaders were implemented + validated this run (see CHANGELOG 2026-06-03 and the deferred-slot follow-ups above); the public report now renders ~22KB of substantive, accurate content.
**Remaining validation:** Run `/pftui-report --mode both` once more (now that the loaders are merged) and diff the markdown + PDFs against the prior Python-orchestrated outputs (allow byte-level whitespace/ordering diffs; flag content discrepancies as TODOs against the assembler, not the skill). Once validated, drop this entry and delete `~/pftui-operator/charts.py`.

---

## P3 - Long Term

---
