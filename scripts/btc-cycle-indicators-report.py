#!/usr/bin/env python3
"""BTC cycle-indicator report wrapper.

This is intentionally a thin wrapper around pftui's mechanical JSON surfaces.
It does not read portfolio tables or private holdings data. It shells out to
`pftui analytics ... --cached-only --json` and summarizes public market-series
cycle/indicator output.
"""

from __future__ import annotations

import argparse
import json
import math
import shutil
import subprocess
import sys
from dataclasses import dataclass
from typing import Any


DEFAULT_TIMEFRAME = "monthly"


@dataclass
class CommandResult:
    argv: list[str]
    data: dict[str, Any]


def run_json(argv: list[str]) -> CommandResult:
    proc = subprocess.run(argv, check=False, text=True, capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(argv)}\n"
            f"stderr: {proc.stderr.strip()}\nstdout: {proc.stdout.strip()}"
        )
    try:
        payload = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            f"command did not emit valid JSON: {' '.join(argv)}\n{exc}\n{proc.stdout[:500]}"
        ) from exc
    if not isinstance(payload, dict):
        raise RuntimeError(f"expected JSON object from {' '.join(argv)}")
    return CommandResult(argv=argv, data=payload)


def pct(x: Any) -> str:
    if x is None:
        return "n/a"
    try:
        return f"{float(x) * 100:.0f}%"
    except (TypeError, ValueError):
        return "n/a"


def num(x: Any, digits: int = 2) -> str:
    if x is None:
        return "n/a"
    try:
        f = float(x)
    except (TypeError, ValueError):
        return "n/a"
    if not math.isfinite(f):
        return "n/a"
    return f"{f:.{digits}f}"


def signed(x: Any, digits: int = 2) -> str:
    if x is None:
        return "n/a"
    try:
        f = float(x)
    except (TypeError, ValueError):
        return "n/a"
    if not math.isfinite(f):
        return "n/a"
    return f"{f:+.{digits}f}"


