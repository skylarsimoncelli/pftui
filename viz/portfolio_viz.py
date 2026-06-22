#!/usr/bin/env python3
"""Portfolio / risk-sizing visualizations (inline SVG) for the pftui report PDFs.

Two decision-useful, single-asset risk charts, each rendered from a hardened
`pftui analytics` `--json` contract (see viz/theme.py for the Rust data boundary).
Both read only the asset's `price_history` — NO portfolio weights/balances — so
they never surface the operator's holdings:

  drawdown  — Drawdown-Survival composite: the depth AND time a long-hold has to
              sit through, plus whether you can be forced out before the cycle
              turns. CDaR-95 / expected-max-DD depth bars, the D/(1-D) recovery
              cliff, time-under-water (i.i.d. vs AR(1) serial-correlation
              corrected), and a risk-of-ruin gauge vs the drawdown budget.
              Data: `analytics survival --asset SYM` (+ falls back to
              `analytics risk-dashboard` which embeds the same `survival` block).
  riskbars  — Risk fingerprint: a compact normalized bar panel of the measured
              risk primitives — CDaR-95/90, Ulcer, max-DD, drawdown-from-ATH,
              annualized vol — with the EVT tail class called out.
              Data: `analytics risk-dashboard --asset SYM`.

CLI:   python portfolio_viz.py drawdown --asset BTC
Token: <!--PORTFOLIO_VIZ:drawdown:BTC-->  (expanded by viz/render.py in the report
       pipeline). Also: <!--PORTFOLIO_VIZ:riskbars:BTC-->

Design note: the "allocation vs risk-parity" chart was intentionally NOT built
here. `analytics basket weights --json` emits only the SUGGESTED (ERC / downside-
RP) weights; it carries no CURRENT book weights, and reading the operator's real
allocation is forbidden in this domain. Without both sides the over/under-weight
comparison is not possible from `--json` alone. (Gap noted for a future Rust CLI
that pairs suggested vs current weights behind the same privacy boundary.)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BG, BLUE, BORDER, CYAN, GREEN, MONO, MUTED, PANEL, RED, TEXT,
    RUIN_OK, RUIN_WATCH, caption, esc, good_bad, pftui_json, svg_open, title,
)


def _nice(asset):
    return asset.upper().replace("GC=F", "GOLD").replace("SI=F", "SILVER")


def _pct(frac):
    """Fraction (0.42) -> '42%'. None -> '--'."""
    return f"{frac * 100:.0f}%" if frac is not None else "--"


def _years(days):
    """Trading/calendar days -> compact duration label. None -> '--'."""
    if days is None:
        return "--"
    if days >= 365:
        return f"{days / 365.25:.1f}y"
    return f"{days:.0f}d"


# --------------------------------------------------- VIZ 1: DRAWDOWN SURVIVAL
def drawdown_survival(surv, ttl):
    """Composite of the `survival` JSON block: depth bars + recovery cliff +
    time-under-water + risk-of-ruin gauge. '' if the block is unusable."""
    if not surv:
        return ""
    ruin = surv.get("ruin_prob")
    budget = surv.get("budget_pct")
    if ruin is None or budget is None:
        return ""

    reliable = bool(surv.get("reliable"))
    cdar = surv.get("cdar95")
    recov = surv.get("recovery_required_at_cdar95")
    max_dd_iid = surv.get("max_dd_iid")
    max_dd_ar1 = surv.get("max_dd_ar1")
    tuw_iid = surv.get("max_tuw_iid_days")
    tuw_ar1 = surv.get("max_tuw_ar1_days")
    regime = surv.get("regime", "")

    W, H, ml, mr = 720, 250, 16, 16
    s = [svg_open(W, H), title(ml, 24, ttl)]
    # reliability badge (top-right)
    bc = GREEN if reliable else RED
    btxt = "drift>0 · reliable" if reliable else "drift<=0 · ruin certain"
    s.append(f'<text x="{W - mr}" y="24" text-anchor="end" fill="{bc}" font-size="9" '
             f'font-family={MONO!r}>{esc(btxt)}</text>')

    # Shared bar grid: BOTH panels start their bars at the same label-offset and
    # use the same bar length, so the left depth-bars and right TUW-bars align to
    # one column rhythm.
    LABEL_W, BAR_LEN = 118, 170

    # ---- LEFT PANEL: depth bars (CDaR-95, expected max-DD i.i.d / AR(1)) ----
    lx, lw = ml, 300
    s.append(f'<text x="{lx}" y="48" fill="{MUTED}" font-size="9" font-family={MONO!r}>'
             f'HOW DEEP &#8212; drawdown depth</text>')
    bars = [
        ("CDaR-95", cdar, RED),
        ("Exp.maxDD i.i.d", max_dd_iid, AMBER),
        ("Exp.maxDD AR(1)", max_dd_ar1, BLUE),
    ]
    present = [b for b in bars if b[1] is not None]
    scale = max([b[1] for b in present] + [0.01]) if present else 1.0
    barx, barw = lx + LABEL_W, BAR_LEN
    by = 60
    for label, val, col in bars:
        s.append(f'<text x="{lx}" y="{by + 11}" fill="{TEXT}" font-size="9" '
                 f'font-family={MONO!r}>{esc(label)}</text>')
        s.append(f'<rect x="{barx}" y="{by + 2}" width="{barw}" height="12" rx="2" '
                 f'fill="{BORDER}" fill-opacity="0.35"/>')
        if val is not None:
            w = max(2.0, val / scale * barw)
            s.append(f'<rect x="{barx}" y="{by + 2}" width="{w:.1f}" height="12" rx="2" '
                     f'fill="{col}" fill-opacity="0.85"/>')
            s.append(f'<text x="{barx + w + 5:.1f}" y="{by + 12}" fill="{col}" font-size="9" '
                     f'font-weight="600" font-family={MONO!r}>{_pct(val)}</text>')
        else:
            s.append(f'<text x="{barx + 5}" y="{by + 12}" fill="{MUTED}" font-size="9" '
                     f'font-family={MONO!r}>--</text>')
        by += 22

    # recovery cliff: gain to erase the CDaR-95 drawdown
    cy = by + 14
    s.append(f'<text x="{lx}" y="{cy}" fill="{MUTED}" font-size="9" '
             f'font-family={MONO!r}>RECOVERY CLIFF &#8212; gain to erase CDaR-95</text>')
    if cdar is not None and recov is not None:
        # The recovery figure is the punchline — give it real visual hierarchy:
        # a large AMBER number, with the framing text small beside/under it.
        s.append(f'<text x="{lx}" y="{cy + 34}" fill="{AMBER}" font-size="26" '
                 f'font-weight="700" font-family={MONO!r}>+{recov * 100:.0f}%</text>')
        s.append(f'<text x="{lx + 118}" y="{cy + 26}" fill="{TEXT}" font-size="9.5" '
                 f'font-family={MONO!r}>to erase a <tspan fill="{RED}" font-weight="600">'
                 f'{_pct(cdar)}</tspan> drop</text>')
        s.append(f'<text x="{lx + 118}" y="{cy + 40}" fill="{MUTED}" font-size="8" '
                 f'font-family={MONO!r}>D/(1&#8722;D) cliff &#183; asymmetry compounds</text>')
    else:
        s.append(f'<text x="{lx}" y="{cy + 26}" fill="{MUTED}" font-size="10" '
                 f'font-family={MONO!r}>(no CDaR-95 recovery figure)</text>')

    # ---- RIGHT PANEL: time-under-water + risk-of-ruin gauge ----
    rx = lx + lw + 36
    s.append(f'<line x1="{rx - 18}" y1="44" x2="{rx - 18}" y2="{H - 16}" '
             f'stroke="{BORDER}" stroke-opacity="0.6"/>')
    s.append(f'<text x="{rx}" y="48" fill="{MUTED}" font-size="9" font-family={MONO!r}>'
             f'HOW LONG &#8212; time under water</text>')
    tw_iidw, tw_ar1w = 0.0, 0.0
    tscale = max([d for d in (tuw_iid, tuw_ar1) if d is not None] + [1.0])
    rtbx, rtbw = rx + LABEL_W, BAR_LEN   # same grid as the left depth-bars
    ty = 60
    for label, val, col in (("i.i.d.", tuw_iid, AMBER), ("AR(1)", tuw_ar1, BLUE)):
        s.append(f'<text x="{rx}" y="{ty + 11}" fill="{TEXT}" font-size="9" '
                 f'font-family={MONO!r}>{esc(label)}</text>')
        s.append(f'<rect x="{rtbx}" y="{ty + 2}" width="{rtbw}" height="12" rx="2" '
                 f'fill="{BORDER}" fill-opacity="0.35"/>')
        if val is not None:
            w = max(2.0, val / tscale * rtbw)
            s.append(f'<rect x="{rtbx}" y="{ty + 2}" width="{w:.1f}" height="12" rx="2" '
                     f'fill="{col}" fill-opacity="0.85"/>')
            s.append(f'<text x="{rtbx + w + 5:.1f}" y="{ty + 12}" fill="{col}" font-size="9" '
                     f'font-weight="600" font-family={MONO!r}>{_years(val)}</text>')
        else:
            s.append(f'<text x="{rtbx + 5}" y="{ty + 12}" fill="{MUTED}" font-size="9" '
                     f'font-family={MONO!r}>--</text>')
        ty += 22
    s.append(f'<text x="{rx}" y="{ty + 8}" fill="{MUTED}" font-size="8" '
             f'font-family={MONO!r}>AR(1) corrects for trending-cycle serial correlation</text>')

    # risk-of-ruin gauge
    gy = ty + 30
    s.append(f'<text x="{rx}" y="{gy}" fill="{MUTED}" font-size="9" '
             f'font-family={MONO!r}>RISK OF RUIN &#8212; breach the {budget:.0f}% budget</text>')
    gbx, gby, gbw, gbh = rx, gy + 12, 240, 16
    s.append(f'<rect x="{gbx}" y="{gby}" width="{gbw}" height="{gbh}" rx="3" '
             f'fill="{BORDER}" fill-opacity="0.35"/>')
    ruin = max(0.0, min(1.0, float(ruin)))
    # Shared semantic thresholds (RUIN_OK / RUIN_WATCH) -> GREEN/AMBER/RED, so the
    # ruin gauge reads on the same 'green = good' scale as every other chart.
    gcol = good_bad(ruin, RUIN_OK, RUIN_WATCH)
    s.append(f'<rect x="{gbx}" y="{gby}" width="{max(2.0, ruin * gbw):.1f}" height="{gbh}" rx="3" '
             f'fill="{gcol}" fill-opacity="0.85"/>')
    s.append(f'<text x="{gbx + gbw + 8}" y="{gby + 13}" fill="{gcol}" font-size="13" '
             f'font-weight="700" font-family={MONO!r}>{ruin * 100:.0f}%</text>')
    verdict = ("survivable" if ruin < RUIN_OK
               else ("watch" if ruin < RUIN_WATCH else "HIGH ruin risk"))
    if not reliable:
        verdict = "no positive drift &#8212; hold only with cycle conviction"
    s.append(f'<text x="{gbx}" y="{gby + gbh + 16}" fill="{gcol}" font-size="9" '
             f'font-weight="600" font-family={MONO!r}>{verdict}</text>')

    # footer regime read (shared caption slot)
    if regime:
        s.append(caption(ml, H - 8, regime))
    return "\n".join(s) + "\n</svg>"


# ------------------------------------------------------ VIZ 2: RISK FINGERPRINT
def risk_fingerprint(dash, ttl):
    """Compact normalized bar panel of the measured risk primitives from
    `risk-dashboard` JSON. '' if nothing plottable."""
    if not dash:
        return ""
    dd = dash.get("drawdown_path") or {}
    rows = []
    # all as POSITIVE magnitudes (deeper/larger = worse), fraction or pct
    cdar95 = dd.get("cdar_95")
    cdar90 = dd.get("cdar_90")
    ulcer = dd.get("ulcer_index_pct")
    max_dd = dash.get("max_drawdown_pct")
    dd_ath = dash.get("drawdown_from_ath_pct")
    vol = dash.get("annualized_vol_pct")
    if cdar95 is not None:
        rows.append(("CDaR-95", cdar95 * 100, RED, "worst-5% mean depth"))
    if cdar90 is not None:
        rows.append(("CDaR-90", cdar90 * 100, AMBER, "worst-10% mean depth"))
    if max_dd is not None:
        rows.append(("Max DD", abs(max_dd), RED, "deepest historical"))
    if dd_ath is not None:
        rows.append(("From ATH", abs(dd_ath), BLUE, "current underwater"))
    if ulcer is not None:
        rows.append(("Ulcer", ulcer, AMBER, "duration-weighted pain"))
    if vol is not None:
        rows.append(("Vol/yr", vol, CYAN, "annualized"))
    if not rows:
        return ""

    n = len(rows)
    W, ml, mr, rowh = 720, 16, 16, 26
    H = 58 + n * rowh + 16   # +16 reserves the relative-scale note row
    scale = max(r[1] for r in rows) or 1.0
    barx, barw = ml + 96, W - 96 - 150 - ml - mr
    s = [svg_open(W, H), title(ml, 24, ttl)]
    # tail-class badge from EVT
    evt = dash.get("tail_risk") or {}
    tc = evt.get("tail_class")
    if tc:
        xi = evt.get("xi")
        xtxt = f"tail {esc(tc)}" + (f" (&#958;={xi:+.2f})" if xi is not None else "")
        tcol = RED if (xi is not None and xi >= 0.25) else MUTED
        s.append(f'<text x="{W - mr}" y="24" text-anchor="end" fill="{tcol}" font-size="9" '
                 f'font-family={MONO!r}>{xtxt}</text>')

    y = 44
    for label, val, col, note in rows:
        s.append(f'<text x="{ml}" y="{y + 13}" fill="{TEXT}" font-size="9.5" '
                 f'font-family={MONO!r}>{esc(label)}</text>')
        s.append(f'<rect x="{barx}" y="{y + 3}" width="{barw}" height="13" rx="2" '
                 f'fill="{BORDER}" fill-opacity="0.35"/>')
        w = max(2.0, val / scale * barw)
        s.append(f'<rect x="{barx}" y="{y + 3}" width="{w:.1f}" height="13" rx="2" '
                 f'fill="{col}" fill-opacity="0.85"/>')
        s.append(f'<text x="{barx + w + 6:.1f}" y="{y + 14}" fill="{col}" font-size="9.5" '
                 f'font-weight="600" font-family={MONO!r}>{val:.0f}%</text>')
        s.append(f'<text x="{W - mr}" y="{y + 14}" text-anchor="end" fill="{MUTED}" '
                 f'font-size="8" font-family={MONO!r}>{esc(note)}</text>')
        y += rowh
    # Faint 100% reference tick + a one-line note that the bar length is
    # normalized to the worst metric (the printed %s are the absolute figures).
    s.append(f'<line x1="{barx + barw:.1f}" y1="44" x2="{barx + barw:.1f}" '
             f'y2="{y - rowh + 19}" stroke="{MUTED}" stroke-width="0.6" '
             f'stroke-dasharray="2 3" stroke-opacity="0.5"/>')
    s.append(caption(ml, H - 7,
                     "bar length = relative to the worst metric (100%); printed % = absolute figure"))
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def render(viz_type, asset, pftui=None):
    """Render one portfolio/risk viz ('drawdown'|'riskbars') for an asset.
    '' on any failure (chart degrades to nothing, never breaks a report)."""
    try:
        nice = _nice(asset)
        if viz_type == "drawdown":
            data = pftui_json(["analytics", "survival", "--asset", asset], pftui)
            surv = (data or {}).get("survival")
            if not surv:
                # fall back to the embedded survival block in the risk dashboard
                dash = pftui_json(["analytics", "risk-dashboard", "--asset", asset], pftui)
                surv = (dash or {}).get("survival")
            return drawdown_survival(surv, f"{nice} — Drawdown Survival (depth · time · ruin)")
        if viz_type == "riskbars":
            dash = pftui_json(["analytics", "risk-dashboard", "--asset", asset], pftui)
            return risk_fingerprint(dash, f"{nice} — Risk Fingerprint")
        return ""
    except Exception:  # never let a chart break a report
        return ""


# Token contract for viz/render.py: <!--PORTFOLIO_VIZ:type:asset-->
TOKEN_RE = re.compile(r"<!--\s*PORTFOLIO_VIZ:([a-z]+):([^\s>]+?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="portfolio-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(
        description="Render a pftui portfolio/risk-sizing visualization as inline SVG.")
    p.add_argument("viz", choices=["drawdown", "riskbars"])
    p.add_argument("--asset", required=True)
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    svg = render(args.viz, args.asset, args.pftui)
    if not svg:
        sys.stderr.write(f"no portfolio viz available for {args.viz}:{args.asset}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
