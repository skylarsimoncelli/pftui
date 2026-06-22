#!/usr/bin/env python3
"""Scenario probability dashboard (inline SVG) for the pftui report PDFs.

The newsletter's named "scenario dashboard" section, rendered from the hardened
`pftui analytics scenario list` `--json` contract (see viz/theme.py for the Rust
data boundary):

  dashboard — the active macro scenarios ranked by probability %, drawn as
              horizontal probability bars. The footer carries the normalized-set
              fill state (modeled-sum / overfill / residual = "Other /
              Unmodelled"), so the reader sees both the named worldviews AND how
              much probability mass is unaccounted for. Each bar carries the
              scenario's key signal (its description's lead clause) when present.
              Answers "what are the live macro stories, and which is the market
              pricing hardest?" — the #1 report chart.

CLI:   python scenario_viz.py dashboard
Token: <!--SCENARIO_VIZ:dashboard:--> (expanded by viz/render.py)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BORDER, GREEN, MONO, MUTED, SANS, TEXT,
    OP_FILL_STRONG, OP_TRACK, OP_WASH,
    caption, esc, pftui_json, ramp, svg_open, title,
)


def _lead(text, limit=72):
    """First sentence/clause of a description — the scenario's key signal."""
    if not text:
        return ""
    t = str(text).strip()
    for sep in (". ", "; "):
        i = t.find(sep)
        if 0 < i <= limit:
            return t[:i]
    return t if len(t) <= limit else t[: limit - 1].rstrip() + "…"


def _bar_color(p):
    """Probability bar color via the shared semantic `ramp`: higher conviction
    reads hotter (green->amber->red), so a 40%+ scenario is the dominant story."""
    return ramp(min(float(p), 40.0) / 40.0)


# --------------------------------------------------------- VIZ: SCENARIO DASHBOARD
def scenario_dashboard(data, ttl="Macro Scenario Dashboard"):
    """Horizontal ranked probability bars over the active scenarios + the
    normalized-set fill footer. '' when there are no active scenarios."""
    if not data:
        return ""
    scen = [s for s in (data.get("scenarios") or [])
            if str(s.get("status", "active")).lower() == "active"
            and s.get("probability") is not None]
    if not scen:
        return ""
    scen.sort(key=lambda s: float(s["probability"]), reverse=True)
    nset = data.get("normalized_set") or {}

    # Append the residual ("Other / Unmodelled") as a final cool bar so the
    # unaccounted probability mass is visible, never hidden.
    rows = [dict(s, _residual=False) for s in scen]
    if nset.get("residual_materialized") and nset.get("residual_probability"):
        rows.append({
            "name": nset.get("residual_scenario_name", "Other / Unmodelled"),
            "probability": float(nset["residual_probability"]),
            "description": "", "_residual": True,
        })

    n = len(rows)
    W, ml, mr, rowh, top = 720, 14, 16, 40, 44
    labw = 196                      # left label gutter (name + signal)
    barx = ml + labw
    barw = W - barx - mr - 52       # leave room for the % readout on the right
    H = top + n * rowh + 30
    pmax = max(float(r["probability"]) for r in rows) or 1.0
    scale = max(20.0, (int(pmax / 10) + 1) * 10.0)  # clean grid max >= largest bar

    s = [svg_open(W, H), title(ml, 24, ttl)]
    osum = nset.get("modeled_sum")
    if osum is not None:
        st = str(nset.get("overfill_state", "")).replace("_", " ")
        oc = AMBER if "over" in st else (MUTED if "under" in st else GREEN)
        s.append(f'<text x="{W-mr}" y="24" text-anchor="end" fill="{oc}" '
                 f'font-size="9" font-family={MONO!r}>modeled {osum:.0f}% '
                 f'· {esc(st)}</text>')

    # Faint gridlines + scale ticks behind the bars.
    for gv in range(0, int(scale) + 1, 10):
        gx = barx + gv / scale * barw
        s.append(f'<line x1="{gx:.1f}" y1="{top}" x2="{gx:.1f}" y2="{top+n*rowh-8}" '
                 f'stroke="{BORDER}" stroke-opacity="{OP_WASH}"/>')
        s.append(f'<text x="{gx:.1f}" y="{top+n*rowh+6}" text-anchor="middle" '
                 f'fill="{MUTED}" font-size="7" font-family={MONO!r}>{gv}%</text>')

    y = top
    for r in rows:
        p = float(r["probability"])
        resid = r.get("_residual")
        col = MUTED if resid else _bar_color(p)
        nm = esc(str(r["name"])[:30])
        s.append(f'<text x="{ml}" y="{y+14}" fill="{TEXT if not resid else MUTED}" '
                 f'font-size="10" font-weight="600" font-family={MONO!r}>{nm}</text>')
        sig = _lead(r.get("description"), 40)
        if sig:
            s.append(f'<text x="{ml}" y="{y+26}" fill="{MUTED}" font-size="7.5" '
                     f'font-family={SANS!r}>{esc(sig)}</text>')
        # Track + value bar.
        s.append(f'<rect x="{barx}" y="{y+4}" width="{barw}" height="16" rx="3" '
                 f'fill="{BORDER}" fill-opacity="{OP_TRACK}"/>')
        bw = max(2.0, p / scale * barw)
        style = (f' stroke="{MUTED}" stroke-dasharray="3 2" fill-opacity="{OP_WASH}"'
                 if resid else f' fill-opacity="{OP_FILL_STRONG}"')
        s.append(f'<rect x="{barx}" y="{y+4}" width="{bw:.1f}" height="16" rx="3" '
                 f'fill="{col}"{style}/>')
        s.append(f'<text x="{W-mr}" y="{y+16}" text-anchor="end" fill="{col}" '
                 f'font-size="11" font-weight="700" font-family={MONO!r}>{p:.0f}%</text>')
        y += rowh

    s.append(caption(ml, H - 9,
                     "bars = active-scenario probability; residual = unmodelled mass"))
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def render(viz_type, _arg="", pftui=None):
    """Render the scenario dashboard. '' on any failure."""
    try:
        if viz_type != "dashboard":
            return ""
        data = pftui_json(["analytics", "scenario", "list"], pftui)
        return scenario_dashboard(data)
    except Exception:  # never let a chart break a report
        return ""


# Token contract: <!--SCENARIO_VIZ:type:--> (arg unused/optional)
TOKEN_RE = re.compile(r"<!--\s*SCENARIO_VIZ:([a-z]+):([^\s>]*?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="scenario-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(description="Render the pftui scenario dashboard as inline SVG.")
    p.add_argument("viz", choices=["dashboard"])
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    svg = render(args.viz, "", args.pftui)
    if not svg:
        sys.stderr.write(f"no scenario viz available for {args.viz}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
