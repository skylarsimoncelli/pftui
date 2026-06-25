#!/usr/bin/env python3
"""Cycle-signal backtest report card (inline SVG) for the pftui report PDFs.

Renders the forward-return EXPECTANCY block from the hardened
`pftui analytics cycles {bottom,top}-signals backtest --expectancy --json`
contract (see viz/theme.py for the Rust data boundary, docs/CYCLE-SIGNALS.md for
the exact JSON shape) as ONE polished per-signal report card:

  expectancy — a header (asset, polarity, timeframe, anchor count, honesty
               badge), a forward-return panel (grouped signal-vs-baseline mean
               bars per 30/90/180/365-day horizon for the headline confluence
               threshold, sign-aware: bottoms want POSITIVE returns, tops want
               NEGATIVE returns), and a confidence/accuracy table (per-threshold
               firings, hit-rate, closeness lead/lag + price-gap + confidence).
               If the block has no usable anchors or zero matched firings it
               degrades to an honest "reliability unmeasurable — directional
               only" caveat card rather than misleading bars.

Public-safe by construction: no portfolio holdings, no indicator brand names —
only the market-data backtest read. Rust owns all the math; this only draws it.

CLI:   python cycle_backtest_viz.py expectancy --asset BTC --polarity bottom [--timeframe monthly]
Token: <!--CYCLE_BACKTEST_VIZ:expectancy:BTC?polarity=bottom&timeframe=monthly-->
       payload = ASSET[?polarity=bottom|top&timeframe=daily|weekly|monthly]
       (polarity defaults bottom, timeframe defaults monthly)
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BORDER, CYAN, GREEN, MONO, MUTED, RED, SANS, TEXT,
    OP_FILL_STRONG, OP_TRACK, OP_WASH,
    caption, esc, pftui_json, svg_open, title,
)

# Friendly, ticker-free asset names (shared convention with the other viz
# modules). Anything else is shown verbatim, uppercased.
NICE = {
    "GC=F": "Gold", "GOLD": "Gold", "SI=F": "Silver", "SILVER": "Silver",
    "BTC-USD": "Bitcoin", "BTC": "Bitcoin", "ETH-USD": "Ethereum", "ETH": "Ethereum",
    "^GSPC": "S&P 500", "SPY": "S&P 500", "QQQ": "Nasdaq 100",
}

HORIZONS = [30, 90, 180, 365]


def _nice(sym):
    if not sym:
        return ""
    return NICE.get(str(sym).upper(), str(sym).upper())


def _fnum(v):
    """Parse a rust_decimal STRING (or number) -> float, or None. The CLI emits
    monetary/stat values as strings (e.g. "6.25"); ints come through as ints."""
    if v is None:
        return None
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def signal_quality(polarity, return_pct):
    """Sign-aware verdict for a forward return given the signal polarity.

    A cycle-BOTTOM signal is good when the asset goes UP afterwards (positive
    return); a cycle-TOP signal is good when it goes DOWN (negative return).
    Returns one of 'good' / 'bad' / 'flat' — the single source of truth for the
    sign-aware coloring in this card (unit-tested directly).
    """
    v = _fnum(return_pct)
    if v is None or abs(v) < 1e-9:
        return "flat"
    bottom = str(polarity).lower() != "top"
    up = v > 0
    return "good" if (up == bottom) else "bad"


def _qcolor(polarity, return_pct):
    q = signal_quality(polarity, return_pct)
    return GREEN if q == "good" else (RED if q == "bad" else MUTED)


def _hit_rate(horizon_row, polarity):
    """The headline hit-rate for one horizon row: positive_rate_pct for bottoms,
    negative_rate_pct for tops (per docs/CYCLE-SIGNALS.md)."""
    key = "negative_rate_pct" if str(polarity).lower() == "top" else "positive_rate_pct"
    return _fnum(horizon_row.get(key))


def _by_horizon(rows):
    """Index a horizons[] / baseline[] list by horizon_days."""
    out = {}
    for r in rows or []:
        if isinstance(r, dict):
            hd = r.get("horizon_days")
            if hd is not None:
                out[int(hd)] = r
    return out


def _pick_headline(confluence):
    """Pick the headline confluence threshold row (prefer >=4/7, else the
    middle available threshold, else the first)."""
    rows = [r for r in (confluence or []) if isinstance(r, dict)]
    if not rows:
        return None
    for r in rows:
        if r.get("threshold") == 4 or r.get("key") == "confluence_ge_4":
            return r
    return rows[len(rows) // 2]


# ------------------------------------------------------------- VIZ: REPORT CARD
def expectancy_card(data, polarity="bottom", asset_hint=""):
    """Render ONE backtest report card from the `--expectancy` JSON. '' on no
    data; an honest caveat card on insufficient anchors / zero matched firings."""
    if not data:
        return ""
    exp = data.get("expectancy") if isinstance(data, dict) else None
    if not isinstance(exp, dict):
        return ""

    polarity = "top" if str(polarity).lower() == "top" else "bottom"
    name = _nice(data.get("resolved_symbol") or data.get("symbol") or asset_hint)
    tf = str(data.get("timeframe") or exp.get("timeframe") or "monthly").strip()
    anchors_used = exp.get("anchors_used")
    anchor_dates = exp.get("price_structure_anchors") or []
    n_anchors = int(anchors_used) if anchors_used is not None else len(anchor_dates)
    small_n = bool(exp.get("small_n"))
    insufficient = bool(exp.get("insufficient_anchors"))
    caveat = str(exp.get("caveat") or "")
    pol_word = "cycle-top" if polarity == "top" else "cycle-bottom"
    anchor_word = "swing highs" if polarity == "top" else "swing lows"

    headline = _pick_headline(exp.get("confluence"))
    matched = 0
    if headline and isinstance(headline.get("closeness"), dict):
        matched = int(headline["closeness"].get("matched_firings") or 0)
    total_firings = sum(int(r.get("firings") or 0)
                        for r in (exp.get("confluence") or []) if isinstance(r, dict))

    ttl = f"{name} {pol_word} backtest".strip() if name else f"{pol_word.capitalize()} backtest"

    # ---- honest empty / unmeasurable state ----
    if insufficient or not headline or total_firings == 0:
        return _caveat_card(ttl, tf, n_anchors, anchor_word, caveat, polarity)

    baseline = _by_horizon(exp.get("baseline"))
    hz = _by_horizon(headline.get("horizons"))

    W = 720
    ml, mr = 16, 16
    H = 372
    s = [svg_open(W, H), title(ml, 24, ttl)]

    # ---- header right: anchors + honesty badge ----
    badge = []
    if small_n:
        badge.append("small-n")
    if insufficient:
        badge.append("weak-anchors")
    bcol = AMBER if badge else MUTED
    btxt = (" · ".join(badge)) if badge else "directional read"
    s.append(f'<text x="{W-mr}" y="20" text-anchor="end" fill="{MUTED}" '
             f'font-size="9" font-family={MONO!r}>'
             f'{esc(f"{n_anchors} {anchor_word} · {tf}")}</text>')
    s.append(f'<text x="{W-mr}" y="33" text-anchor="end" fill="{bcol}" '
             f'font-size="8.5" font-weight="700" font-family={MONO!r}>{esc(btxt)}</text>')

    hl_label = str(headline.get("label") or f">={headline.get('threshold')}/7")
    s.append(f'<text x="{ml}" y="40" fill="{MUTED}" font-size="9.5" '
             f'font-family={SANS!r}>{esc(hl_label)} · {headline.get("firings", 0)} firings</text>')

    # ============================ PANEL 1: forward-return expectancy bars =====
    p1_top, p1_bot = 64, 214
    p1_h = p1_bot - p1_top
    plot_w = W - ml - mr
    s.append(f'<text x="{ml}" y="{p1_top-6}" fill="{CYAN}" font-size="9" '
             f'font-weight="600" font-family={MONO!r}>'
             f'forward return — signal vs baseline</text>')

    # Domain across signal + baseline means over the 4 horizons (sign-aware).
    vals = [0.0]
    for h in HORIZONS:
        if h in hz:
            vals.append(_fnum(hz[h].get("mean_return_pct")) or 0.0)
            bm = _fnum(hz[h].get("baseline_mean_return_pct"))
            if bm is None and h in baseline:
                bm = _fnum(baseline[h].get("mean_return_pct"))
            vals.append(bm or 0.0)
    vlo, vhi = min(vals), max(vals)
    if vhi - vlo < 1e-9:
        vlo, vhi = vlo - 1.0, vhi + 1.0
    pad = (vhi - vlo) * 0.16
    vlo -= pad
    vhi += pad
    vspan = (vhi - vlo) or 1.0
    YV = lambda v: p1_bot - (v - vlo) / vspan * p1_h
    zero_y = YV(0.0)

    # zero line
    s.append(f'<line x1="{ml}" y1="{zero_y:.1f}" x2="{W-mr}" y2="{zero_y:.1f}" '
             f'stroke="{TEXT}" stroke-width="1" stroke-opacity="0.5"/>')

    group_w = plot_w / len(HORIZONS)
    bw = group_w * 0.26
    for i, h in enumerate(HORIZONS):
        gx = ml + i * group_w + group_w * 0.5
        row = hz.get(h)
        if not row:
            s.append(f'<text x="{gx:.1f}" y="{p1_bot+14:.1f}" text-anchor="middle" '
                     f'fill="{MUTED}" font-size="8" font-family={MONO!r}>{h}d</text>')
            continue
        sig = _fnum(row.get("mean_return_pct")) or 0.0
        bm = _fnum(row.get("baseline_mean_return_pct"))
        if bm is None and h in baseline:
            bm = _fnum(baseline[h].get("mean_return_pct"))
        bm = bm or 0.0
        lift = _fnum(row.get("lift_vs_baseline_pct"))
        if lift is None:
            lift = sig - bm

        # baseline bar (muted, left), signal bar (sign-aware color, right)
        bx0 = gx - bw - 3
        sx0 = gx + 3
        # baseline
        by = YV(bm)
        s.append(f'<rect x="{bx0:.1f}" y="{min(by, zero_y):.1f}" width="{bw:.1f}" '
                 f'height="{abs(by-zero_y):.1f}" fill="{MUTED}" '
                 f'fill-opacity="{OP_TRACK}"/>')
        # signal
        scol = _qcolor(polarity, sig)
        sy = YV(sig)
        s.append(f'<rect x="{sx0:.1f}" y="{min(sy, zero_y):.1f}" width="{bw:.1f}" '
                 f'height="{abs(sy-zero_y):.1f}" fill="{scol}" '
                 f'fill-opacity="{OP_FILL_STRONG}"/>')
        # signal value label
        s.append(f'<text x="{sx0+bw/2:.1f}" y="{(sy-4) if sig>=0 else (sy+11):.1f}" '
                 f'text-anchor="middle" fill="{scol}" font-size="8.5" '
                 f'font-weight="700" font-family={MONO!r}>{sig:+.1f}%</text>')
        # baseline value label
        s.append(f'<text x="{bx0+bw/2:.1f}" y="{(by-4) if bm>=0 else (by+11):.1f}" '
                 f'text-anchor="middle" fill="{MUTED}" font-size="7.5" '
                 f'font-family={MONO!r}>{bm:+.1f}</text>')
        # lift annotation under the horizon label
        lcol = _qcolor(polarity, lift)
        s.append(f'<text x="{gx:.1f}" y="{p1_bot+14:.1f}" text-anchor="middle" '
                 f'fill="{MUTED}" font-size="8" font-family={MONO!r}>{h}d</text>')
        s.append(f'<text x="{gx:.1f}" y="{p1_bot+25:.1f}" text-anchor="middle" '
                 f'fill="{lcol}" font-size="8" font-weight="700" '
                 f'font-family={MONO!r}>lift {lift:+.1f}</text>')

    # legend
    lx = ml
    s.append(f'<rect x="{lx}" y="{p1_top-2}" width="9" height="9" fill="{MUTED}" '
             f'fill-opacity="{OP_TRACK}"/>')
    s.append(f'<text x="{lx+13}" y="{p1_top+6}" fill="{MUTED}" font-size="8" '
             f'font-family={MONO!r}>baseline</text>')
    s.append(f'<rect x="{lx+78}" y="{p1_top-2}" width="9" height="9" fill="{GREEN}" '
             f'fill-opacity="{OP_FILL_STRONG}"/>')
    good_word = "fall (good)" if polarity == "top" else "rise (good)"
    s.append(f'<text x="{lx+91}" y="{p1_top+6}" fill="{MUTED}" font-size="8" '
             f'font-family={MONO!r}>signal {good_word}</text>')

    # ============================ PANEL 2: hit-rate + closeness table =========
    p2_top = 268
    rh = 22
    cols_x = [ml, ml + 96, ml + 210, ml + 330, ml + 470, ml + 585]
    hdrs = ["threshold", "firings", "hit-rate (30/90/180/365)",
            "lead/lag", "price gap", "conf."]
    s.append(f'<text x="{ml}" y="{p2_top-6}" fill="{CYAN}" font-size="9" '
             f'font-weight="600" font-family={MONO!r}>'
             f'confidence &amp; closeness to the actual {anchor_word[:-1]}</text>')
    for cx, hd in zip(cols_x, hdrs):
        s.append(f'<text x="{cx}" y="{p2_top+10}" fill="{MUTED}" font-size="7.5" '
                 f'font-weight="700" font-family={MONO!r}>{esc(hd)}</text>')
    s.append(f'<line x1="{ml}" y1="{p2_top+14}" x2="{W-mr}" y2="{p2_top+14}" '
             f'stroke="{BORDER}" stroke-opacity="{OP_TRACK}"/>')

    y = p2_top + 14
    for r in (exp.get("confluence") or []):
        if not isinstance(r, dict):
            continue
        y += rh
        is_head = (r is headline)
        if is_head:
            s.append(f'<rect x="{ml-4}" y="{y-rh+4}" width="{W-2*ml+8}" '
                     f'height="{rh}" rx="3" fill="{CYAN}" fill-opacity="{OP_WASH}"/>')
        thr = r.get("threshold")
        lbl = f">={thr}/7"
        s.append(f'<text x="{cols_x[0]}" y="{y}" fill="{TEXT if is_head else MUTED}" '
                 f'font-size="9" font-weight="{700 if is_head else 600}" '
                 f'font-family={MONO!r}>{esc(lbl)}</text>')
        s.append(f'<text x="{cols_x[1]}" y="{y}" fill="{MUTED}" font-size="9" '
                 f'font-family={MONO!r}>{r.get("firings", 0)}</text>')
        # hit-rates across horizons
        rhz = _by_horizon(r.get("horizons"))
        parts = []
        for h in HORIZONS:
            hr = _hit_rate(rhz[h], polarity) if h in rhz else None
            parts.append(f"{hr:.0f}" if hr is not None else "--")
        s.append(f'<text x="{cols_x[2]}" y="{y}" fill="{TEXT}" font-size="9" '
                 f'font-family={MONO!r}>{esc("/".join(parts))}</text>')
        # closeness
        cl = r.get("closeness") if isinstance(r.get("closeness"), dict) else {}
        ll = cl.get("median_lead_lag_days")
        gap = _fnum(cl.get("median_price_gap_pct"))
        conf = _fnum(cl.get("confidence_pct"))
        ll_txt = f"{int(ll):+d}d" if ll is not None else "--"
        gap_txt = f"{gap:+.0f}%" if gap is not None else "--"
        conf_txt = f"{conf:.0f}%" if conf is not None else "--"
        ccol = GREEN if (conf is not None and conf >= 60) else (
            AMBER if (conf is not None and conf >= 35) else MUTED)
        s.append(f'<text x="{cols_x[3]}" y="{y}" fill="{MUTED}" font-size="9" '
                 f'font-family={MONO!r}>{esc(ll_txt)}</text>')
        s.append(f'<text x="{cols_x[4]}" y="{y}" fill="{MUTED}" font-size="9" '
                 f'font-family={MONO!r}>{esc(gap_txt)}</text>')
        s.append(f'<text x="{cols_x[5]}" y="{y}" fill="{ccol}" font-size="9" '
                 f'font-weight="700" font-family={MONO!r}>{esc(conf_txt)}</text>')

    foot = caveat[:118] if caveat else f"{matched}/{total_firings} firings matched to a {anchor_word[:-1]}"
    s.append(caption(ml, H - 8, foot))
    return "\n".join(s) + "\n</svg>"


def _caveat_card(ttl, tf, n_anchors, anchor_word, caveat, polarity):
    """Honest 'reliability unmeasurable — directional only' card."""
    W, H, ml, mr = 720, 150, 16, 16
    s = [svg_open(W, H), title(ml, 24, ttl)]
    s.append(f'<text x="{W-mr}" y="24" text-anchor="end" fill="{AMBER}" '
             f'font-size="8.5" font-weight="700" font-family={MONO!r}>unmeasurable</text>')
    s.append(f'<rect x="{ml}" y="44" width="{W-2*ml}" height="56" rx="6" '
             f'fill="{AMBER}" fill-opacity="{OP_WASH}" stroke="{AMBER}" '
             f'stroke-opacity="0.4"/>')
    msg = "Reliability unmeasurable — directional only"
    s.append(f'<text x="{W/2}" y="68" text-anchor="middle" fill="{AMBER}" '
             f'font-size="13" font-weight="700" font-family={SANS!r}>{esc(msg)}</text>')
    why = (f"Too few price-structure {anchor_word} ({n_anchors}) or no matched "
           f"firings to grade forward-return reliability.")
    s.append(f'<text x="{W/2}" y="88" text-anchor="middle" fill="{MUTED}" '
             f'font-size="9" font-family={SANS!r}>{esc(why)}</text>')
    foot = (caveat[:118] if caveat
            else f"{tf} {('cycle-top' if polarity=='top' else 'cycle-bottom')} expectancy — anchors insufficient")
    s.append(caption(ml, H - 10, foot))
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def _parse_payload(arg):
    """ASSET[?polarity=bottom&timeframe=monthly] -> (asset, polarity, tf)."""
    asset, polarity, tf = arg, "bottom", "monthly"
    if "?" in arg:
        asset, _, qs = arg.partition("?")
        for part in qs.split("&"):
            k, _, v = part.partition("=")
            k, v = k.strip(), v.strip()
            if k == "polarity" and v:
                polarity = "top" if v.lower() == "top" else "bottom"
            elif k == "timeframe" and v:
                tf = v
    return asset.strip(), polarity, tf


def render(viz_type, arg="", pftui=None):
    """Render the backtest report card. '' on any failure (never load-bearing)."""
    try:
        if viz_type != "expectancy":
            return ""
        asset, polarity, tf = _parse_payload(arg)
        if not asset:
            return ""
        cmd = "top-signals" if polarity == "top" else "bottom-signals"
        data = pftui_json(
            ["analytics", "cycles", cmd, "backtest", "--asset", asset,
             "--timeframe", tf, "--expectancy"],
            pftui,
        )
        return expectancy_card(data, polarity, asset)
    except Exception:  # never let a chart break a report
        return ""


# Token: <!--CYCLE_BACKTEST_VIZ:expectancy:BTC?polarity=bottom&timeframe=monthly-->
TOKEN_RE = re.compile(r"<!--\s*CYCLE_BACKTEST_VIZ:([a-z]+):([^\s>]+?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="cycle-backtest-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(
        description="Render a pftui cycle-signal backtest report card as inline SVG.")
    p.add_argument("viz", choices=["expectancy"])
    p.add_argument("--asset", required=True)
    p.add_argument("--polarity", choices=["bottom", "top"], default="bottom")
    p.add_argument("--timeframe", default="monthly")
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    svg = render(args.viz,
                 f"{args.asset}?polarity={args.polarity}&timeframe={args.timeframe}",
                 args.pftui)
    if not svg:
        sys.stderr.write(f"no cycle-backtest viz available for {args.asset}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
