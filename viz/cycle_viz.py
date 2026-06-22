#!/usr/bin/env python3
"""Cycle-data visualizations (inline SVG) for the pftui report PDFs.

Three flagship charts, each rendered from the hardened `pftui analytics cycles`
`--json` contract (see viz/theme.py for the Rust data boundary):

  map     — the "where are we in the cycle" timeline: past lows, live-cycle
            progress, current top, NOW marker, and the shaded P15-P85 next-low
            window. The flagship.
  dial    — semicircular gauge: % through cycle + accumulation stance, with the
            final ~15% accumulation zone highlighted (BTC + gold).
  ledger  — translation-ledger strip: each completed cycle as a bar with a
            midpoint tick; RT (right-translated) = bull signature.

CLI:   python cycle_viz.py map --asset BTC
Token: <!--CYCLE_VIZ:map:BTC--> (expanded by viz/render.py in the report pipeline)
"""
import math
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BG, BLUE, BORDER, CAPTION_SIZE, CYAN, GREEN, MONO, MUTED, PANEL, RED,
    SANS, TEXT, caption, d2o, esc, pftui_json, svg_open, title, title_centered,
)
from datetime import date  # noqa: E402

# Headline degree per asset (the longest established degree we surface).
HEADLINE_DEGREE = {
    "BTC": "4-year", "BTC-USD": "4-year",
    "GC=F": "major", "GOLD": "major", "SI=F": "major", "SILVER": "major",
}


def _degree(report, asset):
    if not report or not report.get("degrees"):
        return None
    want = HEADLINE_DEGREE.get(asset.upper())
    if want:
        for d in report["degrees"]:
            if d.get("degree") == want:
                return d
    return report["degrees"][0]  # CycleReport lists degrees longest-first


def _clock_for(asset, pftui=None):
    which = "gold" if asset.upper() in ("GC=F", "GOLD", "SI=F", "SILVER") else "btc"
    clk = pftui_json(["analytics", "cycles", "clock", "--asset", asset], pftui)
    return (which, clk.get(which)) if clk else (None, None)


