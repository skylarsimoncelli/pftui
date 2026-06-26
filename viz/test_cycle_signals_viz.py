#!/usr/bin/env python3
"""Smoke tests for the cycle-bottom signal checklist — pure functions, no binary.

Run: python viz/test_cycle_signals_viz.py   (exits nonzero on failure)
Feeds synthetic `analytics cycles bottom-signals --json`-shaped dicts to the
renderer and checks output + graceful degradation. Synthetic data only — no real
portfolio values, no real market reads.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cycle_signals_viz as v  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


def _crit(key, label, met, detail=""):
    return {"key": key, "label": label, "met": met, "detail": detail,
            "components": [{"key": key + "_c", "label": "comp", "met": met}]}


# 4 of 7 firing — "building confluence" band.
DATA = {
    "symbol": "BTC", "resolved_symbol": "BTC-USD", "timeframe": "monthly",
    "as_of": "2026-06-23", "met_count": 4, "total": 7,
    "verdict": "monthly suite: 4/7 — building cycle-bottom confluence",
    "criteria": [
        _crit("momentum_turning_up", "Momentum line turning up", True, "RSI-avg up"),
        _crit("momentum_above_price", "Momentum line crossed above price momentum", True, "x"),
        _crit("dss_bottoming", "Double-smoothed stochastic bottoming", True, "oversold"),
        _crit("roofing_confirming_up", "Roofing filter confirming up", True, "green"),
        _crit("volatility_bands_bullish", "Volatility bands bullish (daily)", False, "bearish"),
        _crit("reversal_dots", "Significant reversal dots (weekly/monthly)", False, "0"),
        _crit("trend_line_reclaimed", "Trend line reclaimed (weekly)", False, "below"),
    ],
    "bonus": {"key": "pi_cycle_bottom",
              "label": "Pi-cycle bottom fired recently (bonus)",
              "met": False, "detail": "last bottom 2022-07-13",
              "last_bottom": "2022-07-13"},
}

# Practitioner / indicator brand names that must NOT leak into the public SVG.
BANNED = ["Loukas", "Bressert", "Hurst", "Mayer", "Olson", "Ehlers", "RSI",
          "DSS", "Cyber", "halving", "BTC-USD", "GC=F"]


def main():
    print("cycle_signals_viz smoke tests:")
    svg = v.cycle_signals_checklist(DATA)
    check("renders svg", svg.startswith("<svg") and svg.rstrip().endswith("</svg>"))
    check("shows friendly asset name (Bitcoin, not ticker)", "Bitcoin" in svg)
    check("shows N/7 gauge readout", ">4/7<" in svg)
    check("shows a firing criterion label", "Momentum line turning up" in svg)
    check("shows a non-firing criterion label", "Trend line reclaimed (weekly)" in svg)
    check("has a check mark for firing", "✓" in svg)
    check("has a cross mark for non-firing", "✗" in svg)
    check("verdict band word present", "building confluence" in svg)
    check("name-free + ticker-free (no banned tokens)",
          all(b not in svg for b in BANNED))

    # all-7 band wording.
    full = dict(DATA, met_count=7,
                criteria=[_crit(c["key"], c["label"], True) for c in DATA["criteria"]])
    check("all-7 verdict band", "very strong confluence" in v.cycle_signals_checklist(full))

    # Graceful degradation.
    check("empty on None", v.cycle_signals_checklist(None) == "")
    check("empty on no criteria", v.cycle_signals_checklist({"criteria": []}) == "")

    # No bonus block tolerated.
    nb = dict(DATA); nb.pop("bonus")
    check("renders without bonus", v.cycle_signals_checklist(nb).startswith("<svg"))

    # Payload parse.
    check("payload parses asset+timeframe",
          v._parse_payload("BTC?timeframe=daily") == ("BTC", "daily"))
    check("payload defaults monthly", v._parse_payload("GC=F") == ("GC=F", "monthly"))

    # Token regex + expand stripping on no data.
    check("token regex matches",
          bool(v.TOKEN_RE.search("x <!--CYCLE_SIGNALS_VIZ:checklist:BTC--> y")))
    check("token regex matches tracked",
          bool(v.TOKEN_RE.search("x <!--CYCLE_SIGNALS_VIZ:tracked:all--> y")))
    out = v.expand("a <!--CYCLE_SIGNALS_VIZ:checklist:BTC--> b",
                   pftui="/nonexistent/pftui-binary")
    check("expand strips token on no data", "CYCLE_SIGNALS_VIZ" not in out)

    # ---- tracked-signals dashboard ----
    print("cycle_signals_viz tracked dashboard tests:")
    tracked = {
        "summary": {"total": 4, "bottom": 3, "top": 1, "fired": 1, "close_to_firing": 1},
        "signals": [
            {"asset": "BTC", "polarity": "bottom", "label": "≥4/7 confluence",
             "timeframe": "monthly", "target": 4, "status": "armed", "fired": False,
             "time_since_last": "never", "fire_count": 0,
             "live": {"met_count": 3, "total": 7, "distance_to_target": 1.0,
                      "summary": "3/7 met, 1 to target"}},
            {"asset": "BTC", "polarity": "top", "label": "≥4/7 top confluence",
             "timeframe": "monthly", "target": 4, "status": "fired", "fired": True,
             "time_since_last": "3d 4h ago", "fire_count": 2,
             "last_fired": "2026-06-22",
             "live": {"met_count": 5, "total": 7, "distance_to_target": -1.0,
                      "summary": "5/7 met"}},
            {"asset": "GC=F", "polarity": "bottom", "label": "DSS turned up",
             "timeframe": "weekly", "target": 1, "status": "armed", "fired": False,
             "time_since_last": "never", "fire_count": 0,
             "live": {"met_count": None, "total": None, "distance_to_target": -2.4,
                      "summary": "not met (dist -2.40)"}},
            {"asset": "SPY", "polarity": "bottom", "label": "≥5/7 confluence",
             "timeframe": "daily", "target": 5, "status": "armed", "fired": False,
             "time_since_last": "never", "fire_count": 0,
             "live": {"met_count": 1, "total": 7, "distance_to_target": 4.0,
                      "summary": "1/7 met"}},
        ],
    }
    tsvg = v.cycle_signals_tracked(tracked)
    check("tracked renders svg",
          tsvg.startswith("<svg") and tsvg.rstrip().endswith("</svg>"))
    check("tracked shows summary counts", "4 tracked" in tsvg and "1 fired" in tsvg)
    check("tracked shows friendly asset (Bitcoin)", "Bitcoin" in tsvg)
    check("tracked shows a live met/total readout", ">3/7<" in tsvg or ">5/7<" in tsvg)
    check("tracked shows fired marker", ">yes<" in tsvg)
    check("tracked shows armed marker", ">armed<" in tsvg)
    check("tracked shows non-confluence summary fallback",
          "not met" in tsvg)
    check("tracked shows time-since-last", "3d 4h" in tsvg)
    check("tracked empty on None", v.cycle_signals_tracked(None) == "")
    check("tracked empty on no signals",
          v.cycle_signals_tracked({"signals": []}) == "")
    check("tracked CLI maps asset filter",
          v._tracked_cli("BTC") == ["analytics", "cycles", "tracked", "--asset", "BTC"])
    check("tracked CLI 'all' = no filter",
          v._tracked_cli("all") == ["analytics", "cycles", "tracked"])
    check("tracked CLI maps polarity",
          v._tracked_cli("all?polarity=top") ==
          ["analytics", "cycles", "tracked", "--polarity", "top"])

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
