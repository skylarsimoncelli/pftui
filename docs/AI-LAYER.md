# AI Layer

## Purpose

The AI Layer is how agents operate pftui as an intelligence system, not just a quote checker.

It sits on top of:

1. Data Aggregation Engine
2. Local Database (SQLite or PostgreSQL)
3. Analytics Engine (LOW/MEDIUM/HIGH/MACRO)

Agents consume and write through CLI commands with `--json`, using the same state the human sees in TUI/web.

The architectural direction is now Rust-first: more of the analytical reasoning lives in the Analytics Engine and database, while agents consume canonical analytics payloads and add judgment, context, and communication.

## Core Workflow

Typical loop:

```bash
pftui data refresh
pftui portfolio brief --agent --json
```

From there, agents can:

- update scenarios, convictions, thesis
- monitor movers, drift, alerts
- log decisions and evidence
- publish daily/weekly briefs
- consume shared `situation`, `deltas`, `catalysts`, `impact`, `opportunities`, `synthesis`, and `narrative` payloads instead of recomputing them ad hoc

## Design Principles

- Human remains decision-maker.
- Agent handles monitoring, judgment, synthesis of external context, and execution of routine analysis.
- All outputs are auditable because they land in the same persistent system.
- No hidden cloud state; database is user-owned.
- Ranking, delta detection, and cross-timeframe state should prefer native analytics outputs over prompt-only logic.

## Command Surface

The AI layer relies on stable JSON-first commands, including:

- `brief`, `summary`, `value`, `performance`
- `macro`, `movers`, `sentiment`, `predictions`, `news`, `status`
- `scenario`, `thesis`, `conviction`, `question`, `predict`
- `journal`, `notes`, `agent-msg`
- `analytics situation`, `deltas`, `catalysts`, `impact`, `opportunities`, `synthesis`, `narrative`

## Rust-First Split

What belongs in the Analytics Engine:

- canonical "what matters now" ranking
- change detection across monitoring windows
- portfolio-impact scoring
- catalyst ranking and countdowns
- cross-timeframe alignment / divergence / constraint state
- structured recap and analytical memory

What belongs in the AI layer:

- external research and source triangulation
- interpretation when evidence is ambiguous
- escalation and operator messaging
- concise briefs and deep narrative synthesis
- identifying where human attention is required

## Multi-Agent Pattern

You can split responsibilities across agents:

- market monitor (refresh + alerts)
- macro analyst (scenario/thesis updates)
- execution planner (drift/rebalance suggestions)
- historian (journal + prediction scoring)

All agents coordinate via one data model and one command surface.

The preferred pattern is:

1. `pftui` computes facts, ranks, deltas, and state transitions.
2. Agents consume those payloads.
3. Agents write back decisions, updated probabilities, predictions, notes, and messages.

## Investor Panel Skill

For multi-lens macro interpretation, use the shipped skill scaffold at:

- `agents/investor-panel/SKILL.md`
- `agents/investor-panel/collect-data.sh`
- `agents/investor-panel/schema.json`
- `agents/investor-panel/personas/`

This pattern runs multiple investor personas against the same `pftui` JSON payload
and aggregates consensus/divergence. It is analysis-only and does not execute trades.

## Practical Deployment

- Local interactive sessions (Codex/Claude/OpenClaw)
- Scheduled cron-based routines
- Server mode with `pftui system web` + authenticated API

Start with one daily morning routine, then expand to market-close and weekly review passes.
