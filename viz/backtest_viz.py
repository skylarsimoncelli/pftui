#!/usr/bin/env python3
"""Strategy backtest tearsheet (inline SVG) for the pftui report PDFs.

The backtest showcase, rendered from the hardened
`pftui analytics strategy backtest --json` contract (see viz/theme.py for the
Rust data boundary):

  tearsheet — the EQUITY CURVE + UNDERWATER strip + stat line. The backtest's
              `trades` list (per-trade `return_pct`, anchored on `exit_date`) is
              compounded into a cumulative-equity curve; the running drawdown
              from that curve is drawn as an underwater strip beneath it. A
              header carries CAGR / Sharpe-proxy / max-DD / win-rate / #trades,
              and the strategy's terminal equity is compared against the
              buy-and-hold benchmark. IF the Monte-Carlo block is present, the
              resampled TERMINAL-return spread (p5 / p50 / p95) is drawn as a
              faint cone fanning off the end of the curve — the luck-vs-skill
              spread. (Per-bar MC path bands are NOT exposed by the CLI, only
              terminal + drawdown percentiles, so the cone is anchored at the
              curve's end rather than tracking every bar — see viz/README.md.)

The equity curve is reconstructed by compounding the per-trade `return_pct`
the CLI already emits (verified to reproduce `total_return_pct` and
`max_drawdown_pct` exactly); no number is invented here — Rust still owns the math.

CLI:   python backtest_viz.py tearsheet --asset BTC --entry "rsi(14)<30" --exit "rsi(14)>70"
Token: <!--BACKTEST_VIZ:tearsheet:BTC?entry=rsi(14)<30&exit=rsi(14)>70-->
       (expanded by viz/render.py in the report pipeline)
"""
import os
import re
import sys
from urllib.parse import parse_qs, unquote

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from theme import (  # noqa: E402
    AMBER, BG, BLUE, BORDER, CYAN, GREEN, MONO, MUTED, PANEL, RED, TEXT,
    d2o, esc, pftui_json, svg_open, title,
)

NICE = {
    "GC=F": "GOLD", "GOLD": "GOLD", "SI=F": "SILVER", "SILVER": "SILVER",
    "BTC-USD": "BTC", "BTC": "BTC", "ETH-USD": "ETH", "ETH": "ETH",
    "^GSPC": "SPX", "SPY": "SPY", "QQQ": "QQQ",
}


def _nice(sym):
    return NICE.get(str(sym).upper(), str(sym).upper())


def _num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def equity_curve(trades):
    """Compound per-trade return_pct into a cumulative-equity curve.

    Returns a list of points {x: ordinal-day, eq: multiple, dd: drawdown frac}
    anchored on each trade's exit_date, with a leading 1.0 anchor at the first
    entry. '' / [] inputs degrade to an empty list.
    """
    pts = []
    eq = 1.0
    peak = 1.0
    # Leading anchor at the first entry so the curve starts at 1.0 on the axis.
    first = None
    for t in trades or []:
        first = t.get("entry_date")
        if first:
            break
    if first:
        try:
            pts.append({"x": d2o(first), "eq": 1.0, "dd": 0.0})
        except (ValueError, TypeError):
            pass
    for t in trades or []:
        r = _num(t.get("return_pct"))
        xd = t.get("exit_date")
        if r is None or not xd:
            continue
        try:
            x = d2o(xd)
        except (ValueError, TypeError):
            continue
        eq *= (1.0 + r / 100.0)
        peak = max(peak, eq)
        pts.append({"x": x, "eq": eq, "dd": (eq / peak - 1.0) if peak else 0.0})
    return pts


