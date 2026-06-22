#!/usr/bin/env python3
"""Token aggregator for the pftui report pipeline.

`gen-report.py` calls `expand_tokens(md)` once, just before markdown->HTML. Each
visualization module (`*_viz.py` in this dir) exposes an `expand(md, pftui)` that
swaps its own tokens for inline SVG. This file AUTO-DISCOVERS every such module,
so adding a new chart family is just dropping a `foo_viz.py` with an `expand()` —
no edits here.

Token convention: `<!--<FAMILY>_VIZ:<type>:<arg>-->` (e.g. `<!--CYCLE_VIZ:map:BTC-->`).
Any token whose data is unavailable expands to an empty string, so report
visualizations are always additive and never load-bearing.
"""
import glob
import importlib
import os
import sys

_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, _DIR)


def _discover():
    """Import every viz/*_viz.py module exposing an `expand` callable."""
    expanders = []
    for path in sorted(glob.glob(os.path.join(_DIR, "*_viz.py"))):
        name = os.path.splitext(os.path.basename(path))[0]
        try:
            mod = importlib.import_module(name)
        except Exception as e:  # a broken module must not kill the report
            sys.stderr.write(f"[viz] skipped {name}: {e}\n")
            continue
        fn = getattr(mod, "expand", None)
        if callable(fn):
            expanders.append(fn)
    return expanders


def expand_tokens(md, pftui=None):
    for expand in _discover():
        md = expand(md, pftui)
    return md


if __name__ == "__main__":
    # Filter mode: stdin markdown -> stdout markdown with viz tokens expanded.
    sys.stdout.write(expand_tokens(sys.stdin.read()))
