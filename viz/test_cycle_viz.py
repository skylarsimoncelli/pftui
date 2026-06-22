#!/usr/bin/env python3
"""Smoke tests for the cycle visualizations — pure functions, no pftui binary.

Run: python viz/test_cycle_viz.py   (exits nonzero on failure)
Feeds synthetic cycle-report dicts (the shape `analytics cycles analyze --json`
emits) to the SVG renderers and checks output + graceful degradation.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cycle_viz  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


# Minimal synthetic report with one degree carrying every field the viz reads.
REPORT = {
    "degrees": [{
        "degree": "4-year",
        "unit": "d",
        "clarity": "amber",
        "cycle_age_bars": 600,
        "expected_len_bars": 1461,
        "band_position": "pre_band",
        "lows": [
            {"date": "2015-01-14"},
            {"date": "2018-12-15"},
            {"date": "2022-11-21"},
        ],
        "last_confirmed_low": {"date": "2022-11-21"},
        "next_low_window": {"start_date": "2028-04-01", "end_date": "2028-08-01"},
        "current_top": {"date": "2025-03-01", "provisional_translation_pct": 0.73},
        "band": {"band_lo_bars": 1314, "band_hi_bars": 1607},
        "ledger": [
            {"class": "RT", "translation_pct": 0.75, "len_bars": 1430,
             "start_date": "2015-01-14", "end_date": "2018-12-15"},
            {"class": "RT", "translation_pct": 0.74, "len_bars": 1437,
             "start_date": "2018-12-15", "end_date": "2022-11-21"},
        ],
    }],
}


def main():
    print("cycle_viz smoke tests:")

    m = cycle_viz.cycle_map(REPORT, "4-year", "BTC — 4-Year Cycle Map")
    check("map renders svg", m.startswith("<svg") and m.rstrip().endswith("</svg>"))
    check("map shows next-low window", "NEXT-LOW WINDOW" in m)
    check("map marks NOW", ">NOW<" in m)

    d = cycle_viz.cycle_dial("BTC 4-YEAR", 91, "accumulate", "wk 187 of 187-229 band", cycle_viz.CYAN)
    check("dial renders svg", d.startswith("<svg"))
    check("dial shows pct", ">91%<" in d)
    check("dial shows stance", "ACCUMULATE" in d)

    t = cycle_viz.translation_strip(REPORT, "4-year", "BTC — Translation Ledger")
    check("ledger renders svg", t.startswith("<svg"))
    check("ledger includes live in-progress bar (RT)", "(now)" in t)
    check("ledger shows RT class", "RT 75%" in t or "RT 73%" in t)

    # Graceful degradation: missing fields -> empty string, never an exception.
    check("map empty on no degrees", cycle_viz.cycle_map({"degrees": []}, "x", "t") == "")
    check("ledger empty on missing band",
          cycle_viz.translation_strip({"degrees": [{"degree": "d", "ledger": []}]}, "d", "t") == "")

    # Token regex matches the documented form.
    md = "before <!--CYCLE_VIZ:map:BTC--> after"
    check("token regex matches", bool(cycle_viz.TOKEN_RE.search(md)))

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