# ----------------------------------------------------------------- VIZ: TEARSHEET
def tearsheet(report, ttl):
    """Render the equity curve + underwater strip + stat lines.

    Needs at least two equity points (one trade beyond the anchor). Returns ''
    otherwise so the chart degrades to nothing in the report.
    """
    pts = equity_curve(report.get("trades"))
    if len(pts) < 2:
        return ""

    W, H = 720, 372
    ml, mr, mt = 60, 96, 82
    eq_h = 150                 # equity panel height
    gap = 26                   # gap between equity panel and underwater strip
    uw_h = 60                  # underwater strip height
    eq_top = mt
    eq_bot = eq_top + eq_h
    uw_top = eq_bot + gap
    uw_bot = uw_top + uw_h
    plot_w = W - ml - mr

    xs = [p["x"] for p in pts]
    x0, x1 = min(xs), max(xs)
    xspan = (x1 - x0) or 1
    X = lambda x: ml + (x - x0) / xspan * plot_w

    # Equity domain: include the MC terminal cone if present so it fits.
    mc = report.get("monte_carlo") if isinstance(report.get("monte_carlo"), dict) else None
    eqs = [p["eq"] for p in pts]
    eq_lo, eq_hi = min(eqs), max(eqs)
    final_eq = pts[-1]["eq"]
    mc_p5 = mc_p50 = mc_p95 = None
    if mc:
        mc_p5 = _num(mc.get("terminal_return_p5_pct"))
        mc_p50 = _num(mc.get("terminal_return_p50_pct"))
        mc_p95 = _num(mc.get("terminal_return_p95_pct"))
        for v in (mc_p5, mc_p50, mc_p95):
            if v is not None:
                eq_hi = max(eq_hi, 1.0 + v / 100.0)
                eq_lo = min(eq_lo, 1.0 + v / 100.0)
    # Log scale for equity (multiplicative returns read far better in log).
    import math
    eq_lo = max(eq_lo, 1e-6)
    lo_l, hi_l = math.log10(eq_lo), math.log10(max(eq_hi, eq_lo * 1.0001))
    if hi_l - lo_l < 1e-9:
        lo_l, hi_l = lo_l - 0.3, hi_l + 0.3
    pad = (hi_l - lo_l) * 0.06
    lo_l -= pad
    hi_l += pad
    lspan = (hi_l - lo_l) or 1.0
    YE = lambda eqv: eq_bot - (math.log10(max(eqv, 1e-6)) - lo_l) / lspan * eq_h

    # Underwater domain: 0 .. worst drawdown.
    dd_min = min(p["dd"] for p in pts)          # most negative
    dd_span = abs(dd_min) or 0.01
    YU = lambda dd: uw_top + (abs(dd) / dd_span) * uw_h

    s = [svg_open(W, H), title(ml, 26, ttl)]

    # ---- header stat line (top-right) ----
    cagr = _num(report.get("cagr_pct"))
    sortino = _num(report.get("sortino_ratio"))
    maxdd = _num(report.get("max_drawdown_pct"))
    winr = _num(report.get("win_rate_pct"))
    ntr = report.get("n_trades")
    pf = _num(report.get("profit_factor"))
    hdr = []
    if cagr is not None:
        hdr.append(f"CAGR {cagr:+.1f}%")
    if sortino is not None:
        hdr.append(f"Sortino {sortino:.2f}")
    if maxdd is not None:
        hdr.append(f"maxDD {maxdd:.0f}%")
    if winr is not None:
        hdr.append(f"win {winr:.0f}%")
    if ntr is not None:
        hdr.append(f"{ntr} trades")
    if pf is not None:
        hdr.append(f"PF {pf:.2f}")
    s.append(f'<text x="{W-mr}" y="44" text-anchor="end" fill="{TEXT}" '
             f'font-size="9.5" font-weight="600" font-family={MONO!r}>'
             f'{esc("  |  ".join(hdr))}</text>')

    # ---- sub-header: validation honesty (PSR / CI) ----
    val = report.get("validation") if isinstance(report.get("validation"), dict) else {}
    psr = _num(val.get("psr_vs_zero"))
    sub = []
    if psr is not None:
        sub.append(f"PSR {psr*100:.0f}%")
    expc = _num(report.get("expectancy_pct"))
    if expc is not None:
        sub.append(f"expectancy {expc:+.1f}%/trade")
    tim = _num(report.get("time_in_market_pct"))
    if tim is not None:
        sub.append(f"time-in-mkt {tim:.0f}%")
    if val.get("anecdotal"):
        sub.append("ANECDOTAL (small sample)")
    if sub:
        col = AMBER if val.get("anecdotal") else MUTED
        s.append(f'<text x="{ml}" y="60" fill="{col}" font-size="9" '
                 f'font-family={MONO!r}>{esc("  ·  ".join(sub))}</text>')

    # ---- equity Y gridlines (decade multiples on log scale) ----
    def _decades(a, b):
        out, e = [], math.floor(a)
        while e <= math.ceil(b):
            out.append(10 ** e)
            e += 1
        return out
    for gv in _decades(lo_l, hi_l):
        gy = YE(gv)
        if eq_top - 1 <= gy <= eq_bot + 1:
            s.append(f'<line x1="{ml}" y1="{gy:.1f}" x2="{W-mr}" y2="{gy:.1f}" '
                     f'stroke="{BORDER}" stroke-width="0.5" stroke-dasharray="2 3" '
                     f'stroke-opacity="0.5"/>')
            lab = f"{gv:.0f}x" if gv >= 1 else f"{gv:g}x"
            s.append(f'<text x="{ml-6}" y="{gy+3:.1f}" text-anchor="end" '
                     f'fill="{MUTED}" font-size="8" font-family={MONO!r}>{lab}</text>')
    # 1.0x baseline (break-even) emphasized.
    if lo_l <= 0 <= hi_l:
        by = YE(1.0)
        s.append(f'<line x1="{ml}" y1="{by:.1f}" x2="{W-mr}" y2="{by:.1f}" '
                 f'stroke="{TEXT}" stroke-width="1" stroke-opacity="0.5"/>')

    # ---- buy-and-hold benchmark line (faint) ----
    bench = report.get("benchmark_hold") if isinstance(report.get("benchmark_hold"), dict) else None
    if bench:
        btot = _num(bench.get("total_return_pct"))
        if btot is not None:
            bench_eq = 1.0 + btot / 100.0
            by0, by1 = YE(1.0), YE(bench_eq)
            s.append(f'<line x1="{X(x0):.1f}" y1="{by0:.1f}" x2="{X(x1):.1f}" '
                     f'y2="{by1:.1f}" stroke="{MUTED}" stroke-width="1.2" '
                     f'stroke-dasharray="4 3" stroke-opacity="0.55"/>')
            s.append(f'<text x="{X(x1)+4:.1f}" y="{by1+3:.1f}" fill="{MUTED}" '
                     f'font-size="8" font-family={MONO!r}>'
                     f'{esc(f"hold {bench_eq:.1f}x")}</text>')

    # ---- Monte-Carlo terminal cone (faint) at the curve's end ----
    if mc_p5 is not None and mc_p95 is not None:
        cx = X(x1)
        y5 = YE(1.0 + mc_p5 / 100.0)
        y95 = YE(1.0 + mc_p95 / 100.0)
        # Fan widening from a fraction before the end to the terminal edge.
        fx = ml + plot_w * 0.62
        midy = YE(final_eq)
        s.append(f'<polygon points="{fx:.1f},{midy:.1f} {cx:.1f},{y95:.1f} '
                 f'{cx:.1f},{y5:.1f}" fill="{BLUE}" fill-opacity="0.10" '
                 f'stroke="none"/>')
        for yv, lab, pv in ((y95, "P95", mc_p95), (y5, "P5", mc_p5)):
            s.append(f'<line x1="{cx-3:.1f}" y1="{yv:.1f}" x2="{cx+3:.1f}" '
                     f'y2="{yv:.1f}" stroke="{BLUE}" stroke-width="1.4" '
                     f'stroke-opacity="0.7"/>')
            s.append(f'<text x="{cx+6:.1f}" y="{yv+3:.1f}" fill="{BLUE}" '
                     f'font-size="7.5" font-family={MONO!r} '
                     f'fill-opacity="0.85">{lab} {1.0+pv/100.0:.1f}x</text>')
        if mc_p50 is not None:
            ym = YE(1.0 + mc_p50 / 100.0)
            s.append(f'<circle cx="{cx:.1f}" cy="{ym:.1f}" r="2.2" '
                     f'fill="{BLUE}" fill-opacity="0.8"/>')

    # ---- equity curve (stepped at trade exits) ----
    path = []
    for i, p in enumerate(pts):
        x, y = X(p["x"]), YE(p["eq"])
        if i == 0:
            path.append(f"M{x:.1f},{y:.1f}")
        else:
            # step: horizontal to the new x at the previous level, then vertical.
            py = YE(pts[i - 1]["eq"])
            path.append(f"L{x:.1f},{py:.1f} L{x:.1f},{y:.1f}")
    # Area fill under the curve (subtle).
    area = path[:] + [f"L{X(pts[-1]['x']):.1f},{eq_bot:.1f}",
                      f"L{X(pts[0]['x']):.1f},{eq_bot:.1f}", "Z"]
    s.append(f'<path d="{" ".join(area)}" fill="{GREEN}" fill-opacity="0.07" '
             f'stroke="none"/>')
    s.append(f'<path d="{" ".join(path)}" fill="none" stroke="{GREEN}" '
             f'stroke-width="2" stroke-linejoin="round"/>')
    # Trade exit markers.
    for p in pts[1:]:
        col = GREEN if p["dd"] >= -1e-9 else (RED if p["dd"] < -0.001 else GREEN)
        s.append(f'<circle cx="{X(p["x"]):.1f}" cy="{YE(p["eq"]):.1f}" r="2.2" '
                 f'fill="{GREEN}" stroke="{BG}" stroke-width="0.6"/>')
    # Terminal equity label.
    s.append(f'<text x="{X(pts[-1]["x"])+4:.1f}" y="{YE(final_eq)-6:.1f}" '
             f'fill="{GREEN}" font-size="9" font-weight="600" '
             f'font-family={MONO!r}>{esc(f"{final_eq:.1f}x")}</text>')

    # Equity panel caption.
    s.append(f'<text x="{ml}" y="{eq_top-6}" fill="{CYAN}" font-size="9" '
             f'font-weight="600" font-family={MONO!r}>equity (log) — strategy</text>')

    # ---- underwater / drawdown strip ----
    s.append(f'<rect x="{ml}" y="{uw_top}" width="{plot_w}" height="{uw_h}" '
             f'fill="{BG}" fill-opacity="0.35"/>')
    s.append(f'<line x1="{ml}" y1="{uw_top}" x2="{W-mr}" y2="{uw_top}" '
             f'stroke="{BORDER}" stroke-width="0.5" stroke-opacity="0.6"/>')
    uw_path = [f"M{X(pts[0]['x']):.1f},{uw_top:.1f}"]
    for i, p in enumerate(pts):
        x = X(p["x"])
        if i > 0:
            uw_path.append(f"L{x:.1f},{YU(pts[i-1]['dd']):.1f}")
        uw_path.append(f"L{x:.1f},{YU(p['dd']):.1f}")
    uw_path.append(f"L{X(pts[-1]['x']):.1f},{uw_top:.1f} Z")
    s.append(f'<path d="{" ".join(uw_path)}" fill="{RED}" fill-opacity="0.22" '
             f'stroke="{RED}" stroke-width="1" stroke-opacity="0.75"/>')
    # Worst-drawdown marker.
    worst = min(pts, key=lambda p: p["dd"])
    worst_lab = f"{worst['dd'] * 100:.0f}%"
    s.append(f'<text x="{X(worst["x"]):.1f}" y="{YU(worst["dd"])+12:.1f}" '
             f'text-anchor="middle" fill="{RED}" font-size="8" '
             f'font-family={MONO!r}>{esc(worst_lab)}</text>')
    s.append(f'<text x="{ml}" y="{uw_top-5}" fill="{RED}" font-size="9" '
             f'font-weight="600" font-family={MONO!r}>underwater (drawdown)</text>')

    # ---- X axis date ticks (first / mid / last) ----
    from datetime import date
    def _dl(o):
        try:
            return date.fromordinal(int(o)).strftime("%Y")
        except (ValueError, OverflowError):
            return ""
    for o in (x0, (x0 + x1) // 2, x1):
        gx = X(o)
        s.append(f'<text x="{gx:.1f}" y="{uw_bot+14:.1f}" text-anchor="middle" '
                 f'fill="{MUTED}" font-size="8" font-family={MONO!r}>{_dl(o)}</text>')

    # ---- footer: MC drawdown honesty + method ----
    foot = []
    if mc:
        ddm = _num(mc.get("drawdown_median_pct"))
        dd95 = _num(mc.get("drawdown_p95_pct"))
        ploss = _num(mc.get("prob_loss_pct"))
        npaths = mc.get("n_paths")
        if ddm is not None and dd95 is not None:
            foot.append(f"MC drawdown med {ddm:.0f}% / P95 {dd95:.0f}%")
        if ploss is not None:
            foot.append(f"P(loss) {ploss:.1f}%")
        if npaths is not None:
            foot.append(f"{npaths} {mc.get('method','resample')} paths")
    if foot:
        s.append(f'<text x="{W-mr}" y="{H-6}" text-anchor="end" fill="{BLUE}" '
                 f'font-size="7.5" font-family={MONO!r} '
                 f'fill-opacity="0.8">{esc("  ·  ".join(foot))}</text>')
    else:
        s.append(f'<text x="{W-mr}" y="{H-6}" text-anchor="end" fill="{AMBER}" '
                 f'font-size="7.5" font-family={MONO!r}>'
                 f'{esc("no Monte-Carlo block in JSON — equity+drawdown only")}</text>')

    return "\n".join(s) + "\n</svg>"


# --------------------------------------------------------- orchestration / API
def _parse_arg(arg):
    """Split 'BTC?entry=...&exit=...&from=...' into (asset, kwargs-for-CLI)."""
    asset, _, qs = arg.partition("?")
    asset = unquote(asset).strip()
    extra = []
    if qs:
        q = parse_qs(qs, keep_blank_values=False)
        for key, flag in (("entry", "--entry"), ("exit", "--exit"),
                          ("stop_loss", "--stop-loss"), ("take_profit", "--take-profit"),
                          ("from", "--from"), ("to", "--to"),
                          ("commission", "--commission"), ("slippage", "--slippage")):
            if key in q and q[key]:
                extra += [flag, unquote(q[key][0])]
    return asset, extra


def render(viz_type, arg, pftui=None):
    """Render one backtest viz ('tearsheet'). '' on any failure."""
    try:
        if viz_type != "tearsheet":
            return ""
        asset, extra = _parse_arg(arg)
        if not asset:
            return ""
        cli = ["analytics", "strategy", "backtest", "--asset", asset]
        if not any(f == "--entry" for f in extra):
            # entry is REQUIRED by the CLI; with none supplied there's no backtest.
            return ""
        cli += extra
        data = pftui_json(cli, pftui)
        if not data:
            return ""
        report = data.get("report") if isinstance(data, dict) else None
        if not report:
            return ""
        tgt = data.get("resolved_symbol") or data.get("asset") or asset
        nice = _nice(tgt)
        return tearsheet(report, f"{nice} — Strategy Backtest Tearsheet")
    except Exception:  # never let a chart break a report
        return ""


# Token contract for viz/render.py: <!--BACKTEST_VIZ:tearsheet:ARG-->
# ARG = ASSET[?entry=...&exit=...&stop_loss=..&take_profit=..&from=..&to=..]
# entry is required. The payload runs to the closing `-->`, so `<`/`>` in
# condition expressions (rsi(14)<30, rsi(14)>70) are allowed verbatim, as are
# percent-encoded forms (%3C / %3E). No whitespace inside the payload.
TOKEN_RE = re.compile(r"<!--\s*BACKTEST_VIZ:([a-z]+):(\S+?)\s*-->")


def expand(md, pftui=None):
    def sub(m):
        svg = render(m.group(1), m.group(2), pftui)
        return f'<div class="backtest-viz">{svg}</div>' if svg else ""
    return TOKEN_RE.sub(sub, md)


def main(argv):
    import argparse
    p = argparse.ArgumentParser(description="Render a pftui backtest tearsheet as inline SVG.")
    p.add_argument("viz", choices=["tearsheet"])
    p.add_argument("--asset", required=True)
    p.add_argument("--entry", required=True)
    p.add_argument("--exit", default=None)
    p.add_argument("--stop-loss", default=None)
    p.add_argument("--take-profit", default=None)
    p.add_argument("--from", dest="from_", default=None)
    p.add_argument("--to", default=None)
    p.add_argument("--pftui", default=None)
    args = p.parse_args(argv)
    # Build the token-style arg so render() shares one code path.
    from urllib.parse import quote
    parts = [f"entry={quote(args.entry)}"]
    if args.exit:
        parts.append(f"exit={quote(args.exit)}")
    if args.stop_loss:
        parts.append(f"stop_loss={quote(args.stop_loss)}")
    if args.take_profit:
        parts.append(f"take_profit={quote(args.take_profit)}")
    if args.from_:
        parts.append(f"from={quote(args.from_)}")
    if args.to:
        parts.append(f"to={quote(args.to)}")
    arg = args.asset + "?" + "&".join(parts)
    svg = render(args.viz, arg, args.pftui)
    if not svg:
        sys.stderr.write(f"no backtest viz available for {args.viz}:{args.asset}\n")
        return 1
    sys.stdout.write(svg + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
