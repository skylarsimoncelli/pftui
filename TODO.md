# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P2 - Coverage And Agent Consumption

### `pftui report build daily` — umbrella tracker (do not pick directly)
**Source:** Skylar (May 28). Depends on both `pftui report` scaffold and the chart-helper-port items above.
**Why:** Once chart rendering is native, the next layer up is the report ASSEMBLY — pulling data, ordering sections, inlining charts, and writing the markdown that feeds the PDF renderer. Today that work lives in the Claude `/pftui-report` skill orchestration plus the ad-hoc Python build script generated per run. Making it a native `pftui report build daily` command means: (1) anyone (not just Claude) can build a daily report from a populated DB; (2) the assembly logic gets `cargo test` coverage; (3) the Claude skill becomes much thinner — it spawns analysts, then calls `pftui report build daily`, then PRs the output. (4) Removes the Python build script entirely from the steady-state pipeline. This does NOT yet replace `gen-report.py` (markdown → PDF) — that is a separate, harder migration.
**Scope:** Build the command incrementally through the focused section TODOs below, then finish with `pftui report build daily — assembler + dry-run`. Command contract: `pftui report build daily [--mode public|private|both] [--date YYYY-MM-DD] [--out-dir <path>]` produces markdown file(s) ready for `gen-report.py`; default date is today, default mode is `both`, default out-dir is `~/pftui/reports/` for public plus `/tmp/` for private. Shared implementation shape: `src/report/build/daily.rs` owns `BuildContext` and shared data loading; each section lives in `src/report/sections/<name>.rs` and exposes `pub fn render_<name>(ctx: &BuildContext) -> Result<String>`. Section output follows `/Users/skylar/.claude/commands/pftui-report.md` Step 5a and Step 5b because `~/.claude/skills/pftui-report.md` does not exist on this machine. Public-mode output must enforce the privacy guarantee before writing: no personal holdings, position sizes, cost basis, PnL figures, user allocation percentages, transactions, or first-person personal portfolio framing. Files across sub-items: `src/commands/report.rs`, `src/report/build/daily.rs`, `src/report/sections/*.rs`, `src/cli.rs`, the report command/skill, `AGENTS.md`. Tests across sub-items: section golden tests against synthetic fixtures, public privacy guard, and dry-run/no-write behavior.
**Implementation plan:** Complete the section TODOs below first; each should be a focused 4-8 hour PR. Do not pick this umbrella item directly. When all section renderers exist, complete `pftui report build daily — assembler + dry-run` to wire them into the CLI and retire the remaining Python/skill-side assembly path.
**Effort:** Incremental; each section item is sized independently.

### `pftui report build daily` — section private bottom line
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_bottom_line(ctx: &BuildContext) -> Result<String>`. Data: private portfolio snapshot, daily PnL context, most material derived actions, binary catalysts, what-changed deltas. Output shape: `## Bottom Line`, 3-5 bullets, and native `{what_changed_strip(deltas)}`. Tests: bullets include regime/action/catalyst coverage, chart helper output is embedded, private-only content is never reused in public mode.
**Effort:** 4-6 hours.

### `pftui report build daily` — section private portfolio snapshot
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_portfolio_snapshot(ctx: &BuildContext) -> Result<String>`. Data: held positions, cached prices, allocation percentages, unrealized PnL, allocation targets, drift rows. Output shape: `## Portfolio Snapshot`, native `{stacked_bar(segments)}`, positions table, `### Drift vs Allocation Targets`, and native drift bars. Tests: synthetic holdings render deterministically, dust positions are handled, target drift bars match fixture values.
**Effort:** 5-7 hours.

### `pftui report build daily` — section private macro context
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_macro_context(ctx: &BuildContext) -> Result<String>`. Data: regime quadrant inputs, active scenarios and 7-day deltas, narrative-vs-money divergence, near-term catalysts. Output shape: `## Macro Context` with side-by-side native `{regime_quadrant(...)}` and `{prob_bar(...)}` output plus <=2 paragraphs. Tests: scenario bars use normalized semantics, narrative-vs-money divergence appears when material, output stays concise.
**Effort:** 5-7 hours.

