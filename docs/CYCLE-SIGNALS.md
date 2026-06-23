# CYCLE-SIGNALS.md — The Mechanical Cycle-Bottom Signal Suite

> Read before touching `src/analytics/cycle_signals.rs`, the `analytics cycles
> bottom-signals` CLI, or any report/analyst prose that itemizes cycle-bottom
> confirmations. Companion to [CYCLE-THEORY.md](CYCLE-THEORY.md) (the timing
> engine) and [EPISTEMICS.md](EPISTEMICS.md) (the measurement discipline).

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
| 4 | **Roofing filter confirming up** | a de-trended cycle (band-pass) filter going constructive | requested | filter green (≥0) **AND** ticked up |
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
  "erf": -25006.78, "erf_green": false, "erf_turned_up": true,
  "cyberbands_state": "bearish", "cyberbands_bullish": false,
  "cyberdots_weekly_strength": 0, "cyberdots_monthly_strength": 0, "cyberdots_bullish": false,
  "cyberline_value": 69907.42, "cyberline_price_above": false, "cyberline_reclaim": false,
  "pi_cycle_bottom": false, "pi_cycle_last_bottom": "2022-07-13"
}
```

The `criteria[]` array is the canonical itemization for display; the flat fields
are a convenience for callers that want one number without walking the array.

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

> **Composite suite alerts (merged):** a rising N/7 (or any single criterion
> firing) arms a notification through the cycle-signal alert engine in
> `src/alerts/cycle_signal_alert.rs`. Use the composite condition
> `cycle_bottom_<tf>_<N>` (e.g. `cycle_bottom_monthly_5` fires when the monthly
> suite reaches ≥ 5/7) or the per-criterion condition
> `cycle_criterion_<tf>_<key>` to watch a single signal. The composite
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

`--window` is the ± match window in **days** around a verified low; it has a floor
of 1 (`--window 0` is rejected as meaningless, since a firing would then have to
land exactly on the verified-low date). Omit `--window` for the default
±90-day window.

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
