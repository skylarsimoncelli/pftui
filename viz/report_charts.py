#!/usr/bin/env python3
"""Insert pftui chart tokens into assembled report markdown at section anchors.

The /pftui-report pipeline assembles section markdown (via `pftui report build
daily`), then `gen-report.py` renders it to PDF — expanding `<!--FAMILY_VIZ:..-->`
tokens to inline SVG (see viz/render.py). This script is the glue between: it
scans the assembled markdown for known section headings and inserts the matching
chart token right after each, so reports get graphics WITHOUT the Rust assembler
or section prompts needing to know about charts.

Design:
- **Additive + idempotent.** A token is inserted only if that heading exists and
  the token isn't already present. Charts render to "" on missing data, so an
  inserted token is always safe — worst case it shows nothing.
- **Public-safe by construction.** Every Python `viz/` chart is market/macro
  analysis (price-history / scenarios / macro series) — none surfaces portfolio
  holdings, PnL, allocation, calibration, or conviction (those are the Rust
  `report chart` set and stay out of the public path). So this script is safe for
  the public newsletter AND the private report.

Usage:
    python viz/report_charts.py REPORT.md            # edit in place
    python viz/report_charts.py REPORT.md --dry-run  # print, don't write
    python viz/report_charts.py --self-test          # no file; run the smoke test
"""
import re
import sys

# (heading regex, [tokens]) — first matching heading per group gets the tokens
# inserted on their own lines right after it. Order doesn't matter (each is
# independent). Headings come from the public newsletter + deep-dive structure.
RULES = [
    (r"^##\s+Macro\b", ["<!--MACRO_VIZ:environment:-->", "<!--RATES_VIZ:realrates:-->"]),
    (r"^##\s+Bitcoin\b", ["<!--CYCLE_VIZ:map:BTC-->", "<!--CYCLE_VIZ:dial:BTC-->"]),
    (r"^##\s+Gold\b", ["<!--CYCLE_VIZ:map:GC=F-->", "<!--CYCLE_VIZ:dial:GC=F-->"]),
    (r"^##\s+(News\s*&\s*Catalysts|Catalysts)\b", ["<!--MACRO_VIZ:catalysts:-->"]),
    (r"^##\s+Scenario\s+Dashboard\b", ["<!--SCENARIO_VIZ:dashboard:-->"]),
]


def inject(md):
    """Return md with chart tokens inserted after their section headings.
    Idempotent: never inserts a token that's already present anywhere in md."""
    lines = md.split("\n")
    out = []
    for line in lines:
        out.append(line)
        for pat, tokens in RULES:
            if re.match(pat, line.strip()):
                fresh = [t for t in tokens if t not in md]
                if fresh:
                    out.append("")
                    out.extend(fresh)
                break
    return "\n".join(out)


def _self_test():
    sample = "\n".join([
        "# Daily", "", "## Macro", "macro prose", "", "## Bitcoin", "btc prose",
        "", "## Gold (and Precious Metals)", "gold prose", "", "## News & Catalysts",
        "### Tomorrow's Calendar", "", "## Scenario Dashboard", "scn prose",
    ])
    out = inject(sample)
    checks = [
        ("macro env after Macro", "<!--MACRO_VIZ:environment:-->" in out),
        ("real-rates after Macro", "<!--RATES_VIZ:realrates:-->" in out),
        ("cycle map after Bitcoin", "<!--CYCLE_VIZ:map:BTC-->" in out),
        ("gold map after Gold", "<!--CYCLE_VIZ:map:GC=F-->" in out),
        ("catalysts after News", "<!--MACRO_VIZ:catalysts:-->" in out),
        ("scenario after Dashboard", "<!--SCENARIO_VIZ:dashboard:-->" in out),
        ("idempotent (no double-insert)", inject(out).count("<!--CYCLE_VIZ:map:BTC-->") == 1),
        ("prose preserved", "btc prose" in out and "gold prose" in out),
    ]
    ok = all(c for _, c in checks)
    for name, c in checks:
        print(f"  {'ok  ' if c else 'FAIL'} {name}")
    print("PASS" if ok else "FAILED")
    return 0 if ok else 1


def main(argv):
    if "--self-test" in argv:
        return _self_test()
    args = [a for a in argv if not a.startswith("-")]
    if not args:
        sys.stderr.write("usage: report_charts.py REPORT.md [--dry-run]\n")
        return 2
    path = args[0]
    with open(path) as f:
        out = inject(f.read())
    if "--dry-run" in argv:
        sys.stdout.write(out)
    else:
        with open(path, "w") as f:
            f.write(out)
        sys.stderr.write(f"[report_charts] inserted chart tokens into {path}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
