# AI Layer

## Purpose

The AI Layer is how agents operate pftui as an intelligence system, not just a quote checker.

It sits on top of:

1. Data Aggregation Engine
2. Local Database (SQLite or PostgreSQL)
3. Analytics Engine (LOW/MEDIUM/HIGH/MACRO)

Agents consume and write through CLI commands with `--json`, using the same state the human sees in TUI/web.

## Core Workflow

Typical loop:

```bash
pftui refresh
pftui brief --agent --json
```

From there, agents can:

- update scenarios, convictions, thesis
- monitor movers, drift, alerts
- log decisions and evidence
- publish daily/weekly briefs

## Design Principles

- Human remains decision-maker.
- Agent handles monitoring, synthesis, and execution of routine analysis.
- All outputs are auditable because they land in the same persistent system.
- No hidden cloud state; database is user-owned.

## Command Surface

The AI layer relies on stable JSON-first commands, including:

- `brief`, `summary`, `value`, `performance`
- `macro`, `movers`, `sentiment`, `predictions`, `news`, `status`
- `scenario`, `thesis`, `conviction`, `question`, `predict`
- `journal`, `notes`, `agent-msg`

## Multi-Agent Pattern

You can split responsibilities across agents:

- market monitor (refresh + alerts)
- macro analyst (scenario/thesis updates)
- execution planner (drift/rebalance suggestions)
- historian (journal + prediction scoring)

All agents coordinate via one data model and one command surface.

## Practical Deployment

- Local interactive sessions (Codex/Claude/OpenClaw)
- Scheduled cron-based routines
- Server mode with `pftui web` + authenticated API

Start with one daily morning routine, then expand to market-close and weekly review passes.
