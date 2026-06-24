# 2026-06-23 Capability Audit Findings

Scope: review of market cycles, technical analysis, indicator primitives, strategy backtesting, and the cycle-bottom signal suite.

Data handling: this review did not inspect portfolio holdings, transactions, cost basis, position sizes, PnL, or other private portfolio data. Findings are based on code, docs, tests, and public market-series analytics.

## Prioritized Findings

### Market Cycles

- P1: Fresh-install `--json` commands can print first-launch setup prompts before the JSON envelope, breaking agent parsing. Repro was seen with temporary `HOME` on `analytics cycles bottom-signals --json --cached-only` and `analytics strategy backtest --json --cached-only`. Suppress onboarding on non-interactive/JSON commands or keep it strictly on stderr after TTY detection.
- P1: Some user-facing cycle CLI text still exposes prohibited labels such as `BTC`, `halving clock`, and raw `series`. The TUI cycle page is much cleaner, but CLI/report prose should use friendly names and functional labels.
- P2: `cycle_engine.rs` still contains production-path `expect()` calls protected by invariants. Replace with guarded branches or `Option`/`Result` plumbing to match the project rule.

Solid: P15/P85 bands, small-n flags, documented anchor verification, bar-to-unit conversion, and “window, never price/date prediction” discipline are present.

### Technical Analysis

- P1: CyberDots is not a literal Pine port where documentation implies it is. Rust intentionally seeds the SuperTrend direction as `1`, while the Pine source has a problematic `nz(dir[1])` reassignment. This is probably a pragmatic fix, but it must be documented as a semantic adaptation and covered with exported Pine golden vectors.
- P1: `technicals indicators` computes range indicators on close-collapsed bars even when OHLC coverage is poor. Votes are gated, but JSON still returns numeric range fields. Prefer `null` for unreliable range-family values or nest under a degraded block.
- P2: Public/report copy still uses names like “Cyber Dots” and some raw symbols. Render functional labels in user-facing text and keep technical source IDs internal or JSON-only.

Solid: Gaussian band constants, CyberLine mapping, MTF completion-date aggregation, and coverage flags are generally well designed.

### Indicator Primitives

- P1: DSS “faithful port” is overstated. Rust substitutes `50.0` for flat stochastic windows, which stabilizes pathological inputs but changes Pine semantics. Either match Pine `na` behavior or document this as a robust variant.
- P1: Pine-fidelity tests are mostly behavioral, not golden. Add CSV/JSON golden vectors for ERF, DSS, CyberBands, CyberLine, CyberDots, Pi-cycle, and MTF RSI.
- P2: `f64` is used for several price-like values in technicals/backtesting. Clarify the policy boundary and consider serializing price fields as `Decimal` where they are displayed as prices rather than pure statistics.

Solid: primitives are mostly pure/deterministic, insufficient input often returns `None`, and the strategy resolver’s OHLC/volume coverage gate is a strong guardrail.

### Backtesting

- P1: Sortino is documented as annualized/risk-adjusted CAGR but implemented as per-trade mean divided by per-trade downside deviation. Rename to `per_trade_sortino` or compute a time-based Sortino.
- P1: Walk-forward partitions full-history completed trades by entry date only. A train trade can exit in the next test segment, leaking future bars into in-sample parameter selection. Assign trades only when entry and exit are inside the segment, or recompute each fold with an `as_of` cutoff.
- P1: Sweep/DSR uses per-trade Sharpe across configs with potentially different holding periods. JSON should explicitly label this as per-trade/not time-based, or DSR should be computed on daily strategy returns.
- P2: Non-finite numeric parameters/results should be rejected or sanitized before sort/serialization paths using `partial_cmp(...).unwrap()`.

Solid: next-bar fill exists, same-bar fill is disclosed, downside-first stop/target ordering is conservative, missing cross-symbol history errors loudly, and higher-timeframe projection uses completed buckets.

### Cycle-Bottom / Cycle-High Signals

- P1: The bottom suite is much more developed than the high/exhaustion side. There is no symmetric cycle-high N-of-N engine, JSON, alert set, or reliability backtest. Build one using momentum rollover, DSS topping, roofing filter rollover, bearish bands, higher-timeframe down dots, weekly line loss, Pi top bonus, and translation/failed-cycle context.
- P2: Reliability backtest uses a 7-day stride, which can miss transient daily signal fires. Evaluate daily for daily timeframe or emit `eval_stride_days` and caveat the missed-transient risk.

