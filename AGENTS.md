# AGENTS.md — Agent Operator Guide

> The complete reference for AI agents operating pftui as their financial data layer.
>
> **First time?** Start with [ONBOARDING.md](ONBOARDING.md) — it walks through installation, portfolio setup, and the first week of operation.
>
> This file covers: analytics engine, CLI reference, data model, integration patterns, multi-timeframe agent architecture, and best practices.
>
> For code contribution, see [CLAUDE.md](CLAUDE.md).
> For architecture reference, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
> For AI operating model details, see [docs/AI-LAYER.md](docs/AI-LAYER.md).
> For the legacy always-on daemon deployment, see [docs/DAEMON.md](docs/DAEMON.md) (optional — not required, see below).

---

## How This System Actually Runs

pftui has **no resident process requirement**. The operating model is agent-session-driven: an agent (e.g. Claude Code running a report or analysis skill) invokes `pftui data refresh`, and the refresh **tail** fires every recurring mechanism the system depends on — prediction auto-scoring, recommendation forward-return scoring, retroactive forecast scoring, forecast-misalignment detection (probation tripwires), daily regime classification, alert evaluation, and a housekeeping summary (thesis sections past review, stale views). Whoever refreshes the data keeps every feedback loop closed; there is no scoring or detection that only a scheduler can trigger.

