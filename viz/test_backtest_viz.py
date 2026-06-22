#!/usr/bin/env python3
"""Smoke tests for the backtest tearsheet viz — pure functions, no pftui binary.

Run: python viz/test_backtest_viz.py   (exits nonzero on failure)
Feeds synthetic backtest-report dicts (the shape `analytics strategy backtest
--json` emits under `.report`) to the SVG renderer and checks output, the
equity-curve reconstruction math, the Monte-Carlo cone, and graceful degradation.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import backtest_viz  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


# Minimal synthetic report carrying every field the viz reads.
REPORT = {
    "trades": [
        {"entry_date": "2014-10-03", "exit_date": "2014-11-12", "return_pct": 17.82},
        {"entry_date": "2015-01-10", "exit_date": "2015-03-11", "return_pct": -7.32},
        {"entry_date": "2016-01-05", "exit_date": "2016-04-20", "return_pct": 21.15},
        {"entry_date": "2018-01-01", "exit_date": "2019-02-19", "return_pct": -31.22},
        {"entry_date": "2020-02-01", "exit_date": "2020-04-29", "return_pct": 77.06},
        {"entry_date": "2022-12-01", "exit_date": "2023-01-11", "return_pct": -40.80},
        {"entry_date": "2025-02-26", "exit_date": "2025-05-02", "return_pct": 14.89},
    ],
    "cagr_pct": 22.94,
    "sortino_ratio": 1.38,
    "max_drawdown_pct": -51.46,
    "win_rate_pct": 80.0,
    "n_trades": 20,
    "profit_factor": 4.48,
    "expectancy_pct": 16.99,
    "time_in_market_pct": 36.28,
    "total_return_pct": 1035.14,
    "benchmark_hold": {"total_return_pct": 13946.51, "cagr_pct": 52.26,
                       "max_drawdown_pct": -83.4},
    "monte_carlo": {
        "method": "bootstrap-resample", "n_paths": 5000,
        "terminal_return_p5_pct": 47.44, "terminal_return_p50_pct": 1076.31,
        "terminal_return_p95_pct": 8281.77, "drawdown_median_pct": -40.80,
        "drawdown_p95_pct": -72.59, "prob_loss_pct": 2.26,
    },
    "validation": {"anecdotal": False, "psr_vs_zero": 0.9934,
                   "mean_return_ci_pct": [6.04, 28.55]},
}


def main():
    print("backtest_viz smoke tests:")

    # ---- equity-curve reconstruction math ----
    pts = backtest_viz.equity_curve(REPORT["trades"])
    # leading anchor + 7 trades = 8 points; anchor eq == 1.0.
    check("equity_curve has anchor + one point per trade", len(pts) == 8)
    check("equity_curve starts at 1.0", abs(pts[0]["eq"] - 1.0) < 1e-9)
    # Compound the 7 returns by hand.
    eq = 1.0
    for t in REPORT["trades"]:
        eq *= (1 + t["return_pct"] / 100)
    check("equity_curve final equity compounds the returns",
          abs(pts[-1]["eq"] - eq) < 1e-6)
    # Drawdown is negative once below the running peak, zero at new highs.
    check("equity_curve tracks drawdown (some point underwater)",
          any(p["dd"] < -0.01 for p in pts))
    check("equity_curve first point flat", abs(pts[0]["dd"]) < 1e-9)

    # ---- full tearsheet render ----
    svg = backtest_viz.tearsheet(REPORT, "BTC — Strategy Backtest Tearsheet")
    check("tearsheet renders svg", svg.startswith("<svg") and svg.rstrip().endswith("</svg>"))
    check("tearsheet shows CAGR", "CAGR +22.9%" in svg)
    check("tearsheet shows Sortino", "Sortino 1.38" in svg)
    check("tearsheet shows maxDD", "maxDD -51%" in svg)
    check("tearsheet shows win-rate", "win 80%" in svg)
    check("tearsheet shows trade count", "20 trades" in svg)
    check("tearsheet shows profit factor", "PF 4.48" in svg)
    check("tearsheet shows PSR honesty", "PSR 99%" in svg)
    check("tearsheet shows benchmark hold line", "hold" in svg and "x<" in svg)
    check("tearsheet draws underwater strip", "underwater" in svg)
    check("tearsheet has equity panel caption", "equity (log)" in svg)

    # ---- Monte-Carlo cone present ----
    check("tearsheet draws MC cone (polygon)", "<polygon" in svg)
    check("tearsheet labels MC P5/P95", "P5 " in svg and "P95 " in svg)
    check("tearsheet footer carries MC drawdown honesty", "MC drawdown" in svg)
    check("tearsheet footer carries P(loss)", "P(loss) 2.3%" in svg)

    # ---- graceful: no Monte-Carlo block -> still renders, notes the gap ----
    no_mc = {k: v for k, v in REPORT.items() if k != "monte_carlo"}
    s2 = backtest_viz.tearsheet(no_mc, "t")
    check("tearsheet renders without MC", s2.startswith("<svg"))
    check("tearsheet notes missing MC", "no Monte-Carlo block" in s2)
    check("tearsheet without MC draws no cone polygon", "<polygon" not in s2)

    # ---- anecdotal flag surfaces ----
    anec = {**REPORT, "validation": {**REPORT["validation"], "anecdotal": True}}
    s3 = backtest_viz.tearsheet(anec, "t")
    check("tearsheet surfaces ANECDOTAL flag", "ANECDOTAL" in s3)

    # ---- graceful degradation ----
    check("tearsheet empty on no trades",
          backtest_viz.tearsheet({"trades": []}, "t") == "")
    check("tearsheet empty on empty report", backtest_viz.tearsheet({}, "t") == "")
    one = {"trades": [{"entry_date": "2020-01-01", "exit_date": "2020-02-01",
                       "return_pct": 5.0}]}
    # anchor + 1 trade = 2 points -> renders (>= 2 required).
    check("tearsheet renders with a single trade (2 points)",
          backtest_viz.tearsheet(one, "t").startswith("<svg"))
    # trades with malformed dates are skipped, not fatal.
    bad = {"trades": [{"entry_date": "x", "exit_date": "y", "return_pct": 5.0}]}
    check("tearsheet empty when all trades unparseable",
          backtest_viz.tearsheet(bad, "t") == "")

    # ---- NEW SHAPE: native equity_curve + per-step path_envelope ----
    # Build a native equity_curve that reproduces the same compounding, plus a
    # per-step MC envelope aligned to those points (step k <-> point k).
    native_pts = [{"date": "2014-10-03", "equity": 1.0, "drawdown_pct": 0.0}]
    eqv, peakv = 1.0, 1.0
    for t in REPORT["trades"]:
        eqv *= (1 + t["return_pct"] / 100)
        peakv = max(peakv, eqv)
        native_pts.append({"date": t["exit_date"], "equity": eqv,
                           "drawdown_pct": (eqv / peakv - 1.0) * 100.0})
    # Envelope: 8 steps (point count), widening p5<=p50<=p95 bands.
    envelope = []
    for k in range(len(native_pts)):
        frac = k / max(len(native_pts) - 1, 1)
        mid = native_pts[k]["equity"]
        envelope.append({"step": k,
                         "p5": max(mid * (1.0 - 0.5 * frac), 1e-3),
                         "p50": mid,
                         "p95": mid * (1.0 + 1.5 * frac)})
    NEW = {**REPORT,
           "equity_curve": native_pts,
           "monte_carlo": {**REPORT["monte_carlo"], "path_envelope": envelope}}

    npts = backtest_viz.native_equity_curve(NEW)
    check("native_equity_curve reads the array", len(npts) == len(native_pts))
    check("native_equity_curve starts at 1.0", abs(npts[0]["eq"] - 1.0) < 1e-9)
    check("native_equity_curve final equity matches compounding",
          abs(npts[-1]["eq"] - eqv) < 1e-6)
    check("native_equity_curve converts drawdown_pct to fraction",
          any(p["dd"] < -0.01 for p in npts) and all(p["dd"] <= 1e-9 for p in npts))

    svg_new = backtest_viz.tearsheet(NEW, "BTC — new shape")
    check("new-shape tearsheet renders", svg_new.startswith("<svg"))
    check("new-shape draws per-step cone (multi-point p95 path)",
          "<polygon" in svg_new and svg_new.count("<path") >= 4)
    check("new-shape labels terminal P5/P95", "P5 " in svg_new and "P95 " in svg_new)
    check("new-shape still draws underwater strip", "underwater" in svg_new)

    # native curve is preferred over the trades reconstruction when present:
    # corrupt trades but keep a valid native curve -> still renders correctly.
    pref = {**NEW, "trades": [{"entry_date": "x", "exit_date": "y", "return_pct": 5.0}]}
    check("native curve preferred over trades reconstruction",
          backtest_viz.tearsheet(pref, "t").startswith("<svg"))

    # OLD SHAPE still works (no equity_curve, no path_envelope -> terminal fan).
    check("old-shape (no native curve) still renders via reconstruction",
          backtest_viz.tearsheet(REPORT, "t").startswith("<svg"))

    # ---- arg parsing (asset + query string) ----
    asset, extra = backtest_viz._parse_arg("BTC?entry=rsi(14)%3C30&exit=rsi(14)%3E70")
    check("parse_arg extracts asset", asset == "BTC")
    check("parse_arg builds --entry flag", "--entry" in extra and "rsi(14)<30" in extra)
    check("parse_arg builds --exit flag", "--exit" in extra and "rsi(14)>70" in extra)
    a2, e2 = backtest_viz._parse_arg("GC=F")
    check("parse_arg handles bare asset", a2 == "GC=F" and e2 == [])

    # render() requires an entry condition; bare asset yields nothing.
    check("render rejects missing entry",
          backtest_viz.render("tearsheet", "BTC", pftui="/nonexistent") == "")
    check("render rejects unknown type",
          backtest_viz.render("nope", "BTC?entry=x", pftui="/nonexistent") == "")

    # Token regex matches the documented form.
    md = "before <!--BACKTEST_VIZ:tearsheet:BTC?entry=rsi(14)<30&exit=rsi(14)>70--> after"
    m = backtest_viz.TOKEN_RE.search(md)
    check("token regex matches", bool(m) and m.group(1) == "tearsheet")

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
