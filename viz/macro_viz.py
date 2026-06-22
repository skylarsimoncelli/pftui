#!/usr/bin/env python3
"""Macro environment + catalyst visualizations (inline SVG) for the report PDFs.

Two charts, each from a hardened `pftui analytics …` `--json` contract (see
viz/theme.py for the Rust data boundary):

  environment — the z-scored macro feature vector (DXY/gold/oil/VIX/yields/curve
                returns + vols + levels) from `analytics environment current`,
                drawn as diverging horizontal bars from a center 0 line. Each bar
                shows where today sits vs its own expanding-window history in
                standard deviations — instantly reads "what's stretched". The
                "what's the regime" companion to the scenario dashboard.

  catalysts   — the upcoming ranked events from `analytics catalysts` on a date
                axis, sized by event-pressure (the engine's `score`) and tinted
                by significance. The newsletter "what to watch": which dated
                event carries the most pressure, and when.

CLI:   python macro_viz.py environment   |   python macro_viz.py catalysts
Token: <!--MACRO_VIZ:environment:-->     |   <!--MACRO_VIZ:catalysts:-->
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BG, BLUE, BORDER, CYAN, GREEN, MONO, MUTED, RED, SANS, TEXT,
    OP_FILL_STRONG, OP_TRACK, OP_WASH,
    caption, d2o, esc, pftui_json, svg_open, title,
)
from datetime import date  # noqa: E402

# Human labels + display order for the z-scored macro features. Keys not present
# in the payload are skipped; payload keys not listed here are appended verbatim,
# so the chart survives the Rust feature set growing.
FEATURE_LABELS = [
    ("dxy_ret20", "DXY 20d return"),
    ("dxy_vol20", "DXY 20d vol"),
    ("tnx_level", "10Y yield level"),
    ("tnx_chg20", "10Y yield 20d chg"),
    ("curve_10y_3m", "Curve 10y-3m"),
    ("gold_ret20", "Gold 20d return"),
    ("gold_vol20", "Gold 20d vol"),
    ("oil_ret20", "Oil 20d return"),
    ("oil_vol20", "Oil 20d vol"),
    ("spx_ret20", "S&P 20d return"),
    ("spx_vol20", "S&P 20d vol"),
    ("vix_level", "VIX level"),
]


def _z_color(z):
    """Diverging color: |z| small = calm (muted), stretched up = amber/red,
    stretched down = blue/cyan. Magnitude, not sign, drives intensity."""
    a = min(abs(float(z)) / 2.5, 1.0)
    if z >= 0:
        return AMBER if a < 0.7 else RED
    return CYAN if a < 0.7 else BLUE


# ------------------------------------------------------- VIZ: ENVIRONMENT Z-STRIP
def environment_strip(data, ttl="Macro Environment — z-scores vs history"):
    """Diverging horizontal z-score bars from a center 0. '' if no features."""
    if not data:
        return ""
    feats = data.get("features_zscored") or {}
    if not feats:
        return ""
    ordered = [(k, lbl) for (k, lbl) in FEATURE_LABELS if k in feats]
    seen = {k for k, _ in ordered}
    ordered += [(k, k.replace("_", " ")) for k in feats if k not in seen]

    n = len(ordered)
    W, ml, mr, rowh, top = 720, 14, 16, 26, 44
    labw = 132
    plot_x0 = ml + labw
    plot_w = W - plot_x0 - mr - 40   # room for the value readout on the right
    cx = plot_x0 + plot_w / 2.0      # the center (z=0) line
    H = top + n * rowh + 28
    ZMAX = 3.0                       # axis half-range in std-devs (clamped)

    s = [svg_open(W, H), title(ml, 24, ttl)]
    asof = data.get("as_of")
    if asof:
        s.append(f'<text x="{W-mr}" y="24" text-anchor="end" fill="{MUTED}" '
                 f'font-size="9" font-family={MONO!r}>as of {esc(asof)}</text>')

    # Center line + faint +/-1,2,3 sigma gridlines.
    plot_bottom = top + n * rowh - 4
    for sig in (-3, -2, -1, 1, 2, 3):
        gx = cx + (sig / ZMAX) * (plot_w / 2.0)
        s.append(f'<line x1="{gx:.1f}" y1="{top}" x2="{gx:.1f}" y2="{plot_bottom}" '
                 f'stroke="{BORDER}" stroke-opacity="{OP_WASH}"/>')
        s.append(f'<text x="{gx:.1f}" y="{plot_bottom+12}" text-anchor="middle" '
                 f'fill="{MUTED}" font-size="7" font-family={MONO!r}>{sig:+d}σ</text>')
    s.append(f'<line x1="{cx:.1f}" y1="{top}" x2="{cx:.1f}" y2="{plot_bottom}" '
             f'stroke="{MUTED}" stroke-width="1"/>')

    y = top
    for k, lbl in ordered:
        z = float(feats[k])
        zc = min(max(z, -ZMAX), ZMAX)
        col = _z_color(z)
        s.append(f'<text x="{ml}" y="{y+15}" fill="{TEXT}" font-size="9" '
                 f'font-family={MONO!r}>{esc(lbl)}</text>')
        half = plot_w / 2.0
        bw = abs(zc) / ZMAX * half
        bx = cx if z >= 0 else cx - bw
        s.append(f'<rect x="{bx:.1f}" y="{y+4}" width="{max(1.5,bw):.1f}" height="14" '
                 f'rx="2" fill="{col}" fill-opacity="{OP_FILL_STRONG}"/>')
        s.append(f'<text x="{W-mr}" y="{y+15}" text-anchor="end" fill="{col}" '
                 f'font-size="9.5" font-weight="600" font-family={MONO!r}>{z:+.2f}</text>')
        y += rowh

    foot = "expanding-window z (no look-ahead); +/- = std-devs from norm"
    hd = data.get("history_days")
    if hd:
        foot += f"  ·  {hd}d history"
    s.append(caption(ml, H - 9, foot))
    return "\n".join(s) + "\n</svg>"


# ------------------------------------------------------------ VIZ: CATALYST TIMELINE
SIG_COLOR = {"high": RED, "medium": AMBER, "low": BLUE}


MAX_CATALYSTS = 18         # safety cap; lowest-pressure events past this are summarized
MAX_PER_DATE = 5           # rows drawn per date column before a "+N more" overflow tag


def catalyst_timeline(data, ttl="Catalyst Timeline — what to watch"):
    """Upcoming events on a date axis, grouped into per-DATE columns so same-day
    events stack vertically (sorted by event-pressure `score`) instead of
    overprinting. Pill width/score = pressure, color = significance. '' if no
    catalysts. The newsletter "what to watch"."""
    if not data:
        return ""
    cats = [c for c in (data.get("catalysts") or []) if c.get("time")]
    if not cats:
        return ""
    # Parse dates; drop anything unparseable (additive, never crash).
    rows = []
    for c in cats:
        try:
            o = d2o(str(c["time"])[:10])
        except (ValueError, TypeError):
            continue
        rows.append(dict(c, _ord=o, _score=float(c.get("score") or 0)))
    if not rows:
        return ""
    # Keep the highest-pressure events if there are a lot of them.
    overflow_total = max(0, len(rows) - MAX_CATALYSTS)
    rows.sort(key=lambda c: -c["_score"])
    rows = rows[:MAX_CATALYSTS]

    # Group by date (column), each column's events sorted by score desc.
    from collections import OrderedDict
    cols = OrderedDict()
    for c in sorted(rows, key=lambda c: (c["_ord"], -c["_score"])):
        cols.setdefault(c["_ord"], []).append(c)

    today = date.today().toordinal()
    smax = max(c["_score"] for c in rows) or 1.0
    max_stack = max(len(v) for v in cols.values())

    W, ml, mr, top = 720, 16, 16, 52
    axis_y = top + 6
    plot_w = W - ml - mr
    rowh = 19
    H = axis_y + max(max_stack, 1) * rowh + 46

    # Evenly space the date COLUMNS (categorical x), so clustered dates don't
    # collapse onto each other — calendar events are discrete, not continuous.
    date_list = list(cols)
    ncol = len(date_list)
    col_w = plot_w / ncol
    cx_of = {d: ml + col_w * (i + 0.5) for i, d in enumerate(date_list)}

    s = [svg_open(W, H), title(ml, 24, ttl)]
    lbl = data.get("label") or data.get("window")
    if lbl:
        s.append(f'<text x="{W-mr}" y="24" text-anchor="end" fill="{MUTED}" '
                 f'font-size="9" font-family={MONO!r}>{esc(str(lbl))}</text>')

    # Date axis (top): a header row with each date label + a NOW pointer.
    s.append(f'<line x1="{ml}" y1="{axis_y}" x2="{W-mr}" y2="{axis_y}" '
             f'stroke="{BORDER}" stroke-width="1"/>')
    today_str = date.fromordinal(today).isoformat()
    # Always anchor "now": if today isn't itself an event column, label it at the
    # axis origin so the reader always sees the reference point.
    if today not in cols:
        s.append(f'<text x="{ml}" y="{axis_y-8}" fill="{GREEN}" font-size="8" '
                 f'font-weight="600" font-family={MONO!r}>NOW {esc(today_str[5:])}</text>')
    for d, cx in cx_of.items():
        ds = date.fromordinal(d).isoformat()
        is_today = d == today
        dc = GREEN if is_today else MUTED
        s.append(f'<line x1="{cx:.1f}" y1="{axis_y-4}" x2="{cx:.1f}" y2="{axis_y+4}" '
                 f'stroke="{dc}" stroke-width="1.2"/>')
        s.append(f'<text x="{cx:.1f}" y="{axis_y-8}" text-anchor="middle" fill="{dc}" '
                 f'font-size="8" font-weight="{"600" if is_today else "400"}" '
                 f'font-family={MONO!r}>{esc(ds[5:])}{" ·NOW" if is_today else ""}</text>')
        # Faint column rule.
        s.append(f'<line x1="{cx:.1f}" y1="{axis_y+4}" x2="{cx:.1f}" y2="{H-30}" '
                 f'stroke="{BORDER}" stroke-opacity="{OP_WASH}"/>')

    # Per-date stacked event pills.
    pill_w = min(col_w - 8, 150)
    for d, evs in cols.items():
        cx = cx_of[d]
        px0 = cx - pill_w / 2
        y = axis_y + 12
        shown = evs[:MAX_PER_DATE]
        for c in shown:
            sig = str(c.get("significance", "")).lower()
            col = SIG_COLOR.get(sig, MUTED)
            frac = 0.35 + 0.65 * (c["_score"] / smax)   # pressure -> fill length
            bw = max(8.0, pill_w * frac)
            s.append(f'<rect x="{px0:.1f}" y="{y:.1f}" width="{pill_w:.1f}" height="15" '
                     f'rx="3" fill="{BORDER}" fill-opacity="{OP_TRACK}"/>')
            s.append(f'<rect x="{px0:.1f}" y="{y:.1f}" width="{bw:.1f}" height="15" '
                     f'rx="3" fill="{col}" fill-opacity="{OP_FILL_STRONG}"/>')
            nm = esc(str(c.get("title", ""))[:20])
            s.append(f'<text x="{px0+4:.1f}" y="{y+11:.1f}" fill="{BG}" font-size="8" '
                     f'font-weight="600" font-family={SANS!r}>{nm}</text>')
            y += rowh
        extra = len(evs) - len(shown)
        if extra > 0:
            s.append(f'<text x="{cx:.1f}" y="{y+9:.1f}" text-anchor="middle" '
                     f'fill="{MUTED}" font-size="7.5" font-family={MONO!r}>+{extra} more</text>')

    foot = "grouped by date; pill length & color = event-pressure / significance"
    if overflow_total:
        foot += f"  ·  +{overflow_total} lower-pressure events not shown"
    s.append(caption(ml, H - 9, foot))
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def render(viz_type, _arg="", pftui=None):
    """Render a macro viz ('environment'|'catalysts'). '' on any failure."""
    try:
        if viz_type == "environment":
            data = pftui_json(["analytics", "environment", "current"], pftui)
            return environment_strip(data)
        if viz_type == "catalysts":
            data = pftui_json(["analytics", "catalysts"], pftui)
            return catalyst_timeline(data)
        return ""
    except Exception:  # never let a chart break a report
        return ""


# Token contract: <!--MACRO_VIZ:type:-->
TOKEN_RE = re.compile(r"<!--\s*MACRO_VIZ:([a-z]+):([^\s>]*?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="macro-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(description="Render a pftui macro visualization as inline SVG.")
    p.add_argument("viz", choices=["environment", "catalysts"])
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    svg = render(args.viz, "", args.pftui)
    if not svg:
        sys.stderr.write(f"no macro viz available for {args.viz}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
