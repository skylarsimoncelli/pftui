# CYCLE-SIGNALS.md — The Mechanical Cycle-Bottom & Cycle-Top Signal Suites

> Read before touching `src/analytics/cycle_signals.rs`, the `analytics cycles
> bottom-signals` / `top-signals` CLI, or any report/analyst prose that itemizes
> cycle-bottom or cycle-top confirmations. Companion to
> [CYCLE-THEORY.md](CYCLE-THEORY.md) (the timing engine) and
> [EPISTEMICS.md](EPISTEMICS.md) (the measurement discipline).
>
> The bulk of this doc describes the cycle-**bottom** suite; the **symmetric
> cycle-TOP suite** at the end is its exact inverted mirror (same engine,
> bearish side).

## What it is

`analytics cycles bottom-signals` is a **deterministic N-of-7 confluence
checklist** that answers one narrow question: *is a cycle low being put in right
now?* It is the mechanical, measurable half of the cycle-bottom "confirm
checklist" — the part the calendar (`cycles clock`) and the structural engine
(`cycles analyze`) cannot tell you. Where `clock`/`analyze` say *when* a low is
due (a window, never a date), `bottom-signals` says *whether the turn is
actually printing on the tape*.

It is **position / measurement only — it never emits a price target**, never a
date, never a recommendation. All math is `f64`; no money flows through it (the
engine reads price history, not holdings).

## Why confluence marks cycle bottoms

A single oscillator turning up is noise — momentum ticks up dozens of times in a
downtrend. The thesis of this suite is **confluence + sequence**: the seven
criteria are *independent* reads (momentum, double-smoothed stochastic, a
de-trended cycle filter, volatility bands, higher-timeframe strength dots, a
volatility-weighted trackline) drawn from different families of math, and at
genuine cycle lows they **historically turn together, in a recognizable order** —
momentum and the stochastic turn first (the oversold thrust), the cycle filter
and bands confirm, and the higher-timeframe dots/trackline reclaim last (the
"bear is basically over" stamp). Any one firing alone is a maybe; five or six
firing at once is the signature of a low being *confirmed*, not predicted. The
N/7 score is just a legible scalar for "how much of that signature is present
today."

The criteria are ported from established practitioner indicator families (a
roofing/cycle filter, a double-smoothed stochastic, an RSI-of-RSI momentum line,
Gaussian volatility bands, multi-timeframe strength dots, a volatility-weighted
trackline, and a pi-cycle bottom). **Internal docs name that lineage where it
aids understanding; user-facing report prose and rendered charts must stay
name-free** — use the plain functional labels below.

## The 7 composite criteria

Ten atomic sub-signals collapse into 7 scored composites (each 0/1), plus one
non-counted bonus. Each composite carries its atomic `components[]` (raw boolean
+ oscillator value) in the JSON so nothing is lost.

| # | Composite (plain label) | What it measures | Natural TF | Fires when |
|---|---|---|---|---|
| 1 | **Momentum line turning up** | RSI's moving average ticked up off a low | requested | the RSI average's slope flips positive |
| 2 | **Momentum line crossed above price momentum** | the RSI average reclaimed the raw RSI | requested | RSI-avg crosses above RSI (momentum re-leading) |
| 3 | **Double-smoothed stochastic bottoming** | a heavily-smoothed stochastic turning up out of oversold | requested | DSS ticked up **AND** crossed its trigger (oversold = qualifying context, not a firing condition) |
| 4 | **Roofing filter confirming up** | a de-trended cycle (band-pass) filter turning from the lower cycle zone | requested | filter in bottom zone (<0) **AND** ticked up |
| 5 | **Volatility bands bullish** | daily Gaussian volatility bands in their bullish state | **daily** | band state == bullish |
| 6 | **Significant reversal dots** | higher-timeframe trend-strength "dots" net-bullish | **weekly + monthly** | an up-dot is active and ≥ any down-dot on either higher TF |
| 7 | **Trend line reclaimed** | price back above the weekly volatility-weighted trackline | **weekly** | price above the weekly line, or a fresh bullish cross on the latest weekly bar |
| bonus | **Pi-cycle bottom (not counted)** | a historical-low oscillator-cross signal fired recently | **daily** | last bottom within the trailing ~120 daily bars |

