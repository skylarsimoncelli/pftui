#!/usr/bin/env python3
"""Smoke tests for the portfolio/risk-sizing visualizations — pure functions,
no pftui binary, 100% SYNTHETIC data (no real weights/balances/holdings).

Run: python viz/test_portfolio_viz.py   (exits nonzero on failure)
Feeds synthetic `survival` / `risk-dashboard` dicts (the shapes the respective
`--json` commands emit) to the SVG renderers and checks output + graceful
degradation.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import portfolio_viz  # noqa: E402

FAILS = []


def check(name, cond):
    if not cond:
        FAILS.append(name)
    print(f'  {"ok  " if cond else "FAIL"} {name}')


# Synthetic `survival` block (shape of `analytics survival --json` -> .survival).
# Numbers are invented, not from any real asset/portfolio.
SURV = {
    "mu_pct": 0.18, "sigma_pct": 3.4, "phi": 0.05,
    "max_dd_iid": 0.52, "max_dd_ar1": 0.61,
    "time_to_dd_days": 210.0, "time_to_dd_ar1_days": 260.0,
    "max_tuw_iid_days": 840.0, "max_tuw_ar1_days": 1040.0,
    "recovery_required_at_cdar95": 1.35, "cdar95": 0.58,
    "ruin_prob": 0.12, "budget_pct": 25.0, "confidence": 0.95,
    "regime": "positive drift — Triple-Penance figures reliable",
    "reliable": True,
}

# Synthetic non-reliable survival (mu<=0 cycle-low case).
SURV_UNRELIABLE = {
    "mu_pct": -0.05, "sigma_pct": 4.1, "phi": 0.02,
    "max_dd_iid": None, "max_dd_ar1": None,
    "time_to_dd_days": None, "max_tuw_iid_days": None, "max_tuw_ar1_days": None,
    "recovery_required_at_cdar95": None, "cdar95": None,
    "ruin_prob": 1.0, "budget_pct": 25.0, "confidence": 0.95,
    "regime": "non-positive drift — recovery unbounded", "reliable": False,
}

# Synthetic `risk-dashboard --json` dict (relevant fields only).
DASH = {
    "annualized_vol_pct": 62.0,
    "max_drawdown_pct": -77.0,
    "drawdown_from_ath_pct": -28.5,
    "tail_risk": {"xi": 0.31, "tail_class": "fat", "var_99_pct": 9.1},
    "drawdown_path": {
        "cdar_95": 0.58, "cdar_90": 0.49, "ulcer_index_pct": 31.0,
        "omega_ratio": 1.22,
    },
    "survival": SURV,
}


def main():
    print("portfolio_viz smoke tests:")

    d = portfolio_viz.drawdown_survival(SURV, "BTC — Drawdown Survival")
    check("drawdown renders svg", d.startswith("<svg") and d.rstrip().endswith("</svg>"))
    check("drawdown shows depth panel", "HOW DEEP" in d)
    check("drawdown shows recovery cliff", "+135%" in d)
    check("drawdown shows time-under-water", "HOW LONG" in d)
    check("drawdown shows ruin gauge", "RISK OF RUIN" in d and "12%" in d)
    check("drawdown shows reliable badge", "reliable" in d)

    du = portfolio_viz.drawdown_survival(SURV_UNRELIABLE, "X — Drawdown Survival")
    check("drawdown handles unreliable (mu<=0)", du.startswith("<svg")
          and "cycle conviction" in du)

    r = portfolio_viz.risk_fingerprint(DASH, "BTC — Risk Fingerprint")
    check("riskbars renders svg", r.startswith("<svg"))
    check("riskbars shows CDaR-95", "CDaR-95" in r)
    check("riskbars shows vol", "Vol/yr" in r)
    check("riskbars shows EVT tail class", "tail fat" in r)
    check("riskbars notes relative-to-worst scale", "relative to the worst metric" in r)

    # Recovery cliff is the punchline -> rendered as a large standalone figure.
    check("drawdown emphasizes recovery cliff figure",
          'font-size="26"' in d and "+135%" in d)

    # Shared semantic helpers: green/amber/red by named thresholds + 0..1 ramp.
    from theme import good_bad, ramp, GREEN, AMBER, RED, RUIN_OK, RUIN_WATCH
    check("good_bad maps below ok -> GREEN", good_bad(0.05, RUIN_OK, RUIN_WATCH) == GREEN)
    check("good_bad maps mid -> AMBER", good_bad(0.30, RUIN_OK, RUIN_WATCH) == AMBER)
    check("good_bad maps high -> RED", good_bad(0.80, RUIN_OK, RUIN_WATCH) == RED)
    check("ramp endpoints GREEN..RED", ramp(0.0) == GREEN and ramp(1.0) == RED)

    # Graceful degradation: empty/missing -> '' (never an exception).
    check("drawdown empty on None", portfolio_viz.drawdown_survival(None, "t") == "")
    check("drawdown empty on missing ruin/budget",
          portfolio_viz.drawdown_survival({"reliable": True}, "t") == "")
    check("riskbars empty on None", portfolio_viz.risk_fingerprint(None, "t") == "")
    check("riskbars empty on no plottable fields",
          portfolio_viz.risk_fingerprint({"drawdown_path": {}}, "t") == "")

    # Token regex matches the documented form (both viz types).
    md = "a <!--PORTFOLIO_VIZ:drawdown:BTC--> b <!--PORTFOLIO_VIZ:riskbars:gold--> c"
    matches = portfolio_viz.TOKEN_RE.findall(md)
    check("token regex matches both", matches == [("drawdown", "BTC"), ("riskbars", "gold")])

    # expand() replaces a token to empty string when data unavailable (binary
    # absent in test env -> pftui_json returns None -> '').
    out = portfolio_viz.expand("x <!--PORTFOLIO_VIZ:drawdown:NOPE--> y",
                               pftui="/nonexistent/pftui-binary")
    check("expand degrades to empty on no data", out == "x  y")

    print(f'\n{"PASS" if not FAILS else "FAILURES: " + ", ".join(FAILS)}')
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
