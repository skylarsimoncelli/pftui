# POSITIONING-MODELS.md — Portfolio Strategy Modeller (design + staged build)

> Status: **design / build-in-progress**. This is the canonical design for the
> positioning modeller. Read before touching `src/analytics/portfolio_sim/` (the
> simulator), `src/models/positioning_model.rs` (the spec), or `analytics models`
> CLI. Two independent design reviews (Codex GPT-5.x, sessions `019f039e…`) shaped
> the contracts here — their corrections are folded in and called out as **[R]**.

## 1. Objective

Define, track, and **backtest** multiple portfolio **models** (strategies). A model =
a **base diversification structure** + a handful of **rebalancing rules**. It must be
asset-agnostic, walk-forward-clean, and generic enough to: compare many models, and
eventually (P5) **search a model's numeric parameters** for an optimal configuration —
without overfitting.

Operator's three reference models (the acceptance set):
- **M1** — hold 20–40% cash always; remainder split equities/bonds by risk-on/off
  (from rates + factors).
- **M2** — add to hard money (BTC, gold) on cycle dips; raise cash on overheated
  cycle tops; uses the backtested cycle top/bottom signal suite.
- **M3** — never average down; enter only on a weekly Weinstein **stage-2** breakout;
  fully exit on a weekly **stage 3→4** breakdown; CyberDots trackline + dots for
  micro-trend confluence.

## 2. What already exists (reuse map — verified by code read)

| Primitive | Reuse as | Caveat |
|---|---|---|
| `src/analytics/strategy/` DSL (parser/eval) | the `when` **condition language** | parser has **hardcoded fn enums**, not a registry — accessors need a typed evaluator boundary **[R]** |
| `cycle_{top,bottom}_signals`, `cyber::analyze`, `market_structure::analyze` | **point-in-time signal layer** for rules | they aggregate the **partial** current week/month — must be wrapped to **completed-bucket** semantics **[R]** |
| `src/regime/` 9-signal risk-on/off + `regime_quad.rs` Quad | M1's regime read | `compute_regime` is **not as-of aligned** — needs a `RegimeAtDate` wrapper **[R]** |
| `src/db/allocation_targets.rs` band math (`floor/ceiling`, `rebalance_pct_for_actual`) | conceptual band model | single-target only; not a multi-bucket solver |
| `TradeReport` stats (Sharpe/Sortino/Calmar/maxDD/CDaR/MC/PSR) | low-level stat fns **after refactor** | `TradeReport` itself is **per-trade-exit, NOT daily MTM** — do **not** extend it; build a new report **[R]** |
| `models/position.rs` (Decimal cost-basis, FX `fx_rate`/`native_currency`), paired cash legs in `add_tx.rs` | the **ledger/FX model** | average-cost (no tax lots) — `no_average_down` defined against this **[R]** |
| `portfolio_snapshots`/`position_snapshots` | validation vs real history | — |
| `viz/backtest_viz.py` inline-SVG style | a **new** portfolio tearsheet contract | don't reuse the single-asset tearsheet shape **[R]** |

**The gap:** every existing backtester is single-asset, one-position, no cash/rebalance.
The **portfolio simulator is the one genuinely new engine.**

## 3. Architecture

### 3.1 Model spec — canonical **TOML in repo** (not SQLite spec_json) **[R]**
Diffable, reviewable, machine-generatable for P5. SQLite stores only a **hash + run
provenance**, never the source of truth. Specs live in `models/` (repo dir).

```toml
[model]
name = "m2-hard-money-cycles"
version = 1
base_currency = "USD"

[universe]                       # assets the model may hold + their class
assets = [ { symbol = "BTC-USD", class = "hard_money" }, { symbol = "GC=F", class = "hard_money" },
           { symbol = "SPY", class = "equity" }, { symbol = "IEF", class = "bond" } ]
cash_class = "cash"

[base_policy]                    # class weights incl. cash sum to 1.0
targets = [ { class = "cash", target = 0.30, floor = 0.20, ceiling = 0.40 },
            { class = "hard_money", target = 0.20, floor = 0.10, ceiling = 0.50 },
            { class = "equity", target = 0.35, floor = 0.0, ceiling = 0.60 },
            { class = "bond", target = 0.15, floor = 0.0, ceiling = 0.40 } ]
within_class = "equal"           # equal | fixed | by_param — how a class weight splits across symbols

[constraints]
max_position = 0.40              # per symbol
no_average_down = false
rebalance_cadence = "weekly"     # daily | weekly | monthly | on_signal
rebalance_band_mode = "to_target" # no_trade_zone | to_edge | to_target
fill = "next_close"              # next_close (default, lookahead-safe) | same_close (optimistic)
cash_yield_proxy = "BIL"         # symbol whose return the cash bucket earns, or "none" [R]

[[rules]]
id = "add-hard-money-on-dip"
when = "cycle_bottom_met('BTC-USD') >= dip_threshold"
then = { kind = "tilt", class = "hard_money", by = "tilt_size", from = "cash" }
priority = 10
cadence = "weekly"

[[rules]]
id = "raise-cash-on-top"
when = "cycle_top_met('BTC-USD') >= top_threshold"
then = { kind = "tilt", class = "hard_money", by = "-tilt_size", to = "cash" }
priority = 10

[params]                         # the P5 optimization surface — rules/policy reference these by name
dip_threshold = 5
top_threshold = 5
tilt_size = 0.10
```

