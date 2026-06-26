#!/usr/bin/env python3
"""Smoke tests for the cycle-signal backtest report card — pure functions, no binary.

Run: python viz/test_cycle_backtest_viz.py   (exits nonzero on failure)
Feeds synthetic `analytics cycles {bottom,top}-signals backtest --expectancy
--json`-shaped dicts (per docs/CYCLE-SIGNALS.md — numbers are rust_decimal
STRINGS) to the renderer + the sign-aware helper. Synthetic data only — no real
portfolio values, no real market reads.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cycle_backtest_viz as v  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


def _horizons(means, hits, base, lifts, rate_key="positive_rate_pct"):
    out = []
    for i, h in enumerate([30, 90, 180, 365]):
        out.append({
            "horizon_days": h, "samples": 20,
            "mean_return_pct": f"{means[i]:.2f}",
            "median_return_pct": f"{means[i]:.2f}",
            rate_key: f"{hits[i]:.1f}",
            "baseline_mean_return_pct": f"{base[i]:.2f}",
            "lift_vs_baseline_pct": f"{lifts[i]:.2f}",
        })
    return out


def _baseline(means):
    return [{"horizon_days": h, "samples": 500,
             "mean_return_pct": f"{means[i]:.2f}",
             "median_return_pct": f"{means[i]:.2f}",
             "positive_rate_pct": "55.0"} for i, h in enumerate([30, 90, 180, 365])]


# ---- BOTTOM expectancy: signal returns positive, beats baseline ----
BOTTOM = {
    "symbol": "BTC", "resolved_symbol": "BTC-USD", "timeframe": "monthly",
    "expectancy": {
        "price_structure_anchors": ["2018-12-15", "2020-03-13", "2022-11-21"],
        "anchors_used": 13, "insufficient_anchors": False, "small_n": False,
        "baseline": _baseline([6.0, 12.0, 24.0, 40.0]),
        "confluence": [
            {"key": "confluence_ge_3", "threshold": 3,
             "label": "Confluence ≥3/7 criteria firing", "firings": 23,
             "horizons": _horizons([7.2, 15.5, 33.0, 70.0], [65, 70, 72, 80],
                                   [6.0, 12.0, 24.0, 40.0], [1.2, 3.5, 9.0, 30.0]),
             "closeness": {"matched_firings": 14, "firings": 23,
                           "median_lead_lag_days": 26, "median_price_gap_pct": "32.89",
                           "confidence_pct": "60.9"}},
            {"key": "confluence_ge_4", "threshold": 4,
             "label": "Confluence ≥4/7 criteria firing", "firings": 12,
             "horizons": _horizons([9.0, 20.0, 45.0, 95.0], [70, 75, 80, 88],
                                   [6.0, 12.0, 24.0, 40.0], [3.0, 8.0, 21.0, 55.0]),
             "closeness": {"matched_firings": 9, "firings": 12,
                           "median_lead_lag_days": 12, "median_price_gap_pct": "18.40",
                           "confidence_pct": "75.0"}},
            {"key": "confluence_ge_5", "threshold": 5,
             "label": "Confluence ≥5/7 criteria firing", "firings": 5,
             "horizons": _horizons([12.0, 28.0, 60.0, 120.0], [80, 80, 100, 100],
                                   [6.0, 12.0, 24.0, 40.0], [6.0, 16.0, 36.0, 80.0]),
             "closeness": {"matched_firings": 5, "firings": 5,
                           "median_lead_lag_days": 3, "median_price_gap_pct": "8.10",
                           "confidence_pct": "100.0"}},
        ],
        "criteria": [],
        "caveat": "Expectancy conditioned on 13 cycle-low anchor(s); price-derived, directional.",
    },
}

# ---- TOP expectancy: signal returns NEGATIVE (good for a top), negative_rate_pct ----
TOP = {
    "symbol": "BTC", "resolved_symbol": "BTC-USD", "timeframe": "monthly",
    "expectancy": {
        "price_structure_anchors": ["2017-12-17", "2021-11-10"],
        "anchors_used": 6, "insufficient_anchors": False, "small_n": True,
        "baseline": _baseline([3.0, 5.0, 9.0, 15.0]),
        "confluence": [
            {"key": "confluence_ge_4", "threshold": 4,
             "label": "Confluence ≥4/7 criteria firing", "firings": 8,
             "horizons": _horizons([-8.0, -18.0, -30.0, -45.0], [70, 75, 80, 85],
                                   [3.0, 5.0, 9.0, 15.0],
                                   [-11.0, -23.0, -39.0, -60.0],
                                   rate_key="negative_rate_pct"),
             "closeness": {"matched_firings": 5, "firings": 8,
                           "median_lead_lag_days": -9, "median_price_gap_pct": "-12.50",
                           "confidence_pct": "62.5"}},
        ],
        "criteria": [],
        "caveat": "No doctrine top anchors; conditioned on 6 swing-high pivots — directional.",
    },
}

# ---- INSUFFICIENT: zero firings / no anchors -> caveat card ----
EMPTY = {
    "symbol": "SPY", "resolved_symbol": "SPY", "timeframe": "monthly",
    "expectancy": {
        "price_structure_anchors": [], "anchors_used": 0,
        "insufficient_anchors": True, "small_n": True,
        "baseline": _baseline([1.0, 2.0, 4.0, 8.0]),
        "confluence": [], "criteria": [],
        "caveat": "insufficient_anchors: too few swing lows to grade reliability.",
    },
}

BANNED = ["Loukas", "Bressert", "Hurst", "Mayer", "Ehlers", "RSI", "DSS",
          "Cyber", "halving", "BTC-USD", "GC=F"]


def main():
    print("cycle_backtest_viz smoke tests:")

    # --- sign-aware helper (the load-bearing logic) ---
    check("bottom + up return = good", v.signal_quality("bottom", 7.2) == "good")
    check("bottom + down return = bad", v.signal_quality("bottom", -7.2) == "bad")
    check("top + down return = good", v.signal_quality("top", -8.0) == "good")
    check("top + up return = bad", v.signal_quality("top", 8.0) == "bad")
    check("flat ~ 0 = flat", v.signal_quality("bottom", 0.0) == "flat")
    check("helper parses decimal STRING", v.signal_quality("bottom", "7.2") == "good")
    check("helper handles None", v.signal_quality("bottom", None) == "flat")

    # --- bottom card ---
    svg = v.expectancy_card(BOTTOM, "bottom")
    check("bottom renders svg",
          svg.startswith("<svg") and svg.rstrip().endswith("</svg>"))
    check("bottom shows friendly name", "Bitcoin" in svg)
    check("bottom titled cycle-bottom", "cycle-bottom backtest" in svg)
    check("bottom shows anchor count", "13 swing lows" in svg)
    check("bottom headline is >=4 (12 firings)", "12 firings" in svg)
    check("bottom shows a lift annotation", "lift +" in svg)
    check("bottom shows confidence column value", "75%" in svg)
    check("bottom green (good) bar present", v.GREEN in svg)
    check("bottom name-free + ticker-free", all(b not in svg for b in BANNED))

    # --- top card (sign-flipped) ---
    tsvg = v.expectancy_card(TOP, "top")
    check("top renders svg", tsvg.startswith("<svg"))
    check("top titled cycle-top", "cycle-top backtest" in tsvg)
    check("top shows swing highs", "swing highs" in tsvg)
    check("top small-n badge", "small-n" in tsvg)
    # a negative mean on a top is GOOD -> green; a positive would be red.
    check("top good (down) colored green", v.GREEN in tsvg)
    check("top legend says fall (good)", "fall (good)" in tsvg)
    check("top negative mean rendered", "-8.0%" in tsvg or "-18.0%" in tsvg)

    # --- insufficient -> honest caveat card ---
    esvg = v.expectancy_card(EMPTY, "bottom")
    check("empty renders caveat card", esvg.startswith("<svg"))
    check("empty says unmeasurable", "Reliability unmeasurable" in esvg)
    check("empty NOT drawing bars (no 'lift +')", "lift +" not in esvg)

    # --- graceful degradation ---
    check("None -> ''", v.expectancy_card(None, "bottom") == "")
    check("no expectancy block -> ''", v.expectancy_card({"symbol": "X"}, "bottom") == "")

    # --- payload + headline picker ---
    check("payload parses polarity+tf",
          v._parse_payload("BTC?polarity=top&timeframe=weekly") == ("BTC", "top", "weekly"))
    check("payload defaults bottom/monthly",
          v._parse_payload("GC=F") == ("GC=F", "bottom", "monthly"))
    check("headline picks >=4",
          v._pick_headline(BOTTOM["expectancy"]["confluence"])["threshold"] == 4)
    check("hit-rate reads negative_rate for tops",
          v._hit_rate(TOP["expectancy"]["confluence"][0]["horizons"][0], "top") == 70.0)
    check("hit-rate reads positive_rate for bottoms",
          v._hit_rate(BOTTOM["expectancy"]["confluence"][0]["horizons"][0], "bottom") == 65.0)

    # --- token regex + expand strip on no data ---
    check("token regex matches", bool(v.TOKEN_RE.search(
        "x <!--CYCLE_BACKTEST_VIZ:expectancy:BTC?polarity=bottom&timeframe=monthly--> y")))
    out = v.expand("a <!--CYCLE_BACKTEST_VIZ:expectancy:BTC--> b",
                   pftui="/nonexistent/pftui-binary")
    check("expand strips token on no data", "CYCLE_BACKTEST_VIZ" not in out)

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
