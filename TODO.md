# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 - Bugs/Regressions

### Extend `--falsify` + confidence caps to the `data/analytics predictions add` alias
**Source:** 2026-06-10 epistemics R2. `journal prediction add` now enforces falsifiability discipline (`--falsify` rule parsing, 0.3 unfalsifiable cap, calibration-derived confidence clamp), but the convenience alias `pftui data predictions add` / `analytics predictions add` routes through the legacy `predict::run("add")` path with none of those mechanisms, so agents can bypass the discipline. Route the alias through `run_add_with_preflight` (or port the cap + falsify logic) and add the `--falsify`/`--override-confidence-cap`/`--cap-rationale` flags there too.
**Why:** A learning loop that can be silently bypassed by using the alias is not binding.

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

### Report — re-enable `report_build_daily_perf` test
**Source:** 2026-06-03. The perf test (`tests/report_build_daily_perf.rs`) is still `#[ignore]`d with the stale message "report build daily CLI not yet wired". The assembler is now wired with real loaders. Re-enable it and confirm the real loaders meet the <2s budget against `tests/fixtures/db/v0.27.0.sqlite` (raise the budget with justification only if a loader legitimately needs it).

### Migrate `/pftui-report` Claude skill — VALIDATION PENDING
**Source:** Skylar (May 28). Rewrite landed in this session (2026-06-03). `~/.claude/commands/pftui-report.md` shrunk from 1430 → 1025 lines: Step 4 now does only targeted web research; Step 5 is a single `pftui report build daily` invocation; the giant in-skill section template was retired. Privacy audit (Step 6), PDF render (Step 7a/b), website registry (Step 8), and PR/auto-merge (Step 9) unchanged. Assembler dry-run verified against the live DB: 11 public + 11 private sections fire in canonical order. `~/pftui-operator/charts.py` carries a DEPRECATED header (2026-06-03) noting the skill no longer imports it.
**Validation progress (2026-06-03):** First end-to-end `/pftui-report --mode both` run completed. It surfaced that `pftui report build daily` produced an entirely empty report — `BuildContext::load` was a documented stub wiring only 3 of ~30 data slots (the TODO above had incorrectly recorded the assembler as "landed"). Loaders were implemented + validated this run (see CHANGELOG 2026-06-03 and the deferred-slot follow-ups above); the public report now renders ~22KB of substantive, accurate content.
**Remaining validation:** Run `/pftui-report --mode both` once more (now that the loaders are merged) and diff the markdown + PDFs against the prior Python-orchestrated outputs (allow byte-level whitespace/ordering diffs; flag content discrepancies as TODOs against the assembler, not the skill). Once validated, drop this entry and delete `~/pftui-operator/charts.py`.

---

### Cycle-clock analytics command (`pftui analytics btc cycle-clock`)
**Source:** Operator directive 2026-06-09 (integrate Loukas/Camel-Finance/Olson/Cowen cycle frameworks; see thesis `cycle-frameworks` + journal note #691). Emit, with `--json`: days/weeks since the 2024-04-19 halving; Olson day-900 counter and days remaining; Loukas week-of-4yr-cycle vs the wk-46 ±10% low band (cycle anchor = prior cycle low); midterm-year H2 flag; current price vs 200W MA (computed from the deep `BTC-USD` series) and Mayer Multiple. One command the analysts and the report can cite instead of re-deriving cycle math each run.

### MVRV Z-Score data source + cache
**Source:** Same directive. Camel Finance's primary bottoming indicator. Needs an on-chain source (free tier: bitcoin-data.com / coinmetrics community / blockchain.com charts API) cached into a new `onchain_mvrv` table via `data refresh`; surface in the BTC per-asset card and `cycle-clock`. Respect the no-new-deps rule — plain reqwest JSON fetch.

### BTC dominance series
**Source:** Same directive. Cowen's rotation lens. CoinGecko `/global` returns market-cap dominance (CoinGecko currently 403s — needs key or alternate source, e.g. coinpaprika `/global`). Cache history daily; surface alongside cycle-clock output.

### Parallels engine: calendar/time-since-event predicates
**Source:** Same directive. `pftui-parallels-run` predicates today are price/MA/RSI/F&G only. Add `days_since_date` (e.g. halving day-count band 850-950) and `month_of_year` / `is_midterm_h2` predicate fns so Loukas/Olson timing-band condition sets can join the catalog (`~/.config/pftui/parallels.yml`), and add the two sets.

### 200W MA in technical snapshots + per-asset report card
**Source:** Same directive. The 200-week MA (1400d window, deep `BTC-USD` series) anchors three of the four external frameworks. Add to `technical_snapshots` and the BTC Key-levels block; guard against short price series (emit null, never a 365-row "200W" MA).

### Report leak-guard over-scrubs legitimate market figures
**Source:** 2026-06-09 run. The render-time scrubber stripped leading dollar-magnitudes from MARKET facts in assembled markdown: "JPM $5,055" → "JPM ,055", "$3.5T" → ".5T", "$965B" → "~B", "BMO $220" → "BMO ". The guard should scrub only operator-portfolio-scale values (or values matching actual portfolio rows), not sell-side price targets / IPO valuations. Fixed by hand at composition this run; make the scrubber context-aware + add tests with market-figure fixtures.

## P3 - Long Term

---
