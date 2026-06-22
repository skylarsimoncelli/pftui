#!/usr/bin/env python3
"""Real-rates / yield differential visualization (inline SVG) for report PDFs.

The gold/dollar driver, rendered from the hardened `pftui analytics real-rates
differentials` `--json` contract (see viz/theme.py for the Rust data boundary):

  realrates — two coupled panels from the latest snapshot:
              (left)  the US 10Y decomposition — nominal = real (TIPS) +
                      breakeven inflation, as a stacked bar so the real-rate
                      share (what actually drives gold) is legible at a glance.
              (right) the US-minus-G10 long-rate differential, per partner
                      country as diverging bars from 0, with the average marked.
                      A positive US premium pulls the dollar; the gold cross-read.

CLI:   python rates_viz.py realrates
Token: <!--RATES_VIZ:realrates:--> (expanded by viz/render.py)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BORDER, CYAN, GREEN, MONO, MUTED, RED, TEXT,
    OP_FILL_STRONG,
    caption, esc, pftui_json, svg_open, title,
)


def _usable(snap):
    """A snapshot is plottable if it has the nominal 10Y OR any G10 spread pair.
    (Some snapshots carry only a breakeven and no nominal/TIPS/pairs — skip
    those for the headline so the chart shows the freshest COMPLETE reading.)"""
    if not snap:
        return False
    pairs = [p for p in (snap.get("pairs") or []) if p.get("spread_bp") is not None]
    return snap.get("us_nominal_10y") is not None or bool(pairs)


def _latest(data):
    """The newest USABLE snapshot (snapshots are date-ascending). Falls back to
    the most recent snapshot with a nominal yield or a G10 spread pair, so a
    trailing breakeven-only snapshot doesn't blank the chart."""
    snaps = (data or {}).get("snapshots") or []
    for snap in reversed(snaps):
        if _usable(snap):
            return snap
    return None


