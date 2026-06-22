#!/usr/bin/env python3
"""Risk & regime visualizations (inline SVG) for the pftui report PDFs.

Flagship chart, rendered from the hardened `pftui analytics tail-dependence`
`--json` contract (see viz/theme.py for the Rust data boundary):

  cocrash — the CO-CRASH MATRIX. A triangular grid over a basket of assets.
            The UPPER triangle is everyday Pearson correlation; the LOWER
            triangle is co-crash lower-tail dependence λ_L = P(Y crashes | X
            crashes). The whole point: two assets can have modest day-to-day
            correlation yet still plunge TOGETHER in a crisis. λ_L is the number
            that exposes (or vindicates) a diversification pair — e.g. does the
            BTC <-> gold book actually hold up when it's needed? Cells are
            colored on a calm->danger ramp and annotated; the gap between a
            cell's upper (Pearson) and lower (λ_L) twin is the diversification
            tell.

CLI:   python risk_viz.py cocrash --assets BTC,gold,SPY
Token: <!--RISK_VIZ:cocrash:BTC,gold,SPY--> (expanded by viz/render.py)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BG, BLUE, BORDER, CYAN, GREEN, MONO, MUTED, PANEL, RED, TEXT,
    esc, pftui_json, svg_open, title,
)

# Pretty labels for common tickers so the matrix reads cleanly.
NICE = {
    "GC=F": "GOLD", "GOLD": "GOLD", "SI=F": "SILVER", "SILVER": "SILVER",
    "BTC-USD": "BTC", "BTC": "BTC", "ETH-USD": "ETH", "ETH": "ETH",
    "^GSPC": "SPX", "SPY": "SPY", "QQQ": "QQQ",
}


def _nice(sym):
    return NICE.get(str(sym).upper(), str(sym).upper())


def _lerp(c0, c1, t):
    """Linear-interpolate two #rrggbb colors; t in [0,1] -> #rrggbb."""
    t = max(0.0, min(1.0, t))
    a = tuple(int(c0[i:i + 2], 16) for i in (1, 3, 5))
    b = tuple(int(c1[i:i + 2], 16) for i in (1, 3, 5))
    return "#" + "".join(f"{round(a[i] + (b[i] - a[i]) * t):02x}" for i in range(3))


def _ramp(v):
    """Map a 0..1 dependence/correlation value to a calm->danger fill.

    Anchored at the report palette: GREEN (diversifying/low) -> AMBER (moderate)
    -> RED (concentrated/high). Values are clamped to [0,1]; negative
    correlations are floored at 0 for the fill but printed signed.
    """
    v = max(0.0, min(1.0, v))
    if v <= 0.5:
        return _lerp(GREEN, AMBER, v / 0.5)
    return _lerp(AMBER, RED, (v - 0.5) / 0.5)


def _fetch_pair(a, b, pftui=None):
    """Pearson + empirical lower-tail λ_L for one pair. None on any failure."""
    d = pftui_json(["analytics", "tail-dependence", "--asset", a, "--vs", b], pftui)
    if not d:
        return None
    t = d.get("tail_dependence") or {}
    pe, ll = t.get("pearson"), t.get("emp_lower_tail_dep")
    if pe is None and ll is None:
        return None
    return {"pearson": pe, "lambda_l": ll, "n": t.get("n"),
            "resolved": d.get("resolved") or [a, b]}


