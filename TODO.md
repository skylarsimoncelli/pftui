# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 - Bugs/Regressions

### Yahoo fetch must never query bare crypto symbols (BTC equity-ticker collision)
**Source:** 2026-06-11 price audit + repair. The canonical `BTC` series contained 237 rows of Yahoo's EQUITY ticker "BTC" (~$28-55) because some Yahoo fetch/backfill path queries the bare symbol instead of mapping crypto symbols to their `-USD` Yahoo tickers. Rows archived + deleted; the #907 ingest guard now rejects future collisions loudly (>20% d/d uncorroborated), but the mis-mapped fetch will retry and spam rejections every refresh. Find the path (grep yahoo fetch/backfill callers for symbol pass-through), add a crypto-symbol→Yahoo-ticker mapping (BTC→BTC-USD, ETH→ETH-USD; reuse the deep_alias machinery in series_registry), and a regression test that bare "BTC" never reaches the Yahoo URL builder. Layer: L0 ingest. Consumer: price_history integrity. Surfaces: CLI refresh output only.
**Why:** symbol-collision corruption poisoned 52w ranges and long-window computations on the canonical series for months without detection until today's audit.


### CLI Perfection Program
**Source:** Operator directive 2026-06-11 ("the CLI must be 100% how you want it — actually great, empowering future agents"). Full mechanical audit (498 help nodes, 403 leaves flag-fingerprinted, ~40 JSON shapes sampled, error paths probed) scoped in **docs/CLI-DESIGN.md** — the canonical design doctrine all items below implement against. Items are dependency-ordered: C1 (vocab) before C2/C3 (paths/flags), C5 before C6 (JSON phases), C9 (doc sweep) last in-repo, C10 orchestrator-side. No new tables anywhere in the program — the "layer" for these briefs is the CLI surface contract itself; the named consumers are the agent routines (`agents/routines/`), report-prompt phases (`agents/report-prompts/`), the `/pftui-report` skill, and `scripts/`.

#### C1. `src/vocab.rs` — central enum vocabulary module
**Scope:** One module owning every cross-command vocabulary (CLI-DESIGN.md §8): Direction, Layer (+`macro-checkpoint`, `cross`), Author registry, Message category, Recommendation action, Conviction band, Tx category, Outcome, Decision response tokens, Urgency, `error.kind`. Each is a Rust enum with `Display`/`FromStr`/serde + clap `ValueEnum`. Port the two divergent `CANONICAL_LAYERS` consts (`src/commands/views_stale.rs` lowercase vs `src/report/charts/conviction_trajectory.rs` `LOW/MED/HIGH/MACRO`) and the `agent_msg.rs` category validator onto it; remaining call sites migrate opportunistically in C2/C3 but all NEW code must consume `vocab`.
**Contract/test (forever):** new `tests/vocab_conformance.rs` — greps `src/**/*.rs` (test modules excluded) for vocabulary literals in `matches!`/array-literal validation position outside `vocab.rs`; failure message points at CLI-DESIGN.md §8. This is what prevents the next decision-card-style writer/reader split.
**Files:** `src/vocab.rs` (new), `src/lib.rs`/`src/main.rs` wiring, `src/commands/agent_msg.rs`, `src/commands/views_stale.rs`, `src/report/charts/conviction_trajectory.rs`, `tests/vocab_conformance.rs` (new).
**Docs:** CLAUDE.md (author-table section gains "enums live in src/vocab.rs"), docs/CLI-DESIGN.md §8 status note.