Solid: ordered `criteria[]`, stable keys, non-counted Pi-cycle bonus, small-n caveat, and point-in-time `history[..=i]` tests are good.

## Roadmap

1. Fix JSON stdout contamination on first launch.
2. Remove walk-forward fold leakage.
3. Add Pine golden-vector tests.
4. Rename/recompute per-trade statistical metrics.
5. Build a symmetric cycle-high/exhaustion suite and reliability backtest.
6. Clean public copy for names/raw tickers.
7. Remove production-path `unwrap()`/`expect()` in reviewed paths and reject non-finite `f64` inputs.

## Real-Data Test Addendum

After the static audit, the reviewed functionality was exercised against the normal pftui cached market database using analytics/market-series commands only. No portfolio holdings, transactions, cost basis, position sizes, or PnL tables were queried or surfaced.

Commands/classes exercised successfully:

- `analytics cycles clock --asset BTC --json --cached-only`
- `analytics cycles analyze --asset BTC|GC=F|SI=F --json --cached-only`
- `analytics cycles ledger --asset BTC --degree 4-year --json --cached-only`
- `analytics cycles bottom-signals --asset BTC|gold|SI=F --timeframe daily|monthly|weekly --json --cached-only`
- `analytics cycles bottom-signals backtest --asset BTC|gold --json --cached-only`
- `analytics technicals indicators BTC|GC=F --json --cached-only`
- `analytics technicals structure BTC --timeframe weekly --json --cached-only`
- `analytics technicals cyber BTC|GC=F --timeframe daily|weekly --json --cached-only`
- legacy `analytics technicals --symbol BTC,GC=F --include all --json --cached-only`
- `analytics strategy explain`, `backtest`, `segment`, `compare`, `sweep`, and `walkforward` on BTC/GC=F/rate-alias expressions
- strategy edge cases: missing cross-symbol history, division-by-zero expression, cross-symbol OHLC indicators

Real-data observations:

- BTC cycle analysis resolved to the deep `BTC-USD` public market series and emitted three degrees; the 4-year ledger emitted two completed rows; monthly bottom-signals emitted exactly 7 criteria and `0/7`; bottom-signal backtest emitted 3 verified anchors with `small_n=true`.
- Gold and silver cycle analysis work with canonical `GC=F` / `SI=F`, but `cycles analyze --asset gold` and `--asset silver` fail even though help advertises `gold` as an alias. This upgrades the alias inconsistency from theoretical doc drift to a real CLI bug.
- Gold weekly bottom-signal backtest emitted 3 anchors and `small_n=true`, as expected.
- BTC technical indicator panel emitted a valid full panel with real OHLC/volume coverage; GC=F emitted `range_indicators_degraded=true` and `volume.has_volume=false`, confirming degradation is active on sparse data.
- `technicals cyber` emits `last_date` rather than a common top-level `as_of`, and its JSON keys are `bands_gaussian` / `bands_zone` rather than nested `bands.*`. This is parseable but inconsistent with other analytics commands.
- `technicals structure` uses `structure` for the trend classification rather than `trend`; the command is parseable but field naming is easy for agents to miss.
- Strategy backtest on BTC with costs and next-bar fill completed with 63 trades and Monte Carlo present; missing-symbol simulation exits nonzero with a structured JSON error; `strategy explain` reports missing symbols structurally with exit 0; divide-by-zero arithmetic propagated to no trades rather than a crash.
- Strategy sweep over BTC RSI thresholds returned a best value but DSR below the pass bar, and walk-forward returned WFE around the fragile range, which confirms the honesty surfaces activate on real data.

Targeted tests:

- `cargo test ehlers_roofing` passed.
- `cargo test dss_bressert` passed.
- `cargo test pi_cycle` passed.
- `cargo test cycle_engine` passed.
- `cargo test cycle_signals` passed.
- `cargo test strategy` passed.
- `cargo test cyber` hit a clap deep-tree stack overflow under default stack, then passed with `RUST_MIN_STACK=8388608`.

## Follow-Up Audit After Cycle-Watch Alert Wiring

Date: 2026-06-23. Scope: re-audit of cycles, technical analysis, indicator primitives, backtesting, and Bitcoin cycle high/low indicators after adding the focused monthly cycle-watch report and alert components.