**Timeframe model.** The momentum/stochastic/roofing criteria (1–4) run on the
**requested** `--timeframe` (`daily`/`weekly`/`monthly`, default `monthly`). The
rest run on their own *fixed* natural aggregation regardless of the request:
bands = daily, dots = weekly + monthly, trackline = weekly, pi-cycle = daily.
This is deliberate — each read lives where it is most meaningful, so a `monthly`
request still gets a daily band read and weekly dots, not a monthly-mangled
version of them. Monthly is the **cycle-low** read; daily is the **tactical**
read of the same machinery.

## Reading the N/7 confluence

`met_count / total` (total is always 7). The `verdict` string bands it:

| N/7 | Verdict band |
|---|---|
| 0 | no cycle-bottom criteria firing |
| 1–2 | early / weak cycle-bottom confluence |
| 3–4 | building cycle-bottom confluence |
| 5–6 | strong cycle-bottom confluence |
| 7 | very strong cycle-bottom confluence (all 7) |

The **pi-cycle bonus is reported separately and NEVER counted in the 7** — it is
a corroborating flag, not a criterion. A high N/7 *with the clock still in its
low band* is the convergence a structural-low call wants. A **low N/7 during an
early-low calendar claim is a divergence** to name explicitly — it is the
mechanical tape disagreeing with the calendar.

This sits inside the EPISTEMICS measurement discipline: the suite measures, it
does not vote. It is an input to an analyst's cycle view, never a substitute for
one.

## How to run it

```bash
# Build first: cargo build --release  (binary at target/release/pftui)

pftui analytics cycles bottom-signals --asset BTC --timeframe monthly --json   # cycle-low read
pftui analytics cycles bottom-signals --asset BTC --timeframe daily   --json   # tactical read
pftui analytics cycles bottom-signals --asset GC=F --timeframe monthly --json  # gold
pftui analytics cycles bottom-signals BTC --json                               # positional symbol; BTC falls back to the deep BTC-USD series
```

`--asset` is an alias for the positional `SYMBOL`. Deep history is required —
the engine returns `null`/no-data below ~120 daily bars (monthly needs years of
history for the smoothing chains). `BTC` auto-falls-back to the deep `BTC-USD`
series; `gold`/`GC=F` resolve to the gold series.

### JSON shape

```jsonc
{
  "command": "analytics cycles bottom-signals",
  "symbol": "BTC-USD",
  "resolved_symbol": "BTC-USD",
  "timeframe": "monthly",
  "as_of": "2026-06-23",
  "met_count": 0,
  "total": 7,
  "verdict": "monthly suite: 0/7 — no cycle-bottom criteria firing",
  "criteria": [
    {
      "key": "momentum_turning_up",
      "label": "Momentum line turning up",
      "met": false,
      "detail": "monthly RSI 42.26 · RSI-avg 56.90",
      "components": [
        { "key": "rsi_ma_turned_up", "label": "RSI average ticked up", "met": false, "value": 56.90 }
      ]
    }
    // ... 6 more composite criteria, in the order of the table above
  ],
  "core_watch": [
    {
      "key": "roofing_confirming_up",
      "label": "Roofing filter confirming up",
      "met": true,
      "met_components": 2,
      "total_components": 2,
      "detail": "monthly value -25006.78",
      "components": [
        {
          "key": "erf_bottom_zone",
          "label": "Roofing filter in bottom zone (<0)",
          "met": true,
          "value": -25006.78,
          "previous_value": -27002.08,
          "comparison_value": 0.0,
          "previous_comparison_value": 0.0,
          "distance_to_trigger": 25006.78
        },
        {
          "key": "erf_turned_up",
          "label": "Roofing filter ticked up",
          "met": true,
          "value": -25006.78,
          "previous_value": -27002.08,
          "distance_to_trigger": 1995.30
        }
      ]
    }
  ],
  "bonus": {
    "key": "pi_cycle_bottom",
    "label": "Pi-cycle bottom fired recently (bonus)",
    "met": false,
    "detail": "last bottom 2022-07-13",
    "last_bottom": "2022-07-13"
  },
  // flat backing fields are also surfaced for direct access:
  "rsi": 42.26, "rsi_ma": 56.90, "rsi_ma_turned_up": false, "rsi_ma_cross_above_rsi": false,
  "dss": 9.33, "dss_trigger": 16.44, "dss_turned_up": false, "dss_cross_above_trigger": false, "dss_oversold": true,
  "erf": -25006.78, "erf_positive": false, "erf_green": false, "erf_bottom_zone": true, "erf_turned_up": true,
  "cyberbands_state": "bearish", "cyberbands_bullish": false,
  "cyberdots_weekly_strength": 0, "cyberdots_monthly_strength": 0, "cyberdots_bullish": false,
  "cyberline_value": 69907.42, "cyberline_price_above": false, "cyberline_reclaim": false,
  "pi_cycle_bottom": false, "pi_cycle_last_bottom": "2022-07-13"
}
```

