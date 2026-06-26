#!/usr/bin/env python3
"""Cycle-bottom signal checklist (inline SVG) for the pftui report PDFs.

Renders the mechanical N-of-7 cycle-bottom confluence from the hardened
`pftui analytics cycles bottom-signals --json` contract (see viz/theme.py for
the Rust data boundary, docs/CYCLE-SIGNALS.md for the doctrine):

  checklist — the 7 composite criteria as a ✓/✗ list (plain, name-free labels +
              their one-line detail), under an N/7 confluence gauge whose fill
              ramps red->green with how many criteria are firing. The optional
              pi-cycle bonus is shown beneath the seven as a non-counted flag.
              Answers "is a cycle low actually being put in on the tape right
              now?" — the mechanical half of the cycle confirm checklist.

Public-safe by construction: the labels carry no practitioner/indicator names
(they come straight from the name-free Rust `label` fields), and the chart shows
NO portfolio holdings — only a market-data read.

  tracked   — a heat-strip table of EVERY armed cycle-signal alert from
              `analytics cycles tracked --json` (both polarities): asset, label,
              timeframe, live met/total confluence bar (colored by closeness to
              firing), distance-to-target, fired?, and time-since-last. The
              status view, not a study. Privacy-safe (metadata + counts only).

CLI:   python cycle_signals_viz.py checklist --asset BTC [--timeframe monthly]
       python cycle_signals_viz.py tracked [--asset BTC] [--polarity top]
Token: <!--CYCLE_SIGNALS_VIZ:checklist:BTC--> (expanded by viz/render.py)
       payload = ASSET[?timeframe=daily|weekly|monthly]  (timeframe optional)
       <!--CYCLE_SIGNALS_VIZ:tracked:all-->
       payload = all | ASSET[?polarity=bottom|top]  (filter optional)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BORDER, GREEN, MONO, MUTED, RED, SANS, TEXT,
    OP_FILL_STRONG, OP_TRACK, OP_WASH,
    caption, esc, pftui_json, ramp, svg_open, title,
)

# Friendly, ticker-free asset names (shared convention with the other viz
# modules). Anything else is shown verbatim, uppercased.
NICE = {
    "GC=F": "Gold", "GOLD": "Gold", "SI=F": "Silver", "SILVER": "Silver",
    "BTC-USD": "Bitcoin", "BTC": "Bitcoin", "ETH-USD": "Ethereum", "ETH": "Ethereum",
    "^GSPC": "S&P 500", "SPY": "S&P 500", "QQQ": "Nasdaq 100",
}


def _nice(sym):
    if not sym:
        return ""
    return NICE.get(str(sym).upper(), str(sym).upper())


# --------------------------------------------------------- VIZ: BOTTOM CHECKLIST
def cycle_signals_checklist(data):
    """7-criterion ✓/✗ checklist under an N/7 confluence gauge. '' on no data."""
    if not data:
        return ""
    criteria = data.get("criteria") or []
    if not criteria:
        return ""
    total = int(data.get("total") or len(criteria) or 7)
    met = int(data.get("met_count") or sum(1 for c in criteria if c.get("met")))
    tf = str(data.get("timeframe") or "").strip()
    name = _nice(data.get("resolved_symbol") or data.get("symbol"))
    as_of = str(data.get("as_of") or "")
    bonus = data.get("bonus") or None

    n = len(criteria)
    W, ml, mr = 720, 16, 16
    top = 70                       # header + gauge band
    rowh = 26
    bonus_h = 22 if bonus else 0
    H = top + n * rowh + bonus_h + 26

    ttl = f"{name} cycle-bottom signals".strip() if name else "Cycle-bottom signals"
    s = [svg_open(W, H), title(ml, 24, ttl)]

    # --- N/7 confluence gauge (header right) ---
    frac = (met / total) if total else 0.0
    gcol = ramp(1.0 - frac)        # more firing = greener (ramp: 0=green)
    gx, gy, gw = ml + 250, 12, W - mr - (ml + 250)
    s.append(f'<rect x="{gx}" y="{gy}" width="{gw}" height="14" rx="3" '
             f'fill="{BORDER}" fill-opacity="{OP_TRACK}"/>')
    s.append(f'<rect x="{gx}" y="{gy}" width="{max(2.0, frac*gw):.1f}" height="14" '
             f'rx="3" fill="{gcol}" fill-opacity="{OP_FILL_STRONG}"/>')
    s.append(f'<text x="{W-mr}" y="{gy+11}" text-anchor="end" fill="{gcol}" '
             f'font-size="12" font-weight="700" font-family={MONO!r}>{met}/{total}</text>')
    # Verdict band word under the gauge.
    band = _band_word(met, total)
    s.append(f'<text x="{gx}" y="{gy+28}" fill="{MUTED}" font-size="8" '
             f'font-family={MONO!r}>{esc(band)}</text>')

    # --- the 7 criteria rows ---
    y = top
    for c in criteria:
        on = bool(c.get("met"))
        mark = "✓" if on else "✗"
        mcol = GREEN if on else RED
        lbl = esc(str(c.get("label") or c.get("key") or ""))
        # NOTE: the Rust `detail` field carries raw indicator names (RSI, DSS,
        # …) — it is deliberately NOT rendered here, because this chart is
        # public-safe and must stay name-free. Only the clean composite `label`
        # (plain functional language) and the ✓/✗ mark are drawn. The firing
        # status is conveyed by mark + color + row wash, not by the detail line.
        if on:
            s.append(f'<rect x="{ml-4}" y="{y-2}" width="{W-2*ml+8}" height="{rowh-4}" '
                     f'rx="3" fill="{GREEN}" fill-opacity="{OP_WASH}"/>')
        s.append(f'<text x="{ml}" y="{y+14}" fill="{mcol}" font-size="13" '
                 f'font-weight="700" font-family={MONO!r}>{mark}</text>')
        s.append(f'<text x="{ml+22}" y="{y+13}" fill="{TEXT if on else MUTED}" '
                 f'font-size="10.5" font-weight="600" font-family={SANS!r}>{lbl}</text>')
        word = "firing" if on else "not yet"
        s.append(f'<text x="{W-mr}" y="{y+13}" text-anchor="end" fill="{mcol}" '
                 f'font-size="8.5" font-family={MONO!r}>{word}</text>')
        y += rowh

    # --- non-counted bonus confirmation (visually set apart) ---
    # The Rust bonus `label` names the pi-cycle indicator; this chart is
    # public-safe, so we render a fixed name-free label here (the bonus is
    # reported, NEVER counted in the seven).
    if bonus:
        on = bool(bonus.get("met"))
        mcol = GREEN if on else MUTED
        mark = "+" if on else "·"
        lbl = "Extra cycle-low confirmation (not counted)"
        word = "firing" if on else "—"
        s.append(f'<line x1="{ml}" y1="{y-2}" x2="{W-mr}" y2="{y-2}" '
                 f'stroke="{BORDER}" stroke-opacity="{OP_TRACK}"/>')
        s.append(f'<text x="{ml}" y="{y+15}" fill="{mcol}" font-size="12" '
                 f'font-weight="700" font-family={MONO!r}>{mark}</text>')
        s.append(f'<text x="{ml+22}" y="{y+14}" fill="{MUTED}" font-size="9" '
                 f'font-style="italic" font-family={SANS!r}>{esc(lbl)}</text>')
        s.append(f'<text x="{W-mr}" y="{y+14}" text-anchor="end" fill="{mcol}" '
                 f'font-size="8" font-family={MONO!r}>{word}</text>')
        y += bonus_h

    foot = f"{tf} confluence" + (f" · as of {as_of}" if as_of else "")
    s.append(caption(ml, H - 9, foot))
    return "\n".join(s) + "\n</svg>"


def _band_word(met, total):
    """Mirror the Rust verdict bands (0..7) in name-free language."""
    if total != 7:
        return f"{met} of {total} firing"
    if met == 0:
        return "no bottom criteria firing"
    if met <= 2:
        return "early / weak confluence"
    if met <= 4:
        return "building confluence"
    if met <= 6:
        return "strong confluence"
    return "very strong confluence (all 7)"


# --------------------------------------------------------- VIZ: TRACKED DASHBOARD
def cycle_signals_tracked(data):
    """Heat-strip table of every tracked cycle-signal alert from
    `analytics cycles tracked --json`. Color-coded by closeness-to-firing.
    '' on no data. Privacy-safe: signal metadata + counts only, no dollars."""
    if not data:
        return ""
    signals = data.get("signals") or []
    if not signals:
        return ""
    summary = data.get("summary") if isinstance(data.get("summary"), dict) else {}

    # Cap the row count so the card stays one screen; order armed-and-close
    # first (most actionable), then the rest, deterministically.
    def _close_frac(sig):
        live = sig.get("live") if isinstance(sig.get("live"), dict) else {}
        met = _ival(live.get("met_count"))
        tot = _ival(live.get("total"))
        return (met / tot) if (met is not None and tot) else -1.0
    rows = sorted(
        signals,
        key=lambda s: (0 if s.get("fired") else 1, -_close_frac(s),
                       str(s.get("asset") or ""), str(s.get("label") or "")),
    )
    capped = rows[:18]
    overflow = len(rows) - len(capped)

    W, ml, mr = 720, 16, 16
    top = 70
    rowh = 22
    H = top + len(capped) * rowh + (16 if overflow else 0) + 24

    s = [svg_open(W, H), title(ml, 24, "Tracked cycle signals")]

    # --- header summary (counts) ---
    tot = _ival(summary.get("total")) or len(signals)
    nb = _ival(summary.get("bottom")) or 0
    nt = _ival(summary.get("top")) or 0
    fired = _ival(summary.get("fired")) or 0
    close = _ival(summary.get("close_to_firing")) or 0
    sumtxt = f"{tot} tracked · {nb} bottom · {nt} top · {fired} fired · {close} close"
    s.append(f'<text x="{W-mr}" y="22" text-anchor="end" fill="{MUTED}" '
             f'font-size="9" font-family={MONO!r}>{esc(sumtxt)}</text>')

    # --- column headers ---
    cols = [ml, ml + 120, ml + 210, ml + 470, ml + 560, ml + 640]
    hdrs = ["asset / signal", "timeframe", "live confluence", "dist", "fired", "last"]
    for cx, hd in zip(cols, hdrs):
        s.append(f'<text x="{cx}" y="{top-12}" fill="{MUTED}" font-size="7.5" '
                 f'font-weight="700" font-family={MONO!r}>{esc(hd)}</text>')
    s.append(f'<line x1="{ml}" y1="{top-8}" x2="{W-mr}" y2="{top-8}" '
             f'stroke="{BORDER}" stroke-opacity="{OP_TRACK}"/>')

    y = top
    for sig in capped:
        polarity = str(sig.get("polarity") or "").lower()
        pol_mark = "▼" if polarity == "top" else "▲"
        pol_col = RED if polarity == "top" else GREEN
        live = sig.get("live") if isinstance(sig.get("live"), dict) else {}
        met = _ival(live.get("met_count"))
        ltot = _ival(live.get("total"))
        frac = (met / ltot) if (met is not None and ltot) else 0.0
        # closeness color: closer to firing = hotter (ramp 0=green..1=red is for
        # risk; here closeness IS the signal so invert: more met = more amber/red
        # "attention"). Use ramp(frac) so a near-full bar reads hot.
        heat = ramp(frac) if (met is not None and ltot) else MUTED
        fired = bool(sig.get("fired"))

        if fired:
            s.append(f'<rect x="{ml-4}" y="{y-2}" width="{W-2*ml+8}" '
                     f'height="{rowh-3}" rx="3" fill="{heat}" fill-opacity="{OP_WASH}"/>')

        asset = _nice(sig.get("asset"))
        label = str(sig.get("label") or "")
        lbl = label if len(label) <= 22 else label[:21] + "…"
        s.append(f'<text x="{cols[0]}" y="{y+13}" fill="{pol_col}" font-size="9" '
                 f'font-weight="700" font-family={MONO!r}>{pol_mark}</text>')
        s.append(f'<text x="{cols[0]+12}" y="{y+13}" fill="{TEXT}" font-size="9" '
                 f'font-weight="600" font-family={SANS!r}>{esc(asset)} '
                 f'<tspan fill="{MUTED}" font-size="8">{esc(lbl)}</tspan></text>')
        tf = str(sig.get("timeframe") or "")
        s.append(f'<text x="{cols[1]}" y="{y+13}" fill="{MUTED}" font-size="8.5" '
                 f'font-family={MONO!r}>{esc(tf)}</text>')

        # live confluence bar (met/total) or summary text
        if met is not None and ltot:
            gx, gw = cols[2], 180
            s.append(f'<rect x="{gx}" y="{y+3}" width="{gw}" height="11" rx="2" '
                     f'fill="{BORDER}" fill-opacity="{OP_TRACK}"/>')
            s.append(f'<rect x="{gx}" y="{y+3}" width="{max(2.0, frac*gw):.1f}" '
                     f'height="11" rx="2" fill="{heat}" fill-opacity="{OP_FILL_STRONG}"/>')
            s.append(f'<text x="{gx+gw+6}" y="{y+12}" fill="{heat}" font-size="8.5" '
                     f'font-weight="700" font-family={MONO!r}>{met}/{ltot}</text>')
        else:
            summ = str(live.get("summary") or "—")
            s.append(f'<text x="{cols[2]}" y="{y+12}" fill="{MUTED}" font-size="8.5" '
                     f'font-family={MONO!r}>{esc(summ[:34])}</text>')

        # distance-to-target
        dist = live.get("distance_to_target")
        dtxt = ""
        if isinstance(dist, (int, float)):
            dtxt = f"{float(dist):+.2f}"
        elif dist is not None:
            dtxt = str(dist)[:8]
        s.append(f'<text x="{cols[3]}" y="{y+12}" fill="{MUTED}" font-size="8" '
                 f'font-family={MONO!r}>{esc(dtxt)}</text>')

        # fired flag
        fcol = AMBER if fired else MUTED
        s.append(f'<text x="{cols[4]}" y="{y+12}" fill="{fcol}" font-size="8" '
                 f'font-weight="{700 if fired else 400}" '
                 f'font-family={MONO!r}>{"yes" if fired else "armed"}</text>')

        # time since last
        tsl = str(sig.get("time_since_last") or "never")
        s.append(f'<text x="{W-mr}" y="{y+12}" text-anchor="end" fill="{MUTED}" '
                 f'font-size="8" font-family={MONO!r}>{esc(tsl[:10])}</text>')
        y += rowh

    if overflow:
        s.append(f'<text x="{ml}" y="{y+11}" fill="{MUTED}" font-size="8" '
                 f'font-style="italic" font-family={SANS!r}>+{overflow} more tracked '
                 f'signals (run `analytics cycles tracked`)</text>')
        y += 16

    s.append(caption(ml, H - 8, "live met-count vs target · color = closeness to firing"))
    return "\n".join(s) + "\n</svg>"


def _ival(v):
    try:
        return int(v)
    except (TypeError, ValueError):
        return None


# --------------------------------------------------------- orchestration / API
def _parse_payload(arg):
    """ASSET[?timeframe=monthly] -> (asset, timeframe). Default monthly."""
    asset, tf = arg, "monthly"
    if "?" in arg:
        asset, _, qs = arg.partition("?")
        for part in qs.split("&"):
            k, _, v = part.partition("=")
            if k.strip() == "timeframe" and v.strip():
                tf = v.strip()
    return asset.strip(), tf.strip()


def _tracked_cli(arg):
    """Parse the tracked payload (ALL | ASSET[?polarity=top]) into CLI flags."""
    cli = ["analytics", "cycles", "tracked"]
    asset, _, qs = arg.partition("?")
    asset = asset.strip()
    if asset and asset.lower() != "all":
        cli += ["--asset", asset]
    for part in qs.split("&"):
        k, _, v = part.partition("=")
        if k.strip() == "polarity" and v.strip() in ("top", "bottom"):
            cli += ["--polarity", v.strip()]
    return cli


def render(viz_type, arg="", pftui=None):
    """Render a cycle-signal viz. '' on any failure (additive, never load-bearing).

    Types: `checklist` (the N/7 bottom confluence list) and `tracked` (the
    tracked-signals dashboard over every armed cycle-signal alert)."""
    try:
        if viz_type == "tracked":
            data = pftui_json(_tracked_cli(arg), pftui)
            return cycle_signals_tracked(data)
        if viz_type != "checklist":
            return ""
        asset, tf = _parse_payload(arg)
        if not asset:
            return ""
        data = pftui_json(
            ["analytics", "cycles", "bottom-signals", "--asset", asset,
             "--timeframe", tf],
            pftui,
        )
        return cycle_signals_checklist(data)
    except Exception:  # never let a chart break a report
        return ""


# Token contract: <!--CYCLE_SIGNALS_VIZ:checklist:BTC--> (arg may carry ?timeframe=)
TOKEN_RE = re.compile(r"<!--\s*CYCLE_SIGNALS_VIZ:([a-z]+):([^\s>]+?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="cycle-signals-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(
        description="Render the pftui cycle-signal checklist / tracked dashboard as inline SVG.")
    p.add_argument("viz", choices=["checklist", "tracked"])
    p.add_argument("--asset", default="all")
    p.add_argument("--timeframe", default="monthly")
    p.add_argument("--polarity", choices=["bottom", "top"], default=None)
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    if args.viz == "tracked":
        arg = args.asset + (f"?polarity={args.polarity}" if args.polarity else "")
    else:
        arg = f"{args.asset}?timeframe={args.timeframe}"
    svg = render(args.viz, arg, args.pftui)
    if not svg:
        sys.stderr.write(f"no cycle-signal viz available for {args.asset}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
