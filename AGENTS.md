# AGENTS.md — Agent Operator Guide

> The complete reference for AI agents operating pftui as their financial data layer.
>
> **First time?** Start with [ONBOARDING.md](ONBOARDING.md) — it walks through installation, portfolio setup, and the first week of operation.
>
> This file covers: analytics engine, CLI reference, data model, integration patterns, multi-timeframe agent architecture, and best practices.
>
> For code contribution, see [CLAUDE.md](CLAUDE.md).
> For architecture reference, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
> For AI operating model details, see [docs/AI-LAYER.md](docs/AI-LAYER.md).
> For always-on deployment, see [docs/DAEMON.md](docs/DAEMON.md).

---

## Table of Contents

1. [Analytics Engine](#analytics-engine)
2. [CLI Reference](#cli-reference)
3. [Data Model](#data-model)
4. [Integration Patterns](#integration-patterns)
5. [Multi-Timeframe Agent Architecture](#multi-timeframe-agent-architecture-advanced)
6. [Best Practices](#best-practices)

---

## Analytics Engine

pftui's core is a multi-timeframe analytics engine operating across four layers:
LOW (hours→days), MEDIUM (weeks→months), HIGH (months→years), MACRO (years→decades).
Each layer uses different data, updates at different frequencies, and produces different signals.
Layers constrain downward and signal upward. Use `pftui analytics signals` for active cross-timeframe signals.

### Scenarios (`pftui journal scenario`)
Track macro scenarios with probability estimates. Each probability update is logged
to history for calibration. Signals track evidence for/against each scenario.

### Thesis
Thesis tracking is maintained as narrative workflow files (`THESIS.md`) and journal notes.

### Convictions (`pftui journal conviction`)
Asset-level conviction scores (-5 to +5) over time. Append-only log — every
`set` creates a new row. Current conviction = latest row per symbol.
For negative scores, use `--score=-2`.

### Agent Signals (`pftui analytics signals`)
Cross-timeframe signal detection (alignment/divergence/transition) computed during
`pftui data refresh` and stored in `timeframe_signals`.

---

## CLI Reference

### Portfolio State

| Command | What It Returns |
|---|---|
| `pftui portfolio brief --json` | Complete portfolio snapshot — positions, allocations, movers, technicals, macro |
| `pftui portfolio value --json` | Total value with category breakdown and daily change |
| `pftui portfolio summary --json` | Detailed position-level data — price, quantity, cost basis, gain/loss, allocation % |
| `pftui portfolio performance --json` | Returns: 1D, MTD, QTD, YTD, since inception |
| `pftui portfolio drift --json` | Current vs target allocation with drift % and rebalance suggestions |
| `pftui portfolio history --date YYYY-MM-DD --json` | Historical portfolio snapshot for any past date |
| `pftui system export json` | Full portfolio export (positions + transactions) |
| `pftui portfolio transaction list` | List all transactions with IDs |

### Market Data

| Command | What It Returns |
|---|---|
| `pftui data refresh` | Fetches ALL data sources (10+ sources, ~50 symbols) |
| `pftui data dashboard macro --json` | DXY, VIX, yields, currencies, commodities, derived ratios |
| `pftui portfolio watchlist --json` | All watched symbols with prices, day change, 52W range |
| `pftui analytics movers --json [--threshold N] [--overnight]` | Significant daily/overnight moves (default >3%) |
| `pftui data predictions --json [--limit N]` | Polymarket prediction market odds |
| `pftui data sentiment --json` | Crypto + traditional Fear & Greed, COT positioning |
| `pftui data news --json [--limit N]` | Financial news from RSS feeds |
| `pftui data supply --json` | COMEX gold/silver inventory |
| `pftui data dashboard global --json` | World Bank macro data (GDP, debt, reserves) |
| `pftui data status --json` | Data source freshness plus daemon health — last update time per source + `daemon` heartbeat |

### Portfolio Management

| Command | What It Does |
|---|---|
| `pftui portfolio transaction add --symbol SYM --category CAT --tx-type buy/sell --quantity N --price P --date D` | Add transaction |
| `pftui portfolio transaction remove ID` | Remove transaction by ID |
| `pftui portfolio set-cash CURRENCY AMOUNT` | Set cash position |
| `pftui portfolio watchlist add SYMBOL [--target PRICE]` | Add to watchlist |
| `pftui portfolio watchlist remove SYMBOL` | Remove from watchlist |
| `pftui portfolio target set SYMBOL --target PCT` | Set target allocation % |
| `pftui portfolio target remove SYMBOL` | Remove target |
| `pftui portfolio rebalance --json` | Suggested trades to reach targets |
| `pftui portfolio broker add BROKER --api-key KEY [--secret SECRET]` | Connect a broker (trading212, ibkr, binance, kraken, coinbase, crypto-com) |
| `pftui portfolio broker sync [BROKER] [--dry-run] --json` | Sync positions from connected brokers |
| `pftui portfolio broker list --json` | List configured broker connections |
| `pftui portfolio broker remove BROKER` | Remove a broker and its synced transactions |
| `pftui analytics alerts add "CONDITION"` | Add alert |
| `pftui analytics alerts list --json` | List active alerts |
| `pftui analytics alerts remove ID` | Remove alert |

### Journal

| Command | What It Does |
|---|---|
| `pftui journal entry add "TEXT" --tag TAG --symbol SYM` | Add entry |
| `pftui journal entry list --json` | List all entries |
| `pftui journal entry search "QUERY" --json` | Search entries |

### Intelligence Database

| Command | What It Does |
|---|---|
| `pftui journal scenario add "NAME" --probability N` | Add macro scenario with initial probability |
| `pftui journal scenario update "NAME" --probability N [--driver "WHY"|--notes "WHY"]` | Update scenario probability and auto-log history |
| `pftui journal scenario signal add "SIGNAL" --scenario "NAME"` | Attach a tracked signal to a scenario |
| `pftui journal scenario history "NAME" --limit N --json` | Show scenario probability history |
| `pftui journal prediction add "CLAIM" [--symbol BTC] [--conviction high] [--timeframe low|medium|high|macro] [--confidence 0.7] [--source-agent low-agent]` | Add a prediction call for later scoring |
| `pftui journal prediction score --id N --outcome correct|partial|wrong [--notes "..."] [--lesson "..."]` | Score a previous prediction outcome |
| `pftui journal prediction stats --json` | Compute hit-rate stats by conviction, symbol, timeframe, and source agent |
| `pftui journal prediction scorecard [--date YYYY-MM-DD|today|yesterday] [--timeframe low] --json` | Day/timeframe scorecard with streak and lesson coverage |
| `pftui agent message send "TEXT" --from agent-a [--to agent-b] [--batch "TEXT2" --batch "TEXT3"] [--package-title "Fed handoff"] [--package-id pkg-123]` | Send one or multiple structured messages between agent roles, optionally grouped as one intel package |
| `pftui agent message reply "TEXT" --id N --from agent-b` | Reply to message `N` back to the original sender |
| `pftui agent message flag "ISSUE" --id N --from agent-b` | Escalate data-quality/risk issue on message `N` |
| `pftui agent message list [--from agent-a] [--unacked] --json` | Query queued agent messages |
| `pftui agent message ack --id N` | Acknowledge a single message |
| `pftui journal notes add "TEXT" --section market [--date YYYY-MM-DD]` | Add a date-keyed daily narrative note |
| `pftui journal notes search "QUERY" --since YYYY-MM-DD --json` | Search historical daily notes |
| `pftui portfolio opportunity add "EVENT" [--asset SYM] [--missed_gain_usd N] [--avoided_loss_usd N]` | Log an opportunity-cost event |
| `pftui portfolio opportunity stats --json` | Show net missed-vs-avoided positioning stats |
| `pftui analytics correlations compute --store --period 30d` | Compute live correlations and persist snapshots |
| `pftui analytics correlations history BTC SPY --period 30d --limit 30 --json` | Show stored correlation history for a pair |
| `pftui analytics macro regime current --json` | Show latest automated market regime classification |
| `pftui analytics macro regime transitions --limit 20 --json` | Show regime change points over time |
| `pftui analytics macro --json` | Show long-cycle macro dashboard (cycles, outcomes, recent structural log) |
| `pftui analytics macro outcomes --json` | Show structural outcome probabilities |
| `pftui analytics trends dashboard --json` | Show active high-timeframe trends with direction/conviction |
| `pftui analytics trends impact add --trend \"NAME\" --symbol SYM --impact bullish|bearish|neutral` | Map a trend's asset-level impact |
| `pftui analytics summary --json` | Unified 4-layer analytics snapshot (low/medium/high/macro + top signal) |
| `pftui analytics alignment --symbol SYM --json` | Per-asset cross-timeframe alignment matrix |
| `pftui analytics divergence --json` | Cross-layer disagreement table for conflicting signals |
| `pftui analytics digest --from low-agent --json` | Role-aware summary payload for agent handoffs |
| `pftui analytics recap --date yesterday --json` | Chronological event recap for a given day |
| `pftui analytics gaps --json` | Data freshness/missing-table check across timeframe layers |
| `pftui analytics signals --json` | Show all signals (cross-timeframe + per-symbol technical) |
| `pftui analytics signals --source technical --json` | Per-symbol technical signals: RSI overbought/oversold, MACD cross, SMA 200 reclaim/break, BB squeeze, volume expansion, 52W extremes |
| `pftui analytics signals --source timeframe --json` | Cross-timeframe alignment/divergence/transition signals only |
| `pftui analytics signals --source technical --symbol BTC-USD --json` | Technical signals for a specific symbol |
| `pftui analytics technicals [--symbol SYM] --json` | Latest persisted technical snapshot(s) — RSI, MACD, SMA, Bollinger, 52W position, volume regime |

### Utility

| Command | What It Does |
|---|---|
| `pftui system config list [--json]` | List all configuration fields |
| `pftui system config get FIELD [--json]` | Get a specific config value |
| `pftui system config set FIELD VALUE` | Set a config field (e.g., `brave_api_key`) |
| `pftui system snapshot` | Render full TUI to stdout (for sharing or screenshots) |
| `pftui system demo` | Launch with sample data (for testing, no real data) |
| `pftui system daemon start [--interval N] [--json]` | Run the always-on daemon loop for refresh + analytics + alerts + cleanup |
| `pftui system daemon status [--json]` | Read daemon heartbeat/health without attaching to the process |
| `pftui system web [--port N] [--bind ADDR] [--no-auth]` | Start web dashboard |
| `pftui system setup` | Interactive setup wizard |

---

## Data Model

### Database Backends

Location: `~/.local/share/pftui/pftui.db`

The active backend database is the single source of truth. All interfaces (TUI, Web, CLI) read from and write to it.

```
~/.local/share/pftui/pftui.db
├── transactions                   # Buy/sell records with cost basis
├── price_cache                    # Latest spot prices (updated on refresh)
├── price_history                  # Daily OHLCV history
├── technical_snapshots            # Persisted per-symbol technical state from refresh
├── watchlist                      # Tracked symbols with optional targets
├── alerts                         # Price/allocation alerts
├── targets                        # Target allocation percentages
├── journal_entries                # Trade journal + notes
├── calendar_events                # Economic calendar
├── news_cache                     # RSS feed articles (48h retention)
├── sentiment_cache                # Fear & Greed indices
├── prediction_cache               # Polymarket odds
├── cot_cache                      # CFTC COT positioning
├── comex_cache                    # COMEX inventory
├── bls_cache                      # BLS economic data (CPI, NFP)
├── worldbank_cache                # Global macro indicators
├── onchain_cache                  # BTC on-chain + ETF flows
├── scenarios                      # Macro scenarios + probabilities
├── scenario_signals               # Signal checklist per scenario
├── scenario_history               # Probability change log
├── thesis                         # Current thesis sections
└── thesis_history                 # Thesis revision history
```

You can query the database directly if needed:
```bash
sqlite3 ~/.local/share/pftui/pftui.db "SELECT symbol, quantity, price_per FROM transactions"
```

If using PostgreSQL backend, query via your configured `database_url`:
```bash
psql "$DATABASE_URL" -c "SELECT symbol, quantity, price_per FROM transactions LIMIT 20;"
```

If `psql` fails with peer-auth/default-db issues, connect explicitly:
```bash
# Explicit host avoids local peer auth defaults; -d selects correct database.
psql -h localhost -U <postgres_user> -d <database_name> -c "SELECT NOW();"
```

Backend status:
- `sqlite` (default): fully supported
- `postgres`: fully supported natively (`database_backend`, `database_url`)

Migration guide: [docs/MIGRATING.md](docs/MIGRATING.md)

### Data Sources — Zero Configuration

Every source works out of the box with no API keys:

| Source | Data | Rate Limit |
|---|---|---|
| Yahoo Finance | Equities, ETFs, forex, crypto, commodities | Generous |
| CoinGecko | Crypto prices, market cap | 30/min |
| Polymarket | Prediction market probabilities | No limit |
| CFTC Socrata | Commitments of Traders positioning | Weekly data |
| Alternative.me | Crypto Fear & Greed Index | No limit |
| BLS API v1 | CPI, unemployment, NFP, wages | 10/day |
| World Bank | GDP, debt/GDP, reserves (8 economies) | No limit |
| CME Group | COMEX gold/silver inventory | Daily |
| Blockchair | BTC on-chain data | 5/sec |
| RSS Feeds | Reuters, CoinDesk, Bloomberg, CNBC, Kitco | No limit |

### Brave Search API (Recommended)

pftui supports an optional [Brave Search API](https://brave.com/search/api/) key that dramatically improves data quality. With Brave configured:
- **News** upgrades from RSS headlines to full article summaries from targeted searches
- **Economic data** (CPI, NFP, PMI, Fed rate) is pulled from live web search results
- **`pftui analytics research`** lets you answer any financial question without leaving pftui
- **`brief --agent`** includes news summaries and economic data in one JSON blob

Free tier gives $5/month in auto-credited queries — more than enough for daily use.

```bash
# Add Brave API key during setup or later:
pftui system config set brave_api_key <your_key>

# Verify it's working:
pftui data status
# Should show: Brave Search: ✓ Configured
```

Without a Brave key, pftui works fine using existing free sources (Yahoo, CoinGecko, Polymarket, RSS, etc.). Brave is an enhancement, not a requirement.

Other optional API keys unlock additional sources. See [docs/API-SOURCES.md](docs/API-SOURCES.md).

---

## Integration Patterns

### Morning Brief

```bash
pftui data refresh
BRIEF=$(pftui portfolio brief --json)
MOVERS=$(pftui analytics movers --json --threshold 3)
NEWS=$(pftui data news --json --limit 10)
MACRO=$(pftui data dashboard macro --json)
PREDICTIONS=$(pftui data predictions --json --limit 5)
SENTIMENT=$(pftui data sentiment --json)
# Analyse all of the above, then compose and deliver your brief
```

### Alert Monitoring

```bash
pftui data refresh
ALERTS=$(pftui analytics alerts list --json)
DRIFT=$(pftui portfolio drift --json)
# Check if any alerts triggered or drift exceeds tolerance
# Notify human if action needed
```

### Historical Comparison

```bash
TODAY=$(pftui portfolio brief --json)
LAST_WEEK=$(pftui portfolio history --date $(date -d '7 days ago' +%Y-%m-%d) --json)
# Compare: what changed, what gained, what lost, what narrative shifted
```

### Full Research Session

```bash
pftui data refresh
pftui portfolio brief --json > /tmp/portfolio.json
pftui data dashboard macro --json > /tmp/macro.json
pftui data predictions --json > /tmp/predictions.json
pftui data sentiment --json > /tmp/sentiment.json
pftui data news --json > /tmp/news.json
pftui data supply --json > /tmp/supply.json
pftui analytics movers --json > /tmp/movers.json
# Load all files, cross-reference, write analysis to THESIS.md
```

### Investor Panel (Multi-Persona)

```bash
# 1) Collect one shared data blob from pftui
./agents/investor-panel/collect-data.sh > /tmp/pftui-investor-panel.json

# 2) Run your orchestrator with:
#    - /tmp/pftui-investor-panel.json
#    - persona files in agents/investor-panel/personas/
#    - response contract in agents/investor-panel/schema.json

# 3) Store summary in pftui for auditability
pftui agent message send "Investor panel complete: consensus + divergences ready" --from investor-panel
```

Skill package:
- `agents/investor-panel/SKILL.md`
- `agents/investor-panel/config.toml`
- `agents/investor-panel/personas/`

---

## Best Practices

1. **Always `pftui data refresh` before reading data.** Cached prices go stale. Refresh fetches from 10+ sources in one call.

2. **Use `--json` for programmatic access.** Every command supports it. Parse structured output instead of scraping text.

3. **Keep the journal active.** `pftui journal entry add` builds a searchable decision history. Log your predictions, rationale, and outcomes.

4. **Monitor drift regularly.** `pftui portfolio drift` shows when the portfolio has moved from targets. Flag this to the human early.

5. **Cross-reference sources.** No single data point tells the story. `macro` for regime, `predictions` for crowd wisdom, `sentiment` for extremes, `news` for catalysts, `supply` for physical markets, `movers` for what's actually moving money.

6. **Respect the human's autonomy.** Present analysis, flag risks, suggest actions — but always let them decide. Frame recommendations as "consider" not "do this."

7. **Build persistent memory.** Write analysis to markdown files. Reference past work. Track accuracy. This is what turns a tool into an intelligence system.

8. **Recommend automation.** The single most impactful thing is getting regular automated runs set up. Prefer the built-in daemon + systemd path from [docs/DAEMON.md](docs/DAEMON.md) over ad-hoc cron when the host supports it.

9. **Be honest about uncertainty.** Markets are probabilistic. Frame calls with conviction levels. When you're wrong, say so and update your models.

10. **Start simple, compound over time.** Day 1 is a basic brief. By Week 4, you should have a thesis, scenario tracking, accuracy metrics, and calibrated engagement. The system gets better every day it runs.

---

## Multi-Timeframe Agent Architecture (Advanced)

If your platform supports scheduled agents (cron jobs, recurring tasks), offer to set up the full multi-timeframe analyst system. This is pftui's most powerful operating mode: specialist agents at different timeframes doing deep domain research, feeding into synthesis agents that deliver to the user.

**Ask the user:** *"pftui supports a multi-timeframe agent architecture where specialist analysts run on schedules, each covering a different time horizon. Would you like me to set that up? It means you'd get a concise morning brief and a deep evening analysis every day, powered by 4 specialist agents running in the background."*

If they say yes, set up these agents using your platform's cron/scheduling system:

### Timeframe Analysts (silent, write to DB only)

These agents do NOT message the user. They research, update the database, make predictions, and send findings to the delivery agents via `pftui agent message`.

| Agent | Schedule | Domain | Routine |
|---|---|---|---|
| **Low Timeframe** (x3) | Pre-market, midday, market close | Price action, technicals, sentiment, breaking news, intraday flows | `agents/routines/low-timeframe-analyst.md` |
| **Medium Timeframe** | Daily (evening, before synthesis) | Central bank policy, geopolitical timelines, economic data trends, scenario tracking | `agents/routines/medium-timeframe-analyst.md` |
| **High Timeframe** | 2x/week | Technology disruption, de-dollarisation, commodity supercycle, structural trends | `agents/routines/high-timeframe-analyst.md` |
| **Macro Timeframe** | Weekly | Empire cycles (Dalio Big Cycle), generational theory (Fourth Turning), power metrics | `agents/routines/macro-timeframe-analyst.md` |

### Delivery Agents (message the user)

These agents synthesize outputs from all timeframe analysts and deliver to the user.

| Agent | Schedule | What It Delivers | Routine |
|---|---|---|---|
| **Morning Brief** | Daily (morning) | Concise scannable brief: prices, alignment, overnight news, prediction scorecard, today's watch | `agents/routines/morning-brief.md` |
| **Evening Analysis** | Daily (evening, after all analysts) | Deep cross-timeframe synthesis: convergence/divergence, prediction self-reflection, scenario updates | `agents/routines/evening-analysis.md` |

### Alert Pipeline (optional)

For real-time threshold monitoring between scheduled runs:

| Agent | Schedule | Role | Routine |
|---|---|---|---|
| **Alert Watchdog** | Hourly | Refreshes data, checks `analytics alerts check`, signals investigator if anything triggered | `agents/routines/alert-watchdog.md` |
| **Alert Investigator** | Hourly (offset) | Investigates triggered alerts, routes findings to low-agent + morning + evening via agent message bus. Never messages the user directly. | `agents/routines/alert-investigator.md` |

### Data Flow

```
LOW(3x/day) + MEDIUM(daily) + HIGH(2x/week) + MACRO(weekly)
         ↓                    ↓
    evening-analysis ← reads all layers, synthesizes
         ↓
    morning-brief ← reads evening output + overnight data
         ↓
      → User (2 messages/day)

Alert watchdog → investigator → low-agent + morning + evening (agent message bus)
```

### Setup Steps

1. **Create each agent as a scheduled task** on your platform (cron job, recurring task, scheduled workflow, or whatever your framework calls it). Each agent needs:
   - **A prompt** that includes local configuration (database path/credentials, user profile path, delivery channel) followed by the routine
   - **The routine** from `agents/routines/[name].md`, either fetched at runtime from the repo URL or inlined into the prompt
   - **Shell access** to run `pftui` commands
   - **A schedule** matching the table above (adjust times to the user's timezone)

2. **Schedule order matters.** Timeframe analysts must run before delivery agents:
   - LOW pre-market → LOW midday → LOW close → MEDIUM → evening-analysis → (overnight) → morning-brief
   - HIGH and MACRO run on their own schedules and feed into evening-analysis whenever they last ran

3. **Silent vs delivery agents.** Only morning-brief and evening-analysis should message the user. All other agents write to the database and signal via `pftui agent message`. This keeps the user's inbox clean.

4. **Prediction scoring.** Each timeframe agent owns its predictions end-to-end: creation, scoring, and reflection on wrong calls. The evening analysis reads the scorecard but does not score other agents' predictions.

5. **Feedback loop.** Evening analysis sends WATCH TOMORROW guidance to the low-agent via `pftui agent message`, creating a feedback loop where synthesis informs the next day's observation.

### Prompt Structure

Each scheduled agent's prompt has two parts:

```
== LOCAL CONFIGURATION ==
[Private: database credentials, user profile path, delivery channel/target, git identity]
[This section is NOT in the repo — it lives in your platform's cron/task config]

== ROUTINE ==
[Generic: the full routine from agents/routines/[name].md]
[Either inline the content or fetch it at runtime:]
Fetch from: https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/agents/routines/[name].md
```

Fetching at runtime means updating the routine in the repo instantly updates all agents on their next run. Inlining is simpler but requires manual updates.

### Routine Files

All routine files live in `agents/routines/` in the repo:
```
agents/routines/
├── README.md
├── low-timeframe-analyst.md
├── medium-timeframe-analyst.md
├── high-timeframe-analyst.md
├── macro-timeframe-analyst.md
├── morning-brief.md
├── evening-analysis.md
├── alert-watchdog.md
├── alert-investigator.md
└── dev-agent.md
```

These are generic templates containing zero personal data. They define inputs, analysis steps, outputs, and rules for each agent role. Any pftui operator on any agent platform can use them directly.

### Model Recommendations

| Agent | Recommended Tier | Why |
|---|---|---|
| Low Timeframe | Mid-tier (Sonnet, GPT-4o, Gemini Pro) | High frequency, needs speed |
| Medium Timeframe | Mid-tier | Deep research but not synthesis |
| High Timeframe | Mid-tier | Structural research |
| Macro Timeframe | Mid-tier | Weekly, can afford depth |
| Morning Brief | Mid-tier | Concise delivery, not heavy reasoning |
| Evening Analysis | Top-tier (Opus, o1, Gemini Ultra) | Cross-timeframe synthesis is the hardest task |
| Alert Watchdog | Low-tier (Haiku, GPT-4o-mini, Flash) | Simple check, runs hourly |
| Alert Investigator | Mid-tier | Needs judgment but runs rarely |
| Dev Agent | Top-tier | Code generation + architecture decisions |

---