### `pftui report build daily` — section private per-asset convergence
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_per_asset_convergence(ctx: &BuildContext) -> Result<String>`. Data: held assets above 1%, `analytics views convergence --asset`, user targets, current allocation, deterministic analyst range. Output shape: `## Per-Asset Convergence` followed by native `{analyst_convergence_card(...)}` per held asset. Tests: missing analyst layers surface `insufficient-views`, derived ranges follow the canonical formula exactly, card count matches held assets above threshold.
**Effort:** 6-8 hours.

### `pftui report build daily` — section private conviction trajectory
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_conviction_trajectory(ctx: &BuildContext) -> Result<String>`. Data: 30-day conviction or analyst-view history by held asset and layer. Output shape: `## Conviction Trajectory (30 days)` plus native `{conviction_trajectory(asset, layer_series)}` for each held position. Tests: sparse series render without panic, layers stay ordered LOW/MEDIUM/HIGH/MACRO, output includes every qualifying held asset.
**Effort:** 4-6 hours.

### `pftui report build daily` — section private outlook by horizon
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_outlook_by_horizon(ctx: &BuildContext) -> Result<String>`. Data: current LOW/MEDIUM/HIGH analyst views or derived convergence horizon fields for held assets. Output shape: `## Outlook by Horizon` table with native `{outlook_arrows(days, weeks, months)}` per held asset and 2-3 sentences interpreting cross-asset alignment. Tests: direction mapping is deterministic, missing horizon data renders neutral/unknown, table order follows portfolio materiality.
**Effort:** 4-6 hours.

### `pftui report build daily` — section private risk concentration
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_risk_concentration(ctx: &BuildContext) -> Result<String>`. Data: scenario exposures, current allocations, factor mapping, active scenario probabilities. Output shape: `## Risk Concentration`, native `{factor_exposure(factors)}`, and one paragraph on correlated exposure and hedge pressure. Tests: exposure percentages come from fixture allocations, high-probability scenario alignment is described, missing factor mapping emits a clear fallback.
**Effort:** 5-7 hours.

### `pftui report build daily` — section private mismatch surface
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_mismatch_surface(ctx: &BuildContext) -> Result<String>`. Data: recent journal entries by `author='skylar'`, held-asset convergence summaries, mismatch thresholds. Output shape: `## Mismatch Surface — Skylar's view vs analyst convergence` with native `{mismatch_card(...)}` for meaningful divergences or one aligned sentence. Tests: synthetic divergence creates a card, aligned fixture creates the one-sentence fallback, no public renderer can call this section.
**Effort:** 5-7 hours.

### `pftui report build daily` — section private news and catalysts
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_news_catalysts(ctx: &BuildContext) -> Result<String>`. Data: last-24h news, source metadata, held assets, active scenarios, news-silence analytics. Output shape: `## News & Catalysts`, 3-5 event blocks with What happened / Where the money moved / Who benefits / What it means and the required source metadata line. Tests: events connect to held assets or scenarios, metadata line is mandatory, insufficient-baseline silence rows are skipped.
**Effort:** 5-7 hours.

### `pftui report build daily` — section private upcoming calendar
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_upcoming_calendar(ctx: &BuildContext) -> Result<String>`. Data: economic calendar, earnings calendar when available, known political/geopolitical dates, held-asset relevance flags. Output shape: `## Upcoming Calendar` with compact per-day bullets for the next 3-7 days and bold items affecting held positions. Tests: dates sort ascending, held-asset relevance is bolded, empty calendar emits a concise no-known-catalysts line.
**Effort:** 4-6 hours.