Data handling: CLI probes used normal cached public market-series analytics only. No portfolio holdings, transactions, cost basis, sizes, or PnL were queried or quoted.

Verification baseline:

- `cargo clippy --all-targets -- -D warnings` passed.
- `RUST_MIN_STACK=8388608 cargo test` passed: 4175 unit tests, 0 failed, 5 ignored; integration/doc/schema suites also passed.
- Created live alert rules for the four monthly composite criteria and six monthly atomic components:
  - composites: `cycle_criterion_monthly_momentum_turning_up`, `cycle_criterion_monthly_roofing_confirming_up`, `cycle_criterion_monthly_momentum_above_price`, `cycle_criterion_monthly_dss_bottoming`
  - components: `cycle_component_monthly_rsi_ma_turned_up`, `cycle_component_monthly_rsi_ma_cross_above_rsi`, `cycle_component_monthly_erf_bottom_zone`, `cycle_component_monthly_dss_cross_above_trigger`, `cycle_component_monthly_erf_turned_up`, `cycle_component_monthly_dss_turned_up`
- The installed PATH binary and `target/release/pftui` were rebuilt to the same checksum.

### Market Cycles

- P1: Cycle clock output still violates the user-facing copy rule by emitting prohibited framework/person labels and `halving` vocabulary.
  - Evidence: `src/analytics/cycle_clock.rs:44-50`, `src/analytics/cycle_clock.rs:78-110`, `src/analytics/cycle_clock.rs:163-234`, `src/analytics/cycle_clock.rs:438-486`, and `src/commands/cycle_clock_cmd.rs:58-95`.
  - Repro: `pftui analytics cycles clock --asset BTC --cached-only --json` emitted `halving_date`, `days_since_halving`, `olson_day900_date`, `loukas`, `mayer_multiple`, and factors such as `Loukas: IN the low band...` and `Mayer 0.82...`.
  - Why it matters: this directly conflicts with the project rule that UI/report copy be free of practitioner/author names and `halving`. Agents consuming JSON can accidentally repeat those strings in reports.
  - Recommended fix: split internal field names from presentation fields. Keep raw technical fields under `developer_context` or `source_model`, and add a sanitized `public_read` block with functional names such as `cycle_event_anchor`, `calendar_window`, `long_cycle_band`, and `value_vs_200d_average`. Update report prompts to consume only sanitized fields.

- P1: `cycles analyze --asset gold` fails even though nearby cycle commands and docs use friendly asset aliases.
  - Evidence: `src/commands/cycle_engine_cmd.rs:21-39` uppercases the input and only tries `<SYM>-USD` fallback; it does not run the shared alias resolver. `pftui analytics cycles analyze --asset gold --cached-only --json` returned `no price history for GOLD`.
  - Why it matters: agents will naturally use `gold` because other commands and docs advertise it. This is an avoidable command inconsistency in a core workflow.
  - Recommended fix: resolve friendly assets before `load_series`: `BTC -> BTC-USD`, `gold -> GC=F`, `silver -> SI=F`, while preserving explicit symbols. Add CLI tests for `BTC`, `gold`, `silver`, `GC=F`, and `SI=F`.

- P2: Production-path `expect()` still exists in cycle engine invariants.
  - Evidence: `src/analytics/cycle_engine.rs:681`, `src/analytics/cycle_engine.rs:740`, `src/analytics/cycle_engine.rs:836`, `src/analytics/cycle_engine.rs:864-865`.
  - Why it matters: the code is mostly invariant-safe today, but the repository rule says no production `unwrap()`/`expect()`. Audited market engines should fail with structured insufficiency, not panic if a future data path breaks an assumption.
  - Recommended fix: replace with explicit empty/non-finite guards returning `None` or degraded `BandStats`; use `total_cmp` or pre-filter finite lengths before sorting.

Solid:

- The engine’s timing discipline is fundamentally sound: it emits windows/positions, not price predictions.
- Small-n band basis is explicit (`empirical-p15-p85` vs `small-n...`).
- TUI cycle matrix has tests for the prior raw-bar-count display bug (`cycles_matrix_age_is_never_a_raw_bar_count`).

### Technical Analysis