# ----------------------------------------------------------- VIZ: CO-CRASH MATRIX
def cocrash_matrix(assets, pairs, ttl):
    """Render the triangular co-crash matrix.

    assets: ordered list of display labels (len 2..6).
    pairs:  dict {(i,j): {'pearson':.., 'lambda_l':..}} for i<j (basket order).
    """
    n = len(assets)
    if n < 2 or not pairs:
        return ""
    cell = 78
    ml, mt = 96, 70          # left gutter for row labels, top for title+col labels
    legend_h = 58
    grid = n * cell
    # Width is the larger of the grid footprint and the header/legend text runs,
    # so the title and captions never clip past the panel edge.
    W = max(ml + grid + 20, 560)
    H = mt + grid + legend_h
    gx0, gy0 = ml, mt

    s = [svg_open(W, H), title(14, 26, ttl)]
    s.append(f'<text x="14" y="44" fill="{MUTED}" '
             f'font-size="9" font-family={MONO!r}>upper triangle = correlation &#183; lower triangle = co-crash &#955;_L</text>')

    def cx(j):
        return gx0 + j * cell

    def cy(i):
        return gy0 + i * cell

    # Column header labels (top) and row header labels (left).
    for k, lab in enumerate(assets):
        s.append(f'<text x="{cx(k)+cell/2:.1f}" y="{gy0-10}" text-anchor="middle" '
                 f'fill="{CYAN}" font-size="11" font-weight="600" font-family={MONO!r}>{esc(lab)}</text>')
        s.append(f'<text x="{gx0-12}" y="{cy(k)+cell/2+4:.1f}" text-anchor="end" '
                 f'fill="{CYAN}" font-size="11" font-weight="600" font-family={MONO!r}>{esc(lab)}</text>')

    def draw_cell(i, j, val, kind):
        if val is None:
            fill, txt, sub = PANEL, "--", ""
        else:
            fill = _ramp(val)
            txt = f"{val:+.2f}" if kind == "corr" else f"{val:.2f}"
            sub = "corr" if kind == "corr" else "&#955;_L"
        x, y = cx(j), cy(i)
        out = [f'<rect x="{x+2:.1f}" y="{y+2:.1f}" width="{cell-4}" height="{cell-4}" '
               f'rx="6" fill="{fill}" fill-opacity="0.88" stroke="{BORDER}"/>']
        # dark legible text on the colored chip
        out.append(f'<text x="{x+cell/2:.1f}" y="{y+cell/2-1:.1f}" text-anchor="middle" '
                   f'fill="{BG}" font-size="17" font-weight="700" font-family={MONO!r}>{esc(txt)}</text>')
        if sub:
            out.append(f'<text x="{x+cell/2:.1f}" y="{y+cell/2+16:.1f}" text-anchor="middle" '
                       f'fill="{BG}" fill-opacity="0.8" font-size="8" font-family={MONO!r}>{sub}</text>')
        return "".join(out)

    for i in range(n):
        for j in range(n):
            if i == j:
                x, y = cx(j), cy(i)
                s.append(f'<rect x="{x+2:.1f}" y="{y+2:.1f}" width="{cell-4}" height="{cell-4}" '
                         f'rx="6" fill="{BG}" stroke="{BORDER}"/>')
                s.append(f'<text x="{x+cell/2:.1f}" y="{y+cell/2+4:.1f}" text-anchor="middle" '
                         f'fill="{MUTED}" font-size="9" font-family={MONO!r}>&#8212;</text>')
                continue
            a, b = (i, j) if i < j else (j, i)
            p = pairs.get((a, b))
            if i < j:   # upper triangle -> Pearson correlation
                s.append(draw_cell(i, j, (p or {}).get("pearson"), "corr"))
            else:       # lower triangle -> co-crash lower-tail dependence
                s.append(draw_cell(i, j, (p or {}).get("lambda_l"), "lambda"))

    # Legend: color ramp + the diversification read.
    ly = gy0 + grid + 22
    lx = 16
    s.append(f'<text x="{lx}" y="{ly}" fill="{MUTED}" font-size="9" font-family={MONO!r}>diversifies in a crash</text>')
    bar_x, bar_w = lx + 168, 150
    steps = 24
    for k in range(steps):
        t = k / (steps - 1)
        s.append(f'<rect x="{bar_x + k*(bar_w/steps):.1f}" y="{ly-9}" '
                 f'width="{bar_w/steps + 0.6:.1f}" height="10" fill="{_ramp(t)}"/>')
    s.append(f'<rect x="{bar_x}" y="{ly-9}" width="{bar_w}" height="10" fill="none" stroke="{BORDER}"/>')
    s.append(f'<text x="{bar_x+bar_w+8}" y="{ly}" fill="{MUTED}" font-size="9" font-family={MONO!r}>co-crashes</text>')
    s.append(f'<text x="{lx}" y="{ly+20}" fill="{MUTED}" font-size="8" font-family={MONO!r}>'
             f'low &#955;_L (lower triangle) = the pair holds up when it&#8217;s needed; high &#955;_L = joint downside.</text>')
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def render(viz_type, arg, pftui=None):
    """Render one risk viz. Currently: 'cocrash' over a comma-separated basket.

    Returns '' on any failure so a report degrades gracefully.
    """
    try:
        if viz_type != "cocrash":
            return ""
        raw = [a.strip() for a in str(arg).split(",") if a.strip()]
        if len(raw) < 2:
            return ""
        raw = raw[:6]  # keep the grid readable
        pairs, labels = {}, [None] * len(raw)
        any_data = False
        for i in range(len(raw)):
            for j in range(i + 1, len(raw)):
                pr = _fetch_pair(raw[i], raw[j], pftui)
                if pr:
                    pairs[(i, j)] = pr
                    any_data = True
                    res = pr.get("resolved") or []
                    if len(res) == 2:
                        labels[i] = labels[i] or _nice(res[0])
                        labels[j] = labels[j] or _nice(res[1])
        if not any_data:
            return ""
        labels = [labels[k] or _nice(raw[k]) for k in range(len(raw))]
        return cocrash_matrix(labels, pairs, "Co-Crash Matrix")
    except Exception:  # never let a chart break a report
        return ""


# Token contract for viz/render.py: <!--RISK_VIZ:type:arg-->
TOKEN_RE = re.compile(r"<!--\s*RISK_VIZ:([a-z]+):([^\s>]*?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="risk-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(description="Render a pftui risk/regime visualization as inline SVG.")
    p.add_argument("viz", choices=["cocrash"])
    p.add_argument("--assets", required=True, help="comma-separated basket, e.g. BTC,gold,SPY")
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    svg = render(args.viz, args.assets, args.pftui)
    if not svg:
        sys.stderr.write(f"no risk viz available for {args.viz}:{args.assets}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
