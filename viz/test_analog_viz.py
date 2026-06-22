#!/usr/bin/env python3
"""Smoke tests for the analog visualization — pure functions, no pftui binary.

Run: python viz/test_analog_viz.py   (exits nonzero on failure)
Feeds synthetic analog-report dicts (the shape `analytics analog --json` emits
under `.report`) to the SVG renderer and checks output + graceful degradation.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import analog_viz  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


# Minimal synthetic report carrying every field the viz reads.
REPORT = {
    "analogs": [
        {"date": "2025-11-05", "distance": 2.55, "forward_return_pct": -27.2, "regime": "inflation"},
        {"date": "2024-06-07", "distance": 2.69, "forward_return_pct": -19.0, "regime": "goldilocks"},
        {"date": "2021-08-10", "distance": 3.03, "forward_return_pct": 48.2, "regime": "goldilocks"},
        {"date": "2017-06-20", "distance": 3.30, "forward_return_pct": 49.4, "regime": "goldilocks"},
        {"date": "2019-03-04", "distance": 3.84, "forward_return_pct": 132.4, "regime": "inflation"},
        {"date": "2006-10-12", "distance": 2.09, "forward_return_pct": None, "regime": "goldilocks"},
    ],
    "horizon_days": 90,
    "k": 25,
    "k_effective": 13,
    "mean_distance": 3.24,
    "mean_forward_ci_pct": [-0.47, 46.63],
    "mean_forward_pct": 22.53,
    "median_forward_pct": 41.18,
    "n_distinct_episodes": 25,
    "n_with_forward": 13,
    "note": "only 13/25 matched analogs had data at +90d — treat as indicative, not robust",
    "p25_forward_pct": -19.01,
    "p75_forward_pct": 49.36,
    "query_date": "2026-06-22",
    "query_regime": "deflation",
    "target_asset": "BTC-USD",
    "up_rate_pct": 53.8,
}


def main():
    print("analog_viz smoke tests:")

    svg = analog_viz.forward_dist(REPORT, "BTC — Analog Forward-Return Distribution")
    check("dist renders svg", svg.startswith("<svg") and svg.rstrip().endswith("</svg>"))
    check("dist shows horizon", "+90d horizon" in svg)
    check("dist shows regime", "regime: deflation" in svg)
    check("dist shows honesty stats (k_eff)", "k_eff 13" in svg)
    check("dist shows episodes", "25 episodes" in svg)
    check("dist shows median", "median +41.2%" in svg)
    check("dist shows mean", "mean +22.5%" in svg)
    check("dist shows p25/p75", "p25 -19%" in svg and "p75 +49%" in svg)
    check("dist draws a zero/flat marker", ">flat<" in svg and ">0%<" in svg)
    # 5 analogs have a forward value (one is None) -> 5 tick dots.
    check("dist draws one tick per forward analog", svg.count("<circle") == 5)

    # Graceful degradation: no quantiles and no forwards -> empty, never raises.
    empty = {"analogs": [{"date": "2020-01-01", "forward_return_pct": None}]}
    check("dist empty on no quantiles + no forwards", analog_viz.forward_dist(empty, "t") == "")
    check("dist empty on empty report", analog_viz.forward_dist({}, "t") == "")

    # Box/whisker still renders from quantiles alone (all forwards missing).
    qonly = {k: v for k, v in REPORT.items() if k != "analogs"}
    qonly["analogs"] = [{"date": "2020-01-01", "forward_return_pct": None}]
    s2 = analog_viz.forward_dist(qonly, "t")
    check("dist renders from quantiles even with no forward ticks",
          s2.startswith("<svg") and s2.count("<circle") == 0)

    # render() guards a non-'dist' type.
    check("render rejects unknown type", analog_viz.render("nope", "BTC", pftui="/nonexistent") == "")

    # Token regex matches the documented form.
    md = "before <!--ANALOG_VIZ:dist:BTC--> after"
    check("token regex matches", bool(analog_viz.TOKEN_RE.search(md)))

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