The `core_watch[]` array is the focused four-item cycle-watch report for the
monthly bottom checklist. `criteria[]` remains the canonical 7-of-7 confluence
itemization; the flat fields are a convenience for callers that want one number
without walking either array.

Each component may also carry `previous_value`, `comparison_value`,
`previous_comparison_value`, and `distance_to_trigger`. The distance is signed:
positive means the latest bar is on the met side of that component's trigger
line/threshold. For example, a DSS cross component reports current/prior DSS,
current/prior trigger, and `distance_to_trigger = DSS - trigger`; a turn-up
component reports `current - previous`.

## Where it's wired

- **Engine:** `src/analytics/cycle_signals.rs` — `cycle_bottom_signals(symbol,
  &[HistoryRecord], timeframe) -> Option<CycleBottomSignals>`. Pure, deterministic
  (cargo-tested for identical output on identical input), returns `None` on
  shallow history.
- **Primitives:** `src/indicators/{rsi_ma, dss_bressert, ehlers_roofing}.rs` and
  `src/analytics/cyber/{bands, dots, line, pi_cycle}.rs`.
- **CLI:** `analytics cycles bottom-signals` (see `cli.rs` + the `cycles`
  command module). `--json` on every call, per CLI design rules.
- **Analyst routines:** the HIGH and MACRO timeframe analysts run it every cycle
  pass (`agents/routines/high-timeframe-analyst.md`,
  `agents/routines/macro-timeframe-analyst.md`) — it is the deterministic half of
  their early-low confirm checklist. The synthesis writer
  (`agents/report-prompts/phase3-synthesis-writer.md`) itemizes the current N/7
  in **name-free** public prose.
- **Report chart:** `viz/cycle_signals_viz.py` renders the checklist + N/7 gauge
  as inline SVG. Token `<!--CYCLE_SIGNALS_VIZ:checklist:BTC-->` (auto-injected at
  the Bitcoin/Gold section headings by `viz/report_charts.py`).

## How to backtest reliability

The claim "these criteria historically turn together at lows" is *measurable*,
not folklore — and per the EPISTEMICS discipline, an asserted edge should be
backtested, not assumed. Today, test the **individual** criteria with the
existing `analytics strategy` engine, expressing a criterion as a trade rule and
measuring forward returns:

```bash
# Forward returns after a monthly double-smoothed-stochastic turn-up out of oversold,
# vs buy-and-hold — does criterion 3 actually precede recovery?
pftui analytics strategy segment --asset BTC --when "rsi(close,14) < 30 @monthly" --json

# Coverage/firings before committing to a rule (thin coverage = untrustworthy edge)
pftui analytics strategy explain --asset BTC --entry "..." --json
```

> **Cycle-signal alerts:** a rising N/7, any single criterion, or any atomic
> component can arm a notification through the cycle-signal alert engine in
> `src/alerts/cycle_signal_alert.rs`. Use the composite condition
> `cycle_bottom_<tf>_<N>` (e.g. `cycle_bottom_monthly_5` fires when the monthly
> suite reaches ≥ 5/7) or the per-criterion condition
> `cycle_criterion_<tf>_<key>` to watch a single composite. Use
> `cycle_component_<tf>_<key>` for atomic subconditions such as
> `cycle_component_monthly_dss_turned_up`,
> `cycle_component_monthly_dss_cross_above_trigger`,
> `cycle_component_monthly_erf_bottom_zone`, or
> `cycle_component_monthly_erf_turned_up`. The composite
> hit-rate-at-historic-lows backtest is also merged — it scores the *whole
> suite's* lead/lag and coverage versus verified cycle lows. The per-criterion
> `analytics strategy` path above remains available for single-indicator drills.

