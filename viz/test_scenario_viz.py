#!/usr/bin/env python3
"""Smoke tests for the scenario dashboard — pure functions, no pftui binary.

Run: python viz/test_scenario_viz.py   (exits nonzero on failure)
Feeds synthetic `analytics scenario list --json`-shaped dicts to the renderer and
checks output + graceful degradation. Synthetic data only (no real values).
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario_viz  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


DATA = {
    "normalized_set": {
        "modeled_sum": 98.0,
        "overfill_state": "underfilled",
        "residual_materialized": True,
        "residual_probability": 2.0,
        "residual_scenario_name": "Other / Unmodelled",
    },
    "scenarios": [
        {"name": "Soft Landing", "probability": 35.0, "status": "active",
         "description": "Disinflation continues. Growth holds. Equities grind higher."},
        {"name": "Hard Recession", "probability": 20.0, "status": "active",
         "description": "Labor market collapse; equities -20%."},
        {"name": "Inflation Spike", "probability": 8.0, "status": "active",
         "description": "CPI re-accelerates; Fed forced to hold."},
        {"name": "Resolved Old", "probability": 50.0, "status": "resolved",
         "description": "should be filtered out"},
    ],
}


def main():
    print("scenario_viz smoke tests:")
    svg = scenario_viz.scenario_dashboard(DATA)
    check("renders svg", svg.startswith("<svg") and svg.rstrip().endswith("</svg>"))
    check("shows top scenario name", "Soft Landing" in svg)
    check("shows probability readout", ">35%<" in svg)
    check("includes residual bar", "Other / Unmodelled" in svg)
    check("shows normalized-set modeled sum", "modeled 98%" in svg)
    check("excludes resolved scenarios", "Resolved Old" not in svg)
    check("orders by probability (Soft Landing before Hard Recession)",
          svg.index("Soft Landing") < svg.index("Hard Recession"))

    # Graceful degradation.
    check("empty on None", scenario_viz.scenario_dashboard(None) == "")
    check("empty on no scenarios", scenario_viz.scenario_dashboard({"scenarios": []}) == "")
    check("empty on all-resolved",
          scenario_viz.scenario_dashboard(
              {"scenarios": [{"name": "X", "probability": 5, "status": "resolved"}]}) == "")

    # Token regex.
    check("token regex matches", bool(scenario_viz.TOKEN_RE.search(
        "x <!--SCENARIO_VIZ:dashboard:--> y")))
    # expand() returns '' (not the token) when render yields nothing.
    out = scenario_viz.expand("a <!--SCENARIO_VIZ:dashboard:--> b",
                              pftui="/nonexistent/pftui-binary")
    check("expand strips token on no data", "SCENARIO_VIZ" not in out)

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