### 3.2 Action algebra + target solver (per rebalance) — **phased, then projected** **[R]**
Operates on a **two-layer** weight model: **class weights**, then **symbol weights
within a class budget** (never a flat mix — symbol actions are intra-class unless
declared `scope = "global"`) **[R]**.

Actions: `set_target(key, v)` (anchor) · `tilt(class, by, from/to=cash)` (delta) ·
`add/trim/exit(symbol, …)` (intra-class) · `gate_block(symbol|class)` (veto).

Solver at rebalance date T — **strict phase order, deterministic**:
1. **Seed** working class-vector `W` = `base_policy.targets`.
2. **Evaluate** every rule's `when` on `history[..=T]` (completed-bucket projection §3.4).
   Collect fired rules (sorted priority asc, then id).
3. **Phase A — anchors:** apply `set_target` in order. Warn on same-key/same-priority
   double-set (model-hygiene error) **[R]**.
4. **Phase B — tilts:** apply `tilt` deltas in order onto the key and its `from/to`
   offset key. (Disjoint from Phase A so behaviour isn't priority-trivia **[R]**.)
5. **Phase C — symbol actions + gates:** record intra-class symbol overrides; mark
   `gate_block` keys. **Gates set ceiling = 0 BEFORE projection** (not after) **[R]**.
6. **Phase D — bounded projection (the normalizer):** **not** fixpoint iteration.
   - Assert feasibility: `Σ floors ≤ 1 ≤ Σ ceilings` across all active buckets (incl.
     cash). If infeasible → emit `infeasible` flag, **hold prior weights**, log **[R]**.
   - Project the desired class-vector onto the box `[floor_i, ceiling_i]` ∩ simplex
     (minimise squared distance; water-filling). Deterministic tie-break by stable key.
   - Then project **symbol** weights within each class's solved budget the same way.
7. **Bands at every bucket** (class + symbol + cash); **symbol hard constraints
   override class no-trade-zone** **[R]**. `rebalance_band_mode` decides which legs
   move and to edge/target.
8. `no_average_down` veto: block a BUY iff `position_qty(sym) > 0 AND
   fill_price_base < avg_entry_price_base(sym)` — literal "don't add below your average
   cost", against the **simulator's own average-cost ledger in base currency** (no tax
   lots). Optional stricter `no_worse_than_last_entry` **[R]**.

### 3.3 Simulator daily-loop contract — owns its own calendar **[R]**
Master calendar = **union of trading days** across universe assets (never primary-asset
anchored). For each T oldest→newest:
- **A. MARK** every position at T close, FX-converted to base via `fx_rate@T` **exactly
  once** (each instrument carries `price_currency`; store native + base notional) **[R]**.
  Missing price for a held asset → carry last close but `tradable=false` at T (visible,
  never fabricate a fill). Cash earns `cash_yield_proxy` daily return (or zero) **[R]**.
  Append `DailyEquityPoint{date, equity, cash, invested, drawdown}`.
- **B. SIGNALS** read `history[..=T]` with **completed-bucket** semantics (weekly = last
  ISO-week fully ended ≤ T; regime via `RegimeAtDate` date-aligned ≤ T) **[R]**.
- **C. REBALANCE** (only on a cadence date or when an `on_signal` rule fires): run the
  §3.2 solver → desired weights → bands → orders. **Decision is made after T close; the
  default fill is the NEXT tradable close** for each asset (`fill = next_close`); same
  fill must be opt-in and flagged optimistic **[R]**. Non-tradable legs are **excluded
  from order generation**; a rebalance needing a stale/untradable sell is **deferred or
  done partial cash-only** **[R]**. Fills = close·(1 ± slippage) + commission;
  **commission reduces cash AND equity** (not a return haircut) **[R]**. Decimal qty
  (fractional allowed; optional round-lot). Record `RebalanceEvent{orders, turnover,
  cost, pre/post weights, infeasible?}`.

**`PortfolioBacktestReport`** (NEW type) — all money **Decimal**, metrics **f64** computed
from the **daily** curve: CAGR, ann_vol, Sharpe, Sortino, Calmar, max_drawdown, CDaR,
Ulcer, time_in_cash, avg_turnover/yr, total_costs, n_rebalances, per-asset contribution,
daily_equity_curve, rebalance_events.
**Benchmarks (three) [R]:** (1) static base_policy, never rebalanced; (2) base_policy
rebalanced on the same cadence+costs (**isolates rule-alpha from rebalance-harvesting**);
(3) equal-weight basket. **Monte-Carlo** = *equity-curve path bootstrap* (stationary
bootstrap, expected block `round(√n)` clamped 5–63d) — labelled path-risk, **not**
strategy robustness; strategy robustness is P5 walk-forward over real folds **[R]**.

