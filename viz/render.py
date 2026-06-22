#!/usr/bin/env python3
"""Token aggregator for the pftui report pipeline.

`gen-report.py` calls `expand_tokens(md)` once, just before markdown->HTML. Each
visualization module exposes an `expand(md, pftui)` that swaps its own tokens for
inline SVG; this file chains them. To add a new chart family, write a module with
an `expand()` and append it to `EXPANDERS`.

Token convention: `<!--<FAMILY>_VIZ:<type>:<arg>-->` (e.g. `<!--CYCLE_VIZ:map:BTC-->`).
Any token whose data is unavailable expands to an empty string, so report
visualizations are always additive and never load-bearing.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cycle_viz  # noqa: E402

# Each entry is a callable (md, pftui) -> md with its tokens expanded.
EXPANDERS = [
    cycle_viz.expand,
]


def expand_tokens(md, pftui=None):
    for expand in EXPANDERS:
        md = expand(md, pftui)
    return md


if __name__ == "__main__":
    # Filter mode: stdin markdown -> stdout markdown with viz tokens expanded.
    sys.stdout.write(expand_tokens(sys.stdin.read()))
