# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P2 - Coverage And Agent Consumption

### [Claude-WIP 2026-06-02f — DO NOT PICK] Enhance `pftui analytics technicals` — signals subset (MTF RSI, Pi Cycle, MTF breakout, Bollinger reversal, RSI extreme)
**Source:** Skylar (June 1). Split from the larger technicals expansion. Builds on the channels subset (extended module).
**Why:** Continuation of the technicals expansion. These signal outputs add multi-timeframe alignment, cycle markers, and reversal/extreme flags the analysts need to reason about breakouts, exhaustion, and frothy conditions.
**Scope:** Implement these as additional outputs from `pftui analytics technicals --symbols <SYM> [--include <feature>] [--json]`:

(5) **Multi-timeframe RSI alignment**: compute RSI on the current timeframe and at 4 higher timeframes (auto-selected from current TF: 5min → [15,30,60,240]; 1h → [4h,1d,1w,1M]; etc). Output: per-TF RSI values + boolean flags `aligned_overbought` (all four HTFs > 70 + current > 70) and `aligned_oversold` (mirror).

(6) **Pi Cycle Top/Bottom signal**: top = 350d SMA × 2 crossing under 111d SMA on daily close; bottom = 471d SMA × 0.745 crossing over 150d EMA. Output: latest crossover date + days-since per signal. Expose for any asset, document that the parameters were calibrated on BTC.

(7) **Multi-timeframe breakout signal**: composite of three sub-signals — (a) MTF-RSI breakout: current RSI just exited a `oversold/overbought across 4 HTFs` zone; (b) 3-Line Strike pattern: 3 consecutive down-closes followed by an up-close that exceeds bar-1 open (mirror for bear); (c) Momentum exhaustion: 5+ closes greater than close[-4] with current close < open AND high >= 25-bar high (mirror for bottom). Output: per-signal boolean + `signal_count` (0-3) + cooldown-aware `breakout_state` (`bull-fresh` / `bull-armed` / `none` / `bear-armed` / `bear-fresh`). Cooldown: minimum 5 bars between signals (configurable).

(8) **Bollinger reversal signals**: cross-under upper band → `top_reversal_signal`; cross-over lower band → `bottom_reversal_signal`. Multi-bar confirmations: `confirmation_1` (price stays below the reversal-bar low for the next 1 bar) and `confirmation_2` (sustains for 2 bars). Output: per-signal boolean + bar offsets where the signal fired.

(9) **RSI extreme highlighting** as a derived flag: when current-TF RSI > 85 AND multi-timeframe alignment is `aligned_overbought` AND current bar makes a new 14-bar high → flag `rsi_extreme_high`. Mirror for low.

CLI: each output selectable via `--include mtf-rsi,pi-cycle,mtf-breakout,bollinger-reversal,rsi-extreme` (or `--include all`).

Implementation: extend the `src/indicators/extended.rs` module landed by the channels subset. Hook computations into `src/commands/technicals.rs`. Files: `src/indicators/extended.rs`, `src/commands/technicals.rs`, `src/cli.rs`, `AGENTS.md`. Tests: each function gets a synthetic-candle fixture test verifying computed values at known bars; integration test that `--include all --json` returns the expected JSON shape.

Naming: canonical TA terminology only — no vendor / indicator brand names.
### [Claude-WIP 2026-06-02e — DO NOT PICK] Enhance `pftui analytics technicals` — channels subset (Gaussian, Zone EMA, Volatility-weighted, Donchian)
**Source:** Skylar (June 1). Split from the larger technicals expansion.
**Why:** pftui's existing `analytics technicals` covers RSI, MACD, SMA, Bollinger Bands, and ATR — limited to single-timeframe indicators with simple parameterisation. The channel/trend-line additions below are well-established TA primitives that mature charting platforms compute; surfacing them via `--json` lets the analyst routines reason about trend strength and regime shifts without external visual indicators.
**Scope:** Implement these as additional outputs from `pftui analytics technicals --symbols <SYM> [--include <feature>] [--json]`:

(1) **Gaussian Channel band**: DEMA → multi-pass Gaussian filter → SMMA chain with σ-bands. Configurable: DEMA length (default 7), Gaussian length (default 4), Gaussian σ (default 2.0), SMMA length (default 12), SD length (default 30), upper/lower SD multipliers (default 2.5 / 1.8). Output: middle line + upper/lower bands + a derived `band_state` enum (`above_upper` / `in_band` / `below_lower`).

(2) **Zone-based EMA channel** (companion to Gaussian): two EMAs (default 144 / 233 periods, timeframe-adapted) forming inner + outer zones with configurable scale and extension. Output: `zone_position` (`upper-outer` / `upper-inner` / `lower-inner` / `lower-outer`) and the four band values.

(3) **Volatility-weighted trend line**: a smoothed momentum line whose smoothing constant is modulated by realised volatility (high-vol = faster reaction, low-vol = slower). Sensitivity: Fast / Medium / Slow → length 9 / 18 / 27. Output: trend value + slope direction + `trend_strength` integer 0-3.