### 3.4 Lookahead / projection policy (one shared rule)
Decisions use only data with completion date ≤ T; weekly/monthly signals use the last
**completed** bucket; FX uses the **fill/mark date**, not the signal date; default fill is
next tradable close. This single policy is owned by the simulator and the signal-accessor
adapter — no per-call ad-hoc slicing.

### 3.5 Storage & CLI
- Canonical specs: `models/*.toml` (repo). `positioning_models` SQLite table = catalog
  index (name, version, **spec_hash**, path) — db-catalog entry required. `model_backtest_runs`
  (L2 cache, rebuildable): spec_hash, cli_version, window, cost params, metrics_json,
  computed_at. Added in P2, **after** semantics are proven **[R]**.
- CLI (under canonical `analytics`): `analytics models list | show <name> | backtest
  <name> [--from --to --json] | compare <a> <b> … | simulate <name>` (`simulate` = "what
  would this model do now" against the live portfolio).

## 4. Staged build plan

Each stage is a shippable, independently-graded increment. **The ordering is
deliberately accounting-first** [R]: prove the ledger + solver before any expressiveness.

### P0 — minimum-correct in-memory simulator (NO storage, NO signals) **← start here**
Prove the **accounting**, not the vocabulary. USD-only, 2–3 USD-priced assets + cash,
**fixed** class/symbol targets with floor/ceiling, weekly **or** monthly cadence,
`rebalance_band_mode = to_target`, next-close fills with commission+slippage, Decimal
ledger, daily equity curve. **Must get right:** (1) deterministic **bounded-projection**
solve with **infeasibility detection**; (2) **Decimal** cash/position/fee ledger;
(3) **lookahead-safe timing** (decide after T close → fill next tradable close).
**Defer:** DSL accessors, FX, `no_average_down`, MC, viz, SQLite, optimization.
Deliverable: `src/analytics/portfolio_sim/` (engine + solver + report) + a Rust test that
runs a hand-checkable fixed-weight model and asserts the ledger/curve/turnover numbers.

### P1 — full daily ledger + accounting realism + stats + benchmarks
FX (multi-currency mark/fill via `fx_rate@date`), missing-price/non-tradable handling,
cash-yield proxy, the three benchmarks, daily-curve metrics module (refactor the reusable
stat fns out of `strategy/engine.rs` to be generic over an equity curve). CLI
`analytics models backtest <name>` reading a TOML spec (parsed in-memory; still no DB).

### P2 — durable specs + run cache/catalog
`models/*.toml` loader + `positioning_models` catalog (hash) + `model_backtest_runs`
cache + db-catalog entries + `models list/show`. Encode **M1** as the first real spec.

### P3 — signal-rule vocabulary + stage proxy
Typed **signal-accessor registry** behind an evaluator boundary (extends the `when`
language: `cycle_bottom_met`, `cycle_top_met`, `regime_score`, `cyber_dot_up`,
`cyberline_lost`, `stage_proxy`). `RegimeAtDate` wrapper. Build **`stage_proxy_v1`**
(weekly: stage-2 = price > rising 40wk MA + structure breakout; stage-4 = price < falling
40wk MA + support break) — **named `stage_proxy`, validated via event-study before any
rule trusts it** [R]. Prove **M2 + M3**.

### P4 — compare + portfolio tearsheet viz + report wiring
`analytics models compare`, a NEW `viz/portfolio_viz.py` tearsheet (daily equity, drawdown,
the three benchmarks, turnover, allocation-over-time band) + token, `/pftui-report` wiring.

### P5 — constrained walk-forward optimization
**Numeric `params` only; frozen topology/universe/rules.** Train/test **walk-forward
folds**, min-trades/rebalances per fold, **turnover + complexity penalty**, **DSR / PBO
multiple-testing adjustment**, report **all** tried configs. **No agent-generated rules
inside the validation loop.** This is the honest version of "generate an optimal
strategy"; structural generation (proposing new rules/indicators) is explicitly **out of
scope** until P5's numeric version is trustworthy.

## 5. Non-negotiable principles / red flags (from both reviews)
1. **New `PortfolioBacktestReport` over a daily MTM curve** — never extend `TradeReport`.
2. **One lookahead/projection policy**, completed-bucket; `RegimeAtDate` aligned.
3. **Simulator owns its calendar/price panel**; non-tradable legs marked, never filled.
4. **Decimal** for cash/qty/fills/fees/FX; f64 only for indicators/metrics after the ledger.
5. **Bounded-projection normalizer with feasibility check** — never fixpoint iteration.
6. **Storage after semantics** — TOML canonical, SQLite provenance; P0 has no DB.
7. **`stage_proxy`, not "stage"** until event-study-validated.
8. **Overfitting is the adversary** — P5 is walk-forward + DSR/PBO-gated or it's fiction.