#### C2. One canonical path per noun — kill the parallel clap surfaces
**Scope:** Per CLI-DESIGN.md §1.1: (a) delete `AnalyticsScenarioCommand` + `AnalyticsConvictionCommand` enums; `analytics scenario`/`analytics conviction` become forwards holding the SAME `JournalScenarioCommand`/`JournalConvictionCommand` type (fixes the live drift: `analytics scenario update` lacks `journal scenario update`'s `--id`); (b) replace the hand-mapped `DataAlertsRedirect` with a forward sharing `AnalyticsAlertsCommand`; (c) remove the top-level `prediction` shortcut (tree-bypass; routes exist under `journal prediction`); (d) remove the analyst-prediction subcommands grafted onto `data predictions`/`analytics predictions` (`add`, `scorecard`, `stats`, `unanswered`) — prediction-market subcommands (`markets/map/unmap/suggest-mappings/--geo`) stay, canonical under `data predictions`. Forwards print a one-line stderr deprecation note naming the canonical path.
**Contract/test (forever):** new `tests/cli_canonical_paths.rs` — for every forward listed in CLI-DESIGN.md §1.1, assert the forward's recursive `--help` flag surface is byte-identical to the canonical path's (the mechanical version of "alias must share the canonical code path"); compile-level sharing enforced by the single-enum design itself.
**Files:** `src/cli.rs` (~enum deletions + forward variants), `src/main.rs` (dispatch arms), `tests/cli_canonical_paths.rs` (new).
**Docs:** docs/CLI-TREE.md, docs/CLI-MIGRATION.md (rows for removed `prediction` + grafted subcommands), AGENTS.md (Enrichment-substrate table rows citing `analytics predictions add` / `data predictions add`; prediction-contract bullets), agents/routines/*.md + agents/report-prompts/*.md (grep for the removed paths; `analyst_routine_commands` + `doc_commands` tests gate this).

#### C3. Flag vocabulary normalization — canonical names + back-compat aliases
**Scope:** Implement the CLI-DESIGN.md §2 matrix mechanically across `src/cli.rs`: `--symbol` gains alias `--asset` on the 25 `--asset` commands (canonical name flips to `--symbol`); one shared `parse_since` behind canonical `--since` absorbing `--days` (15 cmds), `--window-days` (4), lookback-meaning `--window`/`--period`, and bare-int `--since 365`; `--author` absorbs `--analyst` (5), `--source-agent` (5), filter-`--agent` (7); fix the `--from` collision (`analytics macro regime history/summary/transitions/confidence-trend --from <date>` → `--since`, hidden alias kept); `delete`→`remove` verb fix on `analytics views`/`analytics risk-factors` (alias kept). All old spellings remain accepted clap aliases — zero breakage, one vocabulary.
**Contract/test (forever):** new `tests/flag_vocabulary.rs` — walks the help tree like `cli_help_smoke`, asserts every leaf's flags are canonical-or-known-alias against the §2 matrix (embedded as a table in the test), and that no new synonym for symbol/window/author concepts can ship.
**Files:** `src/cli.rs`, touched `src/commands/*` arg structs, `tests/flag_vocabulary.rs` (new).
**Docs:** AGENTS.md CLI tables (canonical spellings), agents/routines/low|medium|high|macro-timeframe-analyst.md + adversary-analyst.md (`--asset`→`--symbol` etc.), agents/report-prompts/phase1*/phase2d/phase4 (flag spellings), README.md examples.

#### C4. Non-TTY prompt discipline — agents never hang, stdout never polluted
**Scope:** Per CLI-DESIGN.md §5. All 4 prompt sites: `src/commands/add_tx.rs` (missing required fields prompt on stdout today; non-TTY → exit 2 usage error naming the flag + valid values from `vocab::TxCategory`), `src/commands/remove_tx.rs` (y/N confirm fires even with `--json`; gains `--confirm`, non-TTY without it → error, plan printed to stderr), `src/config.rs` first-launch wizard (fires on ANY command in a fresh env incl. `system db-info --json`; restrict to TTY + `system setup`, else defaults + one stderr note), `src/commands/setup.rs` (intentional — TTY-gate with a clear non-TTY error). Use `std::io::IsTerminal`.
**Contract/test (forever):** new `tests/non_tty_discipline.rs` — drives the binary with stdin closed in an isolated HOME: `transaction add` minus `--category` exits 2 with the flag named; `transaction remove <id>` without `--confirm` exits non-zero without deleting; fresh-env `system db-info --json` emits pure JSON on stdout. Plus a source-level guard: any new `io::stdin().read_line` outside the four blessed sites fails the test.
**Files:** `src/commands/add_tx.rs`, `src/commands/remove_tx.rs`, `src/config.rs`, `src/commands/setup.rs`, `tests/non_tty_discipline.rs` (new).
**Docs:** AGENTS.md (transaction add/remove rows: `--confirm` + non-TTY behavior), docs/CLI-DESIGN.md §5 status.

#### C5. JSON honesty phase 1 — always-JSON, error objects, no new bare arrays
**Scope:** Per CLI-DESIGN.md §4.3 phase 1. (a) `--json` ALWAYS emits valid JSON — fix `portfolio performance --json` emitting prose ("No portfolio snapshots found… Run `pftui refresh`" — also a removed path) and audit every leaf for empty-state text leaks; (b) failures under `--json` emit the `{error: {kind, message}, meta}` object on stdout (kinds from `vocab::ErrorKind`) while keeping the stderr text + exit codes; central helper in the CLI error path (`main.rs` `anyhow` boundary) so commands get it for free; (c) freeze bare arrays: existing ones keep shape, conformance test forbids NEW top-level arrays.
**Contract/test (forever):** new `tests/json_contract.rs` — for a curated read-only command list against the fixture DB (the `prior_release_schema` isolated-HOME pattern): stdout parses as JSON under `--json` in both success and provoked-failure cases; failure output carries `error.kind`; the bare-array allowlist is explicit and shrinking-only.
**Files:** `src/main.rs` (error boundary), `src/commands/performance.rs` (the prose leak at line ~128; note `src/commands/drawdown.rs` already does the right thing with an ad-hoc error object — converge it on the C5 shape), plus `src/commands/*` for other empty-state leaks found, `tests/json_contract.rs` (new).
**Docs:** AGENTS.md Best Practice 2 (error-object contract for agents), docs/CLI-DESIGN.md §4 status.

#### C6. JSON envelope phase 2 — `{data, warnings, meta}` opt-in + TTY-aware compactness
**Scope:** Per CLI-DESIGN.md §4.2/4.4. Global `--envelope` flag (+ `PFTUI_JSON_ENVELOPE=1`) wrapping any `--json` payload in `{data, warnings, meta:{command, schema_version:1, generated_at}}`; warnings currently stderr-only in JSON mode (e.g. cached-only notes) are mirrored into `warnings`. Pretty-print only when stdout is a TTY; piped output is compact single-line (~30-40% token saving, no flag). Default flip to envelope is explicitly OUT of scope (phase 3, after consumer migration — known consumers catalogued in CLI-DESIGN.md §4.3: the `/pftui-report` skill jq paths, routine jq examples, `collect-data.sh` (already broken on pre-F42 paths — fix its command list while touching it), `scripts/parity_check.sh`).
**Contract/test (forever):** extend `tests/json_contract.rs` — every sampled leaf under `--envelope` parses with exactly the three reserved keys and `meta.command` equal to the canonical path; TTY-vs-pipe compactness unit-tested at the print helper.
**Files:** `src/cli.rs` (global flag), the shared JSON print helper (new, `src/commands/output.rs` or similar), `agents/investor-panel/collect-data.sh`, `tests/json_contract.rs`.
**Docs:** AGENTS.md (envelope section + migration notice), docs/CLI-DESIGN.md §4.3 status, agents/routines/README.md.

#### C7. Zero-effect writes are errors — rows-affected discipline on ledger mutations
**Scope:** Audit finding: `journal prediction score --id 999999 --outcome correct` prints success and exits 0 (`src/db/user_predictions.rs::score_prediction` + `score_prediction_backend` ignore `rows_affected == 0`) — a silent-success lie on an L3 append-only ledger whose whole contract is scoreability. Sweep every UPDATE/DELETE-by-id CLI path (`prediction score/score-batch`, `lessons revive`, `rules cite/retire`, `alerts ack/rearm`, `agent message ack`, `thesis set-review`, `trends update`, `situation resolve`, …) for the same class: zero rows affected → exit 1 + `error.kind = "not-found"`.
**Contract/test (forever):** unit tests per fixed path (nonexistent id → Err) + one integration case in `tests/json_contract.rs`'s provoked-failure list so the class stays covered.
**Files:** `src/db/user_predictions.rs`, `src/db/*` mutation fns surfaced by the sweep, corresponding `src/commands/*`.
**Docs:** docs/CLI-DESIGN.md §3 status; CHANGELOG bug entry.

#### C8. `--json` coverage completion + exemption registry
**Scope:** Per CLI-DESIGN.md §7. Add `--json` to the 16 non-exempt leaves missing it: `portfolio history` (subsumes the standing P2 item — restore the AGENTS.md flag + Historical Comparison pattern), `portfolio target set/remove`, `portfolio watchlist add/remove`, `analytics alerts remove/rearm/seed-defaults` (`alerts add` DONE), `system mirror sync`, `system mobile enable/disable`, `system mobile token generate`. Codify the exemption registry (`console`, `system setup/demo/snapshot/web`, `system mobile serve`, `system export/import`).
**Contract/test (forever):** new assertion inside `tests/cli_help_smoke.rs` (it already walks every leaf): each leaf's help must contain `--json` OR the leaf must be in the explicit exemption list — new commands cannot ship without choosing.
**Files:** `src/cli.rs`, `src/commands/{history,watchlist_cli,alerts,…}.rs`, `tests/cli_help_smoke.rs`.
**Docs:** AGENTS.md Portfolio State table (+ Historical Comparison pattern), remove the P2 `portfolio history` item.

#### C9. Doc conformance sweep + generated CLI-TREE.md
**Scope:** After C2/C3 land: regenerate `docs/CLI-TREE.md` from the binary's help walk (commit the generator as a test-mode like the existing help walker — the doc becomes diff-checked output, not prose); rewrite `docs/CLI-MIGRATION.md` with the C2 forward/removal table; sweep AGENTS.md, README.md, agents/routines/*.md, agents/report-prompts/*.md to canonical paths/flags (the `doc_commands` + `analyst_routine_commands` tests gate literal examples; this item also fixes the two contradictory `agent message list` jq shapes — `.messages[]` is correct, `.[]` in `agents/routines/macro-timeframe-analyst.md` is broken today).
**Contract/test (forever):** new `tests/cli_tree_doc.rs` — regenerates the tree section of docs/CLI-TREE.md and fails on diff (schema-conformance pattern applied to the CLI surface doc).
**Files:** `docs/CLI-TREE.md`, `docs/CLI-MIGRATION.md`, `AGENTS.md`, `README.md`, `agents/routines/*.md`, `agents/report-prompts/*.md`, `tests/cli_tree_doc.rs` (new).
**Docs:** this IS the doc item; also update CLAUDE.md's stale "six canonical domains" list to CLI-DESIGN.md §1's nine.

#### C10. Orchestrator skill update (OUT-OF-REPO — orchestrator-executed, do not pick from cron)
**Scope:** After C2/C3 merge: re-verify the command inventory in `~/.claude/commands/pftui-report.md` against the new binary — every `pftui` invocation and jq path (`.positions[]?.symbol`, `.scored_count`, `.scored/.pending`, `.rows_inserted`) still parses/resolves; adopt canonical flag spellings; note the `--envelope` availability for future skill phases. Executed by the orchestrator on this machine, not by repo cron agents (the file lives outside the repo).

---

## P2 - Coverage And Agent Consumption

### Cycles Engine sub-tab — per-criterion ✓/✗ bottom-signal breakdown for the focused asset
The Matrix now surfaces the monthly N/7 confluence count (`src/tui/views/cycles.rs`); the natural drill-down is the focused asset's per-criterion ✓/✗ checklist (the 7 `Criterion` rows from `cycle_bottom_signals(...).criteria` + the non-counted pi-cycle bonus) rendered in the Engine sub-tab so the user sees WHICH signals are firing, not just how many. Name-free / ticker-free; keep the jargon-guard green. Surfaces: Cycles Engine.

### Cycle-signal expectancy — crypto bull-drift normalization (deeper variant)
**Source:** 2026-06-25 end-to-end eval (crack #1). The expectancy backtest now reports per-row dispersion (`stdev_return_pct`) and a baseline-σ-scaled `effect_size` next to every lift, so the seductive raw lift can't be read alone on a secular-bull asset. The deeper variant still open: a log-return / excess-over-trend expectancy mode (detrend the asset's secular drift before measuring conditional forward returns) so the lift on assets like BTC isn't structurally inflated by the bull baseline. Additive new horizon stat or a `--detrend` flag; report-only, no new table. Consumer: `analytics cycles {bottom,top}-signals backtest --expectancy`.

### Cycle-signal backtest — broaden the verified-anchor base / methodology validation
**Source:** 2026-06-25 end-to-end eval (gap #5). Verified-anchor reliability is permanently small-n: BTC/gold have only ~3 documented cycle-low anchors and cycle TOPS have zero doctrine anchors (price-structure only). Either (a) widen the doctrine-anchor set by adding more verified cycle lows per asset chapter in docs/CYCLE-THEORY.md (research-gated, cite sources), or (b) lean on the now-shipped `synthetic_known_edge_is_recovered_by_expectancy` test as the methodology-validation story and document it in CYCLE-SIGNALS.md so "trust the lift" becomes "the engine provably recovers a known planted edge." Lower code value; mostly research + docs.

### Cycles Engine sub-tab — per-criterion ✓/✗ TOP signal breakdown (mirror the bottom plan above)
**Source:** 2026-06-25 (top suite shipped). The cycle-TOP suite (`cycle_top_signals`) now exists in CLI/backtest/alerts/report but has NO TUI surface, while the bottom suite has the Matrix N/7 column in `src/tui/views/cycles.rs`. When the per-criterion bottom checklist drill-down (item above) lands, mirror it for tops so the Engine sub-tab shows both polarities' ✓/✗ rows. Name-free / jargon-guard green. Surfaces: Cycles Engine.

### Public report renderers should produce PROSE, not data tables
**Source:** 2026-06-19 operator feedback on the 2026-06-18 cycle run — the PUBLIC newsletter was "totally unacceptable": full of per-layer "Multi-Timeframe View" tables, conviction/direction dumps, data-freshness/stale rows, prediction lists, and a scenario-probability table. A public newsletter reader does not care about pftui internals; the public report must be mostly prose (~800-1500 words), explanatory and engaging. The native `pftui report build daily` PUBLIC section renderers (`src/report/sections/public_macro.rs`, `public_bitcoin.rs`, `public_gold_precious_metals.rs`, `public_equities.rs`, `public_scenario_dashboard.rs`, `public_how_we_analyse.rs`, `public_methodology.rs`) emit data tables, so today the only fix is an orchestrator-side prose rewrite (done 2026-06-19: live skill `~/.claude/commands/pftui-report.md` Step 5.3 mandate + new `agents/report-prompts/public-newsletter-writer.md`). Deeper fix: either (a) make the public renderers emit short prose paragraphs (per-theme: what moved / why / what to watch) and drop the internal-machinery sections from the public plan, or (b) add a `--prose` / newsletter mode to `report build daily` that emits a prose skeleton the writer fills. No new tables. Consumer: the `/pftui-report` public publish path + pftui.com readers. Surfaces: `pftui report build daily --mode public`.
**Why:** the public report is the only externally-published pftui artifact; a data-dump newsletter is an embarrassment to ship and undoes the value of the analytical work. Also reconcile the STALE divergent `agents/intelligence-report/SKILL.md` against the live `~/.claude/commands/pftui-report.md` (they have drifted to different structures).

### Dead agri-futures feeds: fix or retire KC=F / ZC=F / ZS=F / ZW=F
**Source:** 2026-06-12 `data audit` (PR #909). All four symbols' Yahoo fetches have returned nothing since 2026-03-13; the F52 stale-stamping bug (fixed in #907) masked it by freezing each at one close for 83 bars (rows repaired + archived 2026-06-12, journal note #735). Decide per symbol: fix the fetch (Yahoo ticker change? continuous-contract suffix?) or retire it (remove from fetch lists + watchlists; series_registry has no entries for them). Either way add them to the registry with SLAs if kept, so the freshness machinery — not an annual audit — catches the next death. Layer: L0 ingest. Consumer: commodity breadth in markets/economy views. Surfaces: TUI markets tab, data prices.
**Why:** dead feeds that keep their last print look alive to every consumer; these four were undetected for 3 months.


### `portfolio history` lacks `--json`
**SUBSUMED by P1 → CLI Perfection Program item C8 (2026-06-11) — do not pick separately; remove this entry when C8 lands.**
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

> **G8/G9 extension (operator directive 2026-06-11):** (1) "surface more of
> the underlying synthesis that goes into making the reports — all of the
> work in the background done by the agents is lost and never surfaced";
> (2) "the technical analysis overhaul needs to make its way to the TUI —
> selecting an asset should show all of the computed technicals, in a
> polished UI/UX." Design + substrate inventory: docs/TUI-GLANCE-PROGRAM.md
> §5 (G8) and §6 (G9). Same program rules apply (pure surfacing, no new
> tables, theme/privacy/loud-empty-state discipline, nothing on the event
> loop). G8.1 blocks G8.2 blocks G8.3; G9.1 blocks G9.2 blocks G9.3.

#### G8.1. SynthesisIndex substrate — load the report-pipeline corpus off the event loop
**What:** A `SynthesisIndex` struct (suggest `src/tui/synthesis.rs`) holding the most recent **14 distinct run-dates** of: `db::daily_notes::list_notes_backend` rows, `db::agent_messages::list_messages_backend` rows (grouped client-side by `created_at` date), `db::analyst_views::get_portfolio_view_matrix_backend` + `get_view_history_backend` pinned per run-date, `db::adversary_synthesis_views::list` and `db::forecast_misalignments::list_all` (both take `&rusqlite::Connection` — use `backend.sqlite()`, same note as the G1 shadow book). Plus the pure presentation classifier `parse_note_tag(&str) -> Option<NoteTag>` for the grep-verified bracket-tag vocabulary (`[synthesis-<SYM>]`, `[synthesis-economy|macro-outlook|closing|operator-wrong|deep-dive|external-ta]`, `[antithesis]`, `[debate-roundup]`, `[operator-wrong {LAYER}]`, `[operator-interview-*]`) — unknown tags classify to `Other`, never dropped (tree counts must sum to window row counts). Loaded in the G1 background-refresh thread + on demand: an older-window request channel (sibling of `background_refresh_complete_rx`) fetches the next 14-date window and appends; `/` search runs `search_notes_backend` on the same worker channel.
**Tests:** tag-parser table test over the full vocabulary incl. unknown tags; window assembly against a synthetic in-memory DB; loud empty state ("no synthesis runs recorded"); count-sum invariant.
**Surfaces:** TUI: this brief (substrate for G8.2/G8.3) · CLI: already (`journal notes list/search`, `agent message list`, `analytics views matrix`, `analytics adversary-synthesis list`, `research misalignments`) · report: already (assembler reads the same tables) · web: none (dashboard removed).
**Docs:** docs/ARCHITECTURE.md Module Index (`tui/synthesis.rs`).
**Effort:** ~2h. Depends G1 (channel/worker pattern).

#### G8.2. Synthesis Browser view — three-pane drill-in from the Intel tab
**What:** `ViewMode::Synthesis` (app.rs:34-43) — a full-screen drill-in, NOT a tenth numbered tab (digit budget spent; `0` is Analytics-scoped); header renders `Intel ▸ Synthesis`. Three panes per docs/TUI-GLANCE-PROGRAM.md §5.3 mockup: left run-date list (note+message counts, `...` sentinel triggers older-window load with skeleton row), middle phase tree in fixed pipeline order (Layers → Views → Adversary → Panel → External TA → Synthesis cards → Deep dive → Debate → Decisions → Dossiers → other), right full-text preview (metadata line: author, timestamp, section, novelty; messages show category/layer/priority/ack; adversary previews bullet the JSON evidence/trigger arrays, fragility ≥ 3 in warning color). Keys (view-scoped, collision-checked 2026-06-11): `s` from Intel opens (lowercase `s` globally unbound; `S` stays Positions-scoped); `h`/`l`/`Tab` panes; `j`/`k`/`gg`/`G`/`Ctrl+d,u` move/scroll; `Enter` expand/focus; `a` author-filter cycle; `@` asset-filter cycle (held assets); `[`/`]` prev/next run-date; `/` intercepted for note search (document the shadowing of global search in help); `Esc` back to Intel; digits pass through to tab switch. **Privacy:** structural panes render normally (counts/authors/tags are value-free); the preview pane renders `content hidden in privacy view — p to toggle` — free text is never partially masked (Transactions-tab precedent).
**Tests:** render smoke (TestBackend) for all three panes; tree-order fixture; privacy preview suppression; empty-window state; selection clamp.
**Surfaces:** TUI: this brief · CLI: already · report: already · web: none (dashboard removed).
**Docs:** docs/KEYBINDINGS.md (Synthesis table + `/` shadowing note), help.rs section, docs/ARCHITECTURE.md TUI Views list.
**Effort:** ~2.5h. Depends G8.1, G2 (Intel tab hosts the entry key).

#### G8.3. Report-claim → reasoning navigation — verdict-board deep link
**What:** The ≤5-keystroke contract (docs/TUI-GLANCE-PROGRAM.md §5.4): `s` on a selected verdict-board row opens the browser PRE-FILTERED — asset = selected symbol, run = latest run-date, middle-pane focus landed on that asset's `[synthesis-<SYM>]` card node (fallback: the asset's Views node when no synthesis card exists for the date, with a one-line notice — never silent). `j` from there walks tree order to the layer views, adversary counter-case, and dossier rows behind the same claim. Also: Enter on a G3 attention-strip PROBATION item jumps to the matching Dossiers node (layer/asset match). Path documented as a worked example in docs/KEYBINDINGS.md.
**Tests:** pre-filter landing (synthetic index: `s` with selection focuses the right node; without selection opens unfiltered); fallback-to-Views notice; probation jump.
**Surfaces:** TUI: this brief · CLI: n/a (navigation) · report: n/a · web: none (dashboard removed).
**Docs:** docs/KEYBINDINGS.md worked example; docs/TUI-GLANCE-PROGRAM.md §5.4 status note.
**Effort:** ~1h. Depends G8.2, G2, G3 (strip jump).

#### G9.1. Asset detail popup restructure — pinned verdict header + five sub-tabs
**What:** Per docs/TUI-GLANCE-PROGRAM.md §6.3 (Layout B, recommended over the single-scroll Layout A — both mocked there): `AssetDetailState` gains `tech_tab: TechTab` (Overview / Structure / Cycles / Cyber / Expectancy) + per-tab scroll. Pinned 3-line header that never scrolls: symbol/name/price/day Δ; the asset's verdict-board row rendered by the SAME function as `verdict_board.rs` (one glyph dialect — never re-implement); sub-tab bar. **Overview = the entire existing popup content unchanged** (Asset, Price, Key Levels, Chart, classic SMA/BB/RSI/MACD, Portfolio, Thesis, BTC intelligence, COT, COMEX, News) plus G6's Verdicts + Expectancy sections until G9.2 gives Expectancy its own tab — nothing the operator has today is demoted. Keys inside `handle_asset_detail_key` (app.rs:5318 — consumes all keys while open, so collision-free by construction): `h`/`l`/`Tab`/`Shift-Tab` cycle tabs, `1`-`5` direct, `j`/`k`/`gg`/`G` scroll, Esc closes.
**Tests:** tab-cycling + per-tab scroll persistence; pinned-header render at minimal heights; Overview content parity (existing build_lines tests keep passing); empty non-Overview tabs render loud placeholders pre-G9.2.
**Surfaces:** TUI: this brief · CLI: already (`analytics technicals structure/cyber`, `analytics cycles analyze`) · report: already (per-asset cards) · web: none (dashboard removed).
**Docs:** docs/ARCHITECTURE.md asset_detail_popup line map; docs/KEYBINDINGS.md popup keys.
**Effort:** ~2h. Depends G6 (absorbs its sections), G2 (shared glyph-row renderer).

#### G9.2. Engine tabs wired — full StructureRead/CycleReport/CyberSnapshot + expectancy, computed off-loop
**What:** Widen G1's `IntelSnapshot` to RETAIN per held asset the full `StructureRead` (daily + weekly), full `CycleReport` (all `DegreeStatus` degrees + btc/gold clocks), and `CyberSnapshot` (daily; weekly QB line if budget allows) — identical compute G1 already does, minus the discard. Render the four tabs per §6.3: Structure (per-timeframe verdict, swing table, BOS events in warning color, MA posture + slopes, extension % + rule-13 gate line), Cycles (composite verdict; per-degree age/expected, band p15-p85 + position + bars-to-edge, translation ledger summary + rt-string/translation-warning, FLD value/side/last-cross target + achieved %, VTL intact/BROKEN + break_confirms, failed-cycle/half-cycle/inversion/small-n flag row, clarity chip), Cyber (QB + since/bars, CyberLine, dot strength + SuperTrend stop, Pi proximity mini-gauges via the rsi_gauge pattern, MTF RSI zone + gating, dated SignalEvent list), Expectancy (fired signals joined to `db::signal_expectancy::latest_rows` at matching `(signal_id, signal_version)`: n_nonoverlap, mean/hit lift vs baseline, MAE mean/worst, significance; unmeasured → "unmeasured — run `pftui research backtest`"). **Non-held symbols:** popup-open for a symbol absent from the snapshot pushes a compute request on the worker channel; tabs render skeleton loaders until mpsc delivery; LRU cache (~16) on `App` keyed `(symbol, history_len)`. Short series → loud per-tab empty state naming the bar requirement (Cyber MIN_BARS=60). Render NEVER calls `analyze`.
**Tests:** section assembly from synthetic engine outputs (incl. broken-VTL/failed-cycle warning rendering, rule-13 gate line both states); version-mismatch expectancy row excluded; on-demand request → skeleton → delivery flow; LRU invalidation on history change; short-series states.
**Surfaces:** TUI: this brief · CLI: already · report: already · web: none (dashboard removed).
**Docs:** docs/ARCHITECTURE.md (intel snapshot note + popup map), docs/TUI-GLANCE-PROGRAM.md §6 status note.
**Effort:** ~2.5h. Depends G9.1, G1.

#### G9.3. STRETCH — price-chart overlay of swings + broken levels
**What:** Explicitly separated, not bundled (rationale in docs/TUI-GLANCE-PROGRAM.md §6.6): overlay `StructureRead.swings` markers (HH/HL/LH/LL glyphs at swing dates) and broken-level horizontal lines (`BreakEvent.level`, dashed, warning color) on the popup chart, behind a popup-scoped toggle key (`o` for overlay — popup-scoped, collision-free). Reuses the SMA/BB overlay path in `price_chart.rs:540-870`; the open problem is glyph placement on the braille grid (markers render in a gutter row above/below the braille area, never inside it — keep the braille renderer untouched). Daily timeframe only in v1. Do NOT start until G9.1+G9.2 are merged and the data plumbing is proven.
**Tests:** marker gutter placement math; level-line clipping at chart bounds; toggle state; render smoke with empty swings.
**Surfaces:** TUI: this brief · CLI: n/a (visual) · report: charts have their own renderers — out of scope · web: none (dashboard removed).
**Docs:** docs/KEYBINDINGS.md popup `o` toggle; docs/ARCHITECTURE.md price_chart note.
**Effort:** ~2h. Depends G9.2. P3-grade priority — pick only when the rest of the program is done.

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

### Web Dashboard Removal
**Source:** operator decision 2026-06-11 — the web dashboard (`pftui system web`, `src/web/`) is explicitly abandoned and deleted. Full inventory + implementing checklist: **docs/WEB-DASHBOARD-REMOVAL.md** — read it before picking any item below. Hard boundaries: `website/` (pftui.com reports site) is UNTOUCHED; `src/mobile/` + `mobile/` (native iOS/macOS API, actively deployed via `pftui-mobile.service`) are KEPT — the only coupling is `src/web/view_model.rs`, which gets relocated, not deleted. Items are dependency-ordered; do 1 before 2-3; 4 last.

- [ ] **1. Code + test-harness deletion.** Relocate `src/web/view_model.rs` → `src/analytics/view_model.rs` (update imports in `src/mobile/server.rs:32`, `src/analytics/situation.rs:15`; its tests move with it). Then delete: `src/web/` (mod/server/api/auth + `static/index.html`); `mod web;` in main.rs:19; the `SystemCommand::Web` mirror-sync guard arm (main.rs:1237-1241) and dispatch (main.rs:1715-1731) — KEEP the Mobile arms; `SystemCommand::Web` variant (cli.rs:2115-2128); Cargo.toml deps `tokio-stream` (web-only) + `tokio-util` (already unused) — axum/tower/tower-http stay for mobile (trim features if the compiler proves them web-only). Reword config.rs:387 comment "TUI/Web" → "TUI". Delete the Playwright harness: `tests/web.{integration,visual}.spec.ts`, `tests/web.mocks.ts`, `playwright.config.ts`, root `package.json` + `package-lock.json`; ci.yml `web-tests` job; release.yml web-parity gate + Playwright steps (KEEP `mobile-ios` job); `scripts/check_web_parity_checklist.sh`. Side benefit to note in CHANGELOG: the 6 flaky `web::api::tests::*` (SQLite shared-memory contention under parallelism) leave the suite. Tests: full `cargo test` green, `cli_help_smoke` green (auto-adapts), clippy clean. Rollback: single focused commit, `git revert` — no data touched.
- [ ] **2. Docs sweep.** Delete: `WEB_DASHBOARD.md`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_CHECKLIST.md`, `docs/WEB_PARITY_MATRIX.md`, `docs/WEB_REBUILD_CHECKLIST.md`, `docs/WEB_STABLE_ROLLOUT.md`. Edit (exact line refs in the inventory doc §3.2): README.md (dashboard section ~72-90, gallery cell, tech-stack bullets, docs-table row — README needs maintainer-approval callout in the PR), AGENTS.md (`system web` row ~426, "TUI, Web, CLI" ~437), ONBOARDING.md (Step 5 ~261-373 + TOC/table/checklist mentions, renumber steps), docs/ARCHITECTURE.md (~151 web-API bullet), docs/DATA-ARCHITECTURE.md (~58 "TUI/web" sink), PRODUCT-PHILOSOPHY.md (~17/82/122), PRODUCT-VISION.md (~17/56/74/82), CLAUDE.md (docs index row + "Three interfaces"), docs/DAEMON.md (line 3 "web UI"), docs/AI-LAYER.md (~104 server-mode bullet), docs/MOBILE-WEBAPP-DESIGN.md (KEEP, prepend webapp-surface-removed note). CHANGELOG/git history untouched. Tests: `cargo test --test doc_commands --test analyst_routine_commands`. Rollback: git revert (docs only).
- [ ] **3. Data-layer/catalog cleanup.** Finding from scoping: NO dashboard-only tables exist; nothing becomes DEAD; the archive-then-drop pattern is NOT triggered (web API wrote only shared tables; auth token never persisted). Remaining hygiene: `docs/db-catalog.toml` — remove `"src/web/api.rs"` from `[tables.journal]` writers (~271) and watchlist writers (~782); fix the stale `[tables.mobile_timeframe_scores]` readers/writers (actual: writers `analytics/synthesis.rs` + `commands/situation.rs`, reader `analytics/situation.rs`) and the matching DATA-ARCHITECTURE.md:209 note. Tests: `cargo test --test schema_conformance`. Rollback: git revert; zero data movement.
- [ ] **4. Final verification.** `cargo build --release` with before/after binary size (nice-to-have: expect ≥113 KB from embedded index.html alone); full `cargo test` green with `web::api` absent; `cargo clippy --all-targets` clean; schema conformance + `pftui system schema verify`; grep-zero for `system web|web dashboard|WEB_DASHBOARD|src/web` outside CHANGELOG*/git history/`website/` (mobile, `data dashboard`, `web_search` hits don't count); `pftui system mobile status` smoke check proves mobile untouched; CI + release workflows parse without the deleted jobs/steps. Delete docs/WEB-DASHBOARD-REMOVAL.md and this TODO block when all green; CHANGELOG entry includes the operator note `systemctl disable --now pftui-web` for any host that copied the old systemd example. Rollback: git revert of the prior commits; no data loss possible (no tables dropped).

## P3 - Long Term

---