# ---------------------------------------------------------------- VIZ 1: MAP
def cycle_map(report, degree_name, ttl, asset_clock=None, avwap=None, max_lows=2):
    deg = next((x for x in report["degrees"] if x["degree"] == degree_name), None)
    if deg is None or not deg.get("lows") or not deg.get("next_low_window"):
        return ""
    # Wider right margin + a taller top lane so the NOW marker, window title,
    # and window date labels each get their own space at the crowded right edge.
    W, H, ml, mr, mt, mb = 720, 228, 14, 64, 58, 40
    plot_w, axis_y = W - ml - mr, H - mb
    all_lows, nlw = deg["lows"], deg["next_low_window"]
    # HEADLINE COMPRESSION: anchoring the time axis at the oldest low jams the
    # live cycle (last low -> now -> next-low window) into the right ~25% and
    # wastes the left ~60% on dead history. For the headline view keep only the
    # most recent `max_lows` lows (the current cycle's start + the prior one for
    # context) so the live window gets most of the width. Dropped earlier lows
    # are surfaced honestly as a "+N earlier" tag at the left edge.
    dropped = max(0, len(all_lows) - max_lows) if max_lows else 0
    lows = all_lows[-max_lows:] if max_lows and len(all_lows) > max_lows else all_lows
    last_low = deg.get("last_confirmed_low") or lows[-1]
    cur_top = deg.get("current_top")
    t0, t1 = d2o(lows[0]["date"]), d2o(nlw["end_date"])
    span = (t1 - t0) or 1
    pad = int(span * 0.03)
    t0 -= pad; t1 += pad; span = t1 - t0
    X = lambda ds: ml + (d2o(ds) - t0) / span * plot_w
    today = date.today().toordinal()

    s = [svg_open(W, H), title(ml, 22, ttl)]
    clar = deg.get("clarity", "").upper()
    cc = {"GREEN": GREEN, "AMBER": AMBER, "RED": RED}.get(clar, MUTED)
    s.append(f'<text x="{W-mr}" y="22" text-anchor="end" fill="{cc}" font-size="9" font-family={MONO!r}>clarity {esc(clar)}</text>')
    s.append(f'<line x1="{ml}" y1="{axis_y}" x2="{W-mr}" y2="{axis_y}" stroke="{BORDER}" stroke-width="1"/>')
    bx0, bx1 = X(nlw["start_date"]), X(nlw["end_date"])
    s.append(f'<rect x="{bx0:.1f}" y="{mt}" width="{(bx1-bx0):.1f}" height="{axis_y-mt}" fill="{GREEN}" fill-opacity="0.10"/>')
    for bx in (bx0, bx1):
        s.append(f'<line x1="{bx:.1f}" y1="{mt}" x2="{bx:.1f}" y2="{axis_y}" stroke="{GREEN}" stroke-width="1" stroke-dasharray="3 2"/>')
    # Window title in the UPPER lane (above the NOW label's lane). Clamp its
    # centre so it can't run off the (now wider) right margin.
    wtx = min(max((bx0 + bx1) / 2, ml + 80), W - mr)
    s.append(f'<text x="{wtx:.1f}" y="{mt-22}" text-anchor="middle" fill="{GREEN}" font-size="9" font-family={MONO!r}>NEXT-LOW WINDOW P15-P85</text>')
    # Window date labels: both on ONE baseline just under the axis; start-date
    # left-anchored at its edge, end-date right-anchored at its edge so they
    # fan outward and never stack/clip.
    s.append(f'<text x="{bx0:.1f}" y="{axis_y+26}" text-anchor="middle" fill="{GREEN}" font-size="8" font-family={MONO!r}>{esc(nlw["start_date"])}</text>')
    s.append(f'<text x="{min(bx1, W-mr+24):.1f}" y="{axis_y+26}" text-anchor="middle" fill="{GREEN}" font-size="8" font-family={MONO!r}>{esc(nlw["end_date"])}</text>')
    for lo in lows:
        x = X(lo["date"])
        s.append(f'<circle cx="{x:.1f}" cy="{axis_y}" r="4.5" fill="{BLUE}" stroke="{BG}" stroke-width="1"/>')
        s.append(f'<text x="{x:.1f}" y="{axis_y+15}" text-anchor="middle" fill="{MUTED}" font-size="7.5" font-family={MONO!r}>{esc(lo["date"][:7])}</text>')
    # Single shared "confirmed lows" key above the first dot (balanced: one key,
    # not one tag on only the first of several identical dots). When the leftmost
    # low hugs the margin and the "(+N earlier)" suffix would clip, left-anchor it
    # at the margin instead of centering.
    lows_key = "lows" + (f" (+{dropped} earlier)" if dropped else "")
    kx = X(lows[0]["date"])
    half_w = len(lows_key) * 2.6   # ~mono char half-width at 8px
    if kx - half_w < ml:
        s.append(f'<text x="{ml}" y="{axis_y-8}" fill="{BLUE}" font-size="8" font-family={MONO!r}>{esc(lows_key)}</text>')
    else:
        s.append(f'<text x="{kx:.1f}" y="{axis_y-8}" text-anchor="middle" fill="{BLUE}" font-size="8" font-family={MONO!r}>{esc(lows_key)}</text>')
    llx = X(last_low["date"])
    tx = ml + (today - t0) / span * plot_w
    cy = mt + 30
    s.append(f'<rect x="{llx:.1f}" y="{cy}" width="{(bx1-llx):.1f}" height="10" rx="5" fill="{BORDER}" fill-opacity="0.5"/>')
    s.append(f'<rect x="{llx:.1f}" y="{cy}" width="{max(0,(tx-llx)):.1f}" height="10" rx="5" fill="{CYAN}" fill-opacity="0.55"/>')
    if cur_top:
        ctx = X(cur_top["date"])
        s.append(f'<line x1="{ctx:.1f}" y1="{cy-8}" x2="{ctx:.1f}" y2="{cy+18}" stroke="{AMBER}" stroke-width="1.2"/>')
        tp = cur_top.get("provisional_translation_pct")
        lab = "top RT %d%%" % round(tp * 100) if tp is not None else "top"
        # If the top sits near the NOW marker, anchor the label to the LEFT of
        # the amber line so it can't overprint NOW.
        near_now = abs(ctx - tx) < 60
        anc, lx = ("end", ctx - 5) if near_now else ("middle", ctx)
        s.append(f'<text x="{lx:.1f}" y="{cy-12}" text-anchor="{anc}" fill="{AMBER}" font-size="8" font-family={MONO!r}>{esc(lab)}</text>')
    s.append(f'<line x1="{tx:.1f}" y1="{mt}" x2="{tx:.1f}" y2="{axis_y}" stroke="{RED}" stroke-width="1.4"/>')
    s.append(f'<polygon points="{tx-4:.1f},{mt} {tx+4:.1f},{mt} {tx:.1f},{mt+6}" fill="{RED}"/>')
    # NOW label in its OWN lane just below the window title.
    s.append(f'<text x="{tx:.1f}" y="{mt-7}" text-anchor="middle" fill="{RED}" font-size="8" font-weight="600" font-family={MONO!r}>NOW</text>')
    bits = [f'age {deg.get("cycle_age_bars")}/{deg.get("expected_len_bars")} bars  band:{(deg.get("band_position") or "").replace("_","-")}']
    if avwap and avwap.get("pct_vs_avwap") is not None:
        bits.append(f'AVWAP {avwap["pct_vs_avwap"]}% ({"above" if avwap.get("above") else "below"})')
    if asset_clock and asset_clock.get("accumulation"):
        bits.append(f'stance:{asset_clock["accumulation"]["stance"].upper()}')
    s.append(caption(ml, H-8, "   |   ".join(bits)))
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------------- VIZ 2: DIAL
def cycle_dial(label, pct, stance, sub, accent):
    W, H, cx, cy, r = 220, 160, 110, 120, 78
    pct = max(0.0, min(100.0, float(pct)))
    a = math.pi * (1 - pct / 100.0)
    px, py = cx + r * math.cos(a), cy - r * math.sin(a)

    def arc(p0, p1, color, wdt):
        a0, a1 = math.pi * (1 - p0 / 100.0), math.pi * (1 - p1 / 100.0)
        x0, y0 = cx + r * math.cos(a0), cy - r * math.sin(a0)
        x1, y1 = cx + r * math.cos(a1), cy - r * math.sin(a1)
        large = 1 if abs(a0 - a1) > math.pi else 0
        return (f'<path d="M {x0:.1f} {y0:.1f} A {r} {r} 0 {large} 1 {x1:.1f} {y1:.1f}" '
                f'fill="none" stroke="{color}" stroke-width="{wdt}" stroke-linecap="round"/>')

    s = [svg_open(W, H)]
    s.append(title_centered(cx, 22, label))
    s.append(arc(0, 100, BORDER, 9))
    s.append(arc(85, 100, GREEN, 9))
    s.append(arc(0, pct, accent, 9))
    # Thin tick at the 85% accumulation-zone edge so the zone boundary (and where
    # the pointer sits relative to it) is legible.
    ea = math.pi * (1 - 85 / 100.0)
    ex0, ey0 = cx + (r - 8) * math.cos(ea), cy - (r - 8) * math.sin(ea)
    ex1, ey1 = cx + (r + 8) * math.cos(ea), cy - (r + 8) * math.sin(ea)
    s.append(f'<line x1="{ex0:.1f}" y1="{ey0:.1f}" x2="{ex1:.1f}" y2="{ey1:.1f}" '
             f'stroke="{GREEN}" stroke-width="1.4" stroke-opacity="0.8"/>')
    s.append(f'<circle cx="{px:.1f}" cy="{py:.1f}" r="6" fill="{accent}" stroke="{BG}" stroke-width="2"/>')
    s.append(f'<text x="{cx}" y="{cy-14}" text-anchor="middle" fill="{TEXT}" font-size="30" font-weight="700" font-family={MONO!r}>{pct:.0f}%</text>')
    s.append(f'<text x="{cx}" y="{cy+4}" text-anchor="middle" fill="{MUTED}" font-size="8" font-family={MONO!r}>through cycle</text>')
    # Stance color via the shared semantic rule: accumulate=green, distribute=red,
    # everything else (mid-cycle / neutral) amber.
    st = str(stance).lower()
    sc = GREEN if st == "accumulate" else (RED if st == "distribute" else AMBER)
    s.append(f'<text x="{cx}" y="{H-24}" text-anchor="middle" fill="{sc}" font-size="11" font-weight="600" font-family={MONO!r}>{esc(str(stance).upper())}</text>')
    # Sub-line raised ~6px and bumped to 8.5px so it isn't clipped at page height.
    s.append(caption(cx, H-9, sub, anchor="middle"))
    return "\n".join(s) + "\n</svg>"