- P1: Cyber zone-band JSON can emit an impossible channel: `outer_lower > outer_upper`.
  - Evidence: `src/analytics/cyber/bands.rs:357-414`. Weekly BTC repro: `bands_zone.inner_lower=70382.8`, `inner_upper=104675.22`, `outer_lower=70382.8`, `outer_upper=62784.13`.
  - Why it matters: agents or dashboards reading channel levels cannot safely assume lower <= upper. This can invert support/resistance logic and produce nonsensical risk levels.
  - Recommended fix: represent the two Pine zone boundaries as named raw lines if ordering is meaningful, or normalize serialized `lower/upper` pairs while keeping `zone_state` separate. Add a test asserting every exported lower/upper pair is ordered.

- P1: `technicals indicators` computes and serializes range-family numbers even when it has declared the range inputs degraded.
  - Evidence: `src/commands/technicals_indicators.rs:30-88` substitutes close for missing high/low and computes Stochastic, Williams %R, CCI, ADX/DMI, and ATR. It gates scorecard votes at `src/commands/technicals_indicators.rs:117-140`, but JSON still emits values at `src/commands/technicals_indicators.rs:160-183`.
  - Repro: `pftui analytics technicals indicators GC=F --cached-only --json` showed `coverage.range_indicators_degraded=true`, `coverage.ohlc=0.7666`, but still emitted `stoch_k`, `williams_r_14`, `cci_20`, `adx_14`, and `atr_14`.
  - Why it matters: machine consumers may use those numeric fields as real signals despite the degradation flag. Hedge-fund-grade agent surfaces should prefer absent/null data over plausible-looking synthetic values.
  - Recommended fix: when `!ohlc_ok`, set range-family JSON fields to `null` and optionally include a `degraded_values` debug block behind a flag.

- P2: Monthly Cyber inspection is missing from the general Cyber CLI.
  - Evidence: `src/analytics/cyber/mod.rs:76-92` only parses daily/weekly. Repro: `pftui analytics technicals cyber BTC --timeframe monthly --cached-only --json` errors with `unknown timeframe 'monthly'`.
  - Why it matters: the cycle-bottom suite has a monthly workflow, but the user cannot inspect the same Cyber primitives through the general TA command at monthly granularity.
  - Recommended fix: add `CyberTimeframe::Monthly`, aggregate completed monthly bars, and add no-lookahead MTF tests for daily/weekly/monthly aggregation.

Solid:

- The Cyber module is deterministic and well decomposed.
- MTF code has tests for completed-bucket behavior.
- Scorecard already excludes degraded range-family votes; the issue is JSON exposure, not the vote logic.

### Indicator Primitives

- P1: DSS is documented as a faithful Pine port but intentionally changes flat-window stochastic semantics.
  - Evidence: `src/indicators/dss_bressert.rs:1-16` says faithful, while `src/indicators/dss_bressert.rs:66-72` documents `50.0` for flat windows because Pine would emit `na`/0.
  - Why it matters: this is a legitimate robustness adaptation, but it is not Pine fidelity. For cycle-low alerts, a flat or low-volatility synthetic series can behave differently than the TradingView reference.
  - Recommended fix: rename the implementation claim to “Pine-inspired robust port,” or add a strict mode matching Pine `na` propagation. Add golden-vector tests exported from TradingView for normal, flat, and V-bottom cases.

- P1: Pine-fidelity coverage is still mostly behavioral rather than golden-vector based.
  - Evidence: tests exercise shape/properties in modules like `src/analytics/cyber/bands.rs`, `src/analytics/cyber/line.rs`, `src/indicators/ehlers_roofing.rs`, and `src/indicators/dss_bressert.rs`, but there are no committed reference output vectors next to `docs/reference/*.pine`.
  - Why it matters: behavioral tests can pass while coefficient, seed, or warm-up drift persists. This is the highest-leverage way to close the “does pftui match my chart?” gap.
  - Recommended fix: generate CSV goldens from the Pine scripts over one synthetic series and one real public BTC sample; assert per-bar tolerances after warm-up for ERF, DSS, Gaussian/Zone bands, CyberLine, CyberDots, MTF RSI, and Pi-cycle.

- P2: The ERF “bottom color” semantics are now operator-correct for the cycle-watch use case, but they are not the same as the reference Pine comment.
  - Evidence: the cycle signal now uses `erf < 0 && turned_up` for `roofing_confirming_up`, and monthly BTC currently reports `erf_bottom_zone=true` and `erf_turned_up=true` with `erf=-25006.78`. The reference Pine comment describes green at `erf >= 0`.
  - Why it matters: this is a deliberate trading-criterion decision, not a neutral indicator port. It needs to stay clearly documented so future maintainers do not “fix” it back to the Pine color comment.
  - Recommended fix: keep the primitive neutral, keep cycle-watch semantics in `cycle_signals`, and add a fixture test that asserts the monthly bottom criterion means “negative/bottom-zone and turning up.”