(4) **Donchian channel midline trend**: midline of conversion-length (default 5) Donchian + baseline-length (default 26) Donchian. Output: trend value + slope. Hybrid mode (configurable weight) blends Volatility-Weighted and Donchian trends.

CLI: each output selectable via `--include gaussian-channel,zone-channel,volatility-trend,donchian-trend` (or `--include all`). Default for backward-compat: existing RSI/MACD/SMA/BB/ATR set only.

Implementation: pure functions over a `&[Candle]` slice. Put them in `src/indicators/extended.rs` (new module). Hook into `src/commands/technicals.rs` (or wherever the existing `--symbols ... --json` handler lives). Files: `src/indicators/extended.rs` (new), `src/commands/technicals.rs`, `src/cli.rs`, `AGENTS.md`. Tests: each function gets a synthetic-candle fixture test verifying the computed value at a known bar; integration test that `--include all --json` returns the expected JSON shape.

Naming: do NOT use vendor / indicator brand names anywhere. Use canonical TA terminology only.
**Effort:** ~1 week.

### `pftui report build daily` — umbrella tracker (do not pick directly)
**Source:** Skylar (May 28). Depends on both `pftui report` scaffold and the chart-helper-port items above.
**Why:** Once chart rendering is native, the next layer up is the report ASSEMBLY — pulling data, ordering sections, inlining charts, and writing the markdown that feeds the PDF renderer. Today that work lives in the Claude `/pftui-report` skill orchestration plus the ad-hoc Python build script generated per run. Making it a native `pftui report build daily` command means: (1) anyone (not just Claude) can build a daily report from a populated DB; (2) the assembly logic gets `cargo test` coverage; (3) the Claude skill becomes much thinner — it spawns analysts, then calls `pftui report build daily`, then PRs the output. (4) Removes the Python build script entirely from the steady-state pipeline.
**Implementation plan:** All section TODOs and the assembler are landed. Remaining work is the skill migration below.
**Effort:** Complete except for the skill migration item.

### Migrate `/pftui-report` Claude skill to use native `pftui report` commands
**Source:** Skylar (May 28). Depends on `pftui report build daily` (above) being landed.
**Why:** Now that `pftui report build daily` exists end-to-end, the Claude skill at `~/.claude/skills/pftui-report.md` can be substantially simplified: no ad-hoc Python build script per run, no per-step data-gathering bash blocks that prepare chart inputs. The skill's responsibilities shrink to: Step 0 health collection + blocker fixes, Step 1 data refresh, Step 3 spawning the four analyst subagents, then calling `pftui report build daily --mode <m>`, then the privacy audit / PDF render / website registry / PR steps.
**Scope:** (1) Rewrite the relevant sections of `~/.claude/skills/pftui-report.md` (Step 2 CLI bundle, Step 2b deep bundle, Step 2c thesis/lessons fetch, Step 4 synthesis, Step 5a public markdown, Step 5b private markdown) to call `pftui report build daily` instead of doing data collection + assembly in skill bash + Python. The bundles can still be staged for the analysts (they need them as input), but the synthesis-and-write step becomes a single CLI call. (2) Decommission `~/pftui-operator/charts.py` once all charts are ported and used by zero remaining code paths — leave the file but mark it deprecated in a header comment and remove the skill's `sys.path.insert` line. (3) Update the skill's failure-modes section: `pftui report build daily` errors should be diagnosed by reading the command's stderr; the skill's responsibility is to surface those errors, not to debug section assembly. (4) Run `/pftui-report` end-to-end at least twice on the new code path before considering this item done; compare the resulting markdown + PDFs against the prior Python-orchestrated outputs and confirm parity. Files: `~/.claude/skills/pftui-report.md` (substantial rewrite), `~/pftui-operator/charts.py` (deprecation header). Tests: not applicable in pftui (skill-side change); verification is the parity comparison.
**Effort:** 4–7 days (mostly skill testing + iteration).

---

## P3 - Long Term

