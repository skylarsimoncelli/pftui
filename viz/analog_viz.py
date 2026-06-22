#!/usr/bin/env python3
"""Analog forward-return distribution (inline SVG) for the pftui report PDFs.

The analog-engine showcase, rendered from the hardened `pftui analytics analog`
`--json` contract (see viz/theme.py for the Rust data boundary):

  dist  — the FORWARD-RETURN DISTRIBUTION. When today's macro environment
          (growth x inflation regime + factor distance) is matched against
          history, each closest analog carries the target asset's realized
          forward return over `horizon_days`. This chart draws that spread as a
          horizontal box/whisker: the p25-median-p75 box, the mean with its CI
          band, every individual analog return as a colored tick, and a hard
          zero line. It answers the one question nothing else in the report
          shows: "when the world looked like today, what did this asset do next,
          and how dispersed was it?" The header carries the honesty stats
          (horizon, regime, k_effective / n_distinct_episodes, up-rate).

CLI:   python analog_viz.py dist --asset BTC
Token: <!--ANALOG_VIZ:dist:BTC--> (expanded by viz/render.py in the report pipeline)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BG, BORDER, CYAN, GREEN, MONO, MUTED, RED, TEXT,
    esc, pftui_json, svg_open, title,
)

NICE = {
    "GC=F": "GOLD", "GOLD": "GOLD", "SI=F": "SILVER", "SILVER": "SILVER",
    "BTC-USD": "BTC", "BTC": "BTC", "ETH-USD": "ETH", "ETH": "ETH",
    "^GSPC": "SPX", "SPY": "SPY", "QQQ": "QQQ",
}


def _nice(sym):
    return NICE.get(str(sym).upper(), str(sym).upper())


def _fwds(report):
    """Per-analog forward returns (only the analogs that had a forward value)."""
    out = []
    for a in report.get("analogs") or []:
        r = a.get("forward_return_pct")
        if r is not None:
            out.append({"r": float(r), "regime": a.get("regime"),
                        "date": a.get("date")})
    return out


# ------------------------------------------------------- VIZ: FWD-RETURN DISTRIBUTION
def forward_dist(report, ttl):
    """Render the horizontal forward-return distribution box/whisker.

    Needs the summary quantiles (p25/median/p75) plus at least one per-analog
    forward return. Returns '' when neither is available so the chart degrades.
    """
    fwds = _fwds(report)
    p25 = report.get("p25_forward_pct")
    p50 = report.get("median_forward_pct")
    p75 = report.get("p75_forward_pct")
    mean = report.get("mean_forward_pct")
    ci = report.get("mean_forward_ci_pct") or [None, None]
    ci_lo, ci_hi = (list(ci) + [None, None])[:2]
    if not fwds and (p25 is None or p75 is None):
        return ""

    W, H = 720, 266
    ml, mr = 56, 24
    plot_w = W - ml - mr
    track_y = 132          # vertical centre of the box/whisker track
    box_h = 46
    tick_h = 30            # height of the individual analog ticks

    # Domain: span all data points + the box, padded.
    vals = [d["r"] for d in fwds]
    for v in (p25, p50, p75, mean, ci_lo, ci_hi):
        if v is not None:
            vals.append(float(v))
    if not vals:
        return ""
    lo, hi = min(vals), max(vals)
    if hi - lo < 1e-9:
        lo, hi = lo - 10, hi + 10
    pad = (hi - lo) * 0.08
    lo -= pad
    hi += pad
    # Always include zero in the domain so the zero line is on the page.
    lo = min(lo, 0.0)
    hi = max(hi, 0.0)
    span = (hi - lo) or 1.0
    X = lambda v: ml + (float(v) - lo) / span * plot_w

    s = [svg_open(W, H), title(ml, 26, ttl)]

    # Header / honesty stats (top-right + a sub line under the title).
    horizon = report.get("horizon_days")
    regime = report.get("query_regime")
    keff = report.get("k_effective")
    ndis = report.get("n_distinct_episodes")
    nfwd = report.get("n_with_forward")
    up = report.get("up_rate_pct")
    qdate = report.get("query_date")
    hdr = []
    if horizon is not None:
        hdr.append(f"+{horizon}d horizon")
    if regime:
        hdr.append(f"regime: {regime}")
    if qdate:
        hdr.append(f"as of {qdate}")
    s.append(f'<text x="{W-mr}" y="26" text-anchor="end" fill="{MUTED}" '
             f'font-size="9" font-family={MONO!r}>{esc("  |  ".join(hdr))}</text>')
    honesty = []
    if keff is not None:
        honesty.append(f"k_eff {keff}")
    if ndis is not None:
        honesty.append(f"{ndis} episodes")
    if nfwd is not None:
        honesty.append(f"{nfwd} w/ forward")
    if up is not None:
        honesty.append(f"up-rate {up:.0f}%")
    s.append(f'<text x="{ml}" y="42" fill="{MUTED}" font-size="9" '
             f'font-family={MONO!r}>{esc("  ·  ".join(honesty))}</text>')

    # X axis with gridline ticks (round-ish %).
    axis_y = H - 30
    s.append(f'<line x1="{ml}" y1="{axis_y}" x2="{W-mr}" y2="{axis_y}" '
             f'stroke="{BORDER}" stroke-width="1"/>')
    rng = hi - lo
    step = 10 if rng <= 80 else (25 if rng <= 200 else 50)
    first = int(lo // step) * step
    g = first
    while g <= hi + 1e-9:
        gx = X(g)
        if ml - 1 <= gx <= W - mr + 1:
            is_zero = abs(g) < 1e-9
            dash = "" if is_zero else 'stroke-dasharray="2 3"'
            s.append(f'<line x1="{gx:.1f}" y1="56" x2="{gx:.1f}" y2="{axis_y}" '
                     f'stroke="{TEXT if is_zero else BORDER}" '
                     f'stroke-width="{1.4 if is_zero else 0.5}" {dash} '
                     f'stroke-opacity="{0.7 if is_zero else 0.5}"/>')
            lab = "0%" if is_zero else f"{g:+.0f}%"
            s.append(f'<text x="{gx:.1f}" y="{axis_y+14}" text-anchor="middle" '
                     f'fill="{TEXT if is_zero else MUTED}" font-size="8" '
                     f'font-family={MONO!r}>{lab}</text>')
        g += step
    # Emphatic zero label.
    s.append(f'<text x="{X(0):.1f}" y="52" text-anchor="middle" fill="{TEXT}" '
             f'font-size="8" font-family={MONO!r}>flat</text>')

    # Whiskers (full data range) behind the box.
    if vals:
        wlo, whi = min(vals), max(vals)
        s.append(f'<line x1="{X(wlo):.1f}" y1="{track_y}" x2="{X(whi):.1f}" '
                 f'y2="{track_y}" stroke="{MUTED}" stroke-width="1.2" '
                 f'stroke-opacity="0.6"/>')
        for wv in (wlo, whi):
            s.append(f'<line x1="{X(wv):.1f}" y1="{track_y-10}" x2="{X(wv):.1f}" '
                     f'y2="{track_y+10}" stroke="{MUTED}" stroke-width="1.2" '
                     f'stroke-opacity="0.6"/>')

    # The IQR box (p25..p75) coloured by sign of the median.
    if p25 is not None and p75 is not None:
        bx0, bx1 = X(p25), X(p75)
        med_col = GREEN if (p50 is not None and p50 >= 0) else RED
        s.append(f'<rect x="{bx0:.1f}" y="{track_y-box_h/2}" '
                 f'width="{max(1.0, bx1-bx0):.1f}" height="{box_h}" rx="5" '
                 f'fill="{med_col}" fill-opacity="0.14" stroke="{med_col}" '
                 f'stroke-opacity="0.7"/>')
        s.append(f'<text x="{bx0:.1f}" y="{track_y-box_h/2-5}" text-anchor="middle" '
                 f'fill="{MUTED}" font-size="8" font-family={MONO!r}>p25 {p25:+.0f}%</text>')
        s.append(f'<text x="{bx1:.1f}" y="{track_y-box_h/2-5}" text-anchor="middle" '
                 f'fill="{MUTED}" font-size="8" font-family={MONO!r}>p75 {p75:+.0f}%</text>')
        # Median spine.
        if p50 is not None:
            mx = X(p50)
            s.append(f'<line x1="{mx:.1f}" y1="{track_y-box_h/2}" x2="{mx:.1f}" '
                     f'y2="{track_y+box_h/2}" stroke="{med_col}" stroke-width="2.4"/>')
            s.append(f'<text x="{mx:.1f}" y="{track_y+box_h/2+14}" text-anchor="middle" '
                     f'fill="{med_col}" font-size="9" font-weight="600" '
                     f'font-family={MONO!r}>median {p50:+.1f}%</text>')

    # Mean CI band + mean diamond, drawn as a thin overlay above the box.
    cy_mean = track_y - box_h / 2 - 22
    if ci_lo is not None and ci_hi is not None:
        s.append(f'<line x1="{X(ci_lo):.1f}" y1="{cy_mean}" x2="{X(ci_hi):.1f}" '
                 f'y2="{cy_mean}" stroke="{CYAN}" stroke-width="2" '
                 f'stroke-linecap="round" stroke-opacity="0.7"/>')
        for cv in (ci_lo, ci_hi):
            s.append(f'<line x1="{X(cv):.1f}" y1="{cy_mean-4}" x2="{X(cv):.1f}" '
                     f'y2="{cy_mean+4}" stroke="{CYAN}" stroke-width="1.4" '
                     f'stroke-opacity="0.7"/>')
    if mean is not None:
        mx = X(mean)
        s.append(f'<polygon points="{mx:.1f},{cy_mean-6} {mx+6:.1f},{cy_mean} '
                 f'{mx:.1f},{cy_mean+6} {mx-6:.1f},{cy_mean}" fill="{CYAN}" '
                 f'stroke="{BG}" stroke-width="0.8"/>')
        ci_tag = " CI" if ci_lo is not None else ""
        s.append(f'<text x="{mx:.1f}" y="{cy_mean-10}" text-anchor="middle" '
                 f'fill="{CYAN}" font-size="9" font-weight="600" '
                 f'font-family={MONO!r}>mean {mean:+.1f}%{ci_tag}</text>')

    # Individual analog ticks (the raw evidence), below the track.
    tick_top = track_y + box_h / 2 + 22
    for d in fwds:
        x = X(d["r"])
        col = GREEN if d["r"] >= 0 else RED
        s.append(f'<line x1="{x:.1f}" y1="{tick_top}" x2="{x:.1f}" '
                 f'y2="{tick_top+tick_h}" stroke="{col}" stroke-width="1.4" '
                 f'stroke-opacity="0.85"/>')
        s.append(f'<circle cx="{x:.1f}" cy="{tick_top}" r="2.2" fill="{col}"/>')
    if fwds:
        s.append(f'<text x="{ml}" y="{tick_top+tick_h+13}" fill="{MUTED}" '
                 f'font-size="8" font-family={MONO!r}>'
                 f'{esc("each tick = one analog episode’s realized forward return")}</text>')

    # Note (the young-series / small-sample caveat), trimmed, on its own line
    # under the axis so it never collides with the tick caption or labels.
    note = report.get("note")
    if note:
        nt = str(note)
        if len(nt) > 116:
            nt = nt[:113] + "..."
        s.append(f'<text x="{W-mr}" y="{H-4}" text-anchor="end" '
                 f'fill="{AMBER}" font-size="7.5" font-family={MONO!r}>{esc(nt)}</text>')

    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def render(viz_type, asset, pftui=None):
    """Render one analog viz ('dist') for an asset. '' on any failure."""
    try:
        if viz_type != "dist":
            return ""
        data = pftui_json(["analytics", "analog", "--asset", asset], pftui)
        if not data:
            return ""
        report = data.get("report") if isinstance(data, dict) else None
        if not report:
            return ""
        tgt = report.get("target_asset") or asset
        nice = _nice(tgt)
        return forward_dist(report, f"{nice} — Analog Forward-Return Distribution")
    except Exception:  # never let a chart break a report
        return ""


# Token contract for viz/render.py: <!--ANALOG_VIZ:type:asset-->
TOKEN_RE = re.compile(r"<!--\s*ANALOG_VIZ:([a-z]+):([^\s>]+?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="analog-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(description="Render a pftui analog visualization as inline SVG.")
    p.add_argument("viz", choices=["dist"])
    p.add_argument("--asset", required=True)
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    svg = render(args.viz, args.asset, args.pftui)
    if not svg:
        sys.stderr.write(f"no analog viz available for {args.viz}:{args.asset}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