Solid:

- Most primitives are pure, deterministic, and return `None` on insufficient input.
- ERF recurrence has hand-check tests.
- RSI/RSI-MA/DSS/ERF component booleans now surface cleanly in `core_watch[]`.

### Backtesting

- P1: Walk-forward optimization can leak future exits into train selection.
  - Evidence: `src/commands/strategy.rs:602-609` documents full-history backtesting followed by entry-date partitioning. Code runs each parameter once over full history at `src/commands/strategy.rs:652-671`, then filters by entry date at `src/commands/strategy.rs:689-698`.
  - Why it matters: for `hold 90d` or risk exits, a trade entered near the end of the train segment can exit in the next test segment. Its return then influences in-sample parameter selection using test-period prices.
  - Recommended fix: for fold scoring, either require both entry and exit inside the segment, or recompute each fold with an `as_of` cutoff and only score closed trades whose exit is <= segment end. Add a regression test with a trade entered in train and exited in test.

- P1: Walk-forward labels can look more robust than the sample supports.
  - Evidence: live probe `pftui analytics strategy walkforward --asset BTC --entry 'rsi(14) < $P' --values '20,25,30,35,40' --exit 'hold 90d' --cached-only --json` returned WFE `0.681`, but every fold had only 5-8 in-sample trades and 5-6 out-of-sample trades.
  - Why it matters: WFE on five trades per fold is mostly noise. The current JSON has counts but no classification warning for “passes threshold but sample too thin.”
  - Recommended fix: add `validation.small_sample=true` when any qualifying fold has <20 OOS trades or total OOS trades <30; classify WFE as `anecdotal`/`fragile` unless the sample clears that bar.

- P1: Strategy validation can overemphasize PSR when confidence intervals say edge is uncertain.
  - Evidence: live backtest for BTC `rsi(14) < 30`, 90-day hold, costs and next-bar fill returned `validation.psr_vs_zero=0.967`, but `mean_return_ci_pct=[-1.365, 33.627]`.
  - Why it matters: the bootstrap CI straddles zero. A report that foregrounds PSR alone can overstate confidence.
  - Recommended fix: add `validation.edge_uncertain=true` whenever the mean-return CI crosses zero; rank this warning above PSR in prose/report rendering.

- P2: Backtest JSON puts core metrics under `report`, while some other analytics commands use top-level metric fields.
  - Evidence: `strategy backtest --json` keys are `asset`, `command`, `costs`, `entry`, `exit`, `report`, `resolved_symbol`; `n_trades`, `validation`, and `monte_carlo` are nested under `report`.
  - Why it matters: not a correctness bug, but agent ergonomics suffer because related commands have inconsistent envelope conventions.
  - Recommended fix: document the schema explicitly and/or add a thin top-level `summary` object with stable fields.

Solid:

- Next-bar fill exists and is explicit.
- Costs are modeled per side and echoed in the JSON.
- Buy-and-hold benchmark, drawdown metrics, Kelly, and Monte Carlo are present and useful.
- Missing referenced symbols error loudly instead of silently producing zero trades.

### Bitcoin Cycle High / Low Indicators

- P0/P1: There is still no symmetric cycle-high/exhaustion suite.
  - Evidence: repository search found bottom suite code (`src/analytics/cycle_signals.rs`, `src/analytics/cycle_signal_backtest.rs`) and generic high/exhaustion primitives (`src/analytics/cyber/breakout.rs`, `src/indicators/extended/mtf_breakout.rs`, Pi-cycle top), but no `cycles top-signals`, no high-side N-of-N JSON, no high-side alerts, and no high-side reliability backtest.
  - Why it matters: pftui can now actively watch bottoms, but a professional market tool also needs distribution/exhaustion monitoring. The asymmetry biases the system toward accumulation narratives.
  - Recommended fix: build `cycle_top_signals` with stable keys and alerts. Suggested criteria: RSI average rolling over, RSI average below RSI or bearish cross depending on doctrine, DSS topping/cross-down from overbought, ERF positive-zone rollover, bearish CyberBands, weekly/monthly down dots, weekly line loss, Pi-cycle top bonus, plus cycle-engine translation/over-band context. Add a no-lookahead reliability backtest around verified cycle highs.