### Running the suite backtest

```bash
pftui analytics cycles bottom-signals backtest --asset BTC --json
pftui analytics cycles bottom-signals backtest --asset gold --timeframe weekly --window 120 --json
```

Each confluence row reports `key` (`confluence_ge_3|4|5`), the numeric
`threshold` (`3|4|5`), and `firings`/`hits`/`precision`/`coverage`. A higher
confluence threshold can show **more** firings than a lower one — this is *not* a
bug. Firings are counted as rising edges *per threshold*: each threshold has its
own armed→fired state machine, so the met-count repeatedly crossing up through a
higher line (e.g. 4→5→4→5) produces a separate firing every time it re-crosses,
which can exceed the number of distinct times it first crossed a lower line.

Backtest JSON includes `eval_stride_days`, the daily-bar cadence used for
point-in-time evaluation. Daily timeframe backtests evaluate every bar so
one-day rising edges are not skipped; weekly and monthly backtests evaluate on a
weekly cadence because their underlying bars are broader and the match window is
measured in calendar days.

`--window` is the ± match window in **days** around a verified low; it has a floor
of 1 (`--window 0` is rejected as meaningless, since a firing would then have to
land exactly on the verified-low date). Omit `--window` for the default
±90-day window.

### Forward-return expectancy (`--expectancy`, asset-agnostic)

The reliability rows above answer *did the signal land near a doctrine low?* — but
they need hand-curated doctrine anchors (BTC/gold only) and report **no forward
returns**. Add `--expectancy` to also compute an **asset-agnostic expectancy
block** that works for any symbol with enough history:

```bash
pftui analytics cycles bottom-signals backtest --asset BTC --expectancy --json
pftui analytics cycles bottom-signals backtest --asset SPY --expectancy            # arbitrary symbol
```

It adds an `expectancy` object to the result (omitted entirely without the flag, so
the legacy payload is byte-for-byte unchanged):

- **Price-structure anchors.** Significant swing lows are derived *from price alone*
  via a prominence-filtered pivot scan (lowest low in a ±90-daily-bar window that is
  followed by a ≥20% recovery). These are **independent of the confluence signal
  being graded** (no circularity) and work for an arbitrary symbol. When doctrine
  anchors exist they are merged in (stronger ground truth), but are not required.
  *Epistemic caveat:* price-derived anchors are weaker ground truth than doctrine
  anchors — read their numbers as directional.
- **Forward-return expectancy conditioned on confluence.** Walking history
  point-in-time (same no-lookahead discipline — at bar `i` the engine sees only
  `history[..=i]`), for each confluence threshold (≥3/≥4/≥5 of 7) and each single
  criterion: `mean`/`median`/`positive_rate_pct` forward return at **30/90/180/365
  calendar-day** horizons, plus the unconditioned same-horizon `baseline` and the
  resulting `lift_vs_baseline_pct` (signal mean − baseline mean). Forward returns
  inherently consume future bars — that is the *outcome*, not the signal; the
  no-lookahead rule governs only the signal read.
- **Closeness to the actual extreme.** Each firing is matched to the nearest
  price-structure low within the match window, reporting BOTH signed lead/lag in
  days AND the signed `price_gap_pct` `(fire_price − low_price)/low_price·100`.
  Aggregated as `median_lead_lag_days`, `median_price_gap_pct`, `matched_firings`,
  and `confidence_pct` (= matched / firings).

The block carries its own honest `small_n` / `insufficient_anchors` flags and
`caveat`. JSON shape:

