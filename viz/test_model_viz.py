#!/usr/bin/env python3
"""Smoke tests for the positioning-model tearsheet — pure functions, no binary.

Run: python viz/test_model_viz.py   (exits nonzero on failure)
Feeds a synthetic `analytics models backtest --json`-shaped dict (per
src/commands/models_cmd.rs — Decimal money/weights are STRINGS, metrics are
floats) to the renderer. Synthetic market-price math only — no real portfolio
values, no real market reads.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model_viz as v  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


# ---- synthetic curve builders (rust_decimal money = STRINGS) ----
def _curve(daily_growth, n=40, p0=100000.0, start="2022-01-03"):
    import datetime
    d0 = datetime.date.fromisoformat(start)
    out = []
    eq = p0
    peak = p0
    for i in range(n):
        d = d0 + datetime.timedelta(days=i)
        eq *= (1.0 + daily_growth)
        peak = max(peak, eq)
        dd = (eq / peak - 1.0) * 100.0
        out.append({
            "date": d.isoformat(),
            "equity": f"{eq:.6f}",
            "cash": "0",
            "invested": f"{eq:.6f}",
            "drawdown_pct": f"{dd:.6f}",
        })
    return out


def _bench(daily_growth, cagr, maxdd):
    return {
        "daily_equity_curve": _curve(daily_growth),
        "metrics": {
            "cagr_pct": cagr, "ann_vol_pct": 8.0, "sharpe": 1.5, "sortino": 2.2,
            "calmar": 3.0, "max_drawdown_pct": maxdd, "cdar_95_pct": 6.0,
            "ulcer_index_pct": 4.0, "time_in_cash_pct": 10.0,
            "avg_turnover_pct_per_yr": 90.0, "total_costs": "90.0",
        },
    }


def _events():
    # two rebalance dates with class-mappable symbol post_weights + CASH.
    return [
        {"date": "2022-01-03", "post_weights": [["SYA", "0.5"], ["SYC", "0.4"], ["CASH", "0.1"]],
         "pre_weights": [["CASH", "1.0"]], "orders": [], "turnover_pct": "90",
         "total_cost": "90", "infeasible": False, "deferred_legs": [],
         "dropped_legs": [], "applied_rule_ids": [], "blocked_no_average_down": []},
        {"date": "2022-01-25", "post_weights": [["SYA", "0.6"], ["SYC", "0.3"], ["CASH", "0.1"]],
         "pre_weights": [["SYA", "0.55"]], "orders": [], "turnover_pct": "12",
         "total_cost": "11", "infeasible": False, "deferred_legs": [],
         "dropped_legs": [], "applied_rule_ids": [], "blocked_no_average_down": []},
    ]


GOOD = {
    "command": "analytics models backtest",
    "window": {"from": "2022-01-03", "to": "2022-02-11", "bars": 40},
    "model": {
        "name": "syn-aggressive", "version": 1,
        "universe": [
            {"symbol": "SYA", "class": "growth", "price_currency": "USD"},
            {"symbol": "SYC", "class": "spec", "price_currency": "USD"},
        ],
    },
    "report": {
        "daily_equity_curve": _curve(0.004),
        "rebalance_events": _events(),
        "n_rebalances": 2,
        "total_costs": "155.44",
        "cagr_pct": 52.7, "max_drawdown_pct": 9.1, "ann_vol_pct": 7.6,
        "time_in_cash_pct": 10.3, "avg_turnover_pct_per_yr": 309.6,
        "metrics": {
            "cagr_pct": 52.7, "ann_vol_pct": 7.6, "sharpe": 3.83, "sortino": 7.15,
            "calmar": 5.78, "max_drawdown_pct": 9.1, "cdar_95_pct": 8.9,
            "ulcer_index_pct": 4.6, "time_in_cash_pct": 10.3,
            "avg_turnover_pct_per_yr": 309.6, "total_costs": "155.44",
        },
        "benchmarks": {
            "static_base_policy": _bench(0.0025, 29.0, 9.18),
            "rebalanced_base_policy": _bench(0.0040, 42.0, 9.55),
            "equal_weight": _bench(0.0044, 44.0, 11.67),
        },
    },
}


# ---- 1. tearsheet renders a non-empty SVG with the key panels ----
svg = v.tearsheet_card(GOOD)
check("tearsheet returns non-empty svg", svg.startswith("<svg") and svg.endswith("</svg>"))
check("tearsheet shows model name", "syn-aggressive" in svg)
# model curve + 3 benchmark curves = 4 polylines (the equity panel)
npoly = svg.count("<polyline")
check("four equity polylines (model + 3 benchmarks)", npoly == 4)
check("benchmark legend labels present",
      "static base" in svg and "rebal. base" in svg and "equal weight" in svg)
check("stat header has CAGR + Calmar", "CAGR" in svg and "Calmar" in svg)
check("rule-alpha delta line present", "rule-alpha vs rebalanced-base" in svg)
check("underwater worst label present", "worst" in svg)
# allocation band: class legend labels for the mapped classes + cash
check("allocation band shows class legend", "growth" in svg and "spec" in svg and "cash" in svg)
check("allocation band renders rects", svg.count("<rect") > 6)

# ---- 2. log-scale kicks in only when the range warrants ----
# GOOD spans <3x so it should be linear; a 10x curve should switch to log.
big = {**GOOD, "report": {**GOOD["report"],
       "daily_equity_curve": _curve(0.06, n=60),
       "benchmarks": GOOD["report"]["benchmarks"]}}
check("log scale on a >3x curve", "log scale" in v.tearsheet_card(big))
check("linear scale on a <3x curve", "log scale" not in svg)

# ---- 3. honest caveat path: empty / missing report ----
cav = v.tearsheet_card({"model": {"name": "broke"}, "window": {"from": "x", "to": "y"}})
check("missing report -> caveat card", "Tearsheet unavailable" in cav)
short = v.tearsheet_card({"model": {"name": "shorty"},
                          "report": {"daily_equity_curve": _curve(0.01, n=1)}})
check("too-short curve -> caveat card", "Tearsheet unavailable" in short)
check("non-dict data -> empty string", v.tearsheet_card(None) == "")

# ---- 4. sign / scale unit logic ----
check("_delta basic", v._delta(52.7, 42.0) == 52.7 - 42.0)
check("_delta None-safe", v._delta(None, 1.0) is None and v._delta(1.0, None) is None)
check("_sgn positive carries +", v._sgn(10.7).startswith("+"))
check("_sgn negative carries -", v._sgn(-3.2).startswith("-"))
check("_sgn None -> --", v._sgn(None) == "--")
reb = v._rebase([(1, 200.0), (2, 300.0), (3, 100.0)])
check("_rebase starts at 100", abs(reb[0][1] - 100.0) < 1e-9)
check("_rebase scales proportionally", abs(reb[1][1] - 150.0) < 1e-9)

# ---- 5. allocation segments: class aggregation + cash-last ordering ----
classes, segs = v._alloc_segments(_events(), {"SYA": "growth", "SYC": "spec"}, 738900)
check("alloc classes are growth/spec/cash", set(classes) == {"growth", "spec", "cash"})
check("cash sorts last in the stack", classes[-1] == "cash")
check("two stepped segments", len(segs) == 2)

# ---- 6. compare overlay needs >= 2 usable curves ----
cmp_svg = v.compare_card([("m1", GOOD), ("m2", GOOD)])
check("compare overlays 2 model curves", cmp_svg.count("<polyline") == 2 and "m1" in cmp_svg)
check("compare with <2 usable -> empty", v.compare_card([("m1", GOOD)]) == "")

# ---- 7. determinism: identical output across renders (no now()/random) ----
check("tearsheet deterministic", v.tearsheet_card(GOOD) == v.tearsheet_card(GOOD))
check("compare deterministic",
      v.compare_card([("m1", GOOD), ("m2", GOOD)]) == v.compare_card([("m1", GOOD), ("m2", GOOD)]))

# ---- 8. expand() swaps a token; unknown token -> empty (no binary needed) ----
# render() with no pftui binary returns a caveat card (command unavailable),
# which still wraps in the model-viz div — so a token always expands cleanly.
out = v.expand("before <!--MODEL_VIZ:tearsheet:does-not-exist--> after",
               pftui="/nonexistent/pftui")
check("expand replaces the token", "MODEL_VIZ" not in out)
check("expand keeps surrounding text", "before" in out and "after" in out)

print()
if FAILS:
    print(f"FAILED: {len(FAILS)} -> {FAILS}")
    sys.exit(1)
print("all model_viz tests passed")
