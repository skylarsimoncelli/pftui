# Agent Routines

Pre-built agent routines for the pftui multi-timeframe intelligence system.

## Architecture

Four specialist timeframe analysts feed into two delivery agents. Each analyst writes structured data to the pftui database and communicates with other agents via `pftui agent-msg`. No agent reads from or writes to shared files for analytical data.

```
LOW (3x daily)  ──┐
MEDIUM (daily)  ──┤── evening-analysis (daily) ── morning-brief (daily)
HIGH (2x week)  ──┤         │
MACRO (weekly)  ──┘         └── WATCH TOMORROW ──► LOW
```

### Timeframe Agents (silent, write to DB only)

| Agent | Scope | Schedule | Domain |
|---|---|---|---|
| [low-timeframe-analyst](low-timeframe-analyst.md) | Hours to days | 3x daily | Price action, technicals, sentiment, breaking news |
| [medium-timeframe-analyst](medium-timeframe-analyst.md) | Weeks to months | Daily | Central bank policy, geopolitics, economic data, scenarios |
| [high-timeframe-analyst](high-timeframe-analyst.md) | Months to years | 2x/week | Technology disruption, structural trends, supercycles |
| [macro-timeframe-analyst](macro-timeframe-analyst.md) | Years to decades | Weekly | Empire cycles, reserve currencies, power transitions |

### Delivery Agents (message user)

| Agent | Model | Schedule | Purpose |
|---|---|---|---|
| [morning-brief](morning-brief.md) | Fast (Sonnet) | Daily AM | Concise scannable brief: prices, alignment, scorecard |
| [evening-analysis](evening-analysis.md) | Deep (Opus) | Daily PM | Cross-timeframe synthesis, prediction reflection, deep research |

## Design Principles

**Constraint flows downward, signals flow upward.** Higher timeframes constrain lower timeframes (MACRO says "late-stage empire" constrains MEDIUM's recession probability, which constrains LOW's risk-on thesis). Anomalies at lower timeframes feed evidence upward.

**Cross-timeframe tension IS the product.** When LOW says risk-on but HIGH says structural headwinds, that disagreement is the intelligence. The evening analysis synthesizes these tensions.

**Predictions are cause-and-effect, not price targets.** Every prediction must state: "[cause] will [effect] by [date]" with a confidence score. Wrong predictions require mandatory reflection.

**Each agent scores its own predictions.** No external grader. The agent that made the call reviews the outcome, writes the lesson, and adjusts.

**Database-first.** All analytical data lives in PostgreSQL via pftui CLI. No shared markdown files for quantitative data. Agents read structured JSON from `--json` flags.

## Setup

These routines are designed for use with [OpenClaw](https://github.com/openclaw/openclaw) cron jobs, but work with any agent orchestration system that can:

1. Run an LLM agent on a schedule
2. Give it shell access to `pftui` CLI
3. Route output messages to a chat channel

### Cron Configuration

Each routine file contains the full agent prompt. Point your cron job's message to the content of the routine file, prepended with any local configuration:

```
# Local pre-prompt (private, not in repo)
Database: postgres://user:pass@localhost:5432/pftui
Git author: your-name <your@email.com>
User profile: [your trading style, risk tolerance, held assets]

# Then the routine content follows
```

### Required pftui Tables

These routines use the full pftui analytics engine:
- `price_history`, `correlation_snapshots`, `regime_snapshots` (LOW)
- `scenarios`, `scenario_signals`, `scenario_history`, `thesis_history` (MEDIUM)
- `trends`, `trend_evidence`, `convictions` (HIGH)
- `structural_cycles`, `power_metrics`, `structural_outcomes`, `historical_parallels` (MACRO)
- `user_predictions`, `agent_messages`, `daily_notes` (ALL)

## Integration Guardrails

- Use `pftui notes add ... --section market` for market-close logs. Do not use `--section eod` (invalid section).
- Run all `pftui` write-back commands (predictions, notes, agent-msg, scenario/conviction updates) before sending Telegram/chat briefs.
- For notable market-close moves, send explicit handoff messaging to the evening planner flow (`pftui agent-msg send ... --from market-close --to evening-planner`).
