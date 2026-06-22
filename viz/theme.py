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


def title(x, y, text, color=CYAN, size=13):
    return (f'<text x="{x}" y="{y}" fill="{color}" font-size="{size}" '
            f'font-weight="600" font-family={MONO!r}>{esc(text)}</text>')


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