# ------------------------------------------------------------- VIZ 3: LEDGER
def translation_strip(report, degree_name, ttl):
    deg = next((x for x in report["degrees"] if x["degree"] == degree_name), None)
    if deg is None or not deg.get("ledger") or not deg.get("band"):
        return ""
    band, cur = deg["band"], deg.get("current_top")
    rows = [dict(r, _live=False) for r in deg["ledger"]]
    if cur and cur.get("provisional_translation_pct") is not None:
        tp = cur["provisional_translation_pct"]
        rows.append({"class": "RT" if tp >= 0.55 else ("LT" if tp < 0.45 else "MID"),
                     "translation_pct": tp, "len_bars": deg.get("cycle_age_bars"), "_live": True,
                     "start_date": (deg.get("last_confirmed_low") or {}).get("date", "????-??-??"),
                     "end_date": "(now)"})
    if not rows:
        return ""
    n, rowh, W, ml, barx = len(rows), 34, 720, 14, 150
    H, barw = 56 + n * rowh, W - 150 - 90
    s = [svg_open(W, H), title(ml, 22, ttl)]
    # Caption slot (entity-bearing arrow, so emitted directly with the shared
    # CAPTION_SIZE/MUTED tokens rather than through caption()'s escaper).
    s.append(f'<text x="{W-14}" y="22" text-anchor="end" fill="{MUTED}" '
             f'font-size="{CAPTION_SIZE}" font-family={MONO!r}>'
             f'bar = translation; tick = cycle midpoint (RT&#8594;bull)</text>')
    lo_b, hi_b = band["band_lo_bars"], band["band_hi_bars"]
    maxlen = max(hi_b * 1.1, max((r["len_bars"] or 0) for r in rows))
    y = 44
    gx0, gx1 = barx + lo_b / maxlen * barw, barx + hi_b / maxlen * barw
    s.append(f'<rect x="{gx0:.1f}" y="{y-4}" width="{(gx1-gx0):.1f}" height="{n*rowh-6}" fill="{GREEN}" fill-opacity="0.07"/>')
    for gx in (gx0, gx1):
        s.append(f'<line x1="{gx:.1f}" y1="{y-4}" x2="{gx:.1f}" y2="{y+n*rowh-10}" stroke="{GREEN}" stroke-opacity="0.4" stroke-dasharray="2 2"/>')
    for r in rows:
        cls, tp, ln, live = r["class"], r.get("translation_pct"), r["len_bars"] or 0, r.get("_live")
        col = {"RT": GREEN, "LT": RED, "MID": AMBER}.get(cls, MUTED)
        lab = f'{esc(r["start_date"][:7])}&#8594;{esc(str(r["end_date"])[:7])}'
        s.append(f'<text x="{ml}" y="{y+15}" fill="{AMBER if live else TEXT}" font-size="9" font-family={MONO!r}>{lab}{" *" if live else ""}</text>')
        bw = ln / maxlen * barw
        s.append(f'<rect x="{barx}" y="{y+3}" width="{bw:.1f}" height="16" rx="3" fill="{col}" fill-opacity="{"0.45" if live else "0.85"}"'
                 + (f' stroke="{col}" stroke-dasharray="3 2"' if live else "") + "/>")
        if tp is not None:
            mxp = barx + (ln * tp) / maxlen * barw
            s.append(f'<line x1="{mxp:.1f}" y1="{y}" x2="{mxp:.1f}" y2="{y+22}" stroke="{BG}" stroke-width="2"/>')
            s.append(f'<line x1="{mxp:.1f}" y1="{y}" x2="{mxp:.1f}" y2="{y+22}" stroke="{TEXT}" stroke-width="1"/>')
        s.append(f'<text x="{W-14}" y="{y+15}" text-anchor="end" fill="{col}" font-size="9" font-weight="600" font-family={MONO!r}>{cls} {f"{tp*100:.0f}%" if tp is not None else "--"}</text>')
        y += rowh
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def render(viz_type, asset, pftui=None):
    """Render one cycle viz ('map'|'dial'|'ledger') for an asset. '' on any failure."""
    try:
        if viz_type == "dial":
            which, clk = _clock_for(asset, pftui)
            if not clk:
                return ""
            if which == "btc":
                lk, acc = clk.get("loukas") or {}, clk.get("accumulation") or {}
                report = pftui_json(["analytics", "cycles", "analyze", "--asset", asset], pftui)
                deg = _degree(report, asset)
                if not deg or not deg.get("expected_len_bars"):
                    return ""
                pct = (deg.get("cycle_age_bars") or 0) / deg["expected_len_bars"] * 100
                sub = f'wk {lk.get("cycle_week","?")} of {lk.get("band_low_week","?")}-{lk.get("band_high_week","?")} band'
                return cycle_dial("BTC 4-YEAR", pct, acc.get("stance", "—"), sub, CYAN)
            pct = clk.get("cycle_position_pct")
            if pct is None:
                return ""
            return cycle_dial("GOLD ~6.9-YR", pct, "mid-cycle",
                              f'yr {clk.get("years_since_cycle_low","?")} of {clk.get("avg_cycle_years","?")}', AMBER)
        report = pftui_json(["analytics", "cycles", "analyze", "--asset", asset], pftui)
        deg = _degree(report, asset)
        if not deg:
            return ""
        dn, nice = deg["degree"], asset.upper().replace("GC=F", "GOLD")
        if viz_type == "map":
            _, clk = _clock_for(asset, pftui)
            avwap = pftui_json(["analytics", "avwap", "--asset", asset, "--anchor", "cycle-low"], pftui)
            return cycle_map(report, dn, f"{nice} — {dn} Cycle Map", clk, avwap)
        if viz_type == "ledger":
            return translation_strip(report, dn, f"{nice} — {dn} Translation Ledger (cycle health)")
        return ""
    except Exception:  # never let a chart break a report
        return ""


# Token contract for viz/render.py: <!--CYCLE_VIZ:type:asset-->
TOKEN_RE = re.compile(r"<!--\s*CYCLE_VIZ:([a-z]+):([^\s>]+?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="cycle-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(description="Render a pftui cycle visualization as inline SVG.")
    p.add_argument("viz", choices=["map", "dial", "ledger"])
    p.add_argument("--asset", required=True)
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    svg = render(args.viz, args.asset, args.pftui)
    if not svg:
        sys.stderr.write(f"no cycle viz available for {args.viz}:{args.asset}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