The `pftui system daemon` command still exists for hosts that want an always-on refresh loop, but it is **legacy/optional** — it adds cadence, not capability. Scheduled multi-agent setups (see [Multi-Timeframe Agent Architecture](#multi-timeframe-agent-architecture-advanced)) are likewise optional patterns layered on the same session-driven core.

---

## Table of Contents

0. [How This System Actually Runs](#how-this-system-actually-runs)
1. [Analytics Engine](#analytics-engine)
2. [CLI Reference](#cli-reference)
3. [Data Model](#data-model)
4. [Integration Patterns](#integration-patterns)
5. [Multi-Timeframe Agent Architecture](#multi-timeframe-agent-architecture-advanced)
6. [Best Practices](#best-practices)

---

## Analytics Engine

pftui's core is a multi-timeframe analytics engine operating across four layers:
LOW (hours→days), MEDIUM (weeks→months), HIGH (months→years), MACRO (years→decades).
Each layer uses different data, updates at different frequencies, and produces different signals.
Layers constrain downward and signal upward. Use `pftui analytics signals` for active cross-timeframe signals.

### Scenarios (`pftui journal scenario`)
Track macro scenarios with probability estimates. Each probability update is logged
to history for calibration. Signals track evidence for/against each scenario.

Scenario probabilities follow the **normalized scenario-set model** (see
`docs/ANALYTICS-SPEC.md` → Scenario Probability Semantics): active modeled
scenarios are mutually exclusive and collectively exhaustive after including an
explicit `Other / Unmodelled` residual row that pftui manages automatically.
Concretely:
- The `Other / Unmodelled` row is system-managed (`status = 'system-managed'`).
  It is seeded on first migration and its probability is recomputed as
  `100 - sum(active modeled scenarios)` after every CRUD on a modeled scenario.
  Do not create, update, or delete this row directly — `pftui journal scenario add`,
  `update`, and `remove` will reject the attempt.
- `pftui journal scenario add` and `pftui journal scenario update --probability` reject any
  write that would push the modeled (non-residual) sum above 100. Rebalance the
  set before writing.
- `pftui journal scenario list --json` returns a `normalized_set` block alongside the
  scenario rows with `modeled_sum`, `residual_probability`,
  `residual_materialized`, and `overfill_state` (one of `ok`, `overfilled`,
  `underfilled`, with a 0.05pp tolerance band). Treat `overfilled` as a
  data-quality warning, not as evidence that scenarios overlap.

### Thesis
Thesis tracking is maintained as narrative workflow files (`THESIS.md`) and journal notes.

### Convictions (`pftui journal conviction`)
Asset-level conviction scores (-5 to +5) over time. Append-only log — every
`set` creates a new row. Current conviction = latest row per symbol.
For negative scores, use `--score=-2`.

### Agent Signals (`pftui analytics signals`)
Cross-timeframe signal detection (alignment/divergence/transition) computed during
`pftui data refresh` and stored in `timeframe_signals`.

### Enrichment Substrate For Analyst Routines

Analyst routines should consume the derived enrichment tables before writing new predictions, scenario updates, or structured views. These tables are the machine-readable memory built from prior prediction outcomes, lessons, scenario links, source influence, and event annotations.

Native CLI surfaces for the enrichment substrate (all support `--json`):

| Command | What It Returns |
|---|---|
| `pftui analytics sources list [--type person\|framework\|institution\|outlet] [--json]` | `sources_registry` rows — canonical people/frameworks/institutions/outlets the substrate cites. |
| `pftui analytics sources set <canonical_id> --display-name <n> --type <t> [--aliases a,b] [--topics x,y] [--accuracy-rating r] [--framework-summary <s>] [--json]` | Upsert a source. |
| `pftui analytics sources remove <canonical_id> [--json]` | Remove a source. |
| `pftui analytics events list [--category <c>] [--since YYYY-MM-DD] [--asset <s>] [--json]` | `event_annotations` rows — operator-curated macro/market event catalogue. |
| `pftui analytics events add --event-date YYYY-MM-DD --category <c> --headline <h> [--detail <d>] [--magnitude 1..5] [--persistence transient\|days\|weeks\|structural] [--asset-impact a,b] [--related-scenario s1,s2] [--related-prediction 1,2] [--source <s>] [--notes <n>] [--json]` | Insert a new event annotation. |
| `pftui analytics fragments list [--type <t>] [--topic <t>] [--cluster <c>] [--for-claim "<text>"] [--json]` | `reasoning_fragments` rows. `--for-claim` runs a keyword-based cluster classifier and returns fragments reachable via `lesson_fragment_edges`. |
| `pftui analytics fragments show <canonical_id> [--json]` | One fragment + its lesson edges. |
| `pftui analytics strategy backtest --asset SYM --entry "<expr>" [--exit "hold 90d"\|"<expr>"] [--stop-loss PCT] [--take-profit PCT] [--trailing-stop PCT] [--commission PCT] [--slippage PCT] [--next-bar-fill] [--vol-target PCT] [--vol-window N] [--max-leverage X] [--from D] [--to D] [--limit N] [--json]` | Backtest a user-defined trade rule against full `price_history`. `--entry` is a boolean expression in the strategy DSL; its rising edge (false→true) opens one position (no pyramiding), closed by the FIRST of: `--exit` (fixed `hold Nd`, default 90d, or an exit-condition expression), `--stop-loss PCT`, `--take-profit PCT`, or `--trailing-stop PCT` — the risk exits are checked **intra-bar against the daily high/low** (downside before upside, conservatively). **Execution realism**: `--commission PCT` (per-side, charged entry+exit) and `--slippage PCT` (per-side; entries fill higher, exits lower) deflate each trade's net return; `--next-bar-fill` fills at the NEXT bar's close instead of the signal bar's close, removing same-bar look-ahead. Defaults are cost-free/same-bar (unchanged behaviour). The `costs` block echoes what was applied. Reports trades, win rate, mean/median/best/worst, compounded total + CAGR + max-DD + time-in-market vs buy-and-hold, a **tearsheet** (profit factor, expectancy, payoff, Sortino, Calmar, avg win/loss, max consecutive losses), a **drawdown-path risk** block (when n≥20): `drawdown_metrics` = CDaR-90/95 (mean depth of the worst-decile/worst-5% drawdowns — the tail of the DRAWDOWN distribution, the path-coherent complement to Calmar's single worst point), Ulcer Index + Martin ratio (duration-aware), and Omega ratio (distribution shape, τ=0), a **Kelly sizing** block (when n≥20): `kelly` = growth-optimal leverage from the realized per-trade edge — `full_kelly_leverage` (μ/σ²), `half_kelly_leverage`, `uncertainty_adjusted_leverage` (full Kelly on the lower-1SE edge, so thin/noisy records size smaller), and a `recommended_leverage` = half-Kelly on the uncertainty-adjusted edge **capped so leverage·CDaR-95 ≤ a 25% drawdown budget** (`binding_constraint`: edge / drawdown-cap / no-edge), an **exit-reason breakdown** (rule/stop/target/trailing), a `validation` honesty block (PSR vs zero when n≥10, bootstrap CI), and a `monte_carlo` block (when n≥20): bootstrap-resamples the per-trade returns over 5000 deterministic paths → terminal-return p5/p50/p95, max-drawdown median/p95/p99 (the realistic worst-case the single historical curve hides), and P(loss). All computed on NET (post-cost) returns. **Position sizing**: `--vol-target PCT` adds a risk-normalized equity curve weighting each trade by `clip(vol_target / trailing_realized_vol_at_entry, 0, --max-leverage)` (default 3×, trailing window `--vol-window`, default 30 bars) — a `sizing` block reports sized total/CAGR/maxDD/Sortino + the leverage range actually used (makes BTC-vs-gold comparable across their ~4× vol gap; opt-in, absent by default). A referenced symbol with no history now **errors loudly** (was silent zero-trades); tickers with `^`/`=`/`-` need their alias in expressions. Stateless. Returns are price-ratio statistics, not money. |
| `pftui analytics strategy segment --asset SYM --when "<expr>" [--from D] [--to D] [--json]` | Partition the asset's forward 1-bar returns by a regime mask: in-state (mask true) vs out-of-state. Per side: n days, share of bars, contiguous episodes, mean daily return, annualized, up-day share — plus the buy-and-hold benchmark. |
| `pftui analytics strategy compare --asset SYM --when "<expr>" --when-label L --vs "<expr>" --vs-label L [--from D] [--to D] [--json]` | Compare forward returns under two INDEPENDENT regime masks (not complements) — e.g. hiking vs cutting. Rate cycles are ordinary MA crossings on a yield series: `us10y`/`^TNX`, `fedfunds`/`^IRX` are alias-resolved symbols. |
| `pftui analytics strategy explain --asset SYM --entry "<expr>" [--json]` | Parse + resolve an expression WITHOUT simulating: reports value kind (numeric/boolean), bars resolved vs master length, first/last resolved date, and firing count. Use to validate an expression and check data coverage before a full run. DSL: `close`/`open`/`high`/`low`/`volume`, `close(SYM)`, `sma`/`ema`/`rsi`, **OHLC indicators `atr(p)`/`cci(p)`/`williams_r(p)`/`roc(p)`/`fisher(p)` (Ehlers Fisher Transform — sharp turning-point oscillator)/`stoch_k(k,d)`/`stoch_d(k,d)`/`adx(p)`/`plus_di(p)`/`minus_di(p)`/`supertrend(p,mult)` (ATR-band trailing-stop LINE)/`supertrend_dir(p,mult)` (regime: +1 up / −1 down)/`macd(f,s,sig)`(=histogram; also `macd_line`/`macd_signal`)/`bb_upper`/`bb_lower`/`bb_mid`/`bb_pct(p,mult)`/`obv()`/`mfi(p)`** — each takes an optional leading symbol (`atr(BTC,14)`, `adx(gold,14)`) and resolves over that symbol's full OHLCV bars (weekly/monthly buckets aggregate high=max/low=min/close=last/volume=sum), **window fns `highest(x,n)`/`lowest(x,n)`/`ago(x,n)`/`pct_change(x,n)`/`abs(x)`** (highest/lowest INCLUDE the current bar — a prior-N high is `ago(highest(high,N),1)`), `@weekly`/`@monthly`, `> < >= <= ==`, `crosses_above`/`crosses_below`, `and`/`or`/`not`. NOTE: `strategy backtest` output carries a `validation` block (PSR vs zero when n≥10, bootstrap CI, anecdotal flag) + tearsheet. |
| `pftui analytics strategy sweep --asset SYM --entry "<expr with $P>" --values "v1,v2,…" [--exit …] [--from D] [--to D] [--json]` | **Parameter sweep with multiple-testing correction.** Substitutes each `--values` entry for the `$P` placeholder in the entry rule (e.g. `rsi(14) < $P` swept over `20,25,30,35,40`, or `rsi($P) < 35`), backtests each, and reports a per-config table (trades, win%, mean, profit factor, per-trade Sharpe) PLUS the **Deflated Sharpe Ratio** judging the BEST config after accounting for selection over N trials. The DSR (López de Prado) deflates the in-sample-best Sharpe by the expected-max-by-luck across the grid; `passes` (DSR>95%) = the edge survives selection, else it's flagged likely in-sample overfitting. The overfitting guard a single backtest can't give — answers "is the best parameter real or did I just pick the luckiest of N?". Best config marked ◀. Needs ≥2 values; the best must have ≥10 trades. |
| `pftui analytics strategy walkforward --asset SYM --entry "<expr with $P>" --values "v1,v2,…" [--exit …] [--folds 4] [--json]` | **Walk-forward optimization** — the out-of-sample complement to `sweep`. Splits the timeline into `--folds` train/test segments; each fold optimizes `$P` (best in-sample Sharpe) on its train segment, then measures that chosen value on the NEXT held-out (OOS) segment. Reports per-fold (test window, best $P, IS Sharpe, OOS Sharpe) + the **Walk-Forward Efficiency** = avg OOS Sharpe / avg in-sample-best Sharpe: WFE≥0.5 ROBUST (generalizes), 0–0.5 FRAGILE (partly curve-fit), ≤0 FAILS-OOS (the IS-best param has no forward edge — overfit). The "does the optimization generalize or is it just curve-fit?" read even a deflated single sweep can't fully give. Warmup-correct (full-history indicators; trades partitioned by date). Live: BTC `rsi(14)<$P` → WFE 0.46 FRAGILE (the recent fold even goes OOS-negative); `rsi($P)<35` period → WFE 0.85 ROBUST. |
| `pftui analytics technicals indicators SYM [--json]` | **Full standard-indicator panel** computed on the fly — Momentum (RSI, Stochastic %K/%D, Williams %R, CCI, ROC), Trend (ADX/DMI +DI/−DI, MACD histogram, SMA50/200), Volume (OBV 20-bar trend, MFI), Volatility (ATR, Bollinger %b) — plus a bull/bear **scorecard** (counts canonical overbought/oversold + trend + cross signals → net BULLISH/BEARISH/mixed). The "all the TA for a symbol at a glance" view; complements the cached `technicals` snapshot and the `structure`/`cyber` subcommands. |
| `pftui analytics environment current [--json]` | The macro **environment feature vector** for today (Environment Engine, docs/ENVIRONMENT-ENGINE.md): each of ~12 features (20d return + realized vol for SPX/gold/oil/DXY, 10y level + 20d change, 10y−3m curve, VIX) rendered as an **expanding-window z-score** (no look-ahead) — how far each reading sits from its own history in standard deviations. The stationary state vector the analog engine matches on. |
| `pftui analytics analog --asset SYM [--horizon 90] [--k 25] [--exclude-days 90] [--json]` | Closest **historic analogs** to today's macro environment via covariance-whitened (Mahalanobis) distance over the environment vector, then the target asset's **forward-return distribution** following those analog dates (median/mean/p25/p75/up-rate + a block-bootstrap 90% CI on the mean). Each analog is tagged with the growth×inflation **regime quad** it occurred in. Analogs are de-clustered to ≥180-day-apart distinct episodes. Honestly reports `n_with_forward`/`k` when a young target (e.g. BTC from 2014) has data for only some analogs — treat thin coverage as indicative, not robust. The macro vector spans ~2003+ (DXY-limited). |
| `pftui analytics positioning --asset SYM [--horizon 90] [--k 25] [--json]` | Synthesized **positioning** (Environment Engine capstone): composes the measured analog forward-return distribution (50%), the regime-quad per-asset lean (30%), and the cycle clock (20%) into one auditable BULLISH/BEARISH/NEUTRAL stance with a confidence. Every driver shows its score, weight, and reason; the analog driver is discounted for thin samples / zero-straddling CIs. Humility default: weak evidence caps confidence and the `honesty_note` says so, and the single-regime backtest limitation is always stated. Foregrounds the measured anchor (n analogs, median forward, 90% CI). Also emits a `supplementary_measurements` block (CONTEXT, not in the weighted blend): the asset's **Hurst regime**, **CUSUM regime-break** (last drift shift), **anchored-VWAP basis** (price vs the cycle-low volume-weighted cost basis), and — for BTC — the **accumulation-clock stance**. One auditable view combining analog + regime + cycle + the standalone measurement primitives. |
| `pftui analytics tail-risk --asset SYM [--lookback N] [--threshold 95] [--json]` | **Extreme-Value-Theory tail risk** (Peaks-Over-Threshold + Generalized Pareto). Gaussian/historical VaR understates crash depth for fat-tailed assets; this fits a GPD to the LEFT TAIL of the asset's daily returns and reports the shape ξ (tail-fatness: >0 = power-law, fatter than normal), fat-tail-aware **1-day VaR at 95/99/99.9%** (the 99.9% is the key extrapolation historical data can't estimate), **Expected Shortfall** (mean loss beyond the 99% VaR), and the historical VaR/ES alongside for cross-check. Closed-form **probability-weighted-moments** GPD fit (auditable, no opaque optimizer; far less shape-biased than plain method-of-moments; valid for ξ<1, flagged `reliable:false` as ξ→0.9 / too few exceedances). VaR below the threshold quantile falls back to the empirical quantile (POT is only valid above it). Needs ≥101 bars. Live: BTC ξ≈0.14 (moderate), 99% VaR ~9.7%, 99.9% VaR ~18%; gold/SPY smaller. Answers "how deep can a cycle-low/crash drawdown realistically get." |
| `pftui analytics tail-dependence --asset X --vs Y [--q 5] [--json]` | **Tail dependence** — do two assets co-crash? Correlation hides the failure mode that matters: assets can have modest correlation yet plunge together in a crash. Reports Pearson + Kendall τ, the **empirical lower/upper tail-dependence** λ_L/λ_U at the `--q` tail quantile (model-free: share of days both assets sit in their joint bottom/top q-tail, normalized), and the **Clayton-copula** λ_L = 2^(−1/α) via τ inversion (α=2τ/(1−τ)). The companion to `tail-risk` (univariate tails) for the JOINT picture. Needs ≥100 common dates. Live: BTC↔gold λ_L≈0.05 (WEAK, at the independence floor — the two stores of value are tail-independent, diversification holds in crises); BTC↔SPY λ_L≈0.22 (MODERATE — BTC partly crashes with equities); gold↔SPY λ_L≈0.10. |
| `pftui analytics avwap --asset SYM [--anchor cycle-low\|halving\|ath] [--anchor-date D] [--json]` | **Anchored VWAP** from a cycle low (default), halving, or ATH — the volume-weighted average cost-basis of everyone who bought since the anchor, and the price's position vs it. Anchored to the last cycle low: price ABOVE = the average post-low buyer is in profit (basis defended, accumulation intact); a break BELOW = that buyer is underwater (accumulation leg in question). `cycle-low` resolves to the cycle clock's verified low for BTC/gold (else trailing-2y low); `halving` is BTC-only. **Volume-quality honest**: if any bar in the window lacks volume it falls back to a FLAT-weight anchored AVERAGE price and reports `quality: flat-weight-degraded` (never a silent fake VWAP); typical price falls back to close when high/low absent. Decimal math. Live: BTC ~20% BELOW its 2022-11-21 cycle-low AVWAP (volume-weighted, 99.8% coverage — the avg post-low buyer is well underwater); gold above its cycle-low basis. Volume-weighted skips no-volume bars; degrades to flat-weight only when coverage <50%. |
| `pftui analytics hurst --asset SYM [--lookback N] [--json]` | **Hurst exponent (R/S)** — a trending-vs-mean-reverting regime gauge over the asset's LOG returns (distinct from the cycle-COUNTING Hurst in the cycle engine). H>0.55 persistent/trending (trend-following has an edge, accumulate dips and ride); H≈0.5 random walk (no autocorrelation edge — trend signals are noise); H<0.45 anti-persistent/mean-reverting (fade extremes). Rescaled-Range over window sizes {8…512}, **Anis-Lloyd/Peters bias-corrected** (the naive R/S slope runs high on finite samples — both `h` and `h_uncorrected` are reported), **cross-validated with an independent DFA-1 estimator** (`dfa_alpha` ≈ H; DFA tolerates non-stationarity/trends far better than R/S, so `agreement` between them = a robust regime read while a DIVERGE flags a trend likely biasing R/S). Needs ≥64 log returns. Live (daily returns, near random-walk; R/S and DFA agree within ~0.01): BTC H≈0.52/α≈0.53, gold ≈0.47/0.47, SPY ≈0.45/0.45, us10y ≈0.52 — daily trend-timing has little edge; persistence lives at the cycle/multi-year scale. |
| `pftui analytics regime-break --asset SYM [--lookback N] [--k 0.5] [--h 5] [--json]` | **Regime-break (CUSUM change-point)** — Page's two-sided cumulative-sum test on daily returns detects when the return DRIFT structurally shifted, distinguishing a healthy dip inside an intact trend from "the trend just broke" (the key call for a dip-accumulator). Reports past change-points (date + up-shift/down-shift), the most recent one (`last_change`), and how close a fresh break is to firing now (`building_up_pct`/`building_down_pct`, as a fraction of the threshold). `k` = slack (σ multiples, default 0.5 → ~1σ shifts), `h` = alarm threshold (default 5σ). Needs ≥30 returns. Detected breaks map to real history (BTC: 2021 bull start, May-2021 crash, 2022 bear legs, Nov-2022 FTX bottom, 2023 recovery, the 2026-01 down-shift). |
| `pftui analytics risk-dashboard --asset SYM [--vs PARTNER] [--json]` | **Risk-side capstone** (the analogue of `positioning` for risk). Composes the measured risk primitives into one auditable view: annualized **vol** + max **drawdown** + current %-from-ATH; **EVT tail-risk** (ξ tail-fatness, 1d VaR 99/99.9%, ES99); the **Hurst/DFA regime** + CUSUM **drift-break** read; **drawdown-path risk** (CDaR-90/95 = mean depth of the worst-decile/worst-5% drawdowns — the tail of the DRAWDOWN distribution, vs max-DD's single point — plus duration-aware Ulcer Index + distribution-shape Omega ratio); and **co-crash tail-dependence** vs a partner (default gold, or BTC for gold) — plus a one-line composite risk read. Each line is the same computation as its dedicated command (EVT/hurst/regime-break/tail-dependence/risk/drawdown_metrics). Live: BTC 55%/yr vol, −83% max DD, ξ 0.14, 99.9% VaR 18%, CDaR-95 78%, Ulcer 43%, random-walk regime, co-crash vs gold WEAK (diversification holds). Needs ≥101 bars. |
| `pftui analytics basket weights --assets A,B,C [--method equal\|inverse-vol\|risk-parity] [--lookback N] [--json]` | **Risk-aware portfolio allocator** over a basket's common price history (read-only; no portfolio data). Three schemes: `equal` (1/N baseline), `inverse-vol` (`w_i ∝ 1/σ_i` — equalizes standalone risk, ignores correlation), `risk-parity` (ERC — equal risk CONTRIBUTION via the Maillard fixed point, using the full covariance so each asset adds the same share of portfolio variance). Reports each asset's weight + annualized vol + risk-contribution, the portfolio vol, and the diversification ratio (`Σwᵢσᵢ/σ_p` ≥1; higher = more benefit captured). `--lookback` windows the common axis (0 = all); needs ≥21 common dates. Tickers with `^`/`=`/`-` use their alias. Live: BTC/gold/SPY risk-parity → BTC 10.8% / gold 47.5% / SPY 41.7% (all RC=33.3%), port vol 14.2%/yr, div-ratio 1.57; BTC/gold inverse-vol RC 50/50 confirms their near-zero correlation. Directly serves capital-split decisions across the held basket. |
| `pftui journal prediction preflight --claim "<text>" [--symbol <s>] [--timeframe <tf>] [--conviction <c>] [--layer <l>] [--topic <t>] [--inline] [--json]` | Cross-table pre-flight check for a draft prediction. Classifies the claim into a `cluster_key`, then returns the matched `reasoning_fragments`, the `calibration_adjustments` row for (layer, topic, conviction), the top-3 similar past `user_predictions` in the cluster with scored outcomes, the highest-share co-failing cluster from `failure_correlations`, the `scenario_prediction_links` distribution for matching scenarios, the most-similar `prediction_falsification_rules` claim, and a 0..=100 `preflight_score` (higher = riskier). `--inline` emits a one-line `[preflight] ...` block for embedding into the prediction reasoning. |
| `pftui journal prediction adversary --claim "<text>" [--symbol <s>] [--timeframe <tf>] [--conviction <c>] [--layer <l>] [--json]` | Compose the write-time deterministic "case against" the draft prediction. Classifies the claim into a `cluster_key`, then emits a single JSON object with `anti_pattern_arguments` (anti-pattern `reasoning_fragments` reachable from the cluster), `cofailure_warnings` (top-3 lessons from the highest co-failing cluster per `failure_correlations`), and `falsification_triggers` (derived conditions under which the claim clearly fails). No live LLM call — fully data-driven so the same claim against the same substrate yields the same view. Companion to `preflight`. Pretty mode renders a compact bullet list. |
| `pftui journal prediction rescore-audit [--apply-high-confidence] [--json]` | Audit legacy LLM-scored outcomes (correct/partial/wrong without `auto-scored:` provenance) against the mechanical falsification scorer: each rule (stored row, or claim/resolution_criteria re-parsed through the falsify grammar) is re-evaluated against `price_history` and classified agree / agree-partial / disagree (generous vs harsh) / unparseable / window-open / unevaluable. Reports overall + by-layer agreement, the recorded-vs-mechanical confusion matrix, the generosity measure, a rule-quality partition (legacy rules encoding negated-claim failure conditions, garbled thresholds, or the wrong measurand are flagged `rule_suspect_flags` and never auto-corrected), and every disagreement with the deciding bar's date+close. Dry by default; `--apply-high-confidence` flips a disagreeing outcome only when the rule parsed HIGH confidence, the deciding close is >1% from threshold, the series is outside the #729/#730/#735 corruption-repair windows, and no rule-defect flag is present — each flip APPENDS `rescore-audit <date>: outcome corrected old→new, evidence: ...` to `score_notes` (original outcome preserved). Rebuild calibration afterwards: `pftui analytics calibration-matrix rebuild --since 365`. |
| `pftui analytics calibration-adjustments [--layer <l>] [--topic <t>] [--conviction <c>] [--json]` | `calibration_adjustments` — per-(layer, topic, conviction) discount/boost factors with `apply_note`. |
| `pftui analytics failures correlations [--cluster <c>] [--min-share 0.5] [--json]` | `failure_correlations` — pairwise co-failure share between lesson clusters. |
| `pftui analytics clusters list [--json]` | Distinct `cluster_key` values present on `prediction_lessons` with lesson counts. |
| `pftui analytics clusters stats [--json]` | Lesson count plus the number of `user_predictions` referencing each cluster via `lessons_applied`. |
| `pftui analytics falsifications [--rule-type <t>] [--auto-eligible] [--for-prediction <id>] [--json]` | `prediction_falsification_rules` filtered by rule type, auto-eligibility, or owning prediction. |
| `pftui analytics thesis-chains list [--state <s>] [--node <n>] [--json]` | `thesis_dependencies` — cross-asset if-then chains. Filter by `current_state` (`confirmed`/`open`/`disconfirmed`/`stale`) or by an antecedent/consequent node id or symbol substring. |
| `pftui analytics thesis-chains show <id> [--json]` | One chain with its source-lesson ids and thesis section ids. |
| `pftui analytics thesis-chains validate <id> [--as-of YYYY-MM-DD] [--json]` | Evaluate the chain's antecedent and consequent against recent prices; persists a new `current_state` and `last_validated_at`. Predicates now accept simple thresholds (`SYMBOL {>,>=,<,<=,==,!=} value`), range thresholds (`BTC between 90000 and 100000`, `DXY in [102, 105]`), and derived metrics resolved from `real_yields_history` (`real_yield`, `breakeven_10y`, `dxy_spread`). Anything else still leaves the chain `open` with note "not yet evaluable". |
| `pftui analytics thesis-chains extract [--from-thesis] [--from-lessons] [--from-messages] [--since <window>] [--dry-run] [--apply] [--json]` | Heuristic backfill — scans `thesis.content`, `prediction_lessons.why_wrong`, and recent `agent_messages.content` for implication phrases (`if X then Y`, `when X, Y`, `X implies Y`, `X -> Y`, `X drives Y`, `X dampens Y`, `X contradicts Y`, `X is contingent on Y`) and proposes new chains. Defaults to all three sources + `--dry-run`. `--apply` writes via the existing insert path with `conviction=medium`; deduplication keys on `(lower(antecedent_text), relation, lower(consequent_text))` so re-runs are idempotent. JSON output is `{proposed, applied, deduped, by_source:{thesis,lessons,messages}, chains:[...]}`. |
| `pftui analytics thesis-chains add --antecedent "<text>" --consequent "<text>" --relation <r> [--conviction <c>] [--antecedent-id <id>] [--consequent-id <id>] [--evidence-count <n>] [--source-lesson-ids 1,2,3] [--source-thesis-sections slug-a,slug-b] [--json]` | Manually author a chain. |
| `pftui journal replies list [--report-date <d>] [--asset <a>] [--decision-type <t>] [--json]` | `operator_replies` — structured per-decision replies the operator wrote against a report. |
| `pftui journal replies add --report-date <d> --decision-type <t> --response-class <c> --raw-content <text> [--asset <a>] [--reply-date <d>] [--conviction-implied <c>] [--horizon <h>] [--reasoning <r>] [--journal-id <id>] [--json]` | Record a new operator reply. |

Use these CLIs in routine prompts instead of raw `sqlite3` calls. The CLIs handle schema-missing-on-fresh-installs gracefully (empty lists rather than errors).

| Table | Routine Use |
|---|---|
| `calibration_adjustments` | Per-layer confidence correction by topic and conviction band. If `adjustment_direction='discount'`, subtract `adjustment_pp` before writing prediction confidence. |
| `reasoning_fragments` + `lesson_fragment_edges` | Reusable lesson fragments for known claim clusters. Cite 2-3 `canonical_id` values when a new prediction uses a learned fragment. |
| `prediction_falsification_rules` | Examples of observable thresholds and evaluation windows. Use them to make new predictions mechanically scorable when possible. |
| `scenario_prediction_links` | Historical scenario context at prediction-write time. Check whether prior calls around a scenario resolved correctly before raising confidence. |
| `failure_correlations` | Cross-cluster failure warnings for synthesis. If a claim cluster often co-fails with another, sanity-check the shared assumption. |
| `sources_registry` | Named source and framework influence ledger. MACRO should explicitly reference high-influence frameworks such as Dixon, Dalio, and Fourth Turning when they shape a call. |
| `event_annotations` | Canonical structured timeline. Prefer this for regime context around a date before fuzzy-searching `news_cache`, notes, or journal rows. |
| `calibration_matrix` | Realized prediction rates by layer/topic/conviction. Use as sample-size context, not proof of precision. |
| `thesis_dependencies` | Cross-asset if-then chains. When a draft prediction's symbol matches a chain antecedent or consequent, `pftui journal prediction preflight` surfaces the chain and its `current_state`. The private daily report now auto-wires the chain block: `BuildContext::load` populates `private_thesis_chains` from `thesis_dependencies::list`, and the `private_macro_thesis_chains` section (placed immediately after `private_macro_context` in the private plan) calls `crate::report::sections::thesis_chains_macro::render_thesis_chains_block` to surface "active confirmed chains" / "newly disconfirmed chains" inline. Public mode never invokes this section — chains can carry portfolio-framed text. To seed the table without manual `add` calls, use `pftui analytics thesis-chains extract [--apply]` (heuristic backfill — see CLI table above). |

Contract for predictions:
- Determine the prediction topic and conviction band first.
- Read the matching `calibration_adjustments` row for the analyst's layer.
- Apply any confidence discount before saving.
- Attach relevant lesson IDs or reasoning-fragment `canonical_id` values in the prediction reasoning.
- Prefer concrete falsification criteria with dates and thresholds.
- **Run `pftui journal prediction preflight --json` BEFORE every `prediction add`.** The preflight surfaces the cluster classification, applicable calibration discount, anti-pattern fragments, top-3 similar past predictions, and the highest-share co-failing cluster. By default `prediction add` auto-runs the preflight and ABORTS the save when `preflight_score >= 50` unless `--accept-preflight` is also passed. Routines must read the preflight findings, then either revise the draft or invoke `prediction add --accept-preflight --inline` so the substrate considered at write time becomes part of the prediction's permanent record (appended to `resolution_criteria`). Pass `--skip-preflight` only for emergency calls where the substrate is not available.
- **Run `pftui journal prediction adversary --json` BEFORE every `prediction add` too**, then pass `--with-adversary` on the `add` call. The adversary composer is deterministic and complementary to the preflight: it builds the "case against" the claim from three substrate sources — anti-pattern `reasoning_fragments` reachable from the cluster, the top-3 lessons of the highest co-failing cluster, and derived falsification triggers. `--with-adversary` persists the computed view to a new `adversary_views` row (`prediction_id` FK → `user_predictions.id`) and appends a compact `[adversary] cluster=...; anti_patterns=[...]; co_failing=...; n_falsification_triggers=N` summary line to the prediction's `resolution_criteria` alongside the preflight inline block. The view is part of the prediction's permanent record and can be retrieved via `crate::db::adversary_views::list_for_prediction(conn, prediction_id)`.

#### Scenario-Conditional Backtest & Regime Presets

`pftui analytics backtest scenario` filters `scenario_prediction_links` by per-scenario probability bands, joins to `user_predictions`, and reports the cohort's hit rate. `pftui analytics backtest layer-bias` returns the same shape as the calibration matrix but conditioned on the regime — surfacing rows like "LOW layer commodities hit rate was 65% during stagflation-iran-cool but 30% during crisis".

Regime presets (priority order — the first matching preset wins; `neutral` is recorded if none match):

| Preset | Exact filter | Intuition |
|---|---|---|
| `stagflation-iran-cool` | `Inflation Spike ≥ 85` AND `Iran-US ≤ 20` | Sticky inflation pressure with no kinetic escalation. |
| `crisis` | `Hard Recession ≥ 40` AND `Iran-US ≥ 30` | Coincident growth break and geopolitical stress. |
| `risk-on` | `Risk-On ≥ 40` | Liquidity-positive regime; soft landing odds dominant. |

Each regime preset is also addressable via individual `--inflation-min/--inflation-max/--recession-min/--recession-max/--iran-min/--iran-max/--risk-on-min/--risk-on-max` flags. Flags **stack on top of** any preset chosen via `--regime`, so analysts can use a preset as a baseline and tighten or loosen any single band.

The `regime_history` table records one classification per UTC date with the full `scenario_state_json` snapshot. Population is automatic during `pftui data refresh` (idempotent via `INSERT ... ON CONFLICT(date) DO UPDATE`). Analysts and report writers should treat `regime_history` as the authoritative regime timeline for any per-day conditional analysis.

---

## CLI Reference

### Report Assembly

| Command | What It Does |
|---|---|
| `pftui report build daily [--mode public\|private\|both] [--date YYYY-MM-DD] [--out-dir <path>] [--dry-run] [--json]` | Assemble the daily intelligence report markdown by calling every registered section renderer in canonical order. `--mode public` writes the public newsletter to `~/pftui/reports/daily-<DATE>.md`; `--mode private` writes the private operator decision document to `<tmp>/pftui-private-<DATE>.md`; `--mode both` (default) writes both. `--out-dir` overrides BOTH destinations. `--dry-run` prints the section plan, data availability summary, output paths, and privacy-audit status without writing files. Public-mode output is enforced through `audit_public_markdown` (no personal portfolio framing) before the file is written. |

### Charts

| Command | What It Returns |
|---|---|
| `pftui report chart stacked-bar --from-db portfolio [--out allocation.svg] [--format svg\|png\|ascii] [--json]` | Native portfolio-allocation chart using the report palette; SVG is the primary output, PNG is rendered via `resvg`, ASCII is terminal-friendly |
| `pftui report chart stacked-bar --from-json segments.json [--format svg\|png\|ascii] [--json]` | Render a stacked bar from JSON `{ "segments": [{"label": "...", "value": 12.3, "color": "#..."}] }` or a bare segment array |
| `pftui report chart prob-bar --from-db "Scenario Name" [--out scenario.svg] [--format svg\|png\|ascii] [--json]` | Native scenario probability bar with 7-day-prior ghost and delta pulled from `scenarios` + `scenario_history` |
| `pftui report chart prob-bar --from-json scenario.json [--format svg\|png\|ascii] [--json]` | Render a probability bar from JSON `{ "name": "...", "current": 88, "prior_7d": 80, "color": "bear" }` |
| `pftui report chart drift-bar --from-db BTC [--out drift.svg] [--format svg\|png\|ascii] [--json]` | Native allocation drift bar using allocation targets plus current portfolio allocation |
| `pftui report chart drift-bar --from-json drift.json [--format svg\|png\|ascii] [--json]` | Render a drift bar from JSON `{ "symbol": "BTC", "target_pct": 25.0, "actual_pct": 31.5, "band_pct": 2.0 }` |
| `pftui report chart what-changed-strip --from-json deltas.json [--format svg\|png\|ascii] [--json]` | Render a since-last-report delta strip from JSON `{ "deltas": [{"label": "BTC", "delta_str": "+3.2%", "direction": "bull"}] }` or a bare delta array |
| `pftui report chart open-predictions-table --from-db pending [--format html\|ascii] [--json]` | Native open-predictions due table from pending `user_predictions` rows with target dates |
| `pftui report chart open-predictions-table --from-json predictions.json --format html [--json]` | Render an HTML-native due table from JSON `{ "predictions": [{"asset": "SPY", "claim": "...", "days_remaining": 1, "confidence": 0.40}] }` or a bare prediction array |
| `pftui report chart outlook-arrows --from-db BTC [--format svg\|png\|ascii] [--json]` | Native horizon outlook arrows using current LOW/MEDIUM/HIGH `analyst_views` as days/weeks/months |
| `pftui report chart outlook-arrows --from-json outlook.json [--format svg\|png\|ascii] [--json]` | Render outlook arrows from JSON `{ "days": ["flat", "medium"], "weeks": ["up", "medium"], "months": ["up_strong", "high"] }` |
| `pftui report chart factor-exposure --from-json factors.json [--format svg\|png\|ascii] [--json]` | Render factor exposure bars from JSON `{ "factors": [{"name": "Inflation Spike", "exposure_pct": 51.0, "direction": "bull", "prob_pct": 88.0}] }` or a bare factor array |
| `pftui report chart conviction-grid --from-db all [--format svg\|png\|ascii] [--json]` | Native multi-timeframe conviction grid from current LOW/MEDIUM/HIGH/MACRO `analyst_views`; pass a symbol instead of `all` for one asset |
| `pftui report chart conviction-grid --from-json rows.json [--format svg\|png\|ascii] [--json]` | Render a conviction grid from JSON `{ "rows": [{"symbol": "BTC", "low": 1, "medium": 1, "high": 3, "macro": 2}] }` or a bare row array |
| `pftui report chart mismatch-card --from-json mismatch.json --format html [--json]` | Render the HTML-native Skylar-vs-analyst mismatch card from JSON `{ "asset": "BTC", "skylar_view": "...", "analyst_summary": "convergent-bull", "analyst_avg_conviction": 1.75 }` |
| `pftui report chart decision-card --from-json decision.json --format html [--json]` | Render the HTML-native operator question card from JSON `{ "question": "...", "context_lines": ["..."], "recommendation": "...", "response_format": ["yes", "no"], "urgency": "high" }`. The private daily-report renderer in `src/report/sections/private_decisions_pending.rs` emits a `## Decisions Pending — Your Reply Requested` section composed of `{decision_card(...)}` placeholders ordered by urgency (high → normal → low) then gap size; cards are derived from `classify_convergence` + drift bands (ADD/TRIM/HOLD), insufficient-views (stale targets), the Skylar-vs-analyst gap (mismatch), and binary catalysts. Allowed response tokens: `yes`, `yes-if`, `no`, `wait`, `other`. |
| `pftui report chart regime-quadrant --from-json regime.json [--format svg\|png\|ascii] [--json]` | Render the growth-vs-inflation macro regime quadrant from JSON `{ "growth": -0.55, "inflation": 0.7, "trail": [[-0.2, 0.4], [-0.3, 0.5]] }` |
| `pftui report chart conviction-trajectory --from-db BTC [--format svg\|png\|ascii] [--json]` | Native per-asset analyst conviction sparkline from `analyst_view_history`; append a window token like `BTC 14d` to override the default 30 days |
| `pftui report chart conviction-trajectory --from-json trajectory.json [--format svg\|png\|ascii] [--json]` | Render a conviction trajectory from JSON `{ "symbol": "Gold", "layer_series": { "LOW": [["d1", 4], ["d2", 3]], "MED": [["d1", 2]] } }` |
| `pftui report chart calibration-reliability --from-db 90d [--format svg\|png\|ascii] [--json]` | Native reliability chart from scored `user_predictions`, grouped by layer and conviction band, with sample size, 1σ uncertainty, and low-sample markers |
| `pftui report chart calibration-reliability --from-json calibration.json [--format svg\|png\|ascii] [--json]` | Render a reliability chart from the nested `by_layer` shape emitted by `pftui analytics calibration --by-layer --json` |
| `pftui report chart analyst-convergence-card --from-db "Gold 30d" --format html [--json]` | Native HTML evidence card from `analyst_view_history` convergence reports; append `all` for an unbounded window |
| `pftui report chart analyst-convergence-card --from-json convergence.json --format html [--json]` | Render the HTML-native convergence card from JSON `{ "asset": "Gold", "views": [{"analyst": "analyst-low", "conviction": 3, "reasoning_summary": "..."}], "summary": "strong-convergent-bull" }` |

### Portfolio State

| Command | What It Returns |
|---|---|
| `pftui portfolio brief --json` | Complete portfolio snapshot — positions, allocations, movers, technicals, macro |
| `pftui portfolio value --json` | Total value with category breakdown and daily change |
| `pftui portfolio summary --json` | Detailed position-level data — price, quantity, cost basis, gain/loss, allocation % |
| `pftui portfolio performance --json` | Returns: 1D, MTD, QTD, YTD, since inception |
| `pftui portfolio drift --json` | Current allocation vs target floor/ceiling ranges, with edge-relative drift and rebalance suggestions |
| `pftui portfolio drawdown --json` | Current drawdown from trailing 90-day high, MTD/YTD max drawdowns, and latest position contribution breakdown |
| `pftui portfolio history --date YYYY-MM-DD` | Historical portfolio snapshot for any past date (text output — no `--json` yet; see TODO) |
| `pftui system export json` | Full portfolio export (positions + transactions) |
| `pftui portfolio transaction list` | List all transactions with IDs |

### Market Data

| Command | What It Returns |
|---|---|
| `pftui data refresh` | Fetches ALL data sources (19+ sources) and runs the recurring tail: prediction auto-score, recommendation forward-score, forecast retro-score, misalignment detection, regime classification, housekeeping summary. Price-ingest guard: a close moving >20% d/d is SUSPECT — rejected unless a wired secondary source (BTC: mempool.space/CoinGecko; GC=F: GeckoTerminal XAUT) confirms within 5%; failed fetches never stamp a stale cached price onto today's date. `--accept-outlier SYM` (repeatable/comma-separated) admits a genuine >20% gap (crash/halt) past the guard |
| `pftui data prices audit [--symbol X] [--json]` | Read-only retro-scan of `price_history` for corrupt prints: bars that jumped >20% d/d AND reverted >15% the next bar (the spike-and-revert signature; genuine crashes persist and are NOT flagged). Reports symbol, dates, closes, sources, jump/revert %. Never deletes — repair is a manual, operator-reviewed DELETE because auto-deleting canonical L1 history is more dangerous than reporting it |
| `pftui data audit [--table X] [--json]` | DB-wide false-value audit (read-only): per-table signature checks with per-table judgment (Apr-2020 negative oil is REAL → info; flow-event portfolio jumps are REAL → info). price_history: spike-revert + cross-population bimodality (two close bands >10x apart — the equity-ticker-collision signature) + ≥5-bar exact-placeholder runs on FX/commodities (USD/cash exempt); economic_data plausible-range violations; sentiment_history 0-100 + dupes; cot_cache sign/net-arithmetic invariants; onchain_cache all-zero runs; forecast_scores/signal_expectancy/recommendations returns outside ±95% (±99.9% crypto); portfolio_snapshots d/d jumps >30% (info; operator-backfilled cash_value=0 rows excluded per journal note #728); scenario_history active-book sums outside [60,110] per recorded date + single-scenario moves >15pp between consecutive records (pre-2026-06-10 ledger discipline → info/expected, on/after → suspect); transactions buy/sell fills >15% from the nearest session close + nonpositive quantities + orphaned paired_tx_id (suspect — operator-entered, report-only; output is row id + symbol + date + percent-deviation ONLY, never quantities or values). Output: severity info\|suspect\|corrupt + row KEYS only. Summary line also in `pftui system doctor` |
| `pftui data decontaminate --symbol SYM [--before DATE] [--confirm] [--json]` | Purge L2 derived rows computed from a corrupt L1 price series after a `price_history` repair (poisoned snapshots never self-heal — they are stamped per refresh run). Scope: technical_snapshots, correlation_snapshots (either side), technical_levels, technical_signals, signal_expectancy. Excluded: timeframe_signals/regime_*/portfolio+position_snapshots (cross-asset aggregates / operator history). DRY-RUN BY DEFAULT — prints counts; `--confirm` executes in a transaction and writes a journal-note audit trail (author `system`, section `system`). Deleted historical rows do NOT regrow; signal_expectancy rebuilds via `pftui research backtest` |
| `pftui data dashboard macro --json` | DXY, VIX, yields, currencies, commodities, derived ratios |
| `pftui data fear-greed --json` | Latest crypto + traditional Fear & Greed readings with optional history |
| `pftui portfolio watchlist --json` | All watched symbols with prices, day change, 52W range |
| `pftui analytics movers --json [--threshold N] [--overnight]` | Significant daily/overnight moves (default >3%) |
| `pftui data predictions --json [--limit N]` | Polymarket prediction market odds |
| `pftui data predictions --geo [--json] [--limit N]` | Curated geopolitics relevance filter over prediction markets: ~45-term keyword list (war/ceasefire/sanctions/nuclear/taiwan/iran/russia/opec/...) matched on word boundaries against question + event title, with stale contracts excluded (resolving >12 months out, already past resolution, or zero 24h volume). Spans all categories — Polymarket's own labels under-tag geopolitics. Conflicts with `--category` |
| `pftui data predictions map --auto-suggest [--scenario "<name>"] [--json]` | Auto-suggest top-3 scenario↔contract mapping candidates per active scenario, scored by keyword overlap and category fit. Restrict to one scenario with `--scenario`. |
| `pftui data predictions map --scenario "<name>" --contract-id <id> [--json]` | Explicit mapping write: link a Polymarket contract to a scenario. `--contract-id` is a visible alias for `--contract`. |
| `pftui data sentiment --json` | Crypto + traditional Fear & Greed, COT positioning |
| `pftui data news --json [--limit N] [--filter-independence independent,wire]` | Financial news from RSS and Brave-backed cache, including `topic`, `bound_markets`, `source_tier`, and `source_independence` |
| `pftui data news feeds list --json` | RSS feed health by feed, including status, failure counts, and last failure reason |
| `pftui data news feeds reset FEED_ID [--json]` | Re-enable a degraded or disabled RSS feed after review |
| `pftui data news sources list --json` | Source-domain tier mappings used by news ingest |
| `pftui data news sources set DOMAIN --tier N [--notes TEXT] [--json]` | Set news source tier 1-4 |
| `pftui data news sources remove DOMAIN [--json]` | Remove a custom news source tier mapping |
| `pftui data news topics list --json` | News-topic to prediction-market bindings used for `bound_markets` |
| `pftui data news topics set TOPIC --primary-market-id ID [--secondary-market-id ID] [--json]` | Bind a news topic such as `iran-hormuz` or `fed-policy` to current market contracts |
| `pftui data news topics remove TOPIC [--json]` | Remove a news-topic market binding |
| `pftui data supply --json` | COMEX gold/silver inventory |
| `pftui data options refresh [--symbol SPY] [--all] [--json]` | Fetch Yahoo options chain(s) + compute GEX. With no `--symbol`/`--all`: refreshes SPY/QQQ/GLD/SLV. Hooked into `pftui data refresh` (source name: `options`). BTC options are not on Yahoo (Deribit provider TBD). |
| `pftui data options show --symbol SPY [--limit N] [--json]` | Read the most-recent cached chain (SQLite, no network), centered on the GEX flip strike. |
| `pftui data options view --symbol AAPL [--expiry YYYY-MM-DD] [--limit N] [--json]` | Live Yahoo chain viewer (no persist; legacy interactive view). |
| `pftui analytics gex --symbol SPY [--json]` | Latest GEX snapshot for a symbol — flip strike, max pain, total call/put gamma, plus a 5% "gamma-neutral zone" band around the flip. Reads from `gex_snapshots`. **Preflight integration:** when `pftui journal prediction add` writes a draft whose claim references a numeric target inside the gamma-neutral zone for the symbol, the preflight result adds a `gamma_neutral_zone:target_X_flip_Y` risk-factor entry (advisory; does not bump the abort score). |
| `pftui data real-yields refresh [--days 90] [--json]` | Fetch US TIPS (`DFII5/10/30`), breakevens (`T5YIE`, `T10YIE`), the US 10Y anchor (`DGS10`), and G10 sovereign 10Y benchmarks (UK/DE/JP/CA) from FRED into `real_yields_history`. Degrades to a no-op when `fred_api_key` is absent or the network is unreachable. |
| `pftui data real-yields show [--series DFII10] [--since 30d] [--json]` | Read cached real-yield rows; `--since` accepts NNd/NNw/NNm or YYYY-MM-DD. |
| `pftui analytics real-rates differentials [--since 7d] [--json]` | Per-day US-vs-G10 differentials computed from `real_yields_history`: US 10Y nominal minus the simple average of GB/DE/JP/CA 10Y (in bp) plus each per-pair spread. **HIGH and MACRO timeframe analysts must call this before writing any gold or DXY view.** Daily report Macro hook lives in `src/report/sections/real_rates_macro.rs::render_real_rates_block`; the assembler populates `BuildContext::real_rates_snapshot` from `commands::real_yields::latest_macro_snapshot`. |
| `pftui data flows refresh [--asset SPY] [--json]` | Pull capital-flow rows from the configured provider (`PFTUI_FLOWS_PROVIDER` env var) into the `capital_flows` table. Default provider is `noop` — returns zero flows + a "capital flows provider not configured" note rather than failing. `etf_com_csv` is live (HTML scraper despite the legacy name — pulls `https://www.etf.com/etfanalytics/etf-fund-flows-tool`, locates the flows table by header content ("Ticker" + "Net Flow"), resolves columns by name, and emits one row per ETF using the daily net-flow column with the weekly column as a fallback when daily is blank; positive flow → `flow_type = "etf_creation"`, negative → `etf_redemption`, `amount_usd` is the absolute value; malformed rows are silently dropped and the provider only fails when ZERO rows parse — the "page structure changed" signal; **daily cadence:** when invoked from `data refresh`, the ETF.com provider is skipped automatically if the most recent `capital_flows.fetched_at` for a row whose `source` begins with `etf.com/` landed within 12 hours; all requests carry the polite `User-Agent: pftui-bot/0.28 https://github.com/skylarsimoncelli/pftui`; **scraping is fragile** — treat scraper failures as a signal to inspect the upstream HTML rather than as a transient error). `sec_edgar_13f` is live: walks the canonical filer roster in `src/data/flows.rs::TRACKED_CIKS` (Berkshire / Bridgewater / Renaissance / Citadel), fetches each filer's most recent 13F-HR filing via `https://data.sec.gov/submissions/CIK{cik}.json` + the filing's `index.json`, parses the `infoTable` XML, and emits one `institutional_13f` row per (filer + issuer CUSIP + quarter). Pre-2023 thousands-of-dollars and post-2023 whole-dollars value regimes are both handled. Per-filer failures are accumulated in the result note; the provider only fails the refresh when EVERY tracked filer fails. **Quarterly cadence:** when invoked from `data refresh`, the SEC EDGAR provider is skipped automatically if the most recent `capital_flows.fetched_at` for an `institutional_13f` row landed within 80 days (13F filings only update once per quarter; manual `data flows refresh` ignores the throttle). All EDGAR requests carry the SEC-required `User-Agent: pftui-bot/0.28 contact@example.com` header. |
| `pftui data flows show [--asset SPY] [--since 30d] [--json]` | Read cached capital-flow rows. `--since` accepts NNd/NNw/NNm or YYYY-MM-DD. Newest `period_end` first. |
| `pftui analytics flows summary [--since 7d] [--json]` | Per-asset rolling-window aggregate of `capital_flows`: net flow, top inflow, top outflow. Redemptions and outflows are signed negative. Output sorted alphabetically by asset for deterministic agent consumption. Daily-report per-asset hook lives in `src/report/sections/capital_flows.rs::render_capital_flows_block`; emits a one-liner when the asset has at least one row in the last 7 days. |
| `pftui data dashboard global --json` | World Bank macro data (GDP, debt, reserves) |
| `pftui data status --json` | Data source freshness plus daemon health — includes `daemon` heartbeat and `news_feeds` RSS health |
| `pftui data series status [--json]` | Registry-driven freshness for every canonical series (`series_registry`): last datapoint, age vs SLA, 2x-SLA flags. The report skill's Step-10 data-freshness section should consume this instead of ad-hoc per-table checks. |
| `pftui data snapshot-line [--json]` | One deterministic market-context line `<YYYY-MM-DD> \| SPX <close> \| BTC <close> \| GOLD <close> \| SILVER <close> \| DXY <close> \| VIX <close>` from latest cached closes (BTC falls back to the deep BTC-USD series; a series with no history is omitted). Used by `journal notes add --stamp` / `journal entry add --stamp` so notes self-contextualize for retro-scoring |

### Portfolio Management

| Command | What It Does |
|---|---|
| `pftui portfolio transaction add --symbol SYM --category CAT --tx-type buy/sell/transfer_in/transfer_out --quantity N --price P --date D [--cash-currency USD] [--no-auto-cash] [--dry-run] [--json]` | Add transaction; non-cash buys/sells auto-insert a paired cash debit/credit unless opted out (transfer types are external flows — never auto-cash-paired, never recommendation-linked); dry-run/JSON include post-add allocation, drift, and cash delta |
| `pftui portfolio transaction remove ID [--unpaired] [--dry-run] [--json]` | Remove transaction by ID; paired cash legs are removed too unless `--unpaired` is passed; dry-run/JSON preview post-remove allocation, drift, and cash delta |
| `pftui portfolio transaction list --paired --json` | List transactions with paired transaction IDs |
| `pftui portfolio transaction repair-pairs [--dry-run] [--confirm] [--skip ID] [--max-days N] [--max-notional-pct PCT] [--json]` | Heuristic backfill for pre-paired-leg-era `paired_tx_id`: matches each unpaired non-cash buy with the closest USD sell within ±2 days and ±10% notional. Idempotent; only touches rows where both legs currently have `paired_tx_id = NULL`. `--dry-run` (default) previews; `--confirm` applies; `--skip` excludes specific ids that need manual review |
| `pftui portfolio transaction import-delta CSV [--dry-run] [--apply] [--json]` | Import a Delta tracker CSV export (full trade + fiat-flow history) as ground truth for its window. SYNC-BASE-HOLDINGS fiat rows become native paired cash legs of their same-timestamp trade; non-sync trades import with NO cash leg (the export's own DEPOSIT/WITHDRAW rows carry the funding — model B, prevents double-counting); plain DEPOSIT/WITHDRAW rows become external `transfer_in`/`transfer_out` flows on the fiat symbol (USD/GBP), with same-window opposite-direction USD/GBP pairs annotated as fx-conversion pairs (both legs kept; implied rate in notes; excluded from the external-capital total). Pre-existing hand rows are reconciled and classified SUPERSEDED (deleted on apply) / KEPT / CONFLICT. Idempotent via `[delta:<key>]` notes markers. DRY-RUN BY DEFAULT; `--apply` backs up the DB (full + transactions JSON to `~/pftui-archives/`) first and writes a journal-note audit trail (author `system`, section `system`). Reports the USD balance equation, per-symbol net quantities, and total external capital contributed (the flow-adjusted-returns input) |
| `pftui portfolio set-cash CURRENCY AMOUNT [--confirm] [--dry-run] [--json]` | Replace cash transactions with an exact cash position; requires `--confirm` when more than one row would be discarded |
| `pftui portfolio watchlist add SYMBOL [--target PRICE]` | Add to watchlist |
| `pftui portfolio watchlist remove SYMBOL` | Remove from watchlist |
| `pftui portfolio target set SYMBOL --floor PCT --ceiling PCT` | Set acceptable allocation range; SYMBOL may be any tradeable symbol or a cash symbol (USD, GBP, EUR — wide bands like `--floor 30 --ceiling 60` model dry-powder optionality while still surfacing drift on breach); legacy `--target PCT --band PCT` is still accepted |
| `pftui portfolio target remove SYMBOL` | Remove target |
| `pftui portfolio rebalance --json` | Suggested trades to reach targets |
| `pftui portfolio broker add BROKER --api-key KEY [--secret SECRET]` | Connect a broker (trading212, ibkr, binance, kraken, coinbase, crypto-com) |
| `pftui portfolio broker sync [BROKER] [--dry-run] --json` | Sync positions from connected brokers |
| `pftui portfolio broker list --json` | List configured broker connections |
| `pftui portfolio broker remove BROKER` | Remove a broker and its synced transactions |
| `pftui analytics alerts add "CONDITION"` | Add alert |
| `pftui analytics alerts list --json` | List active alerts |
| `pftui analytics alerts remove ID` | Remove alert |

### Journal

| Command | What It Does |
|---|---|
| `pftui journal entry add "TEXT" --tag TAG --symbol SYM` | Add entry |
| `pftui journal entry list --json` | List all entries |
| `pftui journal entry search "QUERY" --json` | Search entries |

### Intelligence Database

| Command | What It Does |
|---|---|
| `pftui journal scenario add "NAME" --probability N` | Add macro scenario with initial probability |
| `pftui journal scenario update "NAME" --probability N --evidence "DATA THAT MOVED IT" [--proposer LAYER] [--driver "WHY"\|--notes "WHY"] [--hard-print "EVENT"] [--override-conflict]` | Update scenario probability and auto-log history. `--evidence` is REQUIRED for probability moves; cumulative \|Δ\| per scenario per day is capped at 5pp unless `--hard-print "<event>"` cites a hard data print; a same-day update by a different `--proposer` requires `--override-conflict` |
| `pftui journal scenario set-base-rate "NAME" --rate N --reference "REFERENCE CLASS"` | Anchor a scenario to its reference-class base rate; `scenario list` then shows the deviation (probability − base_rate) |
| `pftui journal scenario signal add "SIGNAL" --scenario "NAME"` | Attach a tracked signal to a scenario |
| `pftui journal scenario history "NAME" --limit N --json` | Show scenario probability history |
| `pftui journal prediction add "CLAIM" [--symbol BTC] [--conviction high] [--timeframe low\|medium\|high\|macro\|macro-checkpoint] [--confidence 0.7] [--source-agent low-agent] [--topic fed] [--source-article-id 123] [--lessons 218,240] [--override-cap]` | Add a prediction call for later scoring, optionally recording lesson IDs and news-source attribution. LOW analyst calls are capped at 5/hour unless `--override-cap` is passed. `--timeframe macro-checkpoint` is reserved for falsifiable 90-day sub-claims attached to a multi-year macro thesis (claim MUST embed `[thesis=<slug>]`) |
| `pftui journal prediction add ... --falsify "<SYMBOL> <close\|closes\|stays\|prints> <above\|below\|between\|in-range\|in-band> <value> [<value2>] by <YYYY-MM-DD>" [--override-confidence-cap --cap-rationale "..."]` | `--falsify` records the claim's machine-scoreable SUCCESS CONDITION (the condition that, if met, scores the prediction CORRECT) as a `prediction_falsification_rules` row. Examples: `--falsify "BTC close below 50000 by 2026-09-30"`, `--falsify "BTC stays in-range 45000 85000 by 2026-12-31"`. Omitting `--falsify` (or supplying an unparseable rule, stored as `rule_type='unstructured'`) caps stated confidence at **0.3** — unfalsifiable predictions cannot carry high confidence. A calibration-derived clamp also applies: when the trailing `calibration_matrix` cell for (timeframe, topic, conviction band) has n ≥ 8 scored calls and stated confidence exceeds hit_rate + 0.15, confidence is clamped to that ceiling; `--override-confidence-cap --cap-rationale "<why>"` bypasses it and appends `[cap-override: <why>]` to `resolution_criteria` |
| `pftui journal prediction auto-score [--dry-run] [--since YYYY-MM-DD] [--force] [--json]` (alias: `score-auto`) | Mechanically score pending predictions from their falsification rules against `price_history` daily closes. `close-*`/`prints-*` rules score CORRECT on the first qualifying close inside the window and WRONG once the window expires without one (prints-* uses closes; intraday data unavailable); `stays-*` rules score WRONG on the first violating close and CORRECT only after the window expires clean. Crypto symbols fall back to the `-USD` suffixed series when the bare series lacks coverage; the series used is recorded in `score_notes`. Never overwrites an already-scored prediction. Also runs automatically as a tail step of every `pftui data refresh` |
| `pftui journal prediction score --id N --outcome correct|partial|wrong [--notes "..."] [--lesson "..."]` | Score a previous prediction outcome |
| `pftui journal prediction stats --json` | Compute hit-rate stats by conviction, symbol, timeframe, and source agent |
| `pftui journal prediction scorecard [--date YYYY-MM-DD|today|yesterday] [--limit N] [--lesson-coverage] --json` | Day scorecard with streak and lesson coverage |
| `pftui journal prediction lessons [--miss-type <t>] [--limit N] [--include-retired] [--json]` | The analyst lesson book. Active lessons only by default; pass `--include-retired` to surface lessons retired by `analytics lessons curate` |
| `pftui agent message send "TEXT" --from agent-a [--to agent-b] [--batch "TEXT2" --batch "TEXT3"] [--package-title "Fed handoff"] [--package-id pkg-123]` | Send one or multiple structured messages between agent roles, optionally grouped as one intel package |
| `pftui agent message reply "TEXT" --id N --from agent-b` | Reply to message `N` back to the original sender |
| `pftui agent message flag "ISSUE" --id N --from agent-b` | Escalate data-quality/risk issue on message `N` |
| `pftui agent message list [--from agent-a] [--unacked] --json` | Query queued agent messages |
| `pftui agent message ack --id N` | Acknowledge a single message |
| `pftui journal notes add "TEXT" --section market [--date YYYY-MM-DD] [--stamp]` | Add a date-keyed daily narrative note. `--stamp` prepends the market snapshot line (`data snapshot-line`) so the note records the tape it was written under; `journal entry add` accepts `--stamp` too |
| `pftui journal notes search "QUERY" --since YYYY-MM-DD --json` | Search historical daily notes |
| `pftui journal notes repetition [--author analyst-medium] [--days 30] [--json]` | Cluster an author's recent notes by mutual trigram similarity ≥0.85 and surface repeated clusters ("you have written this note 9 times"). `notes add` also stores a per-note `novelty_score` (1 − max similarity vs the author's last 20 notes) and warns when a new note is ≥85% similar to an existing one — consolidate into the thesis table instead of re-deriving |
| `pftui portfolio opportunity add "EVENT" [--asset SYM] [--missed-gain-usd N] [--avoided-loss-usd N]` | Log an opportunity-cost event |
| `pftui portfolio opportunity stats --json` | Show net missed-vs-avoided positioning stats |
| `pftui analytics correlations compute --store --period 30d` | Compute live correlations and persist snapshots |
| `pftui analytics correlations history BTC SPY --period 30d --limit 30 --json` | Show stored correlation history for a pair |
| `pftui analytics macro regime current --json` | Show latest automated market regime classification |
| `pftui analytics macro regime transitions --limit 20 --json` | Show regime change points over time |
| `pftui analytics macro --json` | Show long-cycle macro dashboard (cycles, outcomes, recent structural log) |
| `pftui analytics macro outcomes --json` | Show structural outcome probabilities |
| `pftui analytics trends dashboard --json` | Show active high-timeframe trends with direction/conviction |
| `pftui analytics trends impact add --id TREND_ID --symbol SYM --impact bullish|bearish|neutral` | Map a trend's asset-level impact (trend ids from `trends dashboard`) |
| `pftui analytics summary --json` | Unified 4-layer analytics snapshot (low/medium/high/macro + top signal) |
| `pftui analytics situation --json` | Canonical Situation Room payload: headline, summary stats, watch-now priorities, portfolio impacts, risk matrix |
| `pftui analytics deltas --json [--since last-refresh|close|24h|7d]` | Server-owned change radar showing what changed across key monitoring windows |
| `pftui analytics catalysts --json [--window today|tomorrow|week]` | Ranked upcoming catalyst feed with countdowns, significance, and portfolio/scenario linkage |
| `pftui analytics impact --json` | Rank current holdings/watchlist by exposure to active signals, scenarios, trends, and catalysts |
| `pftui analytics opportunities --json` | Rank high-alignment non-held opportunities from the same analytics evidence chain |
| `pftui analytics synthesis --json` | Cross-timeframe synthesis: alignment, divergence, constraint flows, unresolved tensions, watch-tomorrow |
| `pftui analytics alignment --symbol SYM --json` | Per-asset cross-timeframe alignment matrix |
| `pftui analytics alignment current --json` | Today's operator-vs-analyst alignment score (0-100). Aggregates Skylar's journal/operator_replies views vs analyst convergence per held asset above 1% allocation, allocation-weighted, classified aligned/divergent-magnitude/divergent-direction. Returns the stored row if present, otherwise computes on demand. |
| `pftui analytics alignment history --since 90d --json` | Stored alignment-score time series. `--since` accepts Nd/Nw/Nm tokens or a YYYY-MM-DD anchor. |
| `pftui analytics alignment compute --date YYYY-MM-DD [--store] [--json]` | Recompute the score for one date. `--store` persists to `alignment_score_history` and runs the drift-alert check (emits an `agent_messages` row to `synthesis` with priority=normal, category=signal when the score has been below 50 for 2+ consecutive days; idempotent per date). |
| `pftui analytics divergence --json` | Cross-layer disagreement table for conflicting signals |
| `pftui analytics digest --agent-filter low-agent --json` | Role-aware summary payload for agent handoffs |
| `pftui analytics recap --date yesterday --json` | Chronological event recap for a given day |
| `pftui analytics narrative --json` | Structured analytical memory: recap, scenario/conviction/trend shifts, scorecard, surprises, lessons, catalyst outcomes |
| `pftui analytics calibration --by-layer --json [--window-days 90]` | Scenario-vs-market divergences plus realised prediction calibration by layer, sample size, 1σ uncertainty, and conviction band |
| `pftui analytics epistemics record --date YYYY-MM-DD [--agreement X] [--blind-divergence X] [--panel-dispersion X] [--novelty X] [--fallback-warnings N] [--scenario-delta-total X] [--audit-pass-rate X] [--agents N] [--notes "..."] [--json]` | Upsert (field-wise merge) the `run_health` row for a run date. When omitted, Rust derives `blind_divergence` (same-day analyst_views: mean \|canonical-mean − blind\| per asset), `scenario_delta_total` (today's scenario probability ledger), `conviction_price_corr` (max \|r\| across layer × held-asset conviction-price correlations over 90d; pass `--conviction-price-corr X` to override), `forecast_hit_rate` (trailing 30d of `forecast_scores`; `--forecast-hit-rate X` to override), and `active_misalignments` (current ACTIVE `forecast_misalignments` count; `--active-misalignments N` to override) itself |
| `pftui analytics epistemics show [--date D] [--json]` | One run's health row with threshold flags: agreement > 0.85 → echo risk, panel_dispersion < 4.0 → persona washing, blind_divergence > 2.0 → house view far from raw-data read, active_misalignments > 0 → ⚠ probation in force (the active (layer, asset) rows are listed) |
| `pftui analytics epistemics history [--limit N] [--json]` | Run-health trend table, newest first |
| `pftui analytics epistemics rivalry [--json]` | House-vs-antithesis scoreboard: scored `user_predictions` grouped by `source_agent` (n, correct, wrong, partial, hit rate); shows "rivalry accruing" while antithesis only has pending predictions |
| `pftui analytics epistemics conviction-price [--days 90] [--asset X] [--json]` | Per (canonical layer × held asset) Pearson r between the layer's signed conviction trajectory and the asset's closes (needs ≥6 paired observations; `SYM` → `SYM-USD` series fallback). \|r\| > 0.6 flags "momentum dressed as structure" (standing rule 15). Max \|r\| is what `epistemics record` self-derives into `run_health.conviction_price_corr` |
| `pftui analytics recommendations record --symbol X --action add\|wait\|hold\|trim\|avoid [--rationale "..."] [--date YYYY-MM-DD] [--source NAME] [--json]` | Record one recommendation-ledger entry. `entry_price` auto-filled from the latest `price_history` close on or before the date (falls back `SYM` → `SYM-USD`; the series used is stored in `price_series`). WAIT is a first-class, scored action — for physical metal (GC=F/SI=F) the decision space is add/wait, never trim |
| `pftui analytics recommendations list [--symbol X] [--limit N] [--json]` | List ledger rows newest-first with entry price, source, and any scored forward returns |
| `pftui analytics recommendations score [--json]` | Fill `fwd_30d_pct`/`fwd_90d_pct`/`fwd_180d_pct` for any priced row whose horizon has elapsed (close at run_date+N vs entry_price). Idempotent — never overwrites a scored horizon. Also runs automatically in the `data refresh` tail. (`--all`/`--id` keep the legacy outcome-score mode) |
| `pftui analytics recommendations scoreboard [--symbol X] [--json]` | Per symbol × action: n, % positive, and mean forward return at 30/90/180d, plus the per-symbol WINDOW-QUALITY line (mean 90d after ADD − after WAIT; negative = the system's ADD timing was worse than its own WAIT calls). Renders "scoreboard accruing — N unscored" until horizons fill |
| `pftui analytics narrative-divergence --json [--hours 24]` | Active scenario narrative-vs-money scores from topic news pressure versus mapped prediction-market movement (computed live; nothing persisted — the old `rebuild` backfill and `narrative_money_history` table were culled in R3) |
| `pftui analytics news-silence --json [--window-days 90]` | Tier-1/2 topic article volume versus rolling weekday baselines, including silent/saturated status changes |
| `pftui analytics news-silence rebuild-baselines --since 90d --json` | Re-compute per-(topic, weekday) baselines from the trailing news_cache window; idempotent |
| `pftui analytics lessons applied --since 24h --json` | Lessons referenced by this run's predictions, top guards, and strongest historical analog |
| `pftui analytics lessons curate [--dry-run] [--retire-after-days 60] [--json]` | Retire stale uncited active lessons whose topic cluster is idle; journals the change to `agent_messages` |
| `pftui analytics lessons revive <id> [--json]` | Manually un-retire a previously retired lesson (sets status back to `active`) |
| `pftui analytics lessons health [--json]` | Library health summary: total / active / retired / superseded / citations total / avg citations per active |
| `pftui analytics lessons rules add --rule "TEXT" [--rationale "TEXT"] [--sources "12,40,77"] [--enforcement advisory\|validator]` | Add a standing rule — one imperative operational rule consolidated from repeated lessons (with the failure pattern it prevents and its source lesson ids) |
| `pftui analytics lessons rules list [--all] [--json]` | Compact prompt-injectable list of standing rules (active only by default; `--all` includes retired) |
| `pftui analytics lessons rules cite <id> [--json]` | Record a violation of a standing rule (increments `violation_count`) |
| `pftui analytics lessons rules retire <id> [--json]` | Retire a standing rule |
| `pftui analytics views stale [--days 21] [--move-pct 10] [--json]` | Stale-view detector: for each held asset × canonical layer, flag views older than `--days` where price moved more than `--move-pct`% since the view was written ("evidence moved, conviction didn't") |
| `pftui analytics thesis set-review <section> --date YYYY-MM-DD [--json]` | Schedule a review-by date on a thesis section |
| `pftui analytics thesis review-due [--json]` | Thesis sections past their review date, plus unscheduled sections (no `review_by` set) |
| `pftui analytics news-sources accuracy --json [--domain bloomberg.com] [--topic fed] [--include-pre-deployment]` | Per-source hit-rate ledger for predictions derived from news articles. `--include-pre-deployment` emits an explicit forward-only notice (the ledger does NOT retroactively attribute pre-`source_article_id`-deployment predictions to a source) |
| `pftui analytics news-sources rank --topic iran --json` | Rank news sources for a topic using trailing source-attributed prediction outcomes |
| `pftui analytics news-sources rebuild-accuracy [--since 180d] [--dry-run] --json` | Replay `sync_prediction_outcome` for every scored prediction with a `source_article_id`; idempotent backfill |
| `pftui analytics gaps --json` | Data freshness/missing-table check across timeframe layers |
| `pftui analytics signals --json` | Show all signals (cross-timeframe + per-symbol technical) |
| `pftui analytics signals --source technical --json` | Per-symbol technical signals: RSI overbought/oversold, MACD cross, SMA 200 reclaim/break, BB squeeze, volume expansion, 52W extremes |
| `pftui analytics signals --source timeframe --json` | Cross-timeframe alignment/divergence/transition signals only |
| `pftui analytics signals --source technical --symbol BTC-USD --json` | Technical signals for a specific symbol |
| `pftui analytics technicals [--symbol SYM] --json` | Latest persisted technical snapshot(s) — RSI, MACD, SMA, Bollinger, 52W position, volume regime |
| `pftui analytics technicals structure SYM [--timeframe daily\|weekly\|monthly] --json` | Pure price-action market-structure read from price_history: N-bar swing pivots (HH/HL/LH/LL labels), trend classification (uptrend/downtrend/range), most recent support/resistance break-of-structure, MA posture + slope (50d/200d, 10wk/40wk, 10mo/20mo), extension % vs the slow MA (rule-13 20% gate flag), and a one-line `verdict`. Weekly/monthly bars aggregated from daily; `BTC` falls back to the deep `BTC-USD` series |
| `pftui analytics technicals cyber SYM [--timeframe daily\|weekly] [--lookback-signals N] --json` | Composite Cyber Dots read — faithful Rust port of the operator's PineScript indicator (`docs/reference/cyber-dots.pine`, engine in `src/analytics/cyber/`): Gaussian CyberBands with the persistent QB state machine (bullish/bearish/caution + since-date), Zone-Based bands (timeframe-adapted EMA 144/233 zones), CyberLine (VIDYA len 18 + Donchian/hybrid modes, slope + last price-cross), CyberDots strength dots (SuperTrend 12/1.3 SMA-ATR + VMA(4) + SMA(18), strength 0–3), Bollinger reversal T/B signals with 2-bar confirmation ladder, Pi Cycle top/bottom (full historical fire list + proximity ratios where 1.0 = trigger), MTF RSI(6) zones with zone-exit breakouts + RSI(14) extreme flags, hybrid breakout arrows (3-line strike, momentum exhaustion, RSI zone exits, QB-gated, 5-bar cooldown), a one-line composite `verdict`, and the last N dated signal events. Weekly bars aggregated from daily; Pi Cycle always runs on daily closes; `BTC` falls back to the deep `BTC-USD` series |
| `pftui analytics cycles clock [--asset BTC\|GC=F] --json` | Cycle-position clock (POSITION only, never a price prediction). BTC: days since the 2024-04-19 halving, Olson day-900 countdown, Loukas 4yr-cycle week vs the wk-187-229 low band (anchor = verified 2022-11-21 low), midterm-year H2 flag, Mayer Multiple, % vs 200-week MA, and an **accumulation-clock verdict** (`accumulation` block: `stance` ∈ accumulate/window-opening/early/advancing/elevated, a summed `score`, and per-factor reads) that synthesizes Loukas-band proximity + Mayer band + Olson countdown into one plain actionable read for a cycle-low buyer — a POSITIONAL read (where the cycle sits vs the historical low window), explicitly NOT a price signal; a strong "accumulate" requires BOTH band proximity AND Mayer<1 (being far below the ATH is not itself bullish). Gold: position in the ~7yr cycle from verified lows (2008-11, 2015-12, 2022-09 — each checked against price_history minima), half-cycle position, extension vs 200d/40wk MAs |
| `pftui analytics cycles analyze <SYM> [--degree <name>] --json` | Deterministic multi-degree cycle-theory report (engine: `analytics/cycle_engine.rs`, doctrine: docs/CYCLE-THEORY.md). Per degree: dated cycle-low list (swing-low confirmed vs candidate), low-to-low timing band (empirical P15-P85 over the trailing 10 cycles; small-n fallback labeled), cycle age + band position (pre_band/in_band/over_band) + bars-to-band-edges + next-low WINDOW (never a date), translation ledger summary (LT/MID/RT, first-LT-after-RT-string warning), FLD state + last cross + 2× measured-move target, VTL (two most recent confirmed lows; break confirms the peak of the next-longer degree), half-cycle low, failed-cycle flag, possible-inversion FLAG (no verdict — schools disagree), nested synchronicity + green/amber/red clarity, and a one-line composite verdict. BTC emits BOTH the halving clock and the Loukas low-to-low count, labeled. Degrees: BTC daily/investor/4-year; GC=F & SI=F intermediate/major (~6.9y, anchored to verified lows); generic assets daily+intermediate. `BTC` falls back to the deep `BTC-USD` series. Timing only — never a price prediction |
| `pftui analytics cycles ledger <SYM> --degree <d> --json` | Translation ledger table for one degree: per completed cycle the start/end lows, length in bars, top date/price, translation pct (bars low→top / bars low→low), LT/MID/RT class (MID = 0.5±0.05), failed flag (close below origin low); plus RT-string/LT-warning status and the current cycle's provisional top |
| `pftui analytics technicals --symbols SYM --include gaussian-channel,zone-channel,volatility-trend,donchian-trend --json` | Extended channels-subset indicators alongside the snapshot — Gaussian Channel (DEMA → Gaussian filter → SMMA + σ-bands + `band_state`), Zone EMA Channel (`zone_position` + four band values), Volatility-Weighted Trend (`value`, `slope`, `trend_strength` 0–3), Donchian Midline Trend (`value`, `slope`) + a hybrid blend when both volatility-trend and donchian-trend are included |

### Channels subset (Gaussian, Zone EMA, Volatility, Donchian)

The channels subset extends `pftui analytics technicals` with four channel/trend-line outputs. Selection is via `--include <tokens>` (comma-separated). `--include all` enables every extended indicator the CLI knows about.

Indicator outputs:

| Indicator | `--include` token | Output fields | Notes |
|---|---|---|---|
| Gaussian Channel band | `gaussian-channel` | `middle`, `upper`, `lower`, `band_state` (`above_upper` / `in_band` / `below_lower`) | Chain: DEMA → Gaussian filter → SMMA. Defaults DEMA 7, Gaussian length 4, σ 2.0, SMMA 12, SD lookback 30, upper/lower σ multipliers 2.5 / 1.8. |
| Zone EMA Channel | `zone-channel` | `upper_outer`, `upper_inner`, `lower_inner`, `lower_outer`, `zone_position` (`upper-outer` / `upper-inner` / `lower-inner` / `lower-outer`) | Two EMAs (default 144 / 233). Outer bands extend inner bands by a configurable scale (default 1.5×). |
| Volatility-Weighted Trend | `volatility-trend` | `value`, `slope` (`up` / `down` / `flat`), `trend_strength` (0–3 integer) | Smoothing constant α modulated by realised return volatility. Sensitivity Fast / Medium / Slow → length 9 / 18 / 27 (default Medium). |
| Donchian Midline Trend | `donchian-trend` | `value`, `slope` | Mean of conversion-length (default 5) and baseline-length (default 26) Donchian midlines. |
| Hybrid Trend Blend | `donchian-trend` + `volatility-trend` | `value`, `slope`, `volatility_weight`, `donchian_weight` | Emitted automatically when both volatility-trend and donchian-trend are requested. Default 50/50 blend. |

Naming policy: canonical TA terminology only — no vendor / indicator brand names anywhere (table columns, JSON field names, CLI flags, comments). All math runs over the raw price-history closes (highs/lows for Donchian) — `f64` is acceptable here because these are indicator floats, not money / quantities.

JSON shape (single symbol, `--include all`):

```jsonc
{
  "timeframe": "1d",
  "technicals": [/* existing snapshot rows */],
  "count": 1,
  "extended": {
    "TEST": {
      "gaussian_channel": { "middle": …, "upper": …, "lower": …, "band_state": "in_band" },
      "zone_channel":     { "upper_outer": …, "upper_inner": …, "lower_inner": …, "lower_outer": …, "zone_position": "upper-inner" },
      "volatility_trend": { "value": …, "slope": "up", "trend_strength": 2 },
      "donchian_trend":   { "value": …, "slope": "up" },
      "hybrid_trend":     { "value": …, "slope": "up", "volatility_weight": 0.5, "donchian_weight": 0.5 }
    }
  }
}
```
| `pftui analytics technicals --symbols SYM --include mtf-rsi,pi-cycle,mtf-breakout,bollinger-reversal,rsi-extreme [--json]` | Extended signals subset: multi-timeframe RSI alignment, pi-cycle top/bottom crossover, multi-timeframe breakout composite, Bollinger reversal with multi-bar confirmation, RSI extreme highlighting. Pass `--include all` to enable every extended output known to the binary (channels + signals). |

### Research Harness (measured signal expectancy)

The research harness converts the deterministic engines (market structure, Cyber, cycle engine, SMA/RSI/Mayer thresholds) into MEASURED expectancy: dated signal EVENTS (state transitions, never states) studied for forward returns vs the asset's own baseline drift. Stats bind to `(signal_id, signal_version)` and carry a walk-forward `as_of` so citations are lookahead-free. n<10 renders as anecdotal; significance is an exact two-sided binomial test vs the BASELINE up-rate after overlap exclusion.

| Command | What It Does |
|---|---|
| `pftui research signals list [--json]` | The signal registry: ~27 canonical emitters with id, version, description (structure flips/BOS, Cyber QB flips/dots/line crosses/Pi Cycle/MTF RSI/breakouts, cycle timing-band entries/FLD crosses/failed cycles/VTL breaks, 200dma extension/window, RSI(14)<25, Mayer<0.85) |
| `pftui research forecasts score [--json]` | Retroactive forecast scoring: score every `analyst_view_history` row not yet in `forecast_scores` at its layer's canonical horizon (low 7 trading days, medium 45d, high 135d, macro 365d; blind/antithesis at ALL FOUR — see `src/research/forecast_scoring.rs`), plus fill pendings whose horizons elapsed. Direction-authoritative conviction; `SYM` → `SYM-USD` series fallback recorded in `series_used`. Idempotent — scored rows never mutated. Also runs in the `data refresh` tail |
| `pftui research forecasts report [--layer X] [--asset Y] [--window-days N] [--json]` | Per (layer × asset × horizon): n scored, n neutral, hit rate, mean weighted score (sign-match × \|conviction\|/5), mean realized when bullish vs bearish, current wrong-sign streak; plus per-layer TOTALS rows |
| `pftui research forecasts streaks [--threshold 5] [--json]` | Every (layer, asset, horizon) whose CURRENT consecutive wrong-sign streak ≥ threshold, with date span and the cumulative realized move against the calls. Stable structured feed for misalignment tripwires |
| `pftui research forecasts verify [--threshold-pp 0.5] [--reissue] [--json]` | Recompute every SCORED row's realized return against TODAY'S price series WITHOUT mutating the ledger; report per-row drift where \|recomputed − stored\| > threshold, summarized per asset/series_used (catches scores computed before a price-history repair). `--reissue` is the journaled remediation: drifted rows are marked `status='superseded'` (kept — append-only doctrine) and corrected rows inserted; superseded rows are excluded from every report/streak/misalignment aggregation |
| `pftui research misalignments [--all] [--json]` | Active forecast misalignments (default) or the full episode ledger (`--all`). A misalignment trips when a canonical layer's current wrong-sign streak on one asset reaches 5 (detected in the `data refresh` tail). While ACTIVE: the layer's views on that asset are on PROBATION (listed but excluded from convergence voting — `analytics views list/convergence` mark `probation: true`), `journal prediction add` caps that layer's confidence on the symbol at 0.25, and `analytics epistemics record` counts it into `run_health.active_misalignments`. Recovery is mechanical: a scored direction hit ends the episode |
| `pftui research backtest [--signal X] [--asset Y] [--as-of D] [--json]` | Run event studies (default: all signals x held assets + SPY; deep series like BTC-USD substituted automatically), persist `signal_expectancy` rows (L2, rebuildable), print the expectancy table with baseline lift + significance flags. Horizons 5/30/90/180 calendar days; per horizon: n_total/n_evaluable/n_nonoverlap, hit rate vs baseline, mean/median/P25/P75, MAE/MFE, p-value |
| `pftui research expectancy [--signal X] [--asset Y] [--json]` | Read the persisted expectancy table (latest as_of per signal x asset) without recomputing |
| `pftui research events --signal X --asset Y [--limit N] [--json]` | The raw dated event list with per-event forward returns at every horizon ("show me the 12 instances"), plus the overlap-pruned stats summary |
| `pftui research dossier <ta\|cycles\|macro> [--asset X] [--json]` | Competence dossier compiled from EXISTING measured data only: (a) the domain's `signal_expectancy` rows (ta → `structure_`/`cyber_`, cycles → `cycle_`; macro → scenario-ledger discipline stats instead), (b) the scored-forecast record for the domain's layers (ta → low+medium, cycles → medium+high, macro → macro) with current streaks, (c) worked precedents: the 3 highest-\|lift\| SIGNIFICANT signals with dated event lists + forward returns. Empty sections render "no measured evidence yet" — never invented prose |
| `pftui research shadowbook [--json]` | The shadow book: counterfactual portfolio that mechanically executes every recommendations-ledger row (policy v1: add → +1.0pp NAV cash→symbol at the row's entry_price, skipped when cash < 1pp; trim → −1.0pp symbol→cash capped at held value; wait/hold/avoid → no trade; same-day rows in id order). Three books seeded with the operator's ACTUAL holdings at ledger inception: SHADOW (followed the desk), ACTUAL (real transactions), HOLD (frozen) — so "does following the desk beat ignoring it?" is a number. Computed on demand from recommendations + price_history + transactions (no state tables); <90 days of ledger history renders a BENCHMARK ACCRUING banner; a one-line summary appears in `analytics epistemics show` once ≥30 days accrue |
| `pftui research verify-thesis [--section X] [--json]` | Re-verify the thesis evidence contract: extract every `[pftui]` / `[derived]` / `[ext]` claim from curated thesis sections, re-run the embedded verification SQL READ-ONLY (fenced sql blocks and inline backticked SELECTs), recompute mechanically-stated derivations, and check `[ext:N]` reference presence. Classifies: `verified` (±2% numeric, exact dates), `drift` (claimed vs current shown; SNAPSHOT claims age — severity info with staleness; STRUCTURAL anchors drifting are suspect), `broken` (SQL errored / missing reference), `unverifiable` (tag without runnable SQL), `untagged` (numeric claim with no tag in a contract section — the contract-violation class). Also runs as a `system doctor` Data Health check. Repair stays curated: fix wrong STRUCTURAL values on the L4 thesis row and journal old→new (author `system`, section `system`); never rewrite SNAPSHOT values — refresh the as-of line |

### Utility

| Command | What It Does |
|---|---|
| `pftui system config list [--json]` | List all configuration fields |
| `pftui system config get FIELD [--json]` | Get a specific config value |
| `pftui system config set FIELD VALUE` | Set a config field (e.g., `brave_api_key`) |
| `pftui system schema verify [--json]` | Check SQLite schema drift before startup migrations mutate the DB |
| `pftui system schema repair --dry-run [--json]` | Preview safe missing-table/column/index repair SQL |
| `pftui system schema repair --confirm [--json]` | Apply safe schema repairs after reviewing the dry-run plan |
| `pftui system data-coverage [--json]` | Per-enrichment-table row count vs expected minimum; surfaces 0-row and missing tables loudly |
| `pftui system archive-db [--out PATH] [--table X] [--json]` | Back up the whole SQLite DB (`VACUUM INTO`, prints path + size) or export one table as JSON. Default destination `~/pftui-archives/` — always OUTSIDE the repo; never commit archives. |
| `pftui system snapshot` | Render full TUI to stdout (for sharing or screenshots) |
| `pftui system demo` | Launch with sample data (for testing, no real data) |
| `pftui system daemon start [--interval N] [--json]` | LEGACY/optional: always-on refresh loop. Not required — the `data refresh` tail fires every recurring mechanism (see "How This System Actually Runs") |
| `pftui system daemon status [--json]` | Read daemon heartbeat/health without attaching to the process (legacy; reports not-running on session-driven installs) |
| `pftui system web [--port N] [--bind ADDR] [--no-auth]` | Start web dashboard |
| `pftui system setup` | Interactive setup wizard |

---

## Data Model

### Database Backends

Location: `~/.local/share/pftui/pftui.db`

The active backend database is the single source of truth. All interfaces (TUI, Web, CLI) read from and write to it.

```
~/.local/share/pftui/pftui.db
├── transactions                   # Buy/sell records with cost basis
├── price_cache                    # Latest spot prices (updated on refresh)
├── price_history                  # Daily OHLCV history
├── technical_snapshots            # Persisted per-symbol technical state from refresh
├── watchlist                      # Tracked symbols with optional targets
├── alerts                         # Price/allocation alerts
├── targets                        # Target allocation floor/ceiling ranges
├── journal_entries                # Trade journal + notes
├── calendar_events                # Economic calendar
├── news_cache                     # RSS/Brave articles with topic, source tier, and independence metadata (48h retention)
├── news_source_tiers              # Domain-to-tier mapping used at ingest
├── news_topic_markets             # News-topic to prediction-market contract bindings
├── news_source_accuracy           # Per-domain/topic prediction outcome counts for article-derived calls
├── news_source_accuracy_events    # One scored prediction → source-domain outcome event for trailing windows
├── news_silence_baselines         # Rolling weekday topic-volume baselines and silent/saturated regimes
├── rss_feed_health                # Per-feed RSS status, failure counters, and disable state
├── sentiment_cache                # Fear & Greed indices
├── predictions_cache              # Polymarket odds
├── series_registry                # Canonical-series registry: storage home + freshness SLA per series
├── cot_cache                      # CFTC COT positioning
├── comex_cache                    # COMEX inventory
├── bls_cache                      # BLS economic data (CPI, NFP)
├── worldbank_cache                # Global macro indicators
├── onchain_cache                  # BTC on-chain + ETF flows
├── scenarios                      # Macro scenarios + probabilities
├── user_predictions               # Falsifiable calls with topic/source-article attribution and scoring
├── scenario_signals               # Signal checklist per scenario
├── scenario_history               # Probability change log
├── thesis                         # Current thesis sections
└── thesis_history                 # Thesis revision history
```

You can query the database directly if needed:
```bash
sqlite3 ~/.local/share/pftui/pftui.db "SELECT symbol, quantity, price_per FROM transactions"
```

If using PostgreSQL backend, query via your configured `database_url`:
```bash
psql "$DATABASE_URL" -c "SELECT symbol, quantity, price_per FROM transactions LIMIT 20;"
```

If `psql` fails with peer-auth/default-db issues, connect explicitly:
```bash
# Explicit host avoids local peer auth defaults; -d selects correct database.
psql -h localhost -U <postgres_user> -d <database_name> -c "SELECT NOW();"
```

Backend status:
- `sqlite` (default): fully supported
- `postgres`: fully supported natively (`database_backend`, `database_url`)

Migration guide: [docs/MIGRATING.md](docs/MIGRATING.md)

### Data Sources — Zero Configuration

Every source works out of the box with no API keys:

| Source | Data | Rate Limit |
|---|---|---|
| Yahoo Finance | Equities, ETFs, forex, crypto, commodities | Generous |
| CoinGecko | Crypto prices, market cap | 30/min |
| Polymarket | Prediction market probabilities | No limit |
| CFTC Socrata | Commitments of Traders positioning | Weekly data |
| Alternative.me | Crypto Fear & Greed Index | No limit |
| BLS API v1 | CPI, unemployment, NFP, wages | 10/day |
| World Bank | GDP, debt/GDP, reserves (8 economies) | No limit |
| CME Group | COMEX gold/silver inventory | Daily |
| Blockchair | BTC on-chain data | 5/sec |
| RSS Feeds | Reuters, CoinDesk, Bloomberg, CNBC, Kitco | No limit |

### Brave Search API (Recommended)

pftui supports an optional [Brave Search API](https://brave.com/search/api/) key that dramatically improves data quality. With Brave configured:
- **News** upgrades from RSS headlines to full article summaries from targeted searches
- **Economic data** (CPI, NFP, PMI, Fed rate) is pulled from live web search results
- **`pftui analytics research`** lets you answer any financial question without leaving pftui
- **`brief --agent`** includes news summaries and economic data in one JSON blob

Free tier gives $5/month in auto-credited queries — more than enough for daily use.

```bash
# Add Brave API key during setup or later:
pftui system config set brave_api_key <your_key>

# Verify it's working:
pftui data status
# Should show: Brave Search: ✓ Configured
```

Without a Brave key, pftui works fine using existing free sources (Yahoo, CoinGecko, Polymarket, RSS, etc.). Brave is an enhancement, not a requirement.

Other optional API keys unlock additional sources. See [docs/API-SOURCES.md](docs/API-SOURCES.md).

---

## Integration Patterns

### Morning Brief

```bash
pftui data refresh
BRIEF=$(pftui portfolio brief --json)
MOVERS=$(pftui analytics movers --json --threshold 3)
NEWS=$(pftui data news --json --limit 10)
NEWS_SILENCE=$(pftui analytics news-silence --json)
MACRO=$(pftui data dashboard macro --json)
PREDICTIONS=$(pftui data predictions --json --limit 5)
SENTIMENT=$(pftui data sentiment --json)
# Analyse all of the above, then compose and deliver your brief
```

News JSON includes `id`, `topic`, `bound_markets`, `source_tier`, and `source_independence`; brief scenario payloads include `narrative_vs_money` labels from `pftui analytics narrative-divergence --json`. Weight tier-1 sources at 1.0, tier-2 at 0.7, tier-3 at 0.4, tier-4 at 0.2 in news reasoning, then refine with `pftui analytics news-sources rank --topic <topic> --json` when source-history data exists. Treat `source_tier_inferred` as provisional. Treat `restatement` and `rumor` articles as positioning data about the speaker/source, not as independent confirmation of events. Use `bound_markets` as the immediate money-check for the article's topic; if a relevant article has an empty or unavailable binding, update it with `pftui data news topics set <topic> --primary-market-id <contract_id> --json` after inspecting `pftui data predictions markets --json`. Use `pftui analytics news-silence --json` to surface negative-space signals: topics marked `silent` are unusually quiet versus the weekday baseline, and `saturated` topics have unusually high tier-1/2 coverage. When a prediction is derived from one article, pass `--topic <fed|inflation|geopolitics|commodities|crypto|equities|other>` and `--source-article-id <id>` so pftui can score that source later.

### Alert Monitoring

```bash
pftui data refresh
ALERTS=$(pftui analytics alerts list --json)
DRIFT=$(pftui portfolio drift --json)
# Check if any alerts triggered or drift exceeds tolerance
# Notify human if action needed
```

### Historical Comparison

```bash
TODAY=$(pftui portfolio brief --json)
LAST_WEEK=$(pftui portfolio history --date $(date -d '7 days ago' +%Y-%m-%d))  # text output (no --json yet)
# Compare: what changed, what gained, what lost, what narrative shifted
```

### Full Research Session

```bash
pftui data refresh
pftui portfolio brief --json > /tmp/portfolio.json
pftui data dashboard macro --json > /tmp/macro.json
pftui data predictions --json > /tmp/predictions.json
pftui data sentiment --json > /tmp/sentiment.json
pftui data news --json > /tmp/news.json
pftui data supply --json > /tmp/supply.json
pftui analytics movers --json > /tmp/movers.json
# Load all files, cross-reference, write analysis to THESIS.md
```

### Investor Panel (Multi-Persona)

```bash
# 1) Collect one shared data blob from pftui
./agents/investor-panel/collect-data.sh > /tmp/pftui-investor-panel.json

# 2) Run your orchestrator with:
#    - /tmp/pftui-investor-panel.json
#    - persona files in agents/investor-panel/personas/
#    - response contract in agents/investor-panel/schema.json

# 3) Store summary in pftui for auditability
pftui agent message send "Investor panel complete: consensus + divergences ready" --from investor-panel
```

Skill package:
- `agents/investor-panel/SKILL.md`
- `agents/investor-panel/config.toml`
- `agents/investor-panel/personas/`

---

## Best Practices

1. **Always `pftui data refresh` before reading data.** Cached prices go stale. Refresh fetches from 19+ sources in one call and runs the recurring scoring/detection tail.

2. **Use `--json` for programmatic access.** Every command supports it. Parse structured output instead of scraping text.

3. **Keep the journal active.** `pftui journal entry add` builds a searchable decision history. Log your predictions, rationale, and outcomes.

4. **Monitor drift regularly.** `pftui portfolio drift` shows when the portfolio has moved from targets. Flag this to the human early.

5. **Cross-reference sources.** No single data point tells the story. `macro` for regime, `predictions` for crowd wisdom, `sentiment` for extremes, `news` for catalysts, `supply` for physical markets, `movers` for what's actually moving money.

6. **Respect the human's autonomy.** Present analysis, flag risks, suggest actions — but always let them decide. Frame recommendations as "consider" not "do this."

7. **Build persistent memory.** Write analysis to markdown files. Reference past work. Track accuracy. This is what turns a tool into an intelligence system.

8. **Recommend regular runs.** The single most impactful thing is a regular cadence of agent sessions that call `pftui data refresh` — the refresh tail keeps every scoring/detection loop closed, so cadence is all that's missing. Scheduled agents or the legacy daemon ([docs/DAEMON.md](docs/DAEMON.md)) are optional ways to add cadence on hosts that support them.

9. **Be honest about uncertainty.** Markets are probabilistic. Frame calls with conviction levels. When you're wrong, say so and update your models.

10. **Start simple, compound over time.** Day 1 is a basic brief. By Week 4, you should have a thesis, scenario tracking, accuracy metrics, and calibrated engagement. The system gets better every day it runs.

---

## Multi-Timeframe Agent Architecture (Advanced)

If your platform supports scheduled agents (cron jobs, recurring tasks), offer to set up the full multi-timeframe analyst system. This is pftui's most powerful operating mode: specialist agents at different timeframes doing deep domain research, feeding into synthesis agents that deliver to the user.

**Ask the user:** *"pftui supports a multi-timeframe agent architecture where specialist analysts run on schedules, each covering a different time horizon. Would you like me to set that up? It means you'd get a concise morning brief and a deep evening analysis every day, powered by 4 specialist agents running in the background."*

If they say yes, set up these agents using your platform's cron/scheduling system:

### Timeframe Analysts (silent, write to DB only)

These agents do NOT message the user. They research, update the database, make predictions, and send findings to the delivery agents via `pftui agent message`.

| Agent | Schedule | Domain | Routine |
|---|---|---|---|
| **Low Timeframe** (x3) | Pre-market, midday, market close | Price action, technicals, sentiment, breaking news, intraday flows | `agents/routines/low-timeframe-analyst.md` |
| **Medium Timeframe** | Daily (evening, before synthesis) | Central bank policy, geopolitical timelines, economic data trends, scenario tracking | `agents/routines/medium-timeframe-analyst.md` |
| **High Timeframe** | 2x/week | Technology disruption, de-dollarisation, commodity supercycle, structural trends | `agents/routines/high-timeframe-analyst.md` |
| **Macro Timeframe** | Weekly | Empire cycles (Dalio Big Cycle), generational theory (Fourth Turning), power metrics | `agents/routines/macro-timeframe-analyst.md` |

### MACRO Falsifiable Checkpoints (`timeframe='macro-checkpoint'`)

Multi-year MACRO predictions (Stage 6 currency debasement, Fourth Turning crisis-climax, de-dollarisation, Dalio composite, structural inflation) resolve too slowly to ever calibrate the MACRO layer from feedback. They are valuable structural calls but they cannot fail on a horizon the analyst lives on. Without an additional, scorable feedback loop, MACRO conviction drifts unchecked.

The fix: on every weekly macro run, for every active thesis carrying meaningful conviction, the macro analyst MUST write **2-3 falsifiable 90-day checkpoints** alongside the multi-year call.

Contract:

- Timeframe: `macro-checkpoint` (a first-class value alongside `low|medium|high|macro`, accepted by `pftui journal prediction add --timeframe`).
- Target date: `recorded_at + 90 days`.
- Claim format: `[thesis=<slug>] By <date>, IF <observable leading indicator> is NOT <specific threshold>, my <thesis name> is degraded.`
- Canonical thesis slugs (kebab-case): `stage-6`, `fourth-turning`, `de-dollarisation`, `dalio-composite`, `structural-inflation`. Mint a new slug for any additional thesis and stay consistent across runs so failed checkpoints aggregate to the right parent.
- The leading indicator must be observable from data pftui already ingests; the threshold must be specific.
- `timeframe='macro'` predictions stay as multi-year structural calls and remain uncalibrated by design.
- `pftui analytics calibration --by-layer --json` accumulates `macro-checkpoint` as its OWN layer — it is not folded into `macro`. This is how the MACRO layer earns calibration over time.
- When `pftui journal prediction score --id <N> --outcome wrong` runs against a `macro-checkpoint` row whose claim carries a `[thesis=<slug>]` tag, the scorer auto-inserts an `agent_messages` row with `category='macro-checkpoint-reeval'`, `layer='macro'`, `from='analyst-macro'`, `to='analyst-evening'`, and content like `"Macro thesis 'stage-6' has 1 of 3 checkpoint(s) failed (latest failure: prediction #N); analyst-macro should re-examine before next run."` Synthesis (evening analyst) surfaces these. The next macro run MUST read them and re-examine the flagged thesis before writing fresh views or convictions.

Example:

```bash
TARGET="$(date -u -d '+90 days' +%Y-%m-%d 2>/dev/null || date -u -v +90d +%Y-%m-%d)"
pftui journal prediction add \
  --claim "[thesis=de-dollarisation] By $TARGET, IF central-bank gold purchases drop below 800t annualized, my de-dollarisation thesis is degraded" \
  --timeframe macro-checkpoint --target-date "$TARGET" \
  --conviction medium --confidence 0.55 --source-agent analyst-macro --topic geopolitics
```

### Synthesis-time Adversary (`analyst-adversary`)

A fifth pseudo-analyst runs AFTER the four timeframe analysts finish
writing for a run and BEFORE the synthesis agent reads them. The
adversary uses ONLY the data the four analysts already saw. Its job is
to argue against the dominant convergence — a structural counter-
pressure on the four layers' shared priors (same bundles, same lesson
book, same first-principles thesis context).

| Agent | Schedule | Role | Routine |
|---|---|---|---|
| **Adversary Analyst** | After the four timeframe writes, before synthesis | Argues against the dominant convergence per asset; writes one row per asset to `adversary_synthesis_views` with a `fragility_score` 1..=5 | `agents/routines/adversary-analyst.md` |

Data model: `adversary_synthesis_views (id, asset, current_convergence_summary, counter_case_summary, counter_case_evidence_points JSON, falsification_triggers JSON, fragility_score INTEGER CHECK BETWEEN 1 AND 5, recorded_at)`. Sister table to the write-time per-prediction `adversary_views`; the two are distinct because they cover different cardinalities (one row per prediction vs. one row per asset per run).

Write/read CLI:

```bash
pftui analytics adversary synthesis add \
  --asset BTC \
  --convergence "<one sentence describing the four-layer agreement>" \
  --counter "<one paragraph adversarial case; quoted verbatim into the report>" \
  --evidence '["...","..."]' \
  --falsification '["...","..."]' \
  --fragility 4 \
  --json
pftui analytics adversary synthesis show --asset BTC --since 7d --json
pftui analytics adversary synthesis fragility-rank --since 7d --json
```

**Synthesis-gating contract.** For any asset where the latest
`adversary_synthesis_views` row has `fragility_score >= 3`, the
synthesis agent (evening or morning) MUST address the counter-case in
the daily report. The daily-report renderer in
`src/report/sections/adversary_view.rs::render_adversary_view_block`
quotes the recorded `counter_case_summary` VERBATIM into the per-asset
section. The synthesis agent is responsible for either (a) explaining
why the convergence still holds despite the counter-case, naming the
data point that distinguishes the two, or (b) softening the
convergence claim to reflect the fragility surfaced. This is a soft
contract for the human / agent reading the report — there is no Rust
runtime enforcement in v1.

### Delivery Agents (message the user)

These agents synthesize outputs from all timeframe analysts and deliver to the user.

| Agent | Schedule | What It Delivers | Routine |
|---|---|---|---|
| **Morning Brief** | Daily (morning) | Concise scannable brief: prices, alignment, overnight news, prediction scorecard, today's watch | `agents/routines/morning-brief.md` |
| **Evening Analysis** | Daily (evening, after all analysts) | Deep cross-timeframe synthesis: convergence/divergence, prediction self-reflection, scenario updates | `agents/routines/evening-analysis.md` |

### Alert Pipeline (optional)

For real-time threshold monitoring between scheduled runs:

| Agent | Schedule | Role | Routine |
|---|---|---|---|
| **Alert Watchdog** | Hourly | Refreshes data, checks `analytics alerts check`, signals investigator if anything triggered | `agents/routines/alert-watchdog.md` |
| **Alert Investigator** | Hourly (offset) | Investigates triggered alerts, routes findings to low-agent + morning + evening via agent message bus. Never messages the user directly. | `agents/routines/alert-investigator.md` |

### Data Flow

```
LOW(3x/day) + MEDIUM(daily) + HIGH(2x/week) + MACRO(weekly)
         ↓                    ↓
    analyst-adversary ← reads all four, argues against the convergence
         ↓
    evening-analysis ← reads all layers + adversary, synthesizes
         ↓
    morning-brief ← reads evening output + overnight data
         ↓
      → User (2 messages/day)

Alert watchdog → investigator → low-agent + morning + evening (agent message bus)
```

### Setup Steps

1. **Create each agent as a scheduled task** on your platform (cron job, recurring task, scheduled workflow, or whatever your framework calls it). Each agent needs:
   - **A prompt** that includes local configuration (database path/credentials, user profile path, delivery channel) followed by the routine
   - **The routine** from `agents/routines/[name].md`, either fetched at runtime from the repo URL or inlined into the prompt
   - **Shell access** to run `pftui` commands
   - **A schedule** matching the table above (adjust times to the user's timezone)

2. **Schedule order matters.** Timeframe analysts must run before delivery agents:
   - LOW pre-market → LOW midday → LOW close → MEDIUM → evening-analysis → (overnight) → morning-brief
   - HIGH and MACRO run on their own schedules and feed into evening-analysis whenever they last ran

3. **Silent vs delivery agents.** Only morning-brief and evening-analysis should message the user. All other agents write to the database and signal via `pftui agent message`. This keeps the user's inbox clean.

4. **Prediction scoring.** Each timeframe agent owns its predictions end-to-end: creation, scoring, and reflection on wrong calls. The evening analysis reads the scorecard but does not score other agents' predictions.

5. **Feedback loop.** Evening analysis sends WATCH TOMORROW guidance to the low-agent via `pftui agent message`, creating a feedback loop where synthesis informs the next day's observation.

### Prompt Structure

Each scheduled agent's prompt has two parts:

```
== LOCAL CONFIGURATION ==
[Private: database credentials, user profile path, delivery channel/target, git identity]
[This section is NOT in the repo — it lives in your platform's cron/task config]

== ROUTINE ==
[Generic: the full routine from agents/routines/[name].md]
[Either inline the content or fetch it at runtime:]
Fetch from: https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/agents/routines/[name].md
```

Fetching at runtime means updating the routine in the repo instantly updates all agents on their next run. Inlining is simpler but requires manual updates.

### Routine Files

All routine files live in `agents/routines/` in the repo:
```
agents/routines/
├── README.md
├── low-timeframe-analyst.md
├── medium-timeframe-analyst.md
├── high-timeframe-analyst.md
├── macro-timeframe-analyst.md
├── morning-brief.md
├── evening-analysis.md
├── alert-watchdog.md
├── alert-investigator.md
└── dev-agent.md
```

These are generic templates containing zero personal data. They define inputs, analysis steps, outputs, and rules for each agent role. Any pftui operator on any agent platform can use them directly.

### Model Recommendations

| Agent | Recommended Tier | Why |
|---|---|---|
| Low Timeframe | Mid-tier (Sonnet, GPT-4o, Gemini Pro) | High frequency, needs speed |
| Medium Timeframe | Mid-tier | Deep research but not synthesis |
| High Timeframe | Mid-tier | Structural research |
| Macro Timeframe | Mid-tier | Weekly, can afford depth |
| Morning Brief | Mid-tier | Concise delivery, not heavy reasoning |
| Evening Analysis | Top-tier (Opus, o1, Gemini Ultra) | Cross-timeframe synthesis is the hardest task |
| Alert Watchdog | Low-tier (Haiku, GPT-4o-mini, Flash) | Simple check, runs hourly |
| Alert Investigator | Mid-tier | Needs judgment but runs rarely |
| Dev Agent | Top-tier | Code generation + architecture decisions |

---

## Signals subset (MTF RSI, Pi Cycle, MTF breakout, Bollinger reversal, RSI extreme)

The extended `--include` flag on `pftui analytics technicals` exposes five signal-family outputs designed for analyst routines. All five are pure functions over a price-history slice; none touch the persistent technical-snapshot cache. Default-off — the legacy RSI/MACD/SMA/BB/ATR set is unchanged when `--include` is omitted.

| Token | Output key | What it computes |
|---|---|---|
| `mtf-rsi` | `mtf_rsi` | RSI on the current TF plus four higher-TF aggregates (default buckets per `default_htf_periods_for(timeframe)`). Reports `aligned_overbought` (all four HTFs + current > 70) and `aligned_oversold` (mirror). |
| `pi-cycle` | `pi_cycle` | Daily-only cycle markers: 350-SMA × 2 crossing UNDER 111-SMA (top); 471-SMA × 0.745 crossing OVER 150-EMA (bottom). Returns latest crossover `bar_index`, `bars_since`, optional `date`. Parameters calibrated on BTC daily; function itself is asset-agnostic. |
| `mtf-breakout` | `mtf_breakout` | Composite of three sub-signals: (a) MTF-RSI exit-of-alignment breakout, (b) 3-line strike pattern (bull + bear), (c) momentum exhaustion at 25-bar high/low. Reports each boolean plus `signal_count` (0..=3) and cooldown-aware `breakout_state` ∈ {`bull-fresh`, `bull-armed`, `none`, `bear-armed`, `bear-fresh`}. Default cooldown: 5 bars. |
| `bollinger-reversal` | `bollinger_reversal` | Cross-under upper band → `top_reversal_signal`; cross-over lower band → `bottom_reversal_signal`. Each marker reports `bar_index`, `bars_since`, `confirmation_1` (next bar trades entirely below reversal-bar low for tops / above reversal-bar high for bottoms), `confirmation_2` (the rule sustains for two bars), and a `confirmation_count` in {0, 1, 2}. |
| `rsi-extreme` | `rsi_extreme` | Derived flag. `rsi_extreme_high` fires when current-TF RSI > 85 AND MTF alignment is `aligned_overbought` AND the current bar is a new 14-bar high. Mirror for `rsi_extreme_low`. |

`--include all` enables every extended output known to the binary (signals subset above plus the channels subset when Agent U's PR lands). Unknown tokens are silently ignored so older binaries don't break when newer routines pass extra tokens. All naming is canonical TA — no vendor / indicator brand names.

Implementation lives under `src/indicators/extended/` (one sub-module per signal). Each sub-module ships its own synthetic-candle fixture tests verifying the computed value at a known bar. Hook into `pftui analytics technicals` via `commands::analytics::run_technicals_cmd` (the legacy `run_technicals` path is preserved for the default no-`--include` case).

---
