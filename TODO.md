# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 - Bugs/Regressions

---

## P2 - Coverage And Agent Consumption

### `portfolio history` lacks `--json`
**Source:** R6 docs sweep (2026-06-11). `pftui portfolio history --date YYYY-MM-DD` is text-only, violating the "--json on every CLI command" rule; AGENTS.md previously documented a `--json` flag that does not exist (now corrected). Add a `--json` output mirroring `portfolio summary`'s shape, then restore the flag in AGENTS.md's Portfolio State table and the Historical Comparison integration pattern.


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

### TUI Glance-Value Program

> Source: operator directive 2026-06-11 ("if it was more valuable to glance at
> then I would glance at it more"). Design doc + full surfacing inventory:
> `docs/TUI-GLANCE-PROGRAM.md`. **Pure surfacing — NO new analytics, NO new
> tables, NO alternative classifications.** Every widget: stateless render fn
> `(&mut Frame, Rect, &App)`, colors from `app.theme` (all 11 themes),
> privacy-mode support, loud empty states, data from cached tables /
> deterministic engines over `price_history` only — never network or blocking
> I/O in the event loop. Items are dependency-ordered; G1 blocks G2-G5; G6 is
> independent after G1; G7 closes the program.

#### G1. IntelSnapshot substrate — load the cockpit's data off the event loop
**What:** A new `IntelSnapshot` struct (suggest `src/tui/intel.rs`) holding, for each held position (+ core watchlist symbols with history): daily+weekly `market_structure::analyze` `StructureRead`, `analytics::cyber::analyze` `CyberSnapshot`, `cycle_engine::analyze` band position (`DegreeStatus.band_position`, `bars_to_band_start/_end`) + `cycle_clock` for BTC/gold; plus portfolio-wide: `db::analyst_views::get_portfolio_view_matrix_backend` + per-asset `ConvergenceReport` (reuse the convergence-all path — `classify_convergence` is the only classifier), `db::forecast_misalignments::active_misalignments`, `db::run_health::get_latest_run_health` + `threshold_flags` + `compute_forecast_hit_rate(_, 90)`, `db::series_registry::status_all` (keep only rows past 2× SLA), `db::scenarios::list_scenarios(conn, Some("active"))` + `compute_normalized_set`, `research::shadow_book::compute` (note: takes `&rusqlite::Connection` — use `backend.sqlite()`), and recommendation scoreboard rows (`db::recommendations::rolling_hit_rate`/`accuracy_summary`, mirroring `analytics recommendations scoreboard`).
**Where:** Compute in the existing background-refresh thread (app.rs ~1137 `std::thread::spawn` block) and once at `init()` after `load_data()`, delivered via the existing mpsc-completion pattern (extend the channel payload or add a sibling `intel_rx`); store as `Option<IntelSnapshot>` + `intel_computed_at` on `App` (struct fields app.rs:286-492). Engines are pure CPU over cached `price_history` — cap to held assets to bound cost. Every sub-load is individually fallible: a failed loader yields a per-section empty-state string, never a panic and never a silently-absent panel (EPISTEMICS loud-degradation doctrine).
**Tests:** Unit-test snapshot assembly against a synthetic in-memory DB (demo data only); assert empty-DB produces loud empty states, not None-everywhere.
**Docs:** docs/ARCHITECTURE.md Module Index (new `tui/intel.rs` entry).
**Effort:** ~2h.

#### G2. `[9] Intel` tab + Verdict Board widget
**What:** New `ViewMode::Intel` (app.rs:34-43), key `9` (globally unbound — verified 2026-06-11), header tab `[9]`/`Intel` (`widgets/header.rs` push_tab, compact label `I`), wired in `ui.rs`. Core widget `tui/widgets/verdict_board.rs`: one row per held asset —
```
ASSET   STRUCT D/W   CYCLE BAND        CYBER           CONV           FLAGS
BTC     ▲HH·HL ▲HH   in-band 62% 18wk  QB:bull ●3 Pi·far  ++3.2 conv-bull
GC=F    ▲HH·HL ─rng  pre-band -22d     QB:bull ●1 ·       +1.8 cv-neutral ⚠P
```
Columns from G1's `IntelSnapshot`: structure glyph (▲/▼/─ from `StructureClass`, daily + weekly), cycle band position + bars-to-band, Cyber QB state + strength-dot count + Pi-cycle proximity (`CyberSnapshot.dots/pi_cycle`), convergence signed avg conviction + abbreviated summary, `⚠P` when `active_probation_map` hits the (any-layer, asset) pair. j/k row selection; Enter opens the existing `asset_detail_popup` for the symbol; gg/G jump. Theme: map bull/bear/neutral to existing gain/loss/muted slots — no new theme slots needed; if any are added, update ALL 11 themes. Privacy-safe by construction (no dollar values). Full ASCII mockup in docs/TUI-GLANCE-PROGRAM.md §4.
**Keys/help:** `9` in `handle_key` view-switch cluster (app.rs ~2942); help.rs Views section row; docs/KEYBINDINGS.md Views table.
**Tests:** Render-smoke via TestBackend (existing pattern in view tests); selection clamp; empty-snapshot loud state.
**Docs:** docs/KEYBINDINGS.md, docs/ARCHITECTURE.md (TUI Views list + Quick Reference add-view row stays accurate), README feature list/screenshot mention (maintainer approval required for README per CLAUDE.md — flag in PR, don't self-merge the README hunk).
**Effort:** ~2h. Depends G1.

#### G3. Attention strip + epistemics strip (Intel tab, top)
**What:** Two one-to-two-line strips above the Verdict Board, visual pattern of `widgets/regime_bar.rs`. (a) **Attention**: active misalignments as `PROBATION: <layer>/<asset> (N-streak)` (from `active_misalignments`), series past 2× SLA as `STALE: <series> <age>` (from G1's filtered `status_all`), suppressed entirely when clean (`✓ no active alerts`). (b) **Epistemics**: latest `run_health` row — date, agreement_rate, blind_divergence, panel_dispersion, 90d forecast hit rate — with `threshold_flags` rendered in the theme warning color (echo risk > 0.85 etc.); empty table renders "no run recorded — epistemics never written on this machine" (the census found 0 rows; this widget makes that fiction visible, which is the point).
**Tests:** flag-rendering thresholds (reuse `threshold_flags` — do not reimplement), clean-state suppression, empty-state line.
**Docs:** ARCHITECTURE.md widgets list.
**Effort:** ~1.5h. Depends G1, G2 (layout slot).

#### G4. Ledger panel — shadow book 3-NAV + recommendation window-quality (Intel tab, bottom-left)
**What:** `tui/widgets/ledger_panel.rs`. Top: SHADOW / ACTUAL / HOLD NAVs from `ShadowBookReport.nav_points` rendered **indexed to 100** (privacy-safe in both modes — never dollar NAVs) with a mini braille sparkline per book (reuse `price_chart::render_braille_lines` / `build_sparkline_spans` pattern from markets.rs) + the three terminal index values. Bottom: per-symbol `ADD−WAIT 90d` window-quality deltas from the scoreboard rows (G1), colored gain/loss. Empty states: "<90d accrued — shadow book still maturing" / "no scored recommendations yet".
**Tests:** indexing math (Decimal), empty-state strings, render smoke.
**Docs:** ARCHITECTURE.md widgets list.
**Effort:** ~1.5h. Depends G1, G2.

#### G5. Scenario board (Intel tab, bottom-right)
**What:** `tui/widgets/scenario_board.rs`: active scenarios (name, probability, base rate, deviation glyph ▲/▼/· with the pp delta, days since last `scenario_updates` move) from G1's scenario rows + `NormalizedScenarioSet`; one line for modeled-sum/overfill state (`classify_overfill`). A scenario priced far from base rate renders the deviation in warning color — the exaggeration flag from EPISTEMICS §3, surfaced ambiently. Distinguish clearly from the Analytics-tab what-if presets (this is the journal scenario LEDGER); title "Scenario Ledger".
**Tests:** deviation glyph thresholds, overfill line, render smoke.
**Docs:** ARCHITECTURE.md widgets list.
**Effort:** ~1h. Depends G1, G2.

#### G6. Asset detail popup — engine verdicts + measured signal expectancy
**What:** Two new sections in `asset_detail_popup.rs::build_lines` (after "Technicals", same `section_header` pattern): (a) **Verdicts** — the symbol's structure D/W verdict strings (`StructureRead.verdict`), cycle band line, Cyber composite verdict (`CyberSnapshot.verdict`), from G1's snapshot (no recompute in render). (b) **Signal Expectancy** — recent dated `SignalEvent`s for the symbol (from `CyberSnapshot.signals` + structure/cycle events already in the snapshot) joined against `db::signal_expectancy::latest_rows` to show measured lift: `cyber_qb_flip_bull · fired 06-08 · 90d: +6.2% vs +2.1% base (n=14)`. If the expectancy cache is empty for the signal: "unmeasured — run `pftui research backtest`". Mirrors the report per-asset card's "Signal expectancy" line; cite stats only at matching `(signal_id, signal_version)` — never against a changed definition.
**Tests:** section assembly with synthetic snapshot + expectancy rows; version-mismatch row excluded; empty-cache line.
**Docs:** ARCHITECTURE.md asset_detail_popup line map note.
**Effort:** ~1.5h. Depends G1 only (independent of G2-G5).

#### G7. TUI currency rule — SURFACES.md + capability-brief "Surfaces:" contract
**What:** Close the loop that created this gap: a week of substrate (views, ledgers, engines, epistemics) shipped CLI/report surfaces deliberately and TUI absence by omission. Mechanism, three parts: (1) `docs/SURFACES.md` — a capability × surface matrix (report / CLI / TUI / web), one row per operator-meaningful capability, each cell `yes` / `planned (TODO ref)` / `no — <reason>`; seed it from docs/TUI-GLANCE-PROGRAM.md §2. (2) Amend DATA-ARCHITECTURE.md rule 6: capability briefs must carry a **Surfaces:** line giving an explicit verdict per surface — "TUI: none (agent-only)" is valid, silence is not — and add the same sentence to CLAUDE.md's TODO-item guidance. (3) Light enforcement: extend `docs/db-catalog.toml` with an optional `surfaces = ["cli", "report"]` key and have `tests/schema_conformance.rs` require it on **L3 ledger** tables only (the operator-meaningful layer) — a new ledger without a declared surface verdict fails CI, pointing at SURFACES.md. Keep it a declaration check, not a grep-the-renderer check.
**Tests:** the schema_conformance extension (+ backfill `surfaces` keys for existing L3 entries in the same commit).
**Docs:** SURFACES.md (new), DATA-ARCHITECTURE.md, CLAUDE.md, ARCHITECTURE.md doc index if needed.
**Effort:** ~1.5h. Independent; do last so the matrix can cite G2-G6 outcomes.

### MVRV Z-Score data source + cache
**Source:** Operator directive 2026-06-09 (cycle frameworks). Camel Finance's primary bottoming indicator. Needs an on-chain source (free tier: bitcoin-data.com / coinmetrics community / blockchain.com charts API) cached into a new `onchain_mvrv` table via `data refresh`; surface in the BTC per-asset card and `analytics cycles clock`. Respect the no-new-deps rule — plain reqwest JSON fetch.

### BTC dominance series
**Source:** Same directive. Cowen's rotation lens. CoinGecko `/global` returns market-cap dominance (CoinGecko currently 403s — needs key or alternate source, e.g. coinpaprika `/global`). Cache history daily; surface alongside `analytics cycles clock` output.

### Parallels engine: calendar/time-since-event predicates
**Source:** Same directive. `pftui-parallels-run` predicates today are price/MA/RSI/F&G only. Add `days_since_date` (e.g. halving day-count band 850-950) and `month_of_year` / `is_midterm_h2` predicate fns so Loukas/Olson timing-band condition sets can join the catalog (`~/.config/pftui/parallels.yml`), and add the two sets.

### 200W MA in technical snapshots + per-asset report card
**Source:** Same directive. The 200-week MA (1400d window, deep `BTC-USD` series) anchors three of the four external frameworks. Add to `technical_snapshots` and the BTC Key-levels block; guard against short price series (emit null, never a 365-row "200W" MA).

### Report leak-guard over-scrubs legitimate market figures
**Source:** 2026-06-09 run. The render-time scrubber stripped leading dollar-magnitudes from MARKET facts in assembled markdown: "JPM $5,055" → "JPM ,055", "$3.5T" → ".5T", "$965B" → "~B", "BMO $220" → "BMO ". The guard should scrub only operator-portfolio-scale values (or values matching actual portfolio rows), not sell-side price targets / IPO valuations. Fixed by hand at composition this run; make the scrubber context-aware + add tests with market-figure fixtures.

## P3 - Long Term

---