- P1: The bottom-signal reliability backtest currently argues for humility, not confidence.
  - Evidence: `pftui analytics cycles bottom-signals backtest --asset BTC --timeframe monthly --cached-only --json` returned `small_n=true` with 3 anchors. Precision was 0% for `momentum_turning_up`, 0% for `momentum_above_price`, 9% for `dss_bottoming`, 25% for `roofing_confirming_up`; confluence precision was 4% for `>=3`, 3% for `>=4`, and 15% for `>=5`.
  - Why it matters: the suite is useful as a current-state checklist, but its measured historical precision is weak under the current matching methodology. Reports should not imply statistical reliability.
  - Recommended fix: promote the backtest caveat into report prose and JSON summary. Consider evaluating sequence persistence, consecutive-month confirmation, and “within prior bear drawdown context” filters rather than simple rising-edge matching.

- P1: Backtest edge detection evaluates every 7 days and can miss transient daily criteria.
  - Evidence: `src/analytics/cycle_signal_backtest.rs:51-55` sets `EVAL_STRIDE_DAYS=7`; point-in-time loop advances by that stride at `src/analytics/cycle_signal_backtest.rs:342-394`.
  - Why it matters: weekly/monthly reads are likely OK, but daily bottom-signal reliability can miss short-lived fires or mis-date rising edges.
  - Recommended fix: stride by timeframe: daily=1, weekly=7 on completed weeks, monthly=month end. Serialize `eval_stride_days` in JSON.

- P1: Reports are still not deterministically hard-wired to include the new `core_watch[]`.
  - Evidence: daily report enrichment in `src/report/build/daily.rs:6403-6475` loads market structure, daily Cyber verdict, and cycle engine/clock verdict, but does not call `cycle_bottom_signals` or include the four-item cycle-watch progress. Agent prompts reference bottom-signals, but report generation does not enforce it.
  - Why it matters: the CLI can track the four checks now, and alerts exist, but a report can still omit them if the agent prompt path drifts.
  - Recommended fix: add a first-class “Cycle Watch” block to the report model for Bitcoin and gold, sourced from `CycleBottomSignals.core_watch`, with `met_components/total_components`, current values, and alert status.

- P1: Agent prompt copy still instructs analysts to use prohibited names in BTC report context.
  - Evidence: `agents/report-prompts/phase3-synthesis-writer.md:62` asks for “Loukas / Camel Finance / Olson / Cowen”; `agents/routines/high-timeframe-analyst.md:274-282` instructs use of `halving`, `Olson`, `Loukas`, and `Mayer`.
  - Why it matters: even if Rust CLI output is cleaned, the agent layer can reintroduce banned public-report copy.
  - Recommended fix: move named-framework detail to private/developer context and require public prose to use functional labels only.

Solid:

- The focused monthly `core_watch[]` now tracks exactly the four operator checks: RSI average turn-up, RSI average cross/reclaim, DSS turn/cross, and ERF bottom-zone turn-up.
- Alerts can now be raised at both composite and atomic component levels.
- Current monthly BTC state is mechanically clear: `1/7` full suite; `core_watch` has ERF bottom-zone turn-up complete, while RSI and DSS checks are not yet complete.
- Cycle-bottom JSON has stable ordered `criteria[]`, stable component keys, and a non-counted Pi-cycle bonus.

## Highest-Leverage Roadmap From This Follow-Up

1. Build the symmetric `cycles top-signals` / high-exhaustion suite with alerts and no-lookahead reliability backtest.
2. Fix report hard-wiring so every Bitcoin/Gold report includes the four-item `core_watch[]` progress block.
3. Fix walk-forward fold leakage and add small-sample classification for WFE.
4. Add Pine/TradingView golden-vector tests for DSS, ERF, CyberBands, CyberLine, CyberDots, MTF RSI, and Pi-cycle.
5. Normalize user-facing copy: remove prohibited names and `halving` from cycle clock CLI, TUI/report surfaces, and agent prompts.
6. Fix Cyber zone-band ordering or rename raw boundaries so agents cannot misread them as ordered channels.
7. Null out degraded range-family indicator fields in JSON when OHLC coverage is below threshold.
8. Add monthly Cyber CLI support to make monthly cycle primitive inspection symmetrical with bottom-signals.
