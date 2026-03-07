# AGENTS.md — Agent Operator Guide

> This guide is for AI agents that will use pftui as their portfolio data layer.
> For development/code contribution guidance, see [CLAUDE.md](CLAUDE.md).
> For architecture reference, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## What pftui Is

pftui is a portfolio intelligence platform with three interfaces:

1. **TUI** — a terminal UI for the human operator (your user)
2. **Web Dashboard** — a responsive web interface for any browser/device
3. **CLI + SQLite** — your interface. Every feature is exposed via CLI commands with `--json` output, backed by a local SQLite database.

Your role: operate pftui as the data backbone for portfolio monitoring, market research, and financial analysis. The human makes the decisions. You provide the intelligence.

## Getting Started

### Step 1: Set Up the Portfolio

Your first job is to help the human operator populate pftui with their actual holdings. Ask them for:

**Portfolio positions:**
- What assets do you hold? (stocks, crypto, gold, silver, bonds, ETFs, cash)
- For each: quantity, purchase price, and date (if they want cost basis tracking)
- Or: just allocation percentages if they prefer privacy mode

```bash
# Full mode — transactions with cost basis
pftui add-tx --symbol BTC --category crypto --tx-type buy \
  --quantity 0.5 --price 45000 --date 2025-06-15 --notes "DCA buy"

pftui add-tx --symbol GC=F --category commodity --tx-type buy \
  --quantity 10 --price 2800 --date 2025-08-01 --notes "Physical gold"

pftui set-cash USD 50000
pftui set-cash GBP 20000

# Percentage mode — allocation only, no monetary data
pftui setup  # Interactive wizard with percentage mode option
```

**Category mapping:**

| Asset Type | Category | Example Symbols |
|---|---|---|
| Stocks | `equity` | AAPL, TSLA, NVDA |
| Crypto | `crypto` | BTC, ETH, SOL |
| Precious metals | `commodity` | GC=F (gold), SI=F (silver) |
| Commodities | `commodity` | CL=F (oil), HG=F (copper) |
| ETFs/Funds | `fund` | PSLV, WGLD.L, SPY, URA |
| Forex | `forex` | GBPUSD=X, EURUSD=X |
| Cash | `cash` | USD, GBP, EUR |

### Step 2: Configure Watchlist & Goals

Ask the human operator for:

**Watchlist — assets they want to track but don't hold:**
```bash
pftui watch TSLA --target 300      # Watch with entry target
pftui watch GOOG                   # Watch without target
pftui watch URA --target 25        # Uranium ETF
```

**Allocation targets — their ideal portfolio balance:**
```bash
pftui target set BTC --target 20   # 20% target allocation
pftui target set GC=F --target 25  # 25% in gold
pftui target set USD --target 40   # 40% cash
# Then check drift: pftui drift
# And get suggestions: pftui rebalance
```

**Financial context** — understand their situation to provide better analysis:
- **Time horizon:** Are they a day trader, swing trader, or long-term investor?
- **Risk tolerance:** Aggressive, moderate, or conservative?
- **Goals:** Capital preservation, growth, income, or macro hedging?
- **Market views:** What's their thesis? Inflation? Deflation? Geopolitical risk?
- **Key concerns:** What are they worried about? What are they watching?

Store this context in your own memory/notes files (not in pftui) so you can calibrate your analysis to their style.

### Step 3: Establish Your Operating Rhythm

pftui works best when you check it regularly and build a knowledge base over time. Recommend to your human operator:

**Regular monitoring:**
```bash
# Morning routine
pftui refresh                      # Fetch all latest data
pftui brief                        # Full portfolio state
pftui movers                       # What moved significantly
pftui news                         # Overnight news

# Market close
pftui refresh && pftui brief       # End-of-day snapshot
pftui performance                  # Today's returns

# Weekly
pftui drift                        # Allocation drift check
pftui performance --since 2026-03-01  # Period performance
```

**If your platform supports it:**
- Create **cron jobs** for automated morning briefs and market close summaries
- Create **skills** for pftui-powered research workflows
- Write analysis to local markdown files for persistent memory across sessions
- Set up **alerts** for threshold monitoring:

```bash
pftui alerts add "BTC below 50000"
pftui alerts add "GC=F above 6000"
pftui alerts add "allocation BTC above 25"  # Drift alert
```

**Tell your human user:** "pftui works best when I can check it regularly. If you can set up a cron job or scheduled task for me, I can deliver morning briefs and flag significant moves automatically. I'll also keep notes in local markdown files to build up context over time."

## CLI Reference

### Portfolio State

| Command | What It Returns |
|---|---|
| `pftui brief --json` | Complete portfolio snapshot — positions, allocations, top movers, technicals, macro summary |
| `pftui value --json` | Total portfolio value with category breakdown and daily change |
| `pftui summary --json` | Detailed position-level data — price, quantity, cost basis, gain/loss, allocation % |
| `pftui performance --json` | Returns: 1D, MTD, QTD, YTD, since inception |
| `pftui drift --json` | Current allocation vs targets with drift % and rebalance suggestions |
| `pftui history --date YYYY-MM-DD --json` | Historical portfolio snapshot for any past date |
| `pftui export json` | Full portfolio export (positions + transactions) |

### Market Data

