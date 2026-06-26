#!/usr/bin/env python3
"""Positioning-model tearsheet (inline SVG) for the pftui report PDFs.

Renders ONE polished portfolio tearsheet card from the hardened
`pftui analytics models backtest <name> --json` contract (see
src/commands/models_cmd.rs for the `report` payload, docs/POSITIONING-MODELS.md
§3.3 for the benchmark trio):

  tearsheet — a stat header (CAGR / Sharpe / Sortino / MaxDD / Calmar / Vol /
              Turnover / TimeInCash / nRebalances + a "vs rebalanced-base" delta
              on CAGR and MaxDD — the rule-alpha number); an EQUITY panel (the
              model's daily curve, log-scaled when the range warrants, plus the
              three benchmark curves — static / rebalanced-base / equal-weight —
              as faint reference lines so rule-alpha-vs-rebalance is VISIBLE); an
              UNDERWATER drawdown strip (running drawdown %, worst point
              labelled); and an ALLOCATION-OVER-TIME band (stacked class weights
              stepped between rebalance events — the only weights the report
              carries, so this is event-stepped, not true daily drift).

  compare   — a small overlay of 2..N models' equity curves, rebased to 100, for
              a quick rule-alpha eyeball (nice-to-have; the tearsheet is primary).

Honest-by-construction: an errored / empty / too-short backtest degrades to a
caveat card rather than a misleading chart. No portfolio dollars are shown — the
curves are rebased indices and the math is market-price-only (Rust owns it).

CLI:   python model_viz.py tearsheet --model syn-aggressive [--from 2022-01-01 --to 2022-12-31]
Token: <!--MODEL_VIZ:tearsheet:m2-hard-money-cycles-->
       <!--MODEL_VIZ:tearsheet:syn-aggressive?from=2022-01-01&to=2022-12-31-->
       <!--MODEL_VIZ:compare:m1,m2,m3-->
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BLUE, BORDER, CYAN, GREEN, MONO, MUTED, RED, SANS, TEXT,
    OP_FILL_SOFT, OP_FILL_STRONG, OP_TRACK, OP_WASH,
    caption, d2o, esc, pftui_json, svg_open, title,
)
import math  # noqa: E402

# Stack palette for the allocation band; cash always renders as the muted track.
CLASS_PALETTE = [CYAN, GREEN, BLUE, AMBER, RED, "#cba6f7", "#fab387"]
CASH_KEYS = {"cash", "CASH"}


def _fnum(v):
    """rust_decimal STRING (or number) -> float, or None. Money/weights come
    through as strings (e.g. "100000", "0.5"); metrics as floats."""
    if v is None:
        return None
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def _curve_points(curve):
    """[{date, equity}] -> [(ordinal_x, equity_float)] (skips unparseable rows)."""
    out = []
    for p in curve or []:
        if not isinstance(p, dict):
            continue
        e = _fnum(p.get("equity"))
        d = p.get("date")
        if e is None or not d:
            continue
        try:
            out.append((d2o(d), e))
        except (ValueError, TypeError):
            continue
    return out


def _curve_dd(curve):
    """[{date, drawdown_pct}] -> [(ordinal_x, dd_pct_float)] (dd is <= 0)."""
    out = []
    for p in curve or []:
        if not isinstance(p, dict):
            continue
        dd = _fnum(p.get("drawdown_pct"))
        d = p.get("date")
        if dd is None or not d:
            continue
        try:
            out.append((d2o(d), dd))
        except (ValueError, TypeError):
            continue
    return out


def _rebase(points):
    """Rebase an equity curve to 100 at its first point (index, dollar-free)."""
    if not points:
        return []
    base = points[0][1] or 1.0
    return [(x, v / base * 100.0) for x, v in points]


def _class_of(sym, sym2class):
    if sym in CASH_KEYS:
        return "cash"
    return sym2class.get(sym, "cash" if sym in CASH_KEYS else str(sym))


def _alloc_segments(events, sym2class, x_end):
    """Build stepped class-weight segments from rebalance post_weights.

    Returns (classes_ordered, segments) where each segment is
    (x_start, x_end, {class: weight}). Weights are held flat from each event to
    the next event (or the curve end) — event-stepped, since the report carries
    weights only at rebalance decisions, not daily.
    """
    rows = []
    for ev in events or []:
        if not isinstance(ev, dict):
            continue
        d = ev.get("date")
        pw = ev.get("post_weights")
        if not d or not isinstance(pw, list) or not pw:
            continue
        try:
            x = d2o(d)
        except (ValueError, TypeError):
            continue
        cw = {}
        for pair in pw:
            if not isinstance(pair, (list, tuple)) or len(pair) != 2:
                continue
            w = _fnum(pair[1])
            if w is None:
                continue
            cls = _class_of(pair[0], sym2class)
            cw[cls] = cw.get(cls, 0.0) + w
        if cw:
            rows.append((x, cw))
    rows.sort(key=lambda r: r[0])
    if not rows:
        return [], []
    # class order: cash last (top of stack), others by first appearance.
    order = []
    for _, cw in rows:
        for cls in cw:
            if cls not in order:
                order.append(cls)
    non_cash = [c for c in order if c != "cash"]
    classes = non_cash + (["cash"] if "cash" in order else [])
    segs = []
    for i, (x, cw) in enumerate(rows):
        x1 = rows[i + 1][0] if i + 1 < len(rows) else max(x_end, x)
        if x1 <= x:
            x1 = x + 1
        segs.append((x, x1, cw))
    return classes, segs


def _class_color(cls, idx):
    if cls == "cash":
        return MUTED
    return CLASS_PALETTE[idx % len(CLASS_PALETTE)]


# ----------------------------------------------------------------- TEARSHEET
def tearsheet_card(data, name_hint=""):
    """Render ONE tearsheet from a `models backtest --json` payload. '' on no
    data; a caveat card on an errored / empty / too-short backtest."""
    if not isinstance(data, dict):
        return ""
    report = data.get("report") if isinstance(data.get("report"), dict) else None
    model = data.get("model") if isinstance(data.get("model"), dict) else {}
    name = model.get("name") or name_hint or "model"
    win = data.get("window") if isinstance(data.get("window"), dict) else {}
    wf, wt = win.get("from"), win.get("to")

    if not report:
        return _caveat_card(name, "backtest returned no report", wf, wt)
    curve = report.get("daily_equity_curve") or []
    pts = _curve_points(curve)
    if len(pts) < 2:
        return _caveat_card(name, "insufficient daily curve (need >= 2 bars)", wf, wt)

    m = report.get("metrics") or {}
    benches = report.get("benchmarks") or {}

    W = 720
    ml, mr = 16, 16
    H = 504
    s = [svg_open(W, H), title(ml, 24, f"{esc(name)} — positioning tearsheet")]

    win_txt = f"{wf or pts and '?'} → {wt or '?'}  ·  {len(pts)} bars"
    s.append(f'<text x="{W-mr}" y="20" text-anchor="end" fill="{MUTED}" '
             f'font-size="9" font-family={MONO!r}>{esc(win_txt)}</text>')

    # -------- stat header --------
    def g(k):
        return _fnum(m.get(k))
    stats = [
        ("CAGR", g("cagr_pct"), "%", GREEN),
        ("Sharpe", g("sharpe"), "", TEXT),
        ("Sortino", g("sortino"), "", TEXT),
        ("MaxDD", g("max_drawdown_pct"), "%", RED),
        ("Calmar", g("calmar"), "", CYAN),
        ("Vol", g("ann_vol_pct"), "%", AMBER),
        ("Turn/yr", g("avg_turnover_pct_per_yr"), "%", MUTED),
        ("Cash", g("time_in_cash_pct"), "%", MUTED),
        ("nReb", _fnum(report.get("n_rebalances")), "", MUTED),
    ]
    sx0, sy = ml, 50
    cellw = (W - ml - mr) / len(stats)
    for i, (lbl, val, suf, col) in enumerate(stats):
        cx = sx0 + i * cellw
        vtxt = "--" if val is None else (f"{val:.0f}" if lbl == "nReb"
                                         else f"{val:.2f}{suf}")
        s.append(f'<text x="{cx:.1f}" y="{sy}" fill="{MUTED}" font-size="7.5" '
                 f'font-family={MONO!r}>{esc(lbl)}</text>')
        s.append(f'<text x="{cx:.1f}" y="{sy+13}" fill="{col}" font-size="11" '
                 f'font-weight="700" font-family={MONO!r}>{esc(vtxt)}</text>')

    # rule-alpha delta vs the rebalanced-base benchmark (CAGR up = good, MaxDD down = good)
    rb = benches.get("rebalanced_base_policy") or {}
    rbm = rb.get("metrics") or {}
    d_cagr = _delta(g("cagr_pct"), _fnum(rbm.get("cagr_pct")))
    d_dd = _delta(g("max_drawdown_pct"), _fnum(rbm.get("max_drawdown_pct")))
    if d_cagr is not None or d_dd is not None:
        cagr_col = GREEN if (d_cagr or 0) >= 0 else RED
        dd_col = GREEN if (d_dd or 0) <= 0 else RED  # lower DD is better
        parts = "rule-alpha vs rebalanced-base:  "
        s.append(f'<text x="{ml}" y="{sy+30}" fill="{MUTED}" font-size="8.5" '
                 f'font-family={MONO!r}>{esc(parts)}'
                 f'<tspan fill="{cagr_col}" font-weight="700">'
                 f'CAGR {_sgn(d_cagr)}</tspan>'
                 f'<tspan fill="{MUTED}">   ·   </tspan>'
                 f'<tspan fill="{dd_col}" font-weight="700">'
                 f'MaxDD {_sgn(d_dd)}pp</tspan></text>')

    # -------- equity panel --------
    p_top, p_bot = 98, 268
    p_h = p_bot - p_top
    plot_w = W - ml - mr
    s.append(f'<text x="{ml}" y="{p_top-6}" fill="{CYAN}" font-size="9" '
             f'font-weight="600" font-family={MONO!r}>'
             f'equity (rebased 100) — model vs benchmarks</text>')

    bench_curves = []
    for key, col, lab in (
        ("static_base_policy", MUTED, "static base"),
        ("rebalanced_base_policy", BLUE, "rebal. base"),
        ("equal_weight", AMBER, "equal weight"),
    ):
        b = benches.get(key) or {}
        bp = _rebase(_curve_points(b.get("daily_equity_curve") or []))
        if len(bp) >= 2:
            bench_curves.append((bp, col, lab))

    model_pts = _rebase(pts)
    all_pts = list(model_pts)
    for bp, _, _ in bench_curves:
        all_pts.extend(bp)
    xs = [x for x, _ in all_pts]
    ys = [y for _, y in all_pts]
    xlo, xhi = min(xs), max(xs)
    ylo, yhi = min(ys), max(ys)
    if xhi <= xlo:
        xhi = xlo + 1
    if yhi - ylo < 1e-9:
        ylo, yhi = ylo - 1.0, yhi + 1.0
    use_log = ylo > 0 and (yhi / ylo) > 3.0

    def ty(v):
        if use_log:
            lv, llo, lhi = math.log(max(v, 1e-9)), math.log(ylo), math.log(yhi)
            return p_bot - (lv - llo) / (lhi - llo) * p_h
        return p_bot - (v - ylo) / (yhi - ylo) * p_h

    def tx(x):
        return ml + (x - xlo) / (xhi - xlo) * plot_w

    # baseline-100 reference
    y100 = ty(100.0)
    if p_top <= y100 <= p_bot:
        s.append(f'<line x1="{ml}" y1="{y100:.1f}" x2="{W-mr}" y2="{y100:.1f}" '
                 f'stroke="{TEXT}" stroke-width="0.75" stroke-opacity="0.3" '
                 f'stroke-dasharray="3 3"/>')
    # benchmark curves (faint, thin), then the model (bold) on top
    for bp, col, _ in bench_curves:
        s.append(_poly(bp, tx, ty, col, 1.0, OP_TRACK))
    s.append(_poly(model_pts, tx, ty, CYAN, 2.0, OP_FILL_STRONG))

    # legend
    lx = ml
    items = [("model", CYAN)] + [(lab, col) for _, col, lab in bench_curves]
    for lab, col in items:
        s.append(f'<rect x="{lx}" y="{p_top-2}" width="9" height="3" fill="{col}"/>')
        s.append(f'<text x="{lx+12}" y="{p_top+2}" fill="{MUTED}" font-size="7.5" '
                 f'font-family={MONO!r}>{esc(lab)}</text>')
        lx += 14 + len(lab) * 5.4 + 8
    if use_log:
        s.append(f'<text x="{W-mr}" y="{p_top+2}" text-anchor="end" fill="{MUTED}" '
                 f'font-size="7.5" font-family={MONO!r}>log scale</text>')

    # -------- underwater drawdown strip --------
    u_top, u_bot = 290, 348
    u_h = u_bot - u_top
    s.append(f'<text x="{ml}" y="{u_top-5}" fill="{RED}" font-size="9" '
             f'font-weight="600" font-family={MONO!r}>underwater (drawdown %)</text>')
    dd = _curve_dd(curve)
    if dd:
        worst = min(d for _, d in dd)
        worst = worst if worst < 0 else -1e-9
        # baseline (0%) at top of the strip, worst at the bottom.
        def uy(v):
            return u_top + (v / worst) * u_h if worst < 0 else u_top
        # filled area from 0 down to the dd line
        path = [f'M {tx(dd[0][0]):.1f} {u_top:.1f}']
        for x, v in dd:
            path.append(f'L {tx(x):.1f} {uy(v):.1f}')
        path.append(f'L {tx(dd[-1][0]):.1f} {u_top:.1f} Z')
        s.append(f'<path d="{" ".join(path)}" fill="{RED}" '
                 f'fill-opacity="{OP_WASH}" stroke="{RED}" stroke-width="1" '
                 f'stroke-opacity="{OP_FILL_SOFT}"/>')
        # worst-point label
        wx = next((x for x, v in dd if v <= worst + 1e-9), dd[-1][0])
        s.append(f'<text x="{tx(wx):.1f}" y="{u_bot+10:.1f}" text-anchor="middle" '
                 f'fill="{RED}" font-size="8" font-weight="700" '
                 f'font-family={MONO!r}>worst {worst:.1f}%</text>')

    # -------- allocation-over-time band --------
    a_top, a_bot = 372, 462
    a_h = a_bot - a_top
    s.append(f'<text x="{ml}" y="{a_top-5}" fill="{CYAN}" font-size="9" '
             f'font-weight="600" font-family={MONO!r}>'
             f'allocation over time (class weights, event-stepped)</text>')
    sym2class = {}
    for a in (model.get("universe") or []):
        if isinstance(a, dict) and a.get("symbol"):
            sym2class[a["symbol"]] = a.get("class") or "?"
    classes, segs = _alloc_segments(report.get("rebalance_events"), sym2class, xhi)
    if classes and segs:
        # vertical max for normalization (weights ~sum to 1; clamp to >=1).
        ax0, ax1 = tx(segs[0][0]), tx(segs[-1][1])
        s.append(f'<rect x="{ax0:.1f}" y="{a_top}" width="{max(ax1-ax0,1):.1f}" '
                 f'height="{a_h}" fill="{BORDER}" fill-opacity="0.15"/>')
        for seg_i, (x0, x1, cw) in enumerate(segs):
            sx0, sx1 = tx(x0), tx(x1)
            ybase = a_bot
            cum = 0.0
            total = sum(max(w, 0.0) for w in cw.values()) or 1.0
            for ci, cls in enumerate(classes):
                w = max(cw.get(cls, 0.0), 0.0)
                if w <= 0:
                    continue
                frac = w / total
                hgt = frac * a_h
                ytop = ybase - hgt
                s.append(f'<rect x="{sx0:.1f}" y="{ytop:.1f}" '
                         f'width="{max(sx1-sx0,0.5):.1f}" height="{hgt:.1f}" '
                         f'fill="{_class_color(cls, ci)}" '
                         f'fill-opacity="{OP_FILL_SOFT}"/>')
                ybase = ytop
        # legend
        lx = ml
        for ci, cls in enumerate(classes):
            s.append(f'<rect x="{lx}" y="{a_bot+4}" width="8" height="8" '
                     f'fill="{_class_color(cls, ci)}" fill-opacity="{OP_FILL_SOFT}"/>')
            s.append(f'<text x="{lx+11}" y="{a_bot+11}" fill="{MUTED}" '
                     f'font-size="7.5" font-family={MONO!r}>{esc(cls)}</text>')
            lx += 14 + len(cls) * 5.4 + 8
    else:
        s.append(f'<text x="{ml}" y="{a_top+a_h/2}" fill="{MUTED}" font-size="9" '
                 f'font-family={MONO!r}>no rebalance weights to chart</text>')

    foot = (f"{name}  ·  {win_txt}  ·  benchmarks = static / rebalanced-base / "
            f"equal-weight (same calendar + costs)")
    s.append(caption(ml, H - 6, foot[:128]))
    return "\n".join(s) + "\n</svg>"


def _delta(a, b):
    if a is None or b is None:
        return None
    return a - b


def _sgn(v):
    if v is None:
        return "--"
    return f"{v:+.2f}"


def _poly(points, tx, ty, color, width, opacity):
    if len(points) < 2:
        return ""
    pts = " ".join(f"{tx(x):.1f},{ty(v):.1f}" for x, v in points)
    return (f'<polyline points="{pts}" fill="none" stroke="{color}" '
            f'stroke-width="{width}" stroke-opacity="{opacity}" '
            f'stroke-linejoin="round"/>')


def _caveat_card(name, why, wf, wt):
    """Honest 'tearsheet unavailable' card (errored / empty / too-short)."""
    W, H, ml, mr = 720, 132, 16, 16
    s = [svg_open(W, H), title(ml, 24, f"{esc(name)} — positioning tearsheet")]
    s.append(f'<text x="{W-mr}" y="24" text-anchor="end" fill="{AMBER}" '
             f'font-size="8.5" font-weight="700" font-family={MONO!r}>unavailable</text>')
    s.append(f'<rect x="{ml}" y="42" width="{W-2*ml}" height="50" rx="6" '
             f'fill="{AMBER}" fill-opacity="{OP_WASH}" stroke="{AMBER}" '
             f'stroke-opacity="0.4"/>')
    s.append(f'<text x="{W/2}" y="66" text-anchor="middle" fill="{AMBER}" '
             f'font-size="12" font-weight="700" font-family={SANS!r}>'
             f'Tearsheet unavailable</text>')
    s.append(f'<text x="{W/2}" y="84" text-anchor="middle" fill="{MUTED}" '
             f'font-size="9" font-family={SANS!r}>{esc(why)}</text>')
    win = f"{wf or '?'} → {wt or '?'}" if (wf or wt) else "no window"
    s.append(caption(ml, H - 8, f"positioning backtest — {win}"))
    return "\n".join(s) + "\n</svg>"


# ------------------------------------------------------------------- COMPARE
def compare_card(datasets):
    """Overlay 2..N models' rebased equity curves. `datasets` = [(name, data)].
    '' if fewer than 2 have a usable curve."""
    series = []
    for name, data in datasets:
        if not isinstance(data, dict):
            continue
        rep = data.get("report") or {}
        pts = _rebase(_curve_points(rep.get("daily_equity_curve") or []))
        if len(pts) >= 2:
            series.append((name, pts))
    if len(series) < 2:
        return ""
    W, H, ml, mr = 720, 300, 16, 16
    p_top, p_bot = 56, 262
    p_h = p_bot - p_top
    plot_w = W - ml - mr
    s = [svg_open(W, H), title(ml, 24, "positioning models — equity comparison")]

    allp = [p for _, pts in series for p in pts]
    xs = [x for x, _ in allp]
    ys = [y for _, y in allp]
    xlo, xhi = min(xs), max(xs)
    ylo, yhi = min(ys), max(ys)
    if xhi <= xlo:
        xhi = xlo + 1
    if yhi - ylo < 1e-9:
        ylo, yhi = ylo - 1.0, yhi + 1.0
    use_log = ylo > 0 and (yhi / ylo) > 3.0

    def ty(v):
        if use_log:
            return p_bot - (math.log(max(v, 1e-9)) - math.log(ylo)) / (
                math.log(yhi) - math.log(ylo)) * p_h
        return p_bot - (v - ylo) / (yhi - ylo) * p_h

    def tx(x):
        return ml + (x - xlo) / (xhi - xlo) * plot_w

    y100 = ty(100.0)
    if p_top <= y100 <= p_bot:
        s.append(f'<line x1="{ml}" y1="{y100:.1f}" x2="{W-mr}" y2="{y100:.1f}" '
                 f'stroke="{TEXT}" stroke-width="0.75" stroke-opacity="0.3" '
                 f'stroke-dasharray="3 3"/>')
    palette = [CYAN, GREEN, AMBER, BLUE, RED, "#cba6f7"]
    lx = ml
    for i, (name, pts) in enumerate(series):
        col = palette[i % len(palette)]
        s.append(_poly(pts, tx, ty, col, 1.75, OP_FILL_STRONG))
        s.append(f'<rect x="{lx}" y="{p_top-12}" width="9" height="3" fill="{col}"/>')
        s.append(f'<text x="{lx+12}" y="{p_top-9}" fill="{MUTED}" font-size="8" '
                 f'font-family={MONO!r}>{esc(name)}</text>')
        lx += 14 + len(name) * 5.6 + 10
    if use_log:
        s.append(f'<text x="{W-mr}" y="{p_top-9}" text-anchor="end" fill="{MUTED}" '
                 f'font-size="7.5" font-family={MONO!r}>log scale</text>')
    s.append(caption(ml, H - 8, "rebased to 100 at window start — dollar-free index"))
    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def _parse_payload(arg):
    """NAME[?from=YYYY-MM-DD&to=YYYY-MM-DD] -> (name, from, to)."""
    name, frm, to = arg, None, None
    if "?" in arg:
        name, _, qs = arg.partition("?")
        for part in qs.split("&"):
            k, _, v = part.partition("=")
            k, v = k.strip(), v.strip()
            if k == "from" and v:
                frm = v
            elif k == "to" and v:
                to = v
    return name.strip(), frm, to


def _backtest_args(name, frm, to):
    args = ["analytics", "models", "backtest", name]
    if frm:
        args += ["--from", frm]
    if to:
        args += ["--to", to]
    return args


def render(viz_type, arg="", pftui=None):
    """Render a tearsheet/compare card. '' on any failure (never load-bearing)."""
    try:
        if viz_type == "tearsheet":
            name, frm, to = _parse_payload(arg)
            if not name:
                return ""
            data = pftui_json(_backtest_args(name, frm, to), pftui)
            if data is None:
                return _caveat_card(name, "backtest command unavailable / errored", frm, to)
            return tearsheet_card(data, name)
        if viz_type == "compare":
            # arg = "m1,m2,m3" optionally with a shared ?from&to suffix.
            spec, _, qs = arg.partition("?")
            frm = to = None
            for part in qs.split("&"):
                k, _, v = part.partition("=")
                if k.strip() == "from" and v.strip():
                    frm = v.strip()
                elif k.strip() == "to" and v.strip():
                    to = v.strip()
            names = [n.strip() for n in spec.split(",") if n.strip()]
            if len(names) < 2:
                return ""
            datasets = [(n, pftui_json(_backtest_args(n, frm, to), pftui)) for n in names]
            return compare_card(datasets)
        return ""
    except Exception:  # never let a chart break a report
        return ""


# Token: <!--MODEL_VIZ:tearsheet:syn-aggressive?from=2022-01-01&to=2022-12-31-->
#        <!--MODEL_VIZ:compare:m1,m2,m3-->
TOKEN_RE = re.compile(r"<!--\s*MODEL_VIZ:([a-z]+):([^\s>]+?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="model-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(
        description="Render a pftui positioning-model tearsheet as inline SVG.")
    p.add_argument("viz", choices=["tearsheet", "compare"])
    p.add_argument("--model", help="model name/path (tearsheet)")
    p.add_argument("--models", help="comma-separated names (compare)")
    p.add_argument("--from", dest="frm", default=None)
    p.add_argument("--to", dest="to", default=None)
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    if args.viz == "tearsheet":
        name = args.model or ""
        q = []
        if args.frm:
            q.append(f"from={args.frm}")
        if args.to:
            q.append(f"to={args.to}")
        arg = name + ("?" + "&".join(q) if q else "")
    else:
        arg = args.models or ""
        q = []
        if args.frm:
            q.append(f"from={args.frm}")
        if args.to:
            q.append(f"to={args.to}")
        if q:
            arg += "?" + "&".join(q)
    svg = render(args.viz, arg, args.pftui)
    if not svg:
        sys.stderr.write(f"no model viz available for {arg}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
