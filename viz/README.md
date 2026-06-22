# viz/ — Python report visualization library

**Architecture: Rust computes, Python draws.**

- **Rust** owns all data + math. Every chart's input is the hardened `pftui … --json`
  CLI (rust_decimal, cargo-tested). This is the single source of truth and the
  only data boundary.
- **Python** (this dir) owns all report visualization. It shells the `--json`
  contract and renders **inline SVG** — vector, crisp, themeable, zero extra deps
  (no matplotlib/cairosvg). SVG passes through python-markdown untouched and
  WeasyPrint renders it beautifully in the PDF reports.

Charts are **always additive, never load-bearing**: any chart whose data is
unavailable renders to an empty string, so a report never breaks.

## Files

| File | Role |
|---|---|
| `theme.py` | Brand palette (synced with `gen-report.py` CSS), SVG helpers, and `pftui_json()` — the Rust data boundary. |
| `cycle_viz.py` | Cycle charts: `map`, `dial`, `ledger`. CLI + `expand()` token handler. |
| `risk_viz.py` | Risk/regime charts: `cocrash` (co-crash matrix). CLI + `expand()` token handler. |
| `portfolio_viz.py` | Risk-sizing charts: `drawdown` (drawdown-survival composite), `riskbars` (risk fingerprint). CLI + `expand()` token handler. |
| `analog_viz.py` | Analog-engine chart: `dist` (forward-return distribution box/whisker). CLI + `expand()` token handler. |
| `backtest_viz.py` | Strategy backtest chart: `tearsheet` (equity curve + underwater strip + stat line + Monte-Carlo terminal cone). CLI + `expand()` token handler. |
| `render.py` | Aggregator. `expand_tokens(md)` runs every module's token expander. `gen-report.py` imports this once. |

## How it's wired

`agents/intelligence-report/gen-report.py` calls `viz.render.expand_tokens(md)`
just before markdown→HTML. A report's markdown embeds tokens:

```
<!--CYCLE_VIZ:map:BTC-->
<!--CYCLE_VIZ:dial:BTC-->  <!--CYCLE_VIZ:dial:GC=F-->
<!--CYCLE_VIZ:ledger:BTC-->
```

Each is replaced with the rendered SVG. Run standalone too:

```
python viz/cycle_viz.py map --asset BTC          # one SVG to stdout
echo "$MD" | python viz/render.py                 # expand all tokens in markdown
```

Use the report venv python (`~/.local/share/pftui-report-venv/bin/python`) — it
has `markdown` + `weasyprint`.

## Per-chart asset support

Not every chart renders for every symbol. A chart whose engine has no data for
the requested asset expands to an empty string (additive, never load-bearing) —
so a token can *silently* yield nothing. This matrix says where that happens:

| Chart | Token | Renders for | Notes |
|---|---|---|---|
| Cycle **map** | `CYCLE_VIZ:map:SYM` | any asset with a `cycles analyze` degree | needs `lows` + `next_low_window`; headline degree is 4-year for BTC, major for gold/silver, else longest-first |
| Cycle **dial** | `CYCLE_VIZ:dial:SYM` | **BTC + gold-family ONLY** | dial is driven by `cycles clock`, which only emits a `btc` clock (BTC/BTC-USD) or a `gold` clock (GC=F/GOLD/SI=F/SILVER). **Tokenizing a dial for SPY/QQQ/etc. silently renders nothing.** |
| Cycle **ledger** | `CYCLE_VIZ:ledger:SYM` | any asset with a `cycles analyze` degree | needs a `ledger` + `band` on the degree |
| **cocrash** | `RISK_VIZ:cocrash:A,B,…` | any 2–6 assets with `tail-dependence` history | each pair needs Pearson and/or λ_L; missing pairs draw a `--` cell |
| Analog **dist** | `ANALOG_VIZ:dist:SYM` | any asset with an `analytics analog` report | needs the summary quantiles OR ≥1 per-analog forward return |
| **drawdown** | `PORTFOLIO_VIZ:drawdown:SYM` | any asset with a `survival` block | falls back to the `survival` block embedded in `risk-dashboard` |
| **riskbars** | `PORTFOLIO_VIZ:riskbars:SYM` | any asset with a `risk-dashboard` | renders whatever risk primitives are present |
| Backtest **tearsheet** | `BACKTEST_VIZ:tearsheet:SYM?entry=…` | any asset the backtester accepts | `entry` is REQUIRED; needs ≥1 completed trade beyond the anchor |

**cocrash accepted alias set** (display relabeling in `risk_viz.NICE`; resolution
to a real series is done by the Rust `tail-dependence` CLI, which accepts more):
`GC=F`/`GOLD` → GOLD, `SI=F`/`SILVER` → SILVER, `BTC-USD`/`BTC` → BTC,
`ETH-USD`/`ETH` → ETH, `^GSPC` → SPX, `SPY` → SPY, `QQQ` → QQQ. Any other ticker
is shown verbatim (uppercased). The cycle/analog/backtest modules share the same
`NICE` alias table.