```jsonc
"expectancy": {
  "price_structure_lows": ["2018-12-15", "2020-03-13", "2022-11-21", "..."],
  "price_low_pivot_window": 90,
  "price_low_prominence_pct": 20,
  "doctrine_anchors_used": true,
  "anchors_used": 13,
  "insufficient_anchors": false,
  "small_n": false,
  "baseline": [
    { "horizon_days": 30, "samples": 593, "mean_return_pct": "6.25",
      "median_return_pct": "2.76", "positive_rate_pct": "56.8" }
    // 90 / 180 / 365 …
  ],
  "confluence": [
    {
      "key": "confluence_ge_3", "threshold": 3,
      "label": "Confluence ≥3/7 criteria firing", "firings": 23,
      "horizons": [
        { "horizon_days": 30, "samples": 23, "mean_return_pct": "7.18",
          "median_return_pct": "8.55", "positive_rate_pct": "65.2",
          "baseline_mean_return_pct": "6.25", "lift_vs_baseline_pct": "0.93" }
        // 90 / 180 / 365 …
      ],
      "closeness": {
        "matched_firings": 14, "firings": 23, "median_lead_lag_days": 26,
        "median_price_gap_pct": "32.89", "confidence_pct": "60.9"
      }
    }
    // ≥4 / ≥5 …
  ],
  "criteria": [ /* same shape, one row per criterion */ ],
  "caveat": "Expectancy conditioned on 13 cycle-low anchor(s); …"
}
```

### Structured error reasons

Under `--json`, a failed `bottom-signals` / backtest run emits
`{"error": {"command", "message", "reason", "bars_available?}}`. The machine-readable
`reason` is one of:

- `no_history` — the series has zero cached bars (an unknown ticker and an
  uncached-but-valid symbol are indistinguishable without a network call, so both
  collapse to this reason; run `pftui data refresh` for a valid symbol).
- `insufficient_history` — the series resolved with some bars but fewer than the
  ~120-daily-bar floor the smoothing chains need; `bars_available` reports how
  many were found.

---

## The symmetric cycle-TOP suite (`analytics cycles top-signals`)

`analytics cycles top-signals` is the **exact inverted mirror** of
`bottom-signals`. It answers the opposite narrow question: *is a cycle TOP being
put in right now?* Same engine, same 10→7 collapse, same struct shape
(`criteria[]` with atomic `components[]`, `core_watch[]`, `met_count/total`,
non-counted bonus) — every sub-signal just reads the **bearish/topping side** of
the identical underlying indicators. It is position / measurement only; no price
target, all `f64`, no money flows through it. Built in
`src/analytics/cycle_signals.rs::cycle_top_signals` (mirror of
`cycle_bottom_signals`).

### The 7 composite cycle-TOP criteria

| # | Composite (plain label) | Natural TF | Fires when |
|---|---|---|---|
| 1 | **Momentum line turning down** | requested | the RSI average's slope flips negative (`rsi_ma_turned_down`) |
| 2 | **Momentum line crossed below price momentum** | requested | RSI-avg crosses below the raw RSI (`rsi_ma_cross_below_rsi`) |
| 3 | **Double-smoothed stochastic topping** | requested | DSS ticked down **AND** crossed below its trigger (overbought >80 = qualifying context, not a firing condition) |
| 4 | **Roofing filter confirming down** | requested | filter in top zone (>0) **AND** ticked down |
| 5 | **Volatility bands bearish** | **daily** | daily band state == bearish |
| 6 | **Significant reversal dots bearish** | **weekly + monthly** | a down-dot is active and ≥ any up-dot on either higher TF |
| 7 | **Trend line lost** | **weekly** | price below the weekly trackline, or a fresh bearish cross on the latest weekly bar |
| bonus | **Pi-cycle top (not counted)** | **daily** | last pi-cycle TOP within the trailing ~120 daily bars |

The N/7 verdict bands mirror the bottom suite (`no/early/building/strong/very
strong cycle-top confluence`). Same timeframe model: criteria 1–4 run on the
requested `--timeframe` (default `monthly`); bands=daily, dots=weekly+monthly,
line=weekly, pi=daily are fixed.

### Running it

```bash
pftui analytics cycles top-signals --asset BTC                       # text, monthly
pftui analytics cycles top-signals --asset BTC --timeframe monthly --json
pftui analytics cycles top-signals --asset gold --timeframe weekly
```