def by_key(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    return {str(row.get("key")): row for row in rows if row.get("key") is not None}


def component_map(criteria: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for criterion in criteria:
        for component in criterion.get("components", []) or []:
            key = component.get("key")
            if key:
                out[str(key)] = component
    return out


def progress_for(criterion: dict[str, Any]) -> tuple[int, int]:
    components = criterion.get("components") or []
    counted = [
        c
        for c in components
        if c.get("key") != "dss_oversold"
    ]
    total = len(counted)
    met = sum(1 for c in counted if c.get("met") is True)
    return met, total


def closeness_notes(signals: dict[str, Any], criterion: dict[str, Any]) -> list[str]:
    key = criterion.get("key")
    notes: list[str] = []
    comps = component_map([criterion])
    component = lambda name: comps.get(name, {})
    rsi = signals.get("rsi")
    rsi_ma = signals.get("rsi_ma")
    dss = signals.get("dss")
    dss_trigger = signals.get("dss_trigger")
    erf = signals.get("erf")
    cyberline = signals.get("cyberline_value")

    if key == "momentum_turning_up":
        c = component("rsi_ma_turned_up")
        notes.append(
            f"RSI-average change vs prior bar: {signed(c.get('distance_to_trigger'))} "
            f"(current {num(rsi_ma)}, previous {num(c.get('previous_value'))})"
        )
    elif key == "momentum_above_price":
        c = component("rsi_ma_cross_above_rsi")
        notes.append(
            f"RSI-average minus RSI: {signed(c.get('distance_to_trigger'))} "
            f"(current {num(rsi_ma)} vs {num(rsi)})"
        )
        notes.append(
            f"prior spread: {signed(spread(c.get('previous_value'), c.get('previous_comparison_value')))}"
        )
    elif key == "dss_bottoming":
        turn = component("dss_turned_up")
        cross = component("dss_cross_above_trigger")
        oversold = component("dss_oversold")
        notes.append(
            f"DSS change vs prior bar: {signed(turn.get('distance_to_trigger'))} "
            f"(current {num(dss)}, previous {num(turn.get('previous_value'))})"
        )
        notes.append(
            f"DSS minus trigger: {signed(cross.get('distance_to_trigger'))} "
            f"(trigger {num(dss_trigger)}, prior spread {signed(spread(cross.get('previous_value'), cross.get('previous_comparison_value')))})"
        )
        notes.append(f"oversold cushion below 20: {signed(oversold.get('distance_to_trigger'))} points")
    elif key == "roofing_confirming_up":
        zone = component("erf_bottom_zone")
        turn = component("erf_turned_up")
        notes.append(f"bottom-zone distance below zero: {signed(zone.get('distance_to_trigger'))}")
        notes.append(
            f"ERF change vs prior bar: {signed(turn.get('distance_to_trigger'))} "
            f"(current {num(erf)}, previous {num(turn.get('previous_value'))})"
        )
    elif key == "volatility_bands_bullish":
        notes.append(f"daily band state: {signals.get('cyberbands_state') or 'n/a'}")
    elif key == "reversal_dots":
        wk = signals.get("cyberdots_weekly_strength")
        mo = signals.get("cyberdots_monthly_strength")
        notes.append(f"higher-timeframe dot strength: weekly {wk if wk is not None else 'n/a'}, monthly {mo if mo is not None else 'n/a'}")
    elif key == "trend_line_reclaimed":
        c = component("cyberline_reclaim")
        price = signals.get("cyberline_price_above")
        latest = signals.get("cyberline_value")
        if latest is not None:
            notes.append(f"weekly trackline: {num(cyberline)}; price is {'above' if price else 'below'}")
        notes.append(f"price minus weekly trackline: {signed(c.get('distance_to_trigger'))}")
    return notes


def spread(a: Any, b: Any) -> float | None:
    if a is None or b is None:
        return None
    try:
        return float(a) - float(b)
    except (TypeError, ValueError):
        return None


def native_cycle_lows(cycle: dict[str, Any]) -> list[str]:
    for degree in cycle.get("degrees", []) or []:
        if degree.get("degree") == "4-year":
            return [str(low.get("date")) for low in degree.get("lows", []) or [] if low.get("date")]
    return []


def build_report(pftui: str, timeframe: str, window_days: int) -> dict[str, Any]:
    base = [pftui]
    signals = run_json(
        base
        + [
            "analytics",
            "cycles",
            "bottom-signals",
            "--asset",
            "BTC",
            "--timeframe",
            timeframe,
            "--cached-only",
            "--json",
        ]
    ).data
    backtest = run_json(
        base
        + [
            "analytics",
            "cycles",
            "bottom-signals",
            "backtest",
            "--asset",
            "BTC",
            "--timeframe",
            timeframe,
            "--window",
            str(window_days),
            "--cached-only",
            "--json",
        ]
    ).data
    cycle = run_json(base + ["analytics", "cycles", "analyze", "BTC", "--cached-only", "--json"]).data

    backtest_by_key = by_key(backtest.get("criteria", []) or [])
    criteria = []
    for criterion in signals.get("criteria", []) or []:
        key = criterion.get("key")
        met_components, total_components = progress_for(criterion)
        reliability = backtest_by_key.get(str(key), {})
        criteria.append(
            {
                "key": key,
                "label": criterion.get("label"),
                "met": criterion.get("met") is True,
                "component_progress": {
                    "met": met_components,
                    "total": total_components,
                    "pct": (met_components / total_components) if total_components else None,
                },
                "detail": criterion.get("detail"),
                "components": criterion.get("components") or [],
                "closeness": closeness_notes(signals, criterion),
                "backtest": {
                    "firings": reliability.get("firings"),
                    "hits": reliability.get("hits"),
                    "false_positives": reliability.get("false_positives"),
                    "precision": reliability.get("precision"),
                    "coverage": reliability.get("coverage"),
                    "median_lead_lag_days": reliability.get("median_lead_lag_days"),
                    "summary": reliability.get("summary"),
                },
            }
        )

    native_lows = native_cycle_lows(cycle)
    bt_anchors = [str(x) for x in backtest.get("anchors", []) or []]
    return {
        "report": "btc_cycle_indicators",
        "timeframe": timeframe,
        "as_of": signals.get("as_of"),
        "symbol": signals.get("symbol"),
        "resolved_symbol": signals.get("resolved_symbol"),
        "cycle_definition": {
            "source": "pftui analytics cycles analyze BTC, 4-year degree lows",
            "native_4y_lows": native_lows,
            "backtest_anchors": bt_anchors,
            "anchors_match_native_4y_lows": native_lows == bt_anchors,
        },
        "current": {
            "met_count": signals.get("met_count"),
            "total": signals.get("total"),
            "verdict": signals.get("verdict"),
            "core_watch": signals.get("core_watch") or [],
            "bonus": signals.get("bonus"),
        },
        "criteria": criteria,
        "backtest": {
            "window_days": backtest.get("window_days"),
            "small_n": backtest.get("small_n"),
            "headline": backtest.get("headline"),
            "caveat": backtest.get("caveat"),
            "confluence": backtest.get("confluence") or [],
        },
        "limitations": [
            "Scalar distance is exact for oscillator/line criteria that expose a trigger; categorical criteria such as band state and dot state report state/progress instead.",
            "Reliability statistics are small-N for BTC cycle lows and should be read as directional diagnostics, not probabilities.",
            "This wrapper reports the existing bottom/low suite only; pftui does not yet have a symmetric cycle-high suite.",
        ],
    }


def render_markdown(report: dict[str, Any]) -> str:
    lines: list[str] = []
    current = report["current"]
    cycle_def = report["cycle_definition"]
    backtest = report["backtest"]
    lines.append(f"# BTC Cycle Indicator Report ({report['timeframe']})")
    lines.append("")
    lines.append(f"As of: {report.get('as_of') or 'n/a'}")
    lines.append(f"Series: {report.get('resolved_symbol') or report.get('symbol') or 'n/a'}")
    lines.append(f"Current verdict: {current.get('verdict') or 'n/a'}")
    lines.append("")
    lines.append("## Cycle Definition")
    lines.append("")
    lines.append(f"Native 4-year lows: {', '.join(cycle_def['native_4y_lows']) or 'n/a'}")
    lines.append(f"Backtest anchors: {', '.join(cycle_def['backtest_anchors']) or 'n/a'}")
    lines.append(f"Anchors match native cycle lows: {'yes' if cycle_def['anchors_match_native_4y_lows'] else 'no'}")
    lines.append("")
    lines.append("## Current 7-Criterion State")
    lines.append("")
    lines.append(f"Met: {current.get('met_count')}/{current.get('total')}")
    lines.append("")
    lines.append("| Criterion | Met | Progress | Distance / closeness | Backtest accuracy |")
    lines.append("|---|---:|---:|---|---|")
    for row in report["criteria"]:
        progress = row["component_progress"]
        progress_text = f"{progress['met']}/{progress['total']}"
        closeness = "<br>".join(row["closeness"]) if row["closeness"] else row.get("detail") or ""
        bt = row["backtest"]
        lead_lag = (
            f"{bt.get('median_lead_lag_days')}d"
            if bt.get("median_lead_lag_days") is not None
            else "n/a"
        )
        bt_text = (
            f"{bt.get('hits') or 0}/{bt.get('firings') or 0} hits, "
            f"precision {pct(bt.get('precision'))}, coverage {pct(bt.get('coverage'))}, "
            f"median lead/lag {lead_lag}"
        )
        lines.append(
            f"| {row['label']} | {'yes' if row['met'] else 'no'} | {progress_text} | {closeness} | {bt_text} |"
        )
    lines.append("")
    lines.append("## Core Watch")
    lines.append("")
    for item in current.get("core_watch", []) or []:
        lines.append(
            f"- {item.get('label')}: {'met' if item.get('met') else 'not met'} "
            f"({item.get('met_components')}/{item.get('total_components')}) - {item.get('detail')}"
        )
    bonus = current.get("bonus") or {}
    if bonus:
        lines.append(
            f"- Bonus: {bonus.get('label')}: {'met' if bonus.get('met') else 'not met'} - {bonus.get('detail') or 'n/a'}"
        )
    lines.append("")
    lines.append("## Reliability Backtest")
    lines.append("")
    lines.append(backtest.get("headline") or "n/a")
    lines.append("")
    lines.append(backtest.get("caveat") or "n/a")
    lines.append("")
    lines.append("| Confluence | Firings | Hits | False | Precision | Coverage | Median lead/lag |")
    lines.append("|---|---:|---:|---:|---:|---:|---:|")
    for row in backtest.get("confluence", []) or []:
        lines.append(
            f"| {row.get('label')} | {row.get('firings')} | {row.get('hits')} | "
            f"{row.get('false_positives')} | {pct(row.get('precision'))} | "
            f"{pct(row.get('coverage'))} | {row.get('median_lead_lag_days') if row.get('median_lead_lag_days') is not None else 'n/a'}d |"
        )
    lines.append("")
    lines.append("## Limitations")
    lines.append("")
    for item in report["limitations"]:
        lines.append(f"- {item}")
    lines.append("")
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a BTC cycle-indicator report from pftui mechanical JSON outputs."
    )
    parser.add_argument("--pftui", default=shutil.which("pftui") or "pftui", help="pftui binary path")
    parser.add_argument("--timeframe", default=DEFAULT_TIMEFRAME, choices=["daily", "weekly", "monthly"])
    parser.add_argument("--window", type=int, default=90, help="cycle-low match window in days for reliability backtest")
    parser.add_argument("--json", action="store_true", help="emit wrapper JSON instead of Markdown")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.window < 1:
        print("--window must be >= 1", file=sys.stderr)
        return 2
    try:
        report = build_report(args.pftui, args.timeframe, args.window)
    except Exception as exc:  # noqa: BLE001 - command wrapper should print direct operational failure
        print(f"error: {exc}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(render_markdown(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
