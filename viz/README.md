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

**High value, next**
- **Regime quad (Growth×Inflation)** — a 2×2 with the current regime dot + a short
  trail (`regime_quad` / `analytics macro regime`). Instantly reads the macro env.
- **Calibration / reliability diagram** — predicted vs realized probability for
  scored forecasts (forecast-scoring tables). Few tools show their own track
  record; this puts the epistemic honesty on the page. *Signature pftui.*

**Useful (private/portfolio reports)**
- **Allocation vs risk-parity** — current book weights vs ERC/downside-RP suggested
  (`analytics basket`); per-asset over/under-weight bars.
- **Drawdown underwater + survival** — drawdown path with CDaR/ruin annotations
  (`analytics survival` / drawdown-path metrics).

**Lower priority**
- Equity curve / cumulative return, monthly-returns calendar heatmap, sentiment /
  fear-greed gauge. Standard; build only if a report section needs them.