The `--json` payload is the symmetric twin of the bottom payload: top-side flag
fields (`rsi_ma_turned_down`, `dss_turned_down`, `dss_cross_below_trigger`,
`dss_overbought`, `erf_top_zone`, `erf_turned_down`, `erf_negative`,
`cyberbands_bearish`, `cyberdots_bearish`, `cyberline_lost`, `pi_cycle_top`),
the 7 `criteria[]` rows (keys `momentum_turning_down`, `momentum_below_price`,
`dss_topping`, `roofing_confirming_down`, `volatility_bands_bearish`,
`reversal_dots_bearish`, `trend_line_lost`), `core_watch[]` (the four
momentum/stochastic/roofing watch items), `met_count`/`total`, the bonus, and
`verdict`.

### Top backtest — forward-return expectancy vs swing HIGHS

```bash
pftui analytics cycles top-signals backtest --asset BTC --expectancy --json
pftui analytics cycles top-signals backtest --asset gold --timeframe weekly --window 120 --expectancy
```

The top backtest is the asset-agnostic forward-return mirror of the bottom one,
with two honest differences:

1. **No doctrine top anchors.** The documented doctrine anchors (BTC 4-year,
   gold ~6.9-year) are cycle **LOWS** — there are no doctrine TOP anchors. So the
   top backtest's verified-anchor reliability section is **always empty**
   (`anchors: []`, `small_n: true`, `caveat: insufficient_anchors`). The real
   read lives in the **expectancy block**, which conditions forward returns on
   **price-structure swing HIGHS** (`price_structure_highs`: prominence-filtered
   pivot highs followed by a ≥20% decline — the mirror of `price_structure_lows`).
   *(Implementation note: the shared `CycleSignalExpectancy` struct reuses the
   `price_structure_lows` field name to carry the swing-HIGH dates on the top
   path — the field is the anchor-date list regardless of polarity.)*
2. **A good top signal precedes a DECLINE.** So the headline hit-rate is the
   **negative** forward-return rate. Each per-horizon row adds
   `negative_rate_pct` (fraction of firings followed by a strictly-negative
   forward return). `mean_return_pct` / `median_return_pct` are expected to be
   **negative** after a real top, and `lift_vs_baseline_pct`
   (`mean - baseline_mean`) is expected to be **negative** (the asset
   underperformed a random bar after the signal fired). Closeness is measured to
   the nearest swing **high** (days + price-% gap).

No-lookahead discipline is identical: at each evaluated bar `i` the engine reads
only `history[..=i]`, so a firing's index set cannot shift when future bars
arrive. Forward returns deliberately consume future bars — that is the outcome
being graded, not the signal.

### Cycle-TOP alert conditions

Three condition shapes, the mirror of the bottom set, evaluated mechanically on
`data refresh` (same `Technical` alert kind, same edge-trigger machinery; the
engine dispatches on polarity — anything starting `cycle_top_` reads the top
suite):

- **Confluence threshold** — `cycle_top_<timeframe>_<N>` (e.g.
  `cycle_top_monthly_4`): fires when the top `met/7` reaches `<N>`.
- **Single criterion** — `cycle_top_criterion_<timeframe>_<criterion_key>`
  (e.g. `cycle_top_criterion_weekly_trend_line_lost`).
- **Single component** — `cycle_top_component_<timeframe>_<component_key>`
  (e.g. `cycle_top_component_monthly_erf_turned_down`).

Top criterion keys: `momentum_turning_down`, `momentum_below_price`,
`dss_topping`, `roofing_confirming_down`, `volatility_bands_bearish`,
`reversal_dots_bearish`, `trend_line_lost`.
Top component keys: `rsi_ma_turned_down`, `rsi_ma_cross_below_rsi`,
`dss_turned_down`, `dss_cross_below_trigger`, `dss_overbought`, `erf_top_zone`,
`erf_turned_down`, `erf_negative`, `cyberbands_bearish`, `cyberdots_bearish`,
`cyberline_lost`, `pi_cycle_top`.

```bash
pftui analytics alerts add --kind technical --symbol BTC-USD \
  --condition cycle_top_monthly_4
```
