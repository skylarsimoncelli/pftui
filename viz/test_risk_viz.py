#!/usr/bin/env python3
"""Smoke tests for the risk/regime visualizations — pure functions, no pftui binary.

Run: python viz/test_risk_viz.py   (exits nonzero on failure)
Feeds synthetic tail-dependence dicts (the shape `analytics tail-dependence
--json` emits) to the SVG renderers and checks output + graceful degradation.
Uses only synthetic market tickers (BTC/GOLD/SPY) — never real portfolio data.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import risk_viz  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


# Synthetic basket: BTC / GOLD / SPY. Pearson + empirical lower-tail λ_L per pair.
ASSETS = ["BTC", "GOLD", "SPY"]
PAIRS = {
    (0, 1): {"pearson": 0.08, "lambda_l": 0.05},   # BTC vs GOLD: diversifies
    (0, 2): {"pearson": 0.22, "lambda_l": 0.20},   # BTC vs SPY: partial co-crash
    (1, 2): {"pearson": 0.00, "lambda_l": 0.10},   # GOLD vs SPY
}


def main():
    print("risk_viz smoke tests:")

    # λ_L danger ramp + neutral corr ramp endpoints are sane, distinct hex
    g, a, r = risk_viz._danger_ramp(0.0), risk_viz._danger_ramp(0.5), risk_viz._danger_ramp(1.0)
    check("danger ramp returns hex", all(c.startswith("#") and len(c) == 7 for c in (g, a, r)))
    check("danger ramp endpoints differ", g != r)
    # correlation uses a SEPARATE (non-danger) ramp; |corr| drives intensity so
    # a strong negative reads stronger than a near-zero correlation.
    check("corr ramp != danger ramp at high value", risk_viz._corr_fill(0.8) != risk_viz._danger_ramp(0.8))
    check("corr ramp uses |corr|", risk_viz._corr_fill(-0.8) == risk_viz._corr_fill(0.8))

    m = risk_viz.cocrash_matrix(ASSETS, PAIRS, "Co-Crash Matrix")
    check("matrix renders svg", m.startswith("<svg") and m.rstrip().endswith("</svg>"))
    check("matrix shows asset labels", "BTC" in m and "GOLD" in m and "SPY" in m)
    check("matrix shows a correlation cell", "+0.22" in m)
    check("matrix shows a lambda cell", "0.20" in m)
    check("matrix has legend", "co-crashes" in m and "diversifies" in m and "|corr|" in m)
    check("matrix labels triangles", "correlation" in m and "co-crash" in m)

    # Missing pair -> '--' placeholder cell, still renders (graceful).
    partial = risk_viz.cocrash_matrix(ASSETS, {(0, 1): {"pearson": 0.1, "lambda_l": 0.1}}, "t")
    check("matrix tolerates missing pairs", partial.startswith("<svg") and "--" in partial)

    # Graceful degradation: too few assets / no pairs -> empty string, no exception.
    check("matrix empty on <2 assets", risk_viz.cocrash_matrix(["BTC"], PAIRS, "t") == "")
    check("matrix empty on no pairs", risk_viz.cocrash_matrix(ASSETS, {}, "t") == "")

    # render() with an unknown viz type -> empty, never raises.
    check("render unknown type empty", risk_viz.render("nope", "BTC,GOLD", pftui=None) == "")

    # Token regex matches the documented form.
    md = "before <!--RISK_VIZ:cocrash:BTC,GOLD,SPY--> after"
    mt = risk_viz.TOKEN_RE.search(md)
    check("token regex matches", bool(mt))
    check("token captures basket", bool(mt) and mt.group(2) == "BTC,GOLD,SPY")

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