### `pftui report build daily` — section private open predictions
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_open_predictions(ctx: &BuildContext) -> Result<String>`. Data: pending `user_predictions` resolving in the next 7 days, confidence/conviction/target dates, calibration context. Output shape: `## Open Predictions Resolving in Next 7 Days`, native `{open_predictions_table(predictions_from_db)}`, and one interpretation sentence. Tests: pending-window filter is correct, date ordering is stable, no-predictions fixture renders an explicit empty state.
**Effort:** 4-6 hours.

### `pftui report build daily` — section private lessons applied
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_lessons_applied(ctx: &BuildContext) -> Result<String>`. Data: `analytics lessons applied --since 24h`, prediction lessons, historical analog references. Output shape: `## Lessons Applied This Run` with guarded-prediction count, top referenced lessons, strongest analog, or an explicit accountability-gap sentence. Tests: zero-lessons fixture renders the gap, nonzero fixture lists lesson ids, output remains private-only when tied to operator decisions.
**Effort:** 4-6 hours.

### `pftui report build daily` — section private self-retrospective calibration
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_self_retrospective_calibration(ctx: &BuildContext) -> Result<String>`. Data: 90-day calibration rows by layer and conviction band, sample sizes, miscalibration deltas. Output shape: `## Self-Retrospective Calibration`, native `{calibration_dot_plot(...)}`, and 2-3 bullets naming largest over/underconfidence rows. Tests: chart renders from fixture rows, largest absolute miscalibration rows are selected, low-sample caveats appear.
**Effort:** 4-6 hours.

### `pftui report build daily` — section private decisions pending
**Source:** Scaffold breakdown from the report command Step 5b.
**Scope:** Add `pub fn render_private_decisions_pending(ctx: &BuildContext) -> Result<String>`. Data: derived ADD/TRIM/HOLD actions, allocation target drift, stale targets, mismatch cards, catalyst urgency. Output shape: `## Decisions Pending — Your Reply Requested` with native `{decision_card(...)}` questions ordered by urgency and gap size. Tests: recommendations derive from convergence formula, response tokens are short, no imperative trade action appears without evidence reference.
**Effort:** 6-8 hours.

### `pftui report build daily` — assembler + dry-run
**Source:** Scaffold breakdown from the report command Step 5a/5b.
**Scope:** Wire `pftui report build daily [--mode public|private|both] [--date YYYY-MM-DD] [--out-dir <path>] [--dry-run]` through `src/cli.rs` and `src/commands/report.rs`. Add `BuildContext` loading in `src/report/build/daily.rs`, call public/private section renderers in order, write markdown to the correct destinations, and update the report command/skill to call the native command for assembly. `--dry-run` prints the section plan, data availability summary, output paths, and privacy-audit status without writing files. Tests: public/private/both mode output paths, dry-run writes nothing, section ordering fixture, public privacy guard rejects private tokens, assembled markdown golden fixture is stable.
**Effort:** 6-10 hours after section renderers exist.