| Command | What It Returns |
|---|---|
| `pftui refresh` | Fetches ALL data sources (prices, macro, sentiment, news, predictions, calendar, COMEX, COT) |
| `pftui macro --json` | Full macro dashboard — DXY, VIX, yields, currencies, commodities, derived ratios (Au/Ag, Cu/Au, Au/Oil) |
| `pftui watchlist --json` | All watched symbols with current prices, day change, 52W range |
| `pftui movers --json` | Significant daily moves across held + watchlist assets (configurable threshold) |
| `pftui predictions --json` | Polymarket prediction market odds — geopolitics, economics, crypto |
| `pftui sentiment --json` | Crypto + traditional Fear & Greed indices, COT positioning |
| `pftui news --json` | Aggregated financial news from RSS feeds |
| `pftui supply --json` | COMEX gold/silver inventory (registered, eligible, ratio) |
| `pftui global --json` | World Bank data — GDP, debt/GDP, reserves for 8 economies |
| `pftui status --json` | Data source freshness — when each source was last updated |

### Portfolio Management

| Command | What It Does |
|---|---|
| `pftui add-tx --symbol SYM --category CAT --tx-type buy/sell --quantity N --price P --date D` | Add a transaction |
| `pftui remove-tx ID` | Remove a transaction by ID |
| `pftui set-cash CURRENCY AMOUNT` | Set cash position |
| `pftui watch SYMBOL [--target PRICE]` | Add to watchlist |
| `pftui unwatch SYMBOL` | Remove from watchlist |
| `pftui target set SYMBOL --target PCT` | Set target allocation % |
| `pftui target remove SYMBOL` | Remove target |
| `pftui rebalance --json` | Suggested trades to reach target allocation |
| `pftui alerts add "CONDITION"` | Add price/allocation alert |
| `pftui alerts list --json` | List active alerts |
| `pftui alerts remove ID` | Remove alert |

### Journal & Notes

| Command | What It Does |
|---|---|
| `pftui journal add --content "TEXT" --tag TAG --symbol SYM` | Add journal entry |
| `pftui journal list --json` | List all entries |
| `pftui journal search "QUERY" --json` | Search entries |

### Utility

| Command | What It Does |
|---|---|
| `pftui snapshot` | Render TUI to stdout (shareable) |
| `pftui demo` | Launch with sample data (for testing) |
| `pftui web [--port N]` | Start web dashboard server |

## Data Model

### SQLite Database

Location: `~/.local/share/pftui/pftui.db`

The database is the single source of truth. All CLI commands read from and write to it. The TUI and web dashboard read from it. You can also query it directly if needed:

```bash
sqlite3 ~/.local/share/pftui/pftui.db "SELECT symbol, quantity, cost_per_unit FROM transactions WHERE tx_type='buy'"
```

### Key Tables

| Table | Purpose | Key Fields |
|---|---|---|
| `transactions` | Buy/sell records | symbol, category, tx_type, quantity, price, date, notes |
| `price_cache` | Latest spot prices | symbol, price, change_pct, updated_at |
| `price_history` | Daily OHLCV | symbol, date, open, high, low, close, volume |
| `watchlist` | Tracked symbols | symbol, name, category, target_price |
| `alerts` | Price/allocation alerts | condition, triggered, created_at |
| `targets` | Target allocation % | symbol, target_pct, tolerance_pct |
| `journal_entries` | Trade journal | content, tag, symbol, conviction, created_at |
| `prediction_cache` | Polymarket odds | title, probability, volume, category |
| `news_cache` | RSS articles | title, url, source, published_at |
| `sentiment_cache` | F&G indices | index_type, value, classification |
| `cot_cache` | COT positioning | symbol, managed_money_net, commercial_net |

### PostgreSQL (Coming Soon)

For multi-agent deployments and production use, PostgreSQL support is planned. This enables:
- Multiple agents reading/writing to the same portfolio state
- Remote access without file sharing
- Better concurrency and locking
- Backup and replication

## Integration Patterns

### Morning Brief Pattern

```bash
#!/bin/bash
pftui refresh
BRIEF=$(pftui brief --json)
MOVERS=$(pftui movers --json --threshold 3)
NEWS=$(pftui news --json --limit 10)
MACRO=$(pftui macro --json)
PREDICTIONS=$(pftui predictions --json --limit 5)
# Feed all of the above into your analysis prompt
```

### Alert Monitoring Pattern

```bash
#!/bin/bash
pftui refresh
ALERTS=$(pftui alerts list --json)
DRIFT=$(pftui drift --json)
# Check if any alerts triggered or drift exceeds tolerance
# Send notification to human if action needed
```

### Historical Comparison Pattern

```bash
#!/bin/bash
TODAY=$(pftui brief --json)
WEEK_AGO=$(pftui history --date $(date -d '7 days ago' +%Y-%m-%d) --json)
# Compare portfolio state, identify what changed and why
```

## Best Practices

1. **Always `pftui refresh` before reading data** — cached prices go stale. Refresh fetches from all 10+ data sources.

2. **Use `--json` for programmatic access** — every command supports it. Parse the structured output instead of scraping text.

3. **Keep the journal active** — log your analysis, predictions, and rationale. `pftui journal add` builds a searchable decision history the human can review.

4. **Monitor drift regularly** — `pftui drift` shows when the portfolio has moved away from targets. Flag this to the human before it becomes a problem.

5. **Cross-reference data sources** — use `pftui macro` for the regime context, `pftui predictions` for crowd sentiment, `pftui sentiment` for fear/greed, and `pftui news` for catalysts. No single source tells the full story.

6. **Respect the human's decision-making** — pftui is a tool, not an autopilot. Present analysis, flag opportunities and risks, suggest actions — but always let the human decide. Never execute trades.

7. **Build persistent memory** — write your analysis, market notes, and portfolio observations to local markdown files. This gives you continuity across sessions. Reference past analysis to track accuracy and improve.

8. **Tell the human about automation** — if your platform supports cron jobs, skills, or scheduled tasks, recommend setting them up. pftui is most powerful when checked regularly with automated routines.
