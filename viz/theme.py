"""Shared brand theme + the Rust data boundary for pftui report visualizations.

Architecture (see viz/README.md): **Rust computes, Python draws.** Every chart's
data comes from the hardened `pftui ... --json` CLI (rust_decimal, cargo-tested);
these modules only render that JSON to inline SVG for the markdown -> WeasyPrint
report pipeline. No matplotlib/cairosvg needed — SVG is vector, crisp, themeable,
and passes through python-markdown untouched.

Keep the palette in sync with agents/intelligence-report/gen-report.py CSS.
"""
import json
import shutil
import subprocess
from datetime import datetime

# ---- report brand palette (mirrors gen-report.py) ----
BG = "#0d1117"
PANEL = "#161b22"
BORDER = "#30363d"
TEXT = "#c9d1d9"
MUTED = "#8b949e"
CYAN = "#89dceb"
GREEN = "#a6e3a1"
BLUE = "#89b4fa"
RED = "#f38ba8"
AMBER = "#f9e2af"
MONO = "'JetBrains Mono', monospace"
SANS = "'Inter', sans-serif"

# ---- shared presentation tokens (one source of truth across all charts) ----
# Title sizing: ONE convention. Panel titles are 13px/600; the dial variant is
# centered but the SAME size so every chart header reads identically.
TITLE_SIZE = 13
CAPTION_SIZE = 8.5      # the bottom-left meta/footer slot, in MUTED
# Named opacity tokens (consolidates the scattered 0.85 / 0.55 / 0.35 / 0.10).
OP_FILL_STRONG = 0.85   # a value bar / live element at full read
OP_FILL_SOFT = 0.55     # a secondary / live-in-progress element
OP_TRACK = 0.35         # an empty bar track / rail behind a value
OP_WASH = 0.10          # a shaded zone/band wash


def esc(s):
    """XML-escape text for embedding in SVG."""
    return str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def d2o(s):
    """ISO date string -> ordinal day (for time axes)."""
    return datetime.strptime(s, "%Y-%m-%d").date().toordinal()


def svg_open(w, h):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" '
            f'font-family="{SANS}">'
            f'<rect x="0" y="0" width="{w}" height="{h}" rx="8" fill="{PANEL}" stroke="{BORDER}"/>')


def title(x, y, text, color=CYAN, size=TITLE_SIZE):
    """The ONE panel-title convention: 13px/600 mono, left-anchored at (x, y)."""
    return (f'<text x="{x}" y="{y}" fill="{color}" font-size="{size}" '
            f'font-weight="600" font-family={MONO!r}>{esc(text)}</text>')


def title_centered(cx, y, text, color=CYAN, size=TITLE_SIZE):
    """Centered variant of `title()` (for the dial gauges) — SAME size/weight as
    every other chart's title so headers read uniformly."""
    return (f'<text x="{cx}" y="{y}" text-anchor="middle" fill="{color}" '
            f'font-size="{size}" font-weight="600" font-family={MONO!r}>{esc(text)}</text>')


def caption(x, y, text, color=MUTED, size=CAPTION_SIZE, anchor="start"):
    """The ONE caption/footer slot: MUTED, 8.5px mono. Route every chart's meta
    line through this so the footer reads identically everywhere. `anchor` may be
    'start' (bottom-left, the default) or 'end' (bottom-right)."""
    a = '' if anchor == "start" else f' text-anchor="{anchor}"'
    return (f'<text x="{x}" y="{y}"{a} fill="{color}" font-size="{size}" '
            f'font-family={MONO!r}>{esc(text)}</text>')


def ramp(v):
    """Semantic 0..1 ramp GREEN -> AMBER -> RED. Low = good/diversifying/calm,
    high = bad/danger. The single source of truth for 'green = good' across the
    library (the cocrash λ_L ramp in risk_viz mirrors this exactly)."""
    v = max(0.0, min(1.0, float(v)))
    if v <= 0.5:
        return _lerp_hex(GREEN, AMBER, v / 0.5)
    return _lerp_hex(AMBER, RED, (v - 0.5) / 0.5)


# Named risk-band thresholds, so "green = survivable" is identical everywhere.
RUIN_OK, RUIN_WATCH = 0.15, 0.50


def good_bad(v, ok, watch):
    """Map a 0..1 risk value to GREEN/AMBER/RED by two named thresholds:
    v < ok -> GREEN, v < watch -> AMBER, else RED."""
    v = float(v)
    return GREEN if v < ok else (AMBER if v < watch else RED)


def _lerp_hex(c0, c1, t):
    """Linear-interpolate two #rrggbb colors; t in [0,1] -> #rrggbb."""
    t = max(0.0, min(1.0, t))
    a = tuple(int(c0[i:i + 2], 16) for i in (1, 3, 5))
    b = tuple(int(c1[i:i + 2], 16) for i in (1, 3, 5))
    return "#" + "".join(f"{round(a[i] + (b[i] - a[i]) * t):02x}" for i in range(3))


def pftui_bin():
    return shutil.which("pftui") or "pftui"


def pftui_json(args, pftui=None):
    """Run `pftui --cached-only <args> --json` and parse stdout.

    Returns the parsed object, or None on ANY failure (nonzero exit, empty
    output, unparseable JSON, or the uniform {"error":{...}} envelope). Callers
    treat None as "no data" so a chart degrades to nothing rather than breaking
    a report.
    """
    bin_ = pftui or pftui_bin()
    try:
        out = subprocess.run(
            [bin_, "--cached-only", *args, "--json"],
            capture_output=True, text=True, timeout=60,
        )
        if out.returncode != 0 or not out.stdout.strip():
            return None
        data = json.loads(out.stdout)
        if isinstance(data, dict) and "error" in data:
            return None
        return data
    except (subprocess.SubprocessError, json.JSONDecodeError, OSError):
        return None