### Migrate `/pftui-report` Claude skill to use native `pftui report` commands
**Source:** Skylar (May 28). Depends on `pftui report build daily` (above) being landed.
**Why:** Once `pftui report build daily` exists, the Claude skill at `~/.claude/skills/pftui-report.md` can be substantially simplified: it no longer needs to write an ad-hoc Python build script per run, no longer needs to import `pftui-operator/charts.py`, no longer needs the per-step data-gathering bash blocks that prepare chart inputs. The skill's responsibilities shrink to: (1) Step 0 health collection + blocker fixes, (2) Step 1 data refresh, (3) Step 3 spawning the four analyst subagents, (4) call `pftui report build daily --mode <m>`, (5) Step 6 privacy audit on the public markdown, (6) Step 7 PDF render via `gen-report.py`, (7) Step 8 website registry append, (8) Step 9 PR + auto-merge, (9) Step 10 final summary. The Python-orchestrated parts vanish.
**Scope:** (1) Rewrite the relevant sections of `~/.claude/skills/pftui-report.md` (Step 2 CLI bundle, Step 2b deep bundle, Step 2c thesis/lessons fetch, Step 4 synthesis, Step 5a public markdown, Step 5b private markdown) to call `pftui report build daily` instead of doing data collection + assembly in skill bash + Python. The bundles can still be staged for the analysts (they need them as input), but the synthesis-and-write step becomes a single CLI call. (2) Decommission `~/pftui-operator/charts.py` once all charts are ported and used by zero remaining code paths — leave the file but mark it deprecated in a header comment and remove the skill's `sys.path.insert` line. (3) Update the skill's failure-modes section: `pftui report build daily` errors should be diagnosed by reading the command's stderr; the skill's responsibility is to surface those errors, not to debug section assembly. (4) Run `/pftui-report` end-to-end at least twice on the new code path before considering this item done; compare the resulting markdown + PDFs against the prior Python-orchestrated outputs and confirm parity (allow some byte-level diffs for ordering / whitespace, but content should match). Files: `~/.claude/skills/pftui-report.md` (substantial rewrite), `~/pftui-operator/charts.py` (deprecation header). Tests: not applicable in pftui (skill-side change); verification is the parity comparison.
**Effort:** 4–7 days (mostly skill testing + iteration).

### Scenario probability math — enforce normalized scenario-set model
**Source:** Claude review (May 28, post-/pftui-report retrospective).
**Why:** `docs/ANALYTICS-SPEC.md` now defines scenario probabilities as a normalized, mutually exclusive, collectively exhaustive scenario set. The current system still stores `probability` as a single number per scenario without enforcing that modeled rows sum to <=100 or surfacing the `Other / Unmodelled` residual. Until enforcement lands, reports and agents can still misread missing residuals or overfilled sets.
**Scope:** (1) Add a migration or deterministic system-managed row for the `Other / Unmodelled` residual scenario, calculated as `100 - sum(active modeled scenarios)`. (2) Update scenario create/update paths to reject active modeled probabilities whose sum exceeds 100%. (3) Update list/report/JSON outputs to expose normalized-set semantics, modeled sum, residual probability, and invalid-overfill state when reading legacy data. (4) Update daily report rendering and the report skill so the Scenario Dashboard always shows the residual or an explicit data-quality warning. (5) Add tests for probability-sum constraint enforcement, residual calculation, legacy overfill reporting, and report rendering semantics. Files: `src/db/schema.rs` (migration if needed), `src/db/scenarios.rs`, `src/commands/scenarios.rs`, `src/commands/report.rs`, `src/cli.rs`, the report skill. Tests: probability-sum constraint enforcement; residual calculation; daily-report rendering matches the normalized model.
**Effort:** 4–6 hours.

### [Feedback] Add auto-suggest CLI command for scenario-to-prediction-market mappings
**Source:** evening-analysis (Apr 9, 82/79 — "Calibration command still returning empty (no scenario-to-contract mappings after 5+ sessions flagging this). Suggestion: add automated Polymarket mapping for top 4 scenarios on first run, or CLI command to auto-suggest mappings"). Corroborates Apr 6 evening-analysis and multiple prior sessions flagging the same gap.
**Why:** `pftui data predictions calibration` has returned empty for 5+ consecutive sessions because no scenario-to-contract mappings have been created. The calibration system is entirely non-functional without mappings, and agents repeatedly flag this as a gap. Adding auto-suggest would unblock the calibration workflow in a single command.
**Scope:** (1) Add `pftui data predictions map --auto-suggest` that searches tracked Polymarket contracts for keywords matching each active scenario name and outputs the top 3 mapping candidates per scenario. (2) Add `pftui data predictions map --scenario <name> --contract-id <id>` for explicit manual mapping. (3) If calibration is empty and scenarios exist, surface a one-time prompt suggesting the user run `--auto-suggest`. Files: `src/commands/predictions.rs`, `src/data/predictions.rs`.
**Effort:** 2–4 hours.

---