# ------------------------------------------------------------- VIZ: REAL RATES
def real_rates(data, ttl="US Real Rates & G10 Differential"):
    """Render the two-panel real-rates chart. '' if no usable snapshot."""
    snap = _latest(data)
    if not snap:
        return ""
    nom = snap.get("us_nominal_10y")
    tips = snap.get("us_tips_10y")
    be = snap.get("us_breakeven_10y")
    pairs = [p for p in (snap.get("pairs") or []) if p.get("spread_bp") is not None]
    avg_bp = snap.get("us_minus_g10_avg_bp")
    if nom is None and not pairs:
        return ""

    W, H = 720, 232
    s = [svg_open(W, H), title(14, 24, ttl)]
    dt = snap.get("date")
    if dt:
        s.append(f'<text x="{W-16}" y="24" text-anchor="end" fill="{MUTED}" '
                 f'font-size="9" font-family={MONO!r}>{esc(str(dt))}</text>')

    # ---- LEFT PANEL: 10Y decomposition (real + breakeven = nominal) ----
    lx, lw, ltop = 14, 300, 56
    if nom is not None:
        s.append(f'<text x="{lx}" y="{ltop-8}" fill="{MUTED}" font-size="9" '
                 f'font-family={MONO!r}>US 10Y = real (TIPS) + breakeven</text>')
        # Scale so the nominal bar fills the panel width.
        yrange = max(float(nom), (float(tips or 0) + float(be or 0)), 0.1)
        bx, by, bh = lx, ltop + 6, 30
        scale = lw / yrange
        # Real (TIPS) segment.
        if tips is not None:
            wt = max(0.0, float(tips)) * scale
            tcol = GREEN if float(tips) >= 0 else RED
            s.append(f'<rect x="{bx:.1f}" y="{by}" width="{wt:.1f}" height="{bh}" '
                     f'rx="3" fill="{tcol}" fill-opacity="{OP_FILL_STRONG}"/>')
            s.append(f'<text x="{bx+wt/2:.1f}" y="{by+bh+13}" text-anchor="middle" '
                     f'fill="{tcol}" font-size="8.5" font-family={MONO!r}>real {float(tips):.2f}%</text>')
            bx = bx + wt
        # Breakeven segment.
        if be is not None:
            wb = max(0.0, float(be)) * scale
            s.append(f'<rect x="{bx:.1f}" y="{by}" width="{wb:.1f}" height="{bh}" '
                     f'rx="3" fill="{AMBER}" fill-opacity="{OP_FILL_STRONG}"/>')
            s.append(f'<text x="{bx+wb/2:.1f}" y="{by+bh+13}" text-anchor="middle" '
                     f'fill="{AMBER}" font-size="8.5" font-family={MONO!r}>infl {float(be):.2f}%</text>')
        # Nominal total readout.
        s.append(f'<text x="{lx}" y="{by-4}" fill="{TEXT}" font-size="11" '
                 f'font-weight="700" font-family={MONO!r}>nominal {float(nom):.2f}%</text>')
        # Real-rate emphasis line (the gold driver).
        if tips is not None:
            rrc = GREEN if float(tips) >= 0 else RED
            s.append(f'<text x="{lx}" y="{by+bh+34}" fill="{rrc}" font-size="9" '
                     f'font-weight="600" font-family={MONO!r}>'
                     f'real 10Y {float(tips):+.2f}%  '
                     f'{"headwind" if float(tips) >= 1.5 else "neutral" if float(tips) >= 0 else "tailwind"} for gold</text>')

    # Divider.
    s.append(f'<line x1="{330}" y1="{46}" x2="{330}" y2="{H-30}" '
             f'stroke="{BORDER}" stroke-opacity="0.5"/>')

    # ---- RIGHT PANEL: US-minus-G10 differential bars ----
    rx0, rtop = 348, 56
    rw = W - rx0 - 18
    s.append(f'<text x="{rx0}" y="{rtop-8}" fill="{MUTED}" font-size="9" '
             f'font-family={MONO!r}>US 10Y minus G10 partner (bp)</text>')
    if pairs:
        rows = sorted(pairs, key=lambda p: float(p["spread_bp"]), reverse=True)
        n = len(rows)
        rowh = 24
        labw = 30
        cx = rx0 + labw + (rw - labw) / 2.0
        half = (rw - labw) / 2.0
        smax = max(abs(float(p["spread_bp"])) for p in rows) or 1.0
        # round axis half-range up to a clean 50bp grid
        axis = max(50.0, (int(smax / 50) + 1) * 50.0)
        y = rtop + 4
        # center line
        s.append(f'<line x1="{cx:.1f}" y1="{y}" x2="{cx:.1f}" y2="{y+n*rowh-6}" '
                 f'stroke="{MUTED}" stroke-width="1"/>')
        for p in rows:
            v = float(p["spread_bp"])
            col = GREEN if v >= 0 else RED   # US premium green; US discount red
            bw = abs(v) / axis * half
            bx = cx if v >= 0 else cx - bw
            s.append(f'<text x="{rx0}" y="{y+13}" fill="{TEXT}" font-size="9" '
                     f'font-family={MONO!r}>{esc(str(p.get("country","?")))}</text>')
            s.append(f'<rect x="{bx:.1f}" y="{y+2}" width="{max(1.5,bw):.1f}" height="13" '
                     f'rx="2" fill="{col}" fill-opacity="{OP_FILL_STRONG}"/>')
            tx = (cx + bw + 4) if v >= 0 else (cx - bw - 4)
            anc = "start" if v >= 0 else "end"
            s.append(f'<text x="{tx:.1f}" y="{y+13}" text-anchor="{anc}" fill="{col}" '
                     f'font-size="8.5" font-weight="600" font-family={MONO!r}>{v:+.0f}</text>')
            y += rowh
        # average marker
        if avg_bp is not None:
            ax = cx + min(max(float(avg_bp), -axis), axis) / axis * half
            s.append(f'<line x1="{ax:.1f}" y1="{rtop}" x2="{ax:.1f}" y2="{y-4}" '
                     f'stroke="{CYAN}" stroke-width="1.2" stroke-dasharray="3 2"/>')
            s.append(f'<text x="{ax:.1f}" y="{y+8}" text-anchor="middle" fill="{CYAN}" '
                     f'font-size="8" font-family={MONO!r}>avg {float(avg_bp):+.0f}bp</text>')

    foot = "real (TIPS) 10Y is the gold cross-read; US premium pulls the dollar"
    if data.get("since"):
        foot += f"  ·  since {esc(str(data['since']))}"
    s.append(caption(14, H - 9, foot))
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def render(viz_type, _arg="", pftui=None):
    """Render the real-rates chart. '' on any failure."""
    try:
        if viz_type != "realrates":
            return ""
        data = pftui_json(["analytics", "real-rates", "differentials"], pftui)
        return real_rates(data)
    except Exception:  # never let a chart break a report
        return ""


# Token contract: <!--RATES_VIZ:type:-->
TOKEN_RE = re.compile(r"<!--\s*RATES_VIZ:([a-z]+):([^\s>]*?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="rates-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(description="Render the pftui real-rates visualization as inline SVG.")
    p.add_argument("viz", choices=["realrates"])
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    svg = render(args.viz, "", args.pftui)
    if not svg:
        sys.stderr.write(f"no rates viz available for {args.viz}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
