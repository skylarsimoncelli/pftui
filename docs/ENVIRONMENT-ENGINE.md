# ENVIRONMENT-ENGINE.md — historic-analog · regime · cycle · positioning

> Product + technical design for pftui's next analytical engine. Status: **design / phased build**.
> Goal (operator's words): pftui should tell me, **with quantifiable, sourced data**, (1) the closest
> historic parallels to our current multi-asset environment; (2) how per-asset cycles + the business
> cycle + larger cycles map onto now and onto those parallels; (3) the best positioning for the current
> environment, backtested across past analogous environments, supported by technical analysis and
> quantifiable success.
>
> This doc is grounded in research across the quant/data-science literature, how hedge funds
> (Bridgewater, AQR, 42 Macro, CTAs, Fed nowcasts) and insurers/actuaries (ESG, cat models, SR 11-7,
> CCAR) do it, and a full audit of what pftui already has. Read alongside
> [DATA-ARCHITECTURE.md](DATA-ARCHITECTURE.md), [EPISTEMICS.md](EPISTEMICS.md),
> [CYCLE-THEORY.md](CYCLE-THEORY.md), and [pftui-glass-box-quant direction] in memory.

---

## 0. The one-sentence architecture

An `available_at`-guarded daily **environment feature vector** feeds a **Mahalanobis-kNN / Analog-Ensemble**
analog engine and a **MASS** shape-search; **regime** (legible growth×inflation quadrant + change-point alerts)
and **cycle** reads (extended cycle engine + business-cycle dating) tag each day; a **positioning synthesizer**
conditions expected returns on the matched analogs + regime + measured signals — and **nothing is reported to
the operator until it clears a validation gauntlet** (Deflated Sharpe, PBO, multiple-testing haircuts,
block-bootstrap CIs) — all pure Rust plus `statrs`/`rand`/`nalgebra`/`rustfft`.

The guiding principle is pftui's existing one: **measurement loop over garnish; expectancy-gated claims.**
This engine is the measurement loop for the *macro environment*, the way `research/event_study` is for signals.

---

## 1. Output format decision (the operator was undecided)

**Recommendation: data-first, interactive-second, narrative-thin.** Not a PDF.

- The PDF/newsletter is a *presentation* layer for narrative and external readers. This engine is an
  *exploratory decision tool* for the operator — you will want to drill in ("show the 1973 analog's
  forward-return distribution", "re-rank analogs excluding the COVID era", "condition on Quad 3 only").
  A static PDF is the wrong shape for that.
- **Primary artifact = structured computed data (JSON) behind a CLI** (`pftui analytics environment …`,
  `… analog …`, `… positioning …`). Composable, agent-readable, the source of truth. This mirrors how the
  Atlanta Fed ships **GDPNow** — raw computed output + an open, published error track record, *no glossy
  report and "no subjective adjustments"*. That is the credibility model to copy.
- **Secondary = an interactive TUI "Environment" view** for human exploration (drill into analogs, regime
  history, cycle clocks, positioning cards). Fits pftui's existing TUI; this is where the "interactive UI"
  instinct is satisfied without building a separate web app first.
- **Tertiary = a thin narrative section** in the report that *summarizes the computed findings in prose*
  when a report is generated — it cites the engine, never replaces it.

Web dashboard is a later surface once the computed core and TUI exist; don't lead with it.

---

## 2. What pftui already has (reuse, don't rebuild)

| Capability | Where | Reuse for |
|---|---|---|
| Deterministic cycle toolkit (pivots, timing bands, translation ledger, FLD/VTL, nesting, BTC/gold clocks) | `analytics/cycle_engine.rs`, `cycle_clock.rs` | Per-asset cycle read (Q2) |
| Event-study rigor: forward returns, baseline lift, overlap exclusion, exact binomial, era/200dma splits, walk-forward `as_of` | `research/event_study.rs`, `signal_expectancy` (L2) | The stats spine for analogs + positioning |
| Strategy backtester DSL (segment/compare/backtest, lookahead-safe resolver) | `analytics/strategy/` | Positioning rules, regime-conditioned backtests |
| Parallels engine (condition sets → forward-return distributions) | `~/.config/pftui/parallels.yml` + `pftui-parallels-run` + `report/sections/private_parallels.rs` | Seed of the analog engine (but predicate eval lives in an EXTERNAL tool — see gap) |
| Regime classification (scenario-prob presets) + history | `db/regime_history.rs` | Extend into the multi-dimensional regime store |
| Series registry + L0–L4 layering + schema-conformance contract | `series_registry`, `docs/db-catalog.toml`, `tests/schema_conformance.rs` | Where new tables attach |
| Independence layers (blind / adversary / antithesis) | report-prompts + `analyst_views` measurement class | The "effective challenge" governance (SR 11-7) — already built |

**Key gaps blocking the three goals** (from the codebase audit):
1. No **environment feature vector** (a daily, multi-asset, stationary state — everything downstream needs it).
2. Parallels **predicate evaluation is an external binary**, not in-repo; no cross-asset / macro predicates.
3. Event-study never splits by `regime_history.regime` (only by 200dma).
4. No **business-cycle / nowcast** read; `economic_data` is head-row-only (no history); no cycle-clock command.
5. No **total-return series for cash/bonds** → the defensive leg of any positioning is unmeasurable.
6. No **validation gauntlet** (DSR/PBO/haircuts/bootstrap) — the honesty layer the whole thing depends on.
7. No **analog distance engine**, no **positioning synthesizer**.

---

## 3. Component specs

### 3.1 Environment feature vector — `analytics/environment/` + `environment_snapshots` (L2)

A daily row capturing the multi-asset state as a **stationary, z-scored feature vector**. The foundation.

- **Features (d ≈ 12–20, kept small):** asset log-returns & realized vol (BTC, gold, silver, SPX, oil),
  yield level + Δ (10y, 3m), 2s10s curve slope, real-yield proxy, DXY level + Δ, VIX level + term proxy,
  cross-asset correlations (BTC-SPX, gold-DXY), credit proxy (HYG/LQD), breadth (when available), growth &
  inflation rate-of-change (from macro series). Each engineered to be **stationary** (returns/changes/spreads,
  not levels — levels have unit roots; an ADF check gates the feature set) and **expanding-window z-scored**
  (no look-ahead).
- **Layer L2** (rebuildable from L0/L1, stamped per refresh). Catalog entry + conformance test in the same PR.
- **The `available_at` discipline (Tier-0, non-negotiable):** every row carries the date its inputs were
  actually knowable; macro prints lagged to release dates; every downstream query filters by it. This is the
  one thing no statistic can fix after the fact (survivorship/lookahead). Add it at the schema level.
- CLI: `pftui analytics environment current [--json]`, `… environment history [--from --to]`.

### 3.2 Analog engine — `analytics/analog/` + `analog_matches` (L3)

"What past period looks like now," with a *forward-return distribution*, not a point estimate.

- **Distance: Mahalanobis** (covariance-whitened) over the environment vector — beats Euclidean for correlated
  financial features. Build Σ⁻¹ once (`nalgebra`/`faer` Cholesky); PCA (`linfa-reduction`) to keep d small and
  Σ well-conditioned. k-NN is brute-force O(n·d) — microseconds over ~7,500 daily rows.
- **Analog Ensemble (AnEn):** today's vector → k nearest historical days → **the distribution of their realized
  forward returns (per asset, per horizon) is the probabilistic forecast.** Report nearest-neighbor distances as
  an explicit **analog-quality / confidence** score (far neighbors = low confidence).
- **Shape search (MASS):** match the recent N-bar window against all of history via FFT convolution
  (`rustfft`), O(n log n), window-length-independent — surfaces the closest *trajectory* analogs, not just the
  point-in-state ones. (Full Matrix-Profile motif/discord is a later upgrade; MASS is the pragmatic native path.)
- **Honest output (Bridgewater archetype-by-averaging):** don't cherry-pick one analog. Output the *ensemble* and
  "current vs the averaged analog template, and where/why it differs." Block-bootstrap CIs on the analog forward
  distribution (§3.6) so the operator sees error bars, not false precision.
- CLI: `pftui analytics analog current [--horizon 30|90|180] [--exclude-era covid] [--json]`,
  `… analog show <match-date>`.
- **Builds on parallels**, but moves predicate evaluation *in-repo* and generalizes from hand-written condition
  sets to a learned distance over a feature vector. The named parallels (Mayer, 200WMA, etc.) become *labeled*
  analogs layered on top of the distance engine.

### 3.3 Regime engine — `analytics/regime/` (extend `regime_history`)

Multi-dimensional, **legible-first**.

- **Primary spine = growth×inflation quadrant** (42 Macro / Hedgeye GIP lineage): classify by the *rate-of-change*
  (2nd derivative) of growth and inflation → Goldilocks / Reflation / Inflation / Deflation. Transparent,
  explainable, defensible — a glass-box regime, not an opaque score. Store a daily label + the two RoC inputs.
- **Change-point alerts = BOCPD** (`changepoint` crate, native) on the environment vector for daily-refresh
  "a regime break just became probable" alerts; CUSUM as a cheap guardrail.
- **Clustering (measurement layer, optional) = k-means / GMM** (`linfa-clustering`) over the environment vector,
  *validated by silhouette + bootstrap stability (ARI)* — a regime labeling is only trustworthy if it's stable
  under resampling and on the held-out tail. Kept as measurement (published hit-rates), never the sole driver.
- Wire `regime_at_event` into `event_study` so **signal & analog expectancy can be split by regime** (the missing
  conditioning). CLI: `pftui research expectancy --regime <R>`.
- HMM / Markov-switching is deferred (genuine Rust gap → hand-rolled Baum-Welch or Python sidecar) and is *not*
  needed for v1; the quad spine + BOCPD covers the legible case.

### 3.4 Cycle engine — extend `analytics/cycle_engine.rs`; new `analytics/cycle/clock.rs` + business-cycle

- **Build the cycle-clock command** (the documented gap): deterministic days/weeks-post-halving, Loukas
  week-of-cycle band, Olson day-count, midterm-H2 flag, distance from 200-week MA, and the live read on the
  **falsifiable major-vs-4yr test** (does the post-low rally clear the prior cycle high). `pftui analytics cycles clock <SYM>`.
- **Business-cycle dating = Bry-Boschan** (pure Rust, O(n) passes) on macro series + **Christiano-Fitzgerald**
  bandpass for the 6–32-month band (asymmetric → uses the latest obs, right for "where are we now"). Use
  **Hamilton's regression filter**, *not* HP, for detrending (HP endpoints are unreliable for the live read).
- **Character indicators:** FFT / Lomb-Scargle for dominant cycle length; **Hurst/DFA** for trend-vs-mean-revert
  persistence (a character indicator, not a cycle length). All pure Rust.
- **Larger cycles** (Kitchin/Juglar/Kondratiev) surface as *labels mapped onto the dated cycles*, explicitly
  flagged as low-N / interpretive (the engine computes what it can measure and says where it's narrating).

### 3.5 Positioning synthesizer — `analytics/positioning/`

Conditions expected returns on **analogs + regime + measured signals + cycle position**, then gates on the
validation gauntlet before it speaks.

- Inputs: analog forward-return distribution (§3.2), regime-conditioned signal expectancy (§3.3),
  cycle band/clock (§3.4), strategy backtests (regime-filtered, §3.6).
- Output **positioning card** per asset: stance (bull/bear/neutral), confidence, the *drivers* (each with its
  number + source), the matched analogs, and — crucially — the **honesty stats** (DSR, PBO, n, CI, regime n).
- **Humility default (All Weather):** where analog quality is low and regime confidence is low, the honest
  output is "balanced / insufficient edge," and the card *says so* — with the GMO caveat that "balance" still
  embeds assumptions (positive premia, stable crisis correlations), named explicitly. No false confidence.
- Persist runs to `strategy_backtest_runs` / `positioning_runs` (L3) so positioning drift is itself measurable.

### 3.6 Validation gauntlet — `research/validation/` (the honesty layer; highest priority)

The cheap, pure-Rust honesty core (`statrs` + `rand` only). **Nothing is reported until it clears these.**

- **Deflated Sharpe Ratio (DSR)** — Probabilistic Sharpe benchmarked against the best you'd expect by luck after
  N trials, deflated for sample length + skew/kurtosis. Report only DSR > 0.95. (Bailey & López de Prado 2014.)
- **PBO via CSCV** — model-free probability the in-sample-best config is below-median out-of-sample; reuses a
  stored T×N per-config return matrix (free to compute since we store runs in SQLite).
- **Multiple-testing haircuts (Harvey-Liu)** — Bonferroni/Holm/BHY on swept conditions; the "raise the bar to
  t≈3.0" discipline.
- **Stationary block bootstrap (Politis-Romano)** — honest CIs on Sharpe and on *conditional/analog* forward
  returns under serial dependence. This is the right way to put error bars on "what followed the nearest analogs."
- **MinBTL gate** — reject swept results whose history is shorter than the minimum backtest length for N trials.
- All store provenance (inputs, assumptions, version) so a later operator can reproduce — the literal definition
  of glass-box, and the SR 11-7 "documented assumptions + outcomes analysis" charter.

---

## 4. Data gaps to fill (prerequisites)

| Gap | Fix | Priority |
|---|---|---|
| No total-return series for cash/bonds (defensive leg unmeasurable) | Ingest BIL/SHY/IEF/TLT (or synth T-bill total return from `^IRX`); register in `series_registry` | P0 for positioning |
| `economic_data` head-row-only (no history) | Convert to per-indicator time series (or pull FRED history) so growth/inflation RoC is computable | P0 for regime quad |
| Real yields table dormant / unwired | Wire ingest, register | P1 |
| `available_at` not modeled | Add the timestamp column + query discipline across L1/L2 | P0 (data hygiene) |
| Correlation snapshots not queryable as series | Expose as L1/registered | P2 |
| Breadth / VIX term structure / credit spreads absent | Add as feature-vector inputs as sourced | P2/P3 |

---

## 5. Honesty & governance (the most valuable transfer)

Map the institutional "don't-fool-yourself" machinery onto pftui (much of it already exists):

- **Outcomes analysis as a standing loop** (SR 11-7 #3; GDPNow's open error stats): score every environment/
  positioning call against realized outcomes, continuously — and *publish the engine's own error stats*. pftui's
  prediction/forecast scoring is the substrate; this is its macro extension.
- **Effective challenge, simulated for one operator** (SR 11-7; Bridgewater believability-weighting): keep the
  **blind / adversary / antithesis** layers structurally *excluded from consensus* (measurement, not voting) —
  the "separate the challenger from the builder" rule pftui already implements. Extend them to the environment read.
- **Track overrides** (SR 11-7): when the operator overrides the engine, log it and later analyze whether the
  override helped — "overrides indicate the model isn't performing as intended."
- **Require an economic rationale, not just a backtest** (AQR anti-data-mining): gate any regime/positioning
  signal on a stated *why* + out-of-sample evidence. Pairs with expectancy-gating.
- **Benchmark humility** (SR 11-7): agreement with a naive benchmark is *weak* evidence — don't take false comfort.
- **Reverse stress test** (EBA/PRA): a "what would have to be true for this book to blow up?" view, cheap to add.
- **Archetype-by-averaging, not cherry-picking** (Bridgewater): analogs reported as an ensemble + deviation.

---

## 6. Phasing

**Phase 0 — near-term wins (the on-ramp the operator already approved):**
- Total-return series for cash/bonds (§4). Persist strategy backtests to L3 + a report/CLI hook so reports cite
  cached, audited numbers. Port event_study's significance + era/regime splits onto the strategy engine output
  (surface N / anecdotal / CI automatically). Build the **cycle-clock command** (§3.4).

**Phase 1 — environment + analog (the core):**
- `environment_snapshots` (L2) with `available_at`. Mahalanobis-kNN + AnEn + MASS analog engine (§3.2).
  `pftui analytics environment|analog` CLI + JSON. The validation block-bootstrap CIs from day one (§3.6).

**Phase 2 — regime + cycle mapping:**
- Growth×inflation quad spine + BOCPD alerts (§3.3); regime-conditioned expectancy. Business-cycle dating
  (Bry-Boschan + CF + Hamilton filter) and the larger-cycle labels (§3.4). Wire regime/cycle tags into the
  environment vector and the analog output ("how the cycles mapped onto each past parallel").

**Phase 3 — positioning + the full gauntlet:**
- Positioning synthesizer (§3.5) + DSR/PBO/haircuts/MinBTL (§3.6). Positioning cards with honesty stats and the
  humility default. `positioning_runs` (L3) for drift tracking.

**Phase 4 — surfaces:**
- TUI "Environment" view (interactive drill-down). Thin narrative report section. Web dashboard later.

Each phase ships independently and is measurable; later phases are deferred until earlier ones prove their worth
(the AQR/MinBTL discipline applied to our own roadmap).

---

## 7. Tech stack & the sidecar boundary

- **Pure Rust covers the whole MVP:** `nalgebra`/`faer` (covariance/Cholesky/PCA), `rustfft`/`realfft` (MASS,
  FFT, Lomb-Scargle, Hilbert), `linfa-clustering`/`linfa-reduction` (GMM/k-means/PCA), `changepoint` (BOCPD),
  `statrs` + `rand` (DSR/PBO/haircuts/bootstrap). Prefer `faer`/`nalgebra` over `ndarray-linalg` to avoid a
  system-LAPACK build dependency.
- **Reserve a Python sidecar (PyO3 in-process, or subprocess + Arrow handoff) for exactly three genuine
  ecosystem gaps, all deferred/optional:** Markov-switching HMM, full Matrix-Profile (STUMPY), wavelets/EMD.
  The MVP needs none of them.
- **Every new table** gets a `db-catalog.toml` entry + passes `tests/schema_conformance.rs` in the same PR; L0/L1
  carry freshness SLAs, L2 `rebuildable=true`, L3 `append_only=true` (per DATA-ARCHITECTURE.md).

---

## 8. Anti-patterns to avoid (from the research)

- **Backtest theater:** a single in-sample sweep with no DSR/PBO/haircut is a lie; the gauntlet is not optional.
- **One cherry-picked analog** presented as destiny; report the ensemble + quality score + CIs.
- **Opaque ML regime scores** in place of the legible quad; black-box defeats the glass-box mandate.
- **HP-filter / EMD endpoints** for the *current* read (unreliable) — use CF / Hamilton-filter for "now."
- **Level-space distances** (non-stationary) — always returns/changes/spreads, z-scored, ADF-gated.
- **"We don't forecast" as a hidden bet** (All Weather/GMO): name the assumptions behind any "balanced" default.
- **Silent survivorship/lookahead** — the `available_at` discipline is the cheapest, highest-ROI safeguard.

---

### Appendix — primary sources (selected)
Analogs/methods: Delle Monache et al. 2013 (Analog Ensemble); Yeh et al. 2016 (Matrix Profile); Martínez et al.
2021 (k-NN TS). Validation: Bailey & López de Prado 2014 (Deflated Sharpe); Bailey-Borwein-LdP-Zhu 2016 (PBO/CSCV);
Harvey-Liu-Zhu 2016 (multiple testing); Politis-Romano 1994 (stationary bootstrap); López de Prado 2018 (AFML).
Regime/cycle: Hamilton 1989 (Markov-switching), 2018 (regression filter, "Why You Should Never Use the HP Filter");
Adams-MacKay 2007 (BOCPD); Bry-Boschan 1971; Christiano-Fitzgerald 2003; Torrence-Compo 1998 (wavelets); Peng 1994
(DFA). Institutional: Dalio *Principles for Navigating Big Debt Crises* + *Changing World Order*; AQR "It's Not
Data Mining" / "Contrarian Factor Timing is Deceptively Difficult"; 42 Macro / Hedgeye GIP-Quad; Two Sigma regime
GMM; Atlanta Fed GDPNow; NY Fed Staff Nowcast; Bridgewater "All Weather Story"; Fed SR 11-7 (Model Risk Management);
CCAR/DFAST; Solvency II ORSA; NAIC ESG / AIRG→GOES; cat-model blending (Guy Carpenter / IFoA).
