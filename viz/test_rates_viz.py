#!/usr/bin/env python3
"""Smoke tests for the real-rates visualization — pure functions, no pftui binary.

Run: python viz/test_rates_viz.py   (exits nonzero on failure)
Feeds synthetic `analytics real-rates differentials --json`-shaped dicts to the
renderer and checks output + graceful degradation. Synthetic values only.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rates_viz  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


DATA = {
    "since": "2026-06-15",
    "snapshot_count": 2,
    "snapshots": [
        {"date": "2026-06-15", "us_tips_10y": 2.15, "us_breakeven_10y": 2.32,
         "us_nominal_10y": 4.47, "us_minus_g10_avg_bp": 92.5,
         "pairs": [{"country": "GB", "spread_bp": -47.0},
                   {"country": "DE", "spread_bp": 142.0}]},
        {"date": "2026-06-16", "us_tips_10y": 2.10, "us_breakeven_10y": 2.30,
         "us_nominal_10y": 4.40, "us_minus_g10_avg_bp": 88.5,
         "pairs": [{"country": "GB", "spread_bp": -51.0},
                   {"country": "DE", "spread_bp": 138.0},
                   {"country": "JP", "spread_bp": 182.0}]},
    ],
}


def main():
    print("rates_viz smoke tests:")
    svg = rates_viz.real_rates(DATA)
    check("renders svg", svg.startswith("<svg") and svg.rstrip().endswith("</svg>"))
    # Uses the LATEST snapshot (2026-06-16).
    check("uses latest snapshot date", "2026-06-16" in svg)
    check("shows nominal", "nominal 4.40%" in svg)
    check("shows real (TIPS) segment", "real 2.10%" in svg)
    check("shows breakeven segment", "infl 2.30%" in svg)
    check("shows a partner country", ">JP<" in svg or ">DE<" in svg)
    check("shows avg differential", "avg" in svg and "bp" in svg)

    # Degradation.
    check("empty on None", rates_viz.real_rates(None) == "")
    check("empty on no snapshots", rates_viz.real_rates({"snapshots": []}) == "")
    # A snapshot with only pairs (no nominal) still renders the right panel.
    only_pairs = {"snapshots": [{"date": "2026-06-17",
                  "pairs": [{"country": "DE", "spread_bp": 100.0}]}]}
    op = rates_viz.real_rates(only_pairs)
    check("renders with pairs only", op.startswith("<svg") and ">DE<" in op)
    # A snapshot with nothing usable -> empty.
    check("empty when nothing usable",
          rates_viz.real_rates({"snapshots": [{"date": "x", "pairs": []}]}) == "")
    # A trailing breakeven-ONLY snapshot must NOT blank the chart — the renderer
    # falls back to the most recent COMPLETE snapshot (real-data shape).
    trailing_be = {"snapshots": DATA["snapshots"] + [
        {"date": "2026-06-17", "us_breakeven_10y": 2.25,
         "us_nominal_10y": None, "us_tips_10y": None, "pairs": []}]}
    tb = rates_viz.real_rates(trailing_be)
    check("falls back past trailing breakeven-only snapshot",
          tb.startswith("<svg") and "2026-06-16" in tb)

    # Token regex.
    check("token regex matches", bool(rates_viz.TOKEN_RE.search(
        "x <!--RATES_VIZ:realrates:--> y")))
    out = rates_viz.expand("a <!--RATES_VIZ:realrates:--> b",
                           pftui="/nonexistent/pftui-binary")
    check("expand strips token on no data", "RATES_VIZ" not in out)

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