## P3 - Long Term

### Historical-data backfill for newly-introduced feature tables
**Source:** Claude review (May 29). PRs landed between May 28-29 created several new tables that start empty and only accumulate signal as new operations occur: `news_source_accuracy` (PR #751 — populates only as new predictions are scored AND tagged with their source article), `paired_tx_id` column on `transactions` (PR 032b8d6 — populates only on NEW transactions; pre-paired-leg-era buys remain unpaired in the history), `narrative_money_history` (PR #753 — daily writes accumulate forward, no historical view), `news_silence_baselines` (PR #754 — baselines need 30+ days of news_cache history to be meaningful, but populates forward). Some of these can be backfilled from existing data; others can't and just need time.
**Why:** Useful signal that COULD exist from day one of feature deployment instead requires months of forward accumulation. For per-source accuracy, the system has 800+ scored predictions but they aren't tagged with their source article (the `source_article_id` column was added recently — populating it retroactively requires fuzzy matching prediction claims against news_cache content). For paired_tx_id, the system has 22 historical transactions in the test DB that could be paired by date-and-amount matching. For news_silence_baselines, 90+ days of news_cache rows already exist and the baseline computation could simply backfill from them rather than waiting forward.
**Scope:** (1) `news_silence_baselines` backfill: run the baseline computation against all 90+ days of existing news_cache rows. Idempotent — re-running just refreshes. Add `pftui analytics news-silence rebuild-baselines --since 180d`. (2) `paired_tx_id` backfill: heuristic — for each unpaired buy transaction on a non-cash symbol, find the closest USD sell transaction within ±2 days and within ±10% of the buy notional, pair them. Manual review surface: `pftui portfolio transaction repair-pairs --dry-run` shows the proposed pairs; `--confirm` applies. Skip with `--manual <pair-id>` flag for tricky cases. (3) `news_source_accuracy` backfill: NOT attempted. The retroactive prediction→source-article matching is fuzzy and unreliable. Document this in the table's doc-comment: "ledger populates forward from feature deployment; historical predictions are not retroactively attributed." Add a CLI flag `pftui analytics news-sources accuracy --include-pre-deployment` that surfaces a notice when the operator asks for a window predating the feature. (4) `narrative_money_history` backfill: same as silence baselines — historical news + historical Polymarket pricing CAN be reconstructed from the existing caches; add a `--backfill` flag to the relevant analytics command that walks the historical record. Files: `src/commands/analytics.rs`, `src/commands/portfolio.rs` (transaction repair-pairs), `src/db/news_silence.rs`, `src/db/narrative_divergence.rs`, `src/db/news_source_accuracy.rs` (doc-comment only). Tests: backfill is idempotent; transaction-pairing heuristic against synthetic transaction set.
**Effort:** 5–7 hours.

### Performance budget + benchmark for `pftui report build daily`
**Source:** Claude review (May 29). Depends on `pftui report build daily` having landed.
**Why:** Once `build daily` is the primary report generation path, it sits in every operator workflow and every cron-driven autonomous run. Without a stated performance budget, the command can silently degrade as features accrete — a 200ms initial implementation becomes a 30s monster after 20 feature additions. The pattern that produced today's schema race (incremental features, fresh-DB-only CI) is the same pattern that would produce silent perf degradation. The fix is to set a budget early and benchmark in CI.
**Scope:** (1) Once `build daily` lands, measure baseline runtime against the standard test fixture: target <2s end-to-end for `pftui report build daily --mode both` on a populated DB (~90 days of history, 4 positions, 800 predictions). (2) Add `tests/report_build_daily_perf.rs`: runs `pftui report build daily --mode both` against the fixture, asserts wall-time under the budget. (3) On regression, the test failure message names which section was slowest (instrument each section function with `--timing` flag that's already part of the canonical CLI). (4) Track section-level perf in a regular comment in `src/report/build/daily.rs` so reviewers see the budget at the call site. (5) Re-baseline the budget when major features intentionally add cost (e.g., a new heavy aggregate query) — but only with explicit reviewer approval, never silently. Files: `tests/report_build_daily_perf.rs` (new), `src/report/build/daily.rs` (section-level instrumentation). Tests: meta — the test is the perf guard.
**Effort:** 3–4 hours.

### Allocation target for cash position — extend floor/ceiling system to cover the full portfolio
**Source:** Claude review (May 29). The `allocation_targets` table now supports `target_floor_pct + target_ceiling_pct` (PR #746) but cash has no target. The portfolio currently runs ~50% cash by deliberate operator choice; with no formal cash band, the drift system can't signal when cash drifts outside an intended range (e.g., if a series of large equity buys drops cash below an intended floor of 30%).
**Why:** The original argument against a cash target was "cash is optionality, not a position." But with the floor/ceiling model now supporting wide bands, cash CAN be modeled as a position with a wide band (e.g., floor 30%, ceiling 60%) — capturing the "optionality is preserved" intent while still surfacing breach signals. This closes the loop on the drift system: every dollar in the portfolio is now within a tracked range, no silent zone outside the analysis.
**Scope:** (1) Allow allocation_targets entries for cash symbols (`USD`, `GBP`, `EUR`, etc). Today the constraint may implicitly exclude non-tradeable symbols — audit `src/commands/portfolio.rs` target setting for any such restriction and lift it. (2) Add a default cash band on first use: `pftui portfolio target set USD --floor 30 --ceiling 60` (operator-chosen; not auto-seeded). (3) Drift report (`pftui portfolio drift`) treats cash like any other position when a target exists. (4) Daily report Portfolio Snapshot section displays cash drift alongside asset drift. (5) Document the design choice in `docs/ANALYTICS-SPEC.md`: cash bands model optionality without losing visibility. Files: `src/commands/portfolio.rs` (audit), `src/db/allocation_targets.rs` (audit), the report skill (display change), `docs/ANALYTICS-SPEC.md`. Tests: cash target round-trips through DB; drift report includes cash when a target exists; daily report renders cash drift bar.
**Effort:** 2–3 hours.

### F59: Capital Flow Tracking
**Source:** Competitive research (NOFX institutional flow data).
**Why:** Institutional fund flows, ETF creation/redemption, and open interest changes reveal positioning that price alone doesn't show.
**Scope:** New `data flows` source pulling ETF flow data (ETF.com or similar), institutional 13F filings, and crypto exchange flow data. New table `capital_flows`. Integration into agent routines.
**Effort:** 3–4 weeks.

### Adversary pseudo-analyst layer — argue against the convergence
**Source:** Claude review (May 28, post-/pftui-report retrospective).
**Why:** pftui's intelligence platform runs 4 timeframe analysts (LOW / MEDIUM / HIGH / MACRO) that produce "diverse" opinions per asset. In practice the four layers share priors: they read the same data bundle, the same lesson book, the same first-principles thesis context. They are more "the same lens at four focal lengths" than four independent lenses. When they appear to agree, the agreement may be confirmation of shared assumptions rather than independent corroboration. The system needs a structural counter-pressure: a fifth pseudo-layer whose explicit job is to argue against the current convergence using the same data, surface what each layer's assumptions exclude, and flag scenarios where consensus looks fragile. This is closer to a red-team than a fifth analyst. Today's report would have benefited: all four layers agreed today's hard-money capitulation is "positioning-driven, not structural." An adversary layer's job would be to write the strongest "actually, this IS structural" case using the same data, name the falsification triggers, and force the synthesis to address the counter-case explicitly.
**Scope:** (1) Create `agents/routines/adversary-analyst.md` — prompt template instructing the model to read the same bundles + analyst writes from the current run, identify the dominant convergence, and write the strongest opposing case using only data from those bundles. (2) Add a new author identifier `analyst-adversary` to the canonical list in `CLAUDE.md`. (3) The adversary runs AFTER the 4 timeframe analysts on each `/pftui-report` invocation (so it has their writes as input) but BEFORE synthesis. The adversary writes to a new table `adversary_views` with `(asset, current_convergence_summary, counter_case_summary, counter_case_evidence_points JSON, falsification_triggers JSON, fragility_score_1_5, recorded_at)`. (4) Synthesis MUST address the adversary's counter-case for any asset where `fragility_score >= 3`. (5) New CLI: `pftui analytics adversary --asset <SYM> --json`, `pftui analytics adversary fragility-rank --json`. (6) Daily report adds an "Adversary view" sub-section per asset where the fragility score is high — quoted directly from the adversary's write, not paraphrased. (7) Skill update: the report skill spawns the adversary subagent as a 5th parallel call OR sequentially after the 4 layers. Files: new `agents/routines/adversary-analyst.md`, `src/db/schema.rs` (migration), `src/db/adversary_views.rs` (new), `src/commands/analytics.rs`, `src/cli.rs`, `CLAUDE.md`, the report skill. Tests: adversary write/read; synthesis rejects publishing a report where any `fragility_score >= 3` view lacks a counter-case address in the markdown.
**Effort:** 2–3 weeks (substantial — touches the analytical pipeline core).

### MACRO analyst — falsifiable shorter-horizon checkpoints
**Source:** Claude review (May 28, post-/pftui-report retrospective).
**Why:** pftui's MACRO timeframe analyst (years-decades horizon) has 20 currently-open predictions and ZERO scored predictions in the trailing 60 days. By design — macro predictions resolve slowly. But the consequence is that MACRO is effectively uncalibrated: it cannot be wrong on any timescale that produces feedback, so its convictions never get refined by ground truth. Today MACRO held strong views on de-dollarisation, Stage 6 currency debasement, and Fourth Turning crisis-climax — none of which can be falsified on a horizon shorter than years. This is the same epistemic risk identified in the system's blind-spots register ("Geopolitical binary event underpricing" / "magnitude over-prediction" patterns — both were derived from layers that COULD be scored). The fix is to require MACRO to write quarterly checkpoint sub-predictions for each multi-year thesis: leading-indicator metrics that, if absent on a near horizon, would invalidate the macro call.
**Scope:** (1) Update `agents/routines/macro-timeframe-analyst.md`: for every active macro thesis (Stage 6, Fourth Turning, de-dollarisation, Dalio composite, structural inflation), the analyst MUST produce 2-3 quarterly checkpoint predictions on a 90-day horizon. Format: "By 2026-09-28, IF [observable leading indicator] is NOT [specific threshold], my [thesis X] is degraded." (2) Each checkpoint becomes a normal `user_prediction` row with `timeframe='macro-checkpoint'` (new enum value) and `target_date = recorded_at + 90 days`. (3) Existing macro `timeframe='macro'` predictions stay as multi-year structural calls (uncalibrated long-horizon by design). (4) Calibration display (separate TODO above) gets a `macro-checkpoint` row that ACTUALLY accumulates samples over time. (5) When a macro-checkpoint is scored Wrong, it triggers automatic re-evaluation of the parent thesis — surface in synthesis: "Macro thesis [X] has 1 of 3 checkpoints failed; analyst should re-examine before next run." (6) AGENTS.md update documenting the new pattern. Files: `agents/routines/macro-timeframe-analyst.md`, `src/db/schema.rs` (enum value), `src/db/user_predictions.rs`, `src/commands/prediction.rs`, the report skill (for the re-evaluation surface), `AGENTS.md`. Tests: checkpoint creation; parent-thesis re-eval trigger fires on first wrong checkpoint.
**Effort:** 1–2 weeks.

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analysis | 82% | 79% | Apr 9 | ↑ (debate tool returns empty debate_id; calendar garbled 2nd occurrence; calibration empty 5+ sessions; regime lag on ceasefire day.) |
| Evening Analyst | 80% | 78% | Apr 9 | ↑ (news NEWS_UNAVAIL P1; lesson coverage 8% critical; COT 9d stale pre-war.) |
| Medium-Timeframe Analyst | 68% | 76% | Apr 9 | ↓ (FRED 67d+ stale on CPI/PPI/GDP/PCE despite fallback PRs; situation update log --driver recurring + --situation arg exit 1.) |
| Medium-Agent | 72% | 78% | Apr 7 | → (analytics medium improved; FRED staleness persistent.) |
| Low-Timeframe Analyst | 72% | 74% | Apr 8 | → (news descriptions empty; indicator list last_checked stale March 22.) |
| Low-Agent | 70% | 72% | Apr 8 | ↓ (was 72/74 Apr 7 — digest --agent-filter regression post-PR #659; calendar garbled.) |
| High-Agent | 72% | 78% | Apr 6 | new reviewer. fear-greed, COMEX 403, views --layer, docs syntax gaps remain. |
| Morning Brief | 82% | 78% | Apr 5 | → (stable.) |
| Macro-Timeframe Analyst | 55% | 62% | Apr 5 | ↑ (many items shipped Apr 6–7; expect score recovery on next run.) |

**Top 3 priorities based on feedback:**
1. **pftui data news NEWS_UNAVAIL root cause (P1)** — primary signal source completely down for the Apr 9 session; entire news workflow fell back to `web_search`. Fix before next cron run.
2. **FRED fallback activation re-audit (P1)** — CPI/PPI/GDP/GDPNow/DGS10 all stale despite PRs #649–651 being shipped. Fallback logic not activating; macro data degraded across all agents for 5+ sessions.
3. **Prediction lesson bulk command — lesson coverage 8% (P1)** — 92% of wrong predictions have no structured lesson. System self-improvement loop non-functional. Evening-analyst rated this P1; target is 80% coverage.

**Shipped since last review (Apr 7 — previous run):**
- Fix clippy unnecessary_cast in cot.rs test data — `week as i64` → `week` (this PR)
- analytics situation severity validation docs (PR #658) — `--severity` now shows valid values
- analytics digest --from/--agent-filter flags (PR #659) — date + agent filtering *(regression: --agent-filter still throwing unexpected argument error post-merge — see P1)*
- agent message ack --to clarified help text (PR #660) — concrete usage examples
- prediction scorecard --lesson-coverage (PR #656) — annotates unlessoned wrong predictions
- stale data health in analytics guidance (PR #654) — surfaces degraded sources at session start
- analytics medium snapshot improved (PR #653) — now returns useful medium-TF data
- COT schedule metadata + Friday retry (PR #652) — `next_report_date` field, auto-refetch *(COT still 9d stale Apr 9 — Friday retry not firing — see P1)*
- GDPNow fallback + GDP cadence context (PR #651) — fixes 188-day staleness *(GDPNow still 98d stale Apr 9 — fallback not activating — see P1 FRED re-audit)*
- CPI/PPI FRED fallbacks (PR #650) — BLS fallback when FRED fails *(CPI/PPI still 67d stale Apr 9 — fallback not activating — see P1 FRED re-audit)*
- DGS10 Yahoo Finance fallback (PR #649) — ^TNX fallback for 4-day staleness *(DGS10 still stale Apr 7 — fallback not activating — see P1 FRED re-audit)*
- silver stale price status (PR #646) — `stale: true` flag on data prices
- clippy errors in power_signals.rs + supply.rs (PR #648) — unblocked release eligibility

**Release status:** v0.26.0 (Apr 4). **Tests:** 2606 passed / 0 failed / 2 ignored. **Clippy:** ✅ Clean (cot.rs fix this PR). **Release eligibility:** ✅ All conditions met — cut v0.27.0 immediately after this PR merges (84 commits of features/fixes since v0.26.0).

**GitHub stars:** 9 — Homebrew Core requires 50+.