### Options flow + GEX (gamma exposure) ingestion
**Source:** Claude DB enrichment session (June 1). The single most-impactful missing data input identified across the substrate.
**Why:** 27 lessons in the `tight_threshold_close_miss` cluster and 14+ predictions in `options-gamma-pinning` fragment territory all share a root cause that's invisible to the current ingest: options gamma concentration at round-number strikes mechanically pins prices. SPY $700, BTC $75k, gold $5000 — all repeated threshold misses where the prediction direction was right but the close pinned to the level. Without options-flow data, the `options-gamma-pinning` and `tight-threshold-coin-flip` fragments are heuristics applied retrospectively. With it, they become computed: "current SPY GEX puts gamma flip at 745; predictions through 745 need to clear by 1.5xATR + gamma-zone width." This is the single new ingest that would directly upgrade the most-recurring miss pattern.
**Scope:** (1) New data source `pftui data options [--symbol SPY] [--strike-window 10] [--json]` pulling from a free or low-cost options-flow provider (research candidates: Polygon options snapshot endpoint, CBOE OI data, or unofficial gex.app scraping if licensing allows). (2) New tables: `options_chain_snapshots (symbol, strike, expiry, dte, oi_calls, oi_puts, vol_calls, vol_puts, iv_atm, fetched_at)` and `gex_snapshots (symbol, gex_flip_strike, total_gamma_call, total_gamma_put, max_pain, fetched_at)`. (3) Refresh integration: `data refresh` pulls daily snapshots for SPY, QQQ, BTC (via deribit), GLD, SLV, and held single-name positions if any. (4) `pftui analytics gex --symbol <s> [--json]` returns the current snapshot + the "gamma neutral" zone. (5) Pre-flight integration: when a prediction targets a level within a known gamma zone, surface a warning. (6) Daily report: per-asset section adds a one-line "GEX flip at $X, max pain $Y" inline. (7) Backfill from historical OI data if the provider supports it (typically last 90 days). Files: `src/data/options.rs` (new), `src/db/options_chain_snapshots.rs` + `src/db/gex_snapshots.rs` (new), `src/commands/data.rs`, `src/commands/analytics.rs`, `src/cli.rs`, the report skill. Tests: data fetch against mocked provider; gex computation against fixture chain; preflight integration.
**Effort:** 3–4 weeks (most of which is selecting + integrating a data source).

### [Claude-WIP 2026-06-02h — DO NOT PICK] Real-yields curve ingestion (10Y TIPS, breakevens, G10 spreads)
**Source:** Claude DB enrichment session (June 1).
**Why:** The single most-cited missing input across the gold + DXY miss patterns. The `realrates-dominates-gold` and `dxy-two-driver` reasoning fragments are explicitly about real-yields-vs-nominal-yields and rate-differential vs safe-haven dynamics — but pftui only ingests nominal yields (TNX, TYX, FVX, IRX from yahoo). Without TIPS curves + breakevens + G10 sovereign yields, the analysts have to GUESS at real rates. With them, several monthly+ horizon predictions become evaluable.
**Scope:** (1) Add FRED series ingest for: DFII10 (10Y TIPS), DFII5 (5Y TIPS), DFII30 (30Y TIPS), T10YIE (10Y breakeven), T5YIE (5Y breakeven), DGS10 / DGS5 (nominal already in BLS but verify). (2) Add G10 sovereign 10Y via the FRED ECB series (10-year benchmark) for UK, Germany, Japan, Canada — gives the rate-differential decomposition. (3) New table `real_yields_history (date, series, value, source)`. (4) Derived view `real_rate_differentials` computing US vs G10 average + per-pair differentials. (5) `pftui data real-yields --json` exposes current state. (6) Daily report Macro section includes a "Real rates: 10Y TIPS +Xbp this week | Breakeven Y at Z% | US-Eu spread +W" callout. (7) Analyst routine update: gold + DXY views must reference real-yields current state, not just nominal yields. (8) Calibration matrix re-computation: see if `realrates-dominates-gold` fragment's predictive power can be quantified retroactively against the now-ingested data. Files: `src/data/real_yields.rs` (new), `src/db/real_yields_history.rs` (new), `src/commands/data.rs`, `src/commands/analytics.rs`, `src/cli.rs`, the report skill, MACRO + HIGH routine files. Tests: FRED ingestion against fixture response; derived differentials are arithmetically correct; calibration uses the new series.
**Effort:** 1–2 weeks.

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

### Thesis dependency graph — LLM-assisted extraction backfill
**Source:** Follow-up to the 2026-06-02 cross-asset thesis dependency graph PR. That PR landed the `thesis_dependencies` table, `pftui analytics thesis-chains list|show|validate|add`, the price-threshold validator, the `journal prediction preflight` integration, and a `report::sections::thesis_chains_macro::render_thesis_chains_block` renderer.
**Why:** Chains are currently authored by hand via `thesis-chains add`. The fastest way to seed 30-60 high-quality chains is a one-shot Opus extraction pass over `thesis.content` + `prediction_lessons.why_wrong` + last-90d `agent_messages`.
**Scope:** (1) Add an Opus subagent or `pftui agent` command that reads the three sources, emits JSONL `{antecedent_text, relation, consequent_text, conviction, source_lesson_ids, source_thesis_sections, evidence_count}` triples, and calls `analytics thesis-chains add` for each row. (2) Enrich the validator to handle additional predicate shapes (range thresholds, derived metrics like real_yield, DXY-spread). (3) Auto-wire the `thesis_chains_macro::render_thesis_chains_block` output into the daily-report Macro section assembler once the assembler exposes a chain-loading hook. (4) Tests: extraction produces valid triples; auto-wire respects the public-mode privacy guard (chains do not leak portfolio-specific framing).
**Effort:** 1 week (mostly subagent prompt engineering + Opus call budget).

---
