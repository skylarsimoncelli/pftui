#!/usr/bin/env python3
"""Smoke tests for the macro env + catalyst visualizations — no pftui binary.

Run: python viz/test_macro_viz.py   (exits nonzero on failure)
Feeds synthetic `analytics environment current` / `analytics catalysts`-shaped
dicts to the renderers and checks output + graceful degradation. Synthetic only.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import macro_viz  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


ENV = {
    "as_of": "2026-06-22",
    "command": "environment current",
    "features_zscored": {
        "curve_10y_3m": -0.48, "dxy_ret20": 0.80, "dxy_vol20": -1.08,
        "gold_ret20": -1.48, "gold_vol20": 1.75, "oil_ret20": -1.95,
        "oil_vol20": 0.51, "spx_ret20": -0.21, "spx_vol20": 0.16,
        "tnx_chg20": -0.18, "tnx_level": 1.19, "vix_level": -0.19,
    },
    "history_days": 5768,
}

CATS = {
    "window": "week", "label": "this week",
    "catalysts": [
        {"title": "Core PCE Price Index", "time": "2026-06-25", "significance": "high",
         "score": 37, "category": "inflation"},
        {"title": "FOMC Minutes", "time": "2026-06-24", "significance": "medium",
         "score": 18, "category": "fed"},
        {"title": "Jobless Claims", "time": "2026-06-26", "significance": "low",
         "score": 6, "category": "labor"},
    ],
}


def main():
    print("macro_viz smoke tests:")

    e = macro_viz.environment_strip(ENV)
    check("env renders svg", e.startswith("<svg") and e.rstrip().endswith("</svg>"))
    check("env shows a feature label", "DXY 20d return" in e)
    check("env shows a z readout", "+0.80" in e or "+0.8" in e)
    check("env shows as-of date", "as of 2026-06-22" in e)
    check("env draws sigma gridlines", "σ" in e)
    check("env empty on no features", macro_viz.environment_strip({"features_zscored": {}}) == "")
    check("env empty on None", macro_viz.environment_strip(None) == "")
    # Unknown feature key still renders (forward-compatible).
    e2 = macro_viz.environment_strip({"features_zscored": {"new_feature_x": 1.0}})
    check("env tolerates unknown feature key", e2.startswith("<svg") and "new feature x" in e2)

    c = macro_viz.catalyst_timeline(CATS)
    check("cat renders svg", c.startswith("<svg") and c.rstrip().endswith("</svg>"))
    check("cat shows an event title", "Core PCE" in c)
    check("cat marks NOW", "NOW" in c)
    check("cat groups by date (shows a date label)", "06-25" in c or "06-24" in c)
    check("cat shows label", "this week" in c)
    check("cat empty on no catalysts", macro_viz.catalyst_timeline({"catalysts": []}) == "")
    check("cat empty on None", macro_viz.catalyst_timeline(None) == "")
    # Unparseable dates are dropped, not fatal.
    c2 = macro_viz.catalyst_timeline({"catalysts": [{"title": "bad", "time": "not-a-date"}]})
    check("cat empty when all dates unparseable", c2 == "")
    # Many same-date events stack vertically with a "+N more" overflow tag rather
    # than overprinting on one x-position.
    same_day = {"catalysts": [
        {"title": f"Event {i}", "time": "2026-06-25", "significance": "high",
         "score": 30 - i} for i in range(8)]}
    cs = macro_viz.catalyst_timeline(same_day)
    check("cat stacks same-date events with overflow tag", "more" in cs)

    # Token regex.
    check("env token regex matches", bool(macro_viz.TOKEN_RE.search(
        "x <!--MACRO_VIZ:environment:--> y")))
    check("cat token regex matches", bool(macro_viz.TOKEN_RE.search(
        "x <!--MACRO_VIZ:catalysts:--> y")))
    out = macro_viz.expand("a <!--MACRO_VIZ:environment:--> b",
                           pftui="/nonexistent/pftui-binary")
    check("expand strips token on no data", "MACRO_VIZ" not in out)

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