## Adding a new chart family

1. New module `foo_viz.py` (the `*_viz.py` suffix matters — `render.py`
   auto-discovers it) with render functions + `TOKEN_RE` + `expand(md, pftui)`.
2. Pull data ONLY via `theme.pftui_json([...])` — never reach into the DB or
   recompute. If the needed number isn't in any `--json` output, add it to the
   Rust CLI first (that keeps the compute in Rust and tested).
3. Token convention: `<!--FOO_VIZ:type:arg-->`. No wiring needed — `render.py`
   finds your module's `expand()` automatically.
4. Add a `test_foo_viz.py` smoke test (binary-independent, synthetic data).

## Roadmap — other high-value report visualizations

Curated by value, not volume (quality over quantity). Each maps to existing
`--json` engines so it stays "Rust computes, Python draws".

**Shipped**
- Cycle **map** / **dial** / **ledger** (`analytics cycles …`).
- **Co-crash matrix** (`risk_viz.py` `cocrash`) — triangular grid over a basket:
  upper triangle = Pearson correlation, lower triangle = co-crash λ_L from
  `analytics tail-dependence`. Shows whether a pair (e.g. BTC↔gold) actually
  holds up in a crash. Token `<!--RISK_VIZ:cocrash:BTC,gold,SPY-->`.
- Risk **drawdown** (drawdown-survival composite — depth bars, recovery cliff,
  time-under-water i.i.d/AR(1), risk-of-ruin gauge; `analytics survival` /
  `analytics risk-dashboard`) + **riskbars** (risk fingerprint — CDaR/Ulcer/maxDD/
  vol bars with EVT tail class; `analytics risk-dashboard`). Single-asset only,
  read price-history not holdings — no portfolio weights surfaced.
- **Analog forward-return distribution** (`analog_viz.py` `dist`) — horizontal
  box/whisker of the target asset's realized forward returns over `horizon_days`
  after its closest historic macro-environment analogs (`analytics analog`):
  p25–median–p75 IQR box, mean + CI diamond, every analog episode as a colored
  tick, a hard zero line, and a header carrying the honesty stats (regime,
  `k_effective`, `n_distinct_episodes`, up-rate). Answers "when the world looked
  like today, what did this asset do next, and how dispersed was it?" — nothing
  else in the report shows it. Token `<!--ANALOG_VIZ:dist:BTC-->`.
- **Strategy backtest tearsheet** (`backtest_viz.py` `tearsheet`) — the equity
  curve (log, stepped at trade exits) over an underwater/drawdown strip, with a
  header stat line (CAGR / Sortino / max-DD / win-rate / #trades / profit-factor)
  and a validation sub-line (PSR, per-trade expectancy, time-in-market). The
  buy-and-hold benchmark is a faint dashed line; the Monte-Carlo *terminal*-return
  spread (p5/p50/p95) is a faint blue cone fanning off the curve's end (the
  luck-vs-skill spread), and the footer carries the MC drawdown/loss honesty.
  Equity is reconstructed by compounding the CLI's per-trade `return_pct` (verified
  to reproduce `total_return_pct` + `max_drawdown_pct` exactly) — no new math in
  Python. Token `<!--BACKTEST_VIZ:tearsheet:BTC?entry=rsi(14)<30&exit=rsi(14)>70-->`
  (payload = `ASSET[?entry=..&exit=..&stop_loss=..&take_profit=..&from=..&to=..]`,
  `entry` required; `<`/`>` allowed verbatim or percent-encoded). *Data gap:* the
  CLI exposes only MC **terminal + drawdown percentiles**, not per-bar MC path
  bands, so the cone is anchored at the curve's end rather than tracking every bar
  — a full per-bar percentile cone would need the Rust backtester to emit the path
  envelope.

**High value, next**
- **Regime quad (Growth×Inflation)** — a 2×2 with the current regime dot + a short
  trail (`regime_quad` / `analytics macro regime`). Instantly reads the macro env.
- **Calibration / reliability diagram** — predicted vs realized probability for
  scored forecasts (forecast-scoring tables). Few tools show their own track
  record; this puts the epistemic honesty on the page. *Signature pftui.*

**Useful (private/portfolio reports)**
- **Allocation vs risk-parity** — current book weights vs ERC/downside-RP suggested
  (`analytics basket`); per-asset over/under-weight bars. *Blocked:* `basket
  weights --json` emits only the SUGGESTED weights — it carries no current book
  weights, and reading the operator's real allocation is forbidden in this domain.
  Needs a Rust CLI that pairs suggested-vs-current behind the same privacy
  boundary before this chart can be built.

**Lower priority**
- Monthly-returns calendar heatmap, sentiment / fear-greed gauge. Standard; build
  only if a report section needs them.
