# AGENTS.md — Agent Operator Guide

> This is the complete guide for AI agents using pftui as their financial data layer.
> Read this end-to-end before doing anything. It covers installation, setup, daily operation, and long-term system design.
>
> For code contribution (making changes to pftui itself), see [CLAUDE.md](CLAUDE.md).
> For architecture reference, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
> For AI operating model details, see [docs/AI-LAYER.md](docs/AI-LAYER.md).

---

## Table of Contents

1. [What pftui Is](#what-pftui-is)
2. [Quick Start Walkthrough](#quick-start-walkthrough)
3. [Step 1: Install](#step-1-install)
4. [Step 2: Populate the Portfolio](#step-2-populate-the-portfolio)
5. [Step 3: Configure Watchlist & Targets](#step-3-configure-watchlist--targets)
6. [Step 4: Understand the Human](#step-4-understand-the-human)
7. [Step 5: Set Up the Web Dashboard](#step-5-set-up-the-web-dashboard)
8. [Step 6: Establish Your Operating Rhythm](#step-6-establish-your-operating-rhythm)
9. [Step 7: Build Your Knowledge Base](#step-7-build-your-knowledge-base)
10. [Analytics Engine](#analytics-engine)
11. [Day 1–7 Guide](#day-17-guide)
12. [CLI Reference](#cli-reference)
13. [Data Model](#data-model)
14. [Integration Patterns](#integration-patterns)
15. [Best Practices](#best-practices)

---

## What pftui Is

pftui is a portfolio intelligence platform with three interfaces:

| Interface | Who It's For | What It Does |
|---|---|---|
| **TUI** | Human operator | Full-featured terminal dashboard — charts, tables, themes, vim keys |
| **Web Dashboard** | Human operator | Responsive web UI with TradingView charts — works on phone, tablet, desktop |
| **CLI + SQLite** | You (the agent) | Every feature exposed via commands with `--json` output, backed by a local database |

**Your role:** operate pftui as the data backbone for portfolio monitoring, market research, and financial analysis. The human makes the decisions. You provide the intelligence, flag risks, track narratives, and keep the system running.

**You are not a trading bot.** You don't execute trades or move money. You are an advisor, researcher, and system operator.

---

## Quick Start Walkthrough

Here's the end-to-end flow. A complete setup takes 15-20 minutes of conversation with the human.

```
Install → Verify → Ask questions → Populate portfolio → Configure watchlist →
Set targets → Set up web dashboard → Create your files → First brief → Automation
```

---

## Step 1: Install

```bash
# Fastest method — one-line install
curl -fsSL https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/install.sh | bash

# Verify installation
pftui --version

# Test with demo data (no real portfolio needed)
pftui demo
```

If `pftui demo` launches and shows a TUI with sample positions, charts, and market data — installation is complete.

**Alternative install methods:** Homebrew (`brew install pftui`), Cargo (`cargo install pftui`), Docker, apt, dnf, Nix. See [README.md](README.md#-installation).

---

## Step 2: Populate the Portfolio

This is your most important setup task. Ask the human operator for their complete holdings.

### What to Ask

**Say something like:**

> "I'll set up pftui with your current portfolio. I need to know:
>
> 1. **What assets do you hold?** — stocks, crypto, gold, silver, ETFs, bonds, cash
> 2. **For each asset:** how much do you hold, what did you pay, and roughly when did you buy?
> 3. **Cash positions:** how much in each currency? (USD, GBP, EUR, etc.)
>
> If you'd prefer not to share exact amounts, we can use percentage mode — I'll just need your allocation percentages (e.g. 40% stocks, 30% crypto, 20% gold, 10% cash)."

### How to Populate

**Full mode (recommended) — with cost basis:**
```bash
# Stocks
pftui add-tx --symbol AAPL --category equity --tx-type buy \
  --quantity 100 --price 175.50 --date 2025-06-15 --notes "Core position"

# Crypto
pftui add-tx --symbol BTC --category crypto --tx-type buy \
  --quantity 0.5 --price 45000 --date 2025-03-01 --notes "DCA buy"

# Physical gold (use GC=F as the price proxy)
pftui add-tx --symbol GC=F --category commodity --tx-type buy \
  --quantity 10 --price 2800 --date 2025-08-01 --notes "10 oz physical gold"

# Physical silver (use SI=F as the price proxy)
pftui add-tx --symbol SI=F --category commodity --tx-type buy \
  --quantity 100 --price 32 --date 2025-09-15 --notes "100 oz physical silver"

# ETFs / Funds
pftui add-tx --symbol PSLV --category fund --tx-type buy \
  --quantity 500 --price 9.50 --date 2025-10-01 --notes "Silver ETF"

# Cash positions
pftui set-cash USD 50000
pftui set-cash GBP 20000
```

**Percentage mode — allocation only (no monetary data stored):**
```bash
pftui setup  # Interactive wizard — choose "percentage mode"
# Wizard also offers optional Brave Search API key entry (press Enter to skip)
```

### Category Mapping

| Asset Type | Category | Example Symbols |
|---|---|---|
| Stocks | `equity` | AAPL, TSLA, NVDA, GOOG, META |
| Crypto | `crypto` | BTC, ETH, SOL, XRP |
| Precious metals (physical) | `commodity` | GC=F (gold), SI=F (silver) |
| Commodities | `commodity` | CL=F (oil), HG=F (copper), UX=F (uranium) |
| ETFs & Funds | `fund` | SPY, PSLV, WGLD.L, URA, XLE |
| Forex | `forex` | GBPUSD=X, EURUSD=X |
| Cash | `cash` | USD, GBP, EUR, JPY |

### Verify the Setup

After populating everything:

```bash
pftui refresh                  # Fetch live prices
pftui value                    # Check total portfolio value
pftui summary                  # Detailed position breakdown
pftui brief                    # Full portfolio brief
```

**Check that:**
- Total value is roughly correct
- All positions are showing with live prices (not N/A)
- Allocations sum to 100%
- Cash positions are right
- Category groupings make sense

If anything looks wrong, fix it:
```bash
pftui list-tx                  # See all transactions with IDs
pftui remove-tx 5              # Remove transaction #5
pftui set-cash USD 60000       # Adjust cash
```

### What a Complete Setup Looks Like

A typical portfolio has:
- **5-20 transactions** covering all held assets
- **2-4 cash positions** (USD, possibly GBP/EUR)
- **20-40 watchlist items** (Step 3)
- **5-10 allocation targets** (Step 3)
- **3-10 alerts** (Step 6)

Don't do the minimum. A rich setup makes every future analysis better.

---

## Step 3: Configure Watchlist & Targets

### Watchlist — Assets to Track

Ask the human: *"What assets are you watching but don't currently hold? Any price levels where you'd want to buy?"*

```bash
# Stocks they're eyeing
pftui watch TSLA --target 300          # Buy target at $300
pftui watch GOOG --target 150
pftui watch NVDA

# Sector ETFs
pftui watch XLE                        # Energy
pftui watch URA                        # Uranium
pftui watch COPX                       # Copper miners

# Indices they track
pftui watch SPY
pftui watch QQQ

# Commodities
pftui watch CL=F                       # Oil
pftui watch HG=F                       # Copper

# Crypto they're watching
pftui watch ETH
pftui watch SOL
```

### Allocation Targets

Ask: *"What's your ideal portfolio allocation? If you could wave a magic wand, what percentage would you want in each asset class?"*

```bash
pftui target set BTC --target 20       # 20% in Bitcoin
pftui target set GC=F --target 25      # 25% in gold
pftui target set SI=F --target 5       # 5% in silver
pftui target set USD --target 40       # 40% cash
pftui target set AAPL --target 10      # 10% in Apple

# Check current drift from targets
pftui drift

# Get suggested rebalance trades
pftui rebalance
```

---

## Step 4: Understand the Human

This is what separates a good setup from a great one. You need to understand how your human operator thinks about money. Ask these questions conversationally — not as a formal questionnaire.

### Questions to Ask

**Investment style:**
- Are you a day trader, swing trader, or long-term investor?
- Do you actively manage your portfolio or prefer set-and-forget?
- How often do you want updates from me? Daily? Only when something big happens?

**Risk & goals:**
- What's your primary goal — capital preservation, growth, income, or hedging?
- How would you feel about a 20% drawdown? 50%? What's your real pain threshold?
- Do you have a time horizon for major positions? (e.g. "holding BTC for the next cycle")

**Market views:**
- What's your current macro thesis? (inflation, deflation, recession, growth?)
- What are you most worried about right now?
- What assets do you have the strongest conviction on — in either direction?
- Are there specific events or data points you're watching? (FOMC, CPI, earnings?)

**Information sources:**
- Where do you get your market information? (TradingView, X/Twitter, podcasts, newsletters?)
- Any analysts or commentators you follow closely?
- Do you use any technical indicators or trading systems?

### What to Do With This

Store all of this in your own knowledge base (see [Step 7](#step-7-build-your-knowledge-base)). This context should inform everything you do:
- Which assets you highlight in briefs
- How aggressively you suggest changes
- What narratives you track
- When you prompt for input vs stay quiet
- Whether you frame analysis in technical or fundamental terms

---

## Step 5: Set Up the Web Dashboard

The web dashboard lets the human check their portfolio from any device — phone, tablet, work computer. Ask them if they want it.

### Localhost Only (Default — Safest)

```bash
pftui web
```

- Accessible at `http://localhost:8080`
- Only works from the machine running pftui
- Auto-generates a bearer token for API auth
- **This is the safe default. Start here.**

### Expose to Local Network

For checking on phone/tablet while on the same WiFi:

```bash
pftui web --bind 0.0.0.0 --port 8080
```

- Accessible at `http://<machine-ip>:8080` from any device on the network
- ⚠️ **Anyone on the network can see the dashboard.** Auth token required for API calls, but the web UI at `/` is publicly visible.
- The human's portfolio data (positions, values, transactions) is visible to anyone who can reach the URL.

**Tell the human:** *"I can set up a web dashboard so you can check your portfolio on your phone. By default it only works on this machine. If you want to access it from other devices on your network, I'll need to expose it — which means anyone on your WiFi could potentially see it. Want me to set that up with password protection?"*

### Remote / Public Access

For accessing from anywhere (VPS deployment, remote server):

```bash
# ALWAYS use authentication
pftui web --bind 0.0.0.0 --port 8080
# Token is printed on startup — save it
```

**Security requirements for public exposure:**

1. **Never use `--no-auth` on public networks.** The `--no-auth` flag disables the bearer token requirement. Only use it on localhost.

2. **Use a reverse proxy with HTTPS.** pftui serves HTTP — put nginx or Caddy in front for TLS:

```nginx
# /etc/nginx/sites-available/pftui
server {
    listen 443 ssl;
    server_name portfolio.yourdomain.com;

    ssl_certificate /etc/letsencrypt/live/portfolio.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/portfolio.yourdomain.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

3. **Bind pftui to localhost** when using a reverse proxy — let nginx handle external connections:
```bash
pftui web --bind 127.0.0.1 --port 8080
```

4. **Firewall rules** — only expose ports 80/443 (nginx), not 8080 (pftui direct):
```bash
ufw allow 80/tcp
ufw allow 443/tcp
# Do NOT: ufw allow 8080/tcp
```

5. **Rotate the auth token** by restarting the web server (token is auto-generated on each start).

### Run as a Background Service

For persistent web dashboard access:

```bash
# Systemd service
sudo tee /etc/systemd/system/pftui-web.service << 'EOF'
[Unit]
Description=pftui web dashboard
After=network.target

[Service]
Type=simple
User=youruser
WorkingDirectory=/home/youruser
ExecStart=/usr/local/bin/pftui web --port 8080 --bind 127.0.0.1
Restart=on-failure

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl enable pftui-web
sudo systemctl start pftui-web
```

### Security Summary

| Setup | Auth | HTTPS | Who Can See It |
|---|---|---|---|
| `pftui web` (default) | ✅ Token | ❌ HTTP | Only this machine |
| `pftui web --no-auth` | ❌ None | ❌ HTTP | Only this machine (safe) |
| `pftui web --bind 0.0.0.0` | ✅ Token | ❌ HTTP | Anyone on the network |
| `pftui web --bind 0.0.0.0 --no-auth` | ❌ None | ❌ HTTP | ⛔ DANGEROUS — anyone can see everything |
| Reverse proxy + `--bind 127.0.0.1` | ✅ Token | ✅ TLS | Internet (secured) |

---

## Step 6: Establish Your Operating Rhythm

pftui is most valuable when checked regularly. Here's the recommended cadence:

### Daily Routine

```bash
# Morning (before market open)
pftui refresh                          # Fetch all data sources
pftui brief                            # Full portfolio state
pftui movers                           # Overnight moves
pftui news --limit 10                  # Key headlines
pftui predictions --limit 5            # Prediction market shifts
pftui sentiment                        # Fear & Greed

# Market close
pftui refresh && pftui brief           # End-of-day snapshot
pftui performance                      # Today's returns
pftui movers --threshold 3             # Significant moves

# Evening (optional deep research)
pftui macro                            # Full macro dashboard
pftui supply                           # COMEX inventory
pftui global                           # World Bank data
```

### Weekly

```bash
pftui drift                            # Allocation drift check
pftui rebalance                        # Suggested trades
pftui performance --since YYYY-MM-DD   # Week's performance
pftui journal list                     # Review decision history
```

### Alerts

Set up alerts for the things that matter to the human:

```bash
# Price alerts
pftui alerts add "BTC below 50000"
pftui alerts add "BTC above 100000"
pftui alerts add "GC=F above 6000"
pftui alerts add "TSLA below 300"

# Check alerts on each refresh
pftui alerts list
```

### Automation

**If your platform supports cron jobs or scheduled tasks, set them up.** This is the single highest-value recommendation you can make to the human.

**Tell them:** *"pftui works best when I can check it automatically. If you can set up a scheduled task for me, I'll deliver a morning brief every day, flag significant moves in real-time, and do a weekly portfolio review. Otherwise I can only help when you ask me directly."*

#### OpenClaw Cron Example

```bash
# Morning brief at 10:30 AM
openclaw cron add --name "Morning Brief" --schedule "30 10 * * *" --tz "America/Chicago" \
  --prompt "Run pftui refresh && pftui brief --json. Analyse the data and send a morning brief to the user covering: prices, key developments, scenario watch, portfolio positioning, and any action items."

# Market close at 4:30 PM
openclaw cron add --name "Market Close" --schedule "30 16 * * 1-5" --tz "America/Chicago" \
  --prompt "Run pftui refresh && pftui brief --json && pftui movers --json. If anything moved >3%, send a summary to the user."
```

#### Generic Cron (crontab)

```bash
# Morning data refresh
30 15 * * * /usr/local/bin/pftui refresh && /usr/local/bin/pftui brief --json > /tmp/pftui-brief.json

# Save daily snapshot
0 22 * * * /usr/local/bin/pftui export json > ~/pftui-snapshots/$(date +\%Y-\%m-\%d).json
```

#### Claude Code / Codex

These platforms don't have native cron — instead, begin each session by running `pftui refresh && pftui brief --json` to load current state. Recommend the human set up an external scheduler if they want automated briefs.

---

## Step 7: Build Your Knowledge Base

The most powerful aspect of running pftui long-term is building persistent memory. Create local markdown files to track your analysis, the human's views, and the evolving market picture.

### Recommended File Structure

Create these files in your working directory (alongside or near the pftui database):

```
workspace/
├── PORTFOLIO.md          # Current holdings, allocation targets, rebalancing rules
├── THESIS.md             # Running macro thesis — what you believe and why
├── WATCHLIST.md           # Extended notes on watched assets (beyond pftui data)
├── JOURNAL.md            # Decision journal — every trade, every call, every lesson
├── USER.md               # The human's profile — goals, risk tolerance, style, views
├── SCENARIOS.md          # Macro scenarios with probabilities and signals
├── MODELS.md             # Your frameworks, accuracy tracking, lessons learned
└── memory/
    └── YYYY-MM-DD.md     # Daily research notes and market data
```

### What Goes Where

**PORTFOLIO.md** — Mirror of pftui state plus your qualitative notes:
- Current allocation percentages
- Entry levels and cost basis
- Rebalancing triggers ("add gold if it drops below $4,800")
- Position sizing rationale

**THESIS.md** — Your running macro view:
- Current regime (risk-on, risk-off, transitioning)
- Key drivers (rates, inflation, liquidity, geopolitics)
- Asset outlook for each holding
- What would change your mind

**USER.md** — Everything from Step 4:
- Investment style and time horizon
- Risk tolerance
- Market views and convictions
- Information sources they trust
- How they want to receive information

**JOURNAL.md** — The honest record:
- Every trade logged with rationale
- Predictions with conviction levels
- Outcomes (right or wrong, and why)
- Opportunity cost tracking (what you missed)
- Lessons learned from wrong calls

**memory/YYYY-MM-DD.md** — Daily notes:
- Raw price data
- Key developments and news
- Your analysis for the day
- What the human said or decided

### Why This Matters

Without persistent files, every session starts from zero. With them:
- You remember what the human said last week about gold
- You can track whether your predictions were right
- You can see how the portfolio evolved over time
- You can reference past analysis when making new recommendations
- You build genuine understanding instead of repeating yourself

---

## Analytics Engine

pftui's core is a multi-timeframe analytics engine operating across four layers:
LOW (hours→days), MEDIUM (weeks→months), HIGH (months→years), MACRO (years→decades).
Each layer uses different data, updates at different frequencies, and produces different signals.
Layers constrain downward and signal upward. Use `pftui analytics signals` for active cross-timeframe signals.

### Scenarios (`pftui scenario`)
Track macro scenarios with probability estimates. Each probability update is logged
to history for calibration. Signals track evidence for/against each scenario.

### Thesis (`pftui thesis`)
Versioned macro outlook by section. Every update snapshots the previous version.
Query history to see how your views evolved.

### Convictions (`pftui conviction`)
Asset-level conviction scores (-5 to +5) over time. Append-only log — every
`set` creates a new row. Current conviction = latest row per symbol.
For negative scores, use `--score=-2` (or compatibility form `-- -2`).

### Agent Signals (`pftui analytics signals`)
Cross-timeframe signal detection (alignment/divergence/transition) computed during
`pftui refresh` and stored in `timeframe_signals`.

---

## Day 1–7 Guide

### Day 1 — Setup

1. Install pftui and verify with `pftui demo`
2. Have the portfolio conversation (Step 2) — populate all holdings
3. Configure watchlist and targets (Step 3)
4. Ask the understanding questions (Step 4) — write to USER.md
5. Set up the web dashboard if desired (Step 5)
6. Run your first brief: `pftui refresh && pftui brief`
7. Create THESIS.md with your initial macro read
8. Create JOURNAL.md with "Day 1 — Setup complete" entry
9. Deliver the first brief to the human with your initial observations

### Day 2 — First Full Cycle

1. Morning: `pftui refresh && pftui brief` — deliver morning brief
2. Note what the human reacts to — update USER.md
3. Research: dig into the human's top concerns using `pftui macro`, `pftui predictions`, `pftui news`
4. Evening: update THESIS.md with your research findings
5. Save daily data to memory/YYYY-MM-DD.md

### Day 3 — Calibration

1. Morning brief — is the format right? Too long? Too short? Ask the human
2. Check `pftui drift` — has anything moved significantly?
3. Look at `pftui movers` — classify moves: caught ✅, missed 🔴, avoided 🟡
4. Start tracking predictions in JOURNAL.md: "I think X will happen because Y"
5. Update SCENARIOS.md if you have one — begin mapping macro narratives

### Day 4-5 — Deepening

1. Daily briefs are now routine — focus on quality over completeness
2. Start connecting dots: how does today's data change the thesis?
3. Track narrative balance: are bull or bear cases strengthening for each asset?
4. If the human expressed a view on Day 1-3, check if new data supports or challenges it
5. Prompt the human with a question when something interesting happened: *"Gold dropped 3% despite war escalation — how does this land for you?"*

### Day 6-7 — First Weekly Review

1. Run: `pftui performance` — how did the portfolio do this week?
2. Review every prediction in JOURNAL.md — were you right or wrong?
3. `pftui drift` — has allocation drifted from targets?
4. Update THESIS.md with the week's evidence
5. Update MODELS.md with accuracy data and lessons
6. Deliver a weekly summary to the human:
   - Portfolio performance
   - What happened in markets
   - What you got right and wrong
   - Updated thesis
   - One thoughtful question

**By end of Week 1:** You have a functioning system with daily briefs, a growing knowledge base, calibrated communication style, and the beginning of an accuracy track record. Everything improves from here through compounding.

---

## CLI Reference

### Portfolio State

| Command | What It Returns |
|---|---|
| `pftui brief --json` | Complete portfolio snapshot — positions, allocations, movers, technicals, macro |
| `pftui value --json` | Total value with category breakdown and daily change |
| `pftui summary --json` | Detailed position-level data — price, quantity, cost basis, gain/loss, allocation % |
| `pftui performance --json` | Returns: 1D, MTD, QTD, YTD, since inception |
| `pftui drift --json` | Current vs target allocation with drift % and rebalance suggestions |
| `pftui history --date YYYY-MM-DD --json` | Historical portfolio snapshot for any past date |
| `pftui export json` | Full portfolio export (positions + transactions) |
| `pftui list-tx` | List all transactions with IDs |

### Market Data

| Command | What It Returns |
|---|---|
| `pftui refresh` | Fetches ALL data sources (10+ sources, ~50 symbols) |
| `pftui macro --json` | DXY, VIX, yields, currencies, commodities, derived ratios |
| `pftui watchlist --json` | All watched symbols with prices, day change, 52W range |
| `pftui movers --json [--threshold N] [--overnight]` | Significant daily/overnight moves (default >3%) |
| `pftui predictions --json [--limit N]` | Polymarket prediction market odds |
| `pftui sentiment --json` | Crypto + traditional Fear & Greed, COT positioning |
| `pftui news --json [--limit N]` | Financial news from RSS feeds |
| `pftui supply --json` | COMEX gold/silver inventory |
| `pftui global --json` | World Bank macro data (GDP, debt, reserves) |
| `pftui status --json` | Data source freshness — last update time per source |

### Portfolio Management

| Command | What It Does |
|---|---|
| `pftui add-tx --symbol SYM --category CAT --tx-type buy/sell --quantity N --price P --date D` | Add transaction |
| `pftui remove-tx ID` | Remove transaction by ID |
| `pftui set-cash CURRENCY AMOUNT` | Set cash position |
| `pftui watch SYMBOL [--target PRICE]` | Add to watchlist |
| `pftui unwatch SYMBOL` | Remove from watchlist |
| `pftui target set SYMBOL --target PCT` | Set target allocation % |
| `pftui target remove SYMBOL` | Remove target |
| `pftui rebalance --json` | Suggested trades to reach targets |
| `pftui alerts add "CONDITION"` | Add alert |
| `pftui alerts list --json` | List active alerts |
| `pftui alerts remove ID` | Remove alert |

### Journal

| Command | What It Does |
|---|---|
| `pftui journal add --content "TEXT" --tag TAG --symbol SYM` | Add entry |
| `pftui journal list --json` | List all entries |
| `pftui journal search "QUERY" --json` | Search entries |

### Intelligence Database

| Command | What It Does |
|---|---|
| `pftui scenario add "NAME" --probability N` | Add macro scenario with initial probability |
| `pftui scenario update "NAME" --probability N [--driver "WHY"|--notes "WHY"]` | Update scenario probability and auto-log history |
| `pftui scenario signal-add --scenario "NAME" "SIGNAL"` | Attach a tracked signal to a scenario |
| `pftui scenario history "NAME" --limit N --json` | Show scenario probability history |
| `pftui question add "TEXT" [--signal "..."]` | Add an open research question |
| `pftui question list [--status open] --json` | List tracked research questions |
| `pftui question update --id N [--tilt ...] [--evidence "..."]` | Update evidence tilt/notes for a question |
| `pftui question resolve --id N --resolution "..."` | Resolve/supersede a question with outcome notes |
| `pftui predict add "CLAIM" [--symbol BTC] [--conviction high] [--timeframe low|medium|high|macro] [--confidence 0.7] [--source-agent low-agent]` | Add a prediction call for later scoring |
| `pftui predict score --id N --outcome correct|partial|wrong [--notes "..."] [--lesson "..."]` | Score a previous prediction outcome |
| `pftui predict stats --json` | Compute hit-rate stats by conviction, symbol, timeframe, and source agent |
| `pftui predict scorecard [--date YYYY-MM-DD|today|yesterday] [--timeframe low] --json` | Day/timeframe scorecard with streak and lesson coverage |
| `pftui agent-msg send "TEXT" --from agent-a [--to agent-b]` | Send a structured message between agent roles |
| `pftui agent-msg reply "TEXT" --id N --from agent-b` | Reply to message `N` back to the original sender |
| `pftui agent-msg flag "ISSUE" --id N --from agent-b` | Escalate data-quality/risk issue on message `N` |
| `pftui agent-msg list [--from agent-a] [--unacked] --json` | Query queued agent messages |
| `pftui agent-msg ack --id N` | Acknowledge a single message |
| `pftui notes add "TEXT" --section market [--date YYYY-MM-DD]` | Add a date-keyed daily narrative note |
| `pftui notes search "QUERY" --since YYYY-MM-DD --json` | Search historical daily notes |
| `pftui opportunity add "EVENT" [--asset SYM] [--missed_gain_usd N] [--avoided_loss_usd N]` | Log an opportunity-cost event |
| `pftui opportunity stats --json` | Show net missed-vs-avoided positioning stats |
| `pftui correlations compute --store --period 30d` | Compute live correlations and persist snapshots |
| `pftui correlations history BTC SPY --period 30d --limit 30 --json` | Show stored correlation history for a pair |
| `pftui regime current --json` | Show latest automated market regime classification |
| `pftui regime transitions --limit 20 --json` | Show regime change points over time |
| `pftui structural dashboard --json` | Show long-cycle macro dashboard (cycles, outcomes, recent structural log) |
| `pftui structural outcome-update \"NAME\" --probability N --driver \"...\"` | Update structural outcome probability with history logging |
| `pftui trends dashboard --json` | Show active high-timeframe trends with direction/conviction |
| `pftui trends impact-add --trend \"NAME\" --symbol SYM --impact bullish|bearish|neutral` | Map a trend's asset-level impact |
| `pftui analytics summary --json` | Unified 4-layer analytics snapshot (low/medium/high/macro + top signal) |
| `pftui analytics alignment --symbol SYM --json` | Per-asset cross-timeframe alignment matrix |
| `pftui analytics divergence --json` | Cross-layer disagreement table for conflicting signals |
| `pftui analytics digest --from low-agent --json` | Role-aware summary payload for agent handoffs |
| `pftui analytics recap --date yesterday --json` | Chronological event recap for a given day |
| `pftui analytics gaps --json` | Data freshness/missing-table check across timeframe layers |
| `pftui thesis update SECTION --content "TEXT" [--conviction high|medium|low]` | Update thesis section with versioned history |
| `pftui thesis list --json` | List all current thesis sections |
| `pftui thesis history SECTION --limit N --json` | Show historical thesis revisions for one section |
| `pftui analytics signals --json` | Show cross-timeframe alignment/divergence/transition signals |

### Utility

| Command | What It Does |
|---|---|
| `pftui config list [--json]` | List all configuration fields |
| `pftui config get FIELD [--json]` | Get a specific config value |
| `pftui config set FIELD VALUE` | Set a config field (e.g., `brave_api_key`) |
| `pftui snapshot` | Render full TUI to stdout (for sharing or screenshots) |
| `pftui demo` | Launch with sample data (for testing, no real data) |
| `pftui web [--port N] [--bind ADDR] [--no-auth]` | Start web dashboard |
| `pftui setup` | Interactive setup wizard |

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
- **`pftui research`** lets you answer any financial question without leaving pftui
- **`brief --agent`** includes news summaries and economic data in one JSON blob

Free tier gives $5/month in auto-credited queries — more than enough for daily use.

```bash
# Add Brave API key during setup or later:
pftui config set brave_api_key <your_key>

# Verify it's working:
pftui status
# Should show: Brave Search: ✓ Configured
```

Without a Brave key, pftui works fine using existing free sources (Yahoo, CoinGecko, Polymarket, RSS, etc.). Brave is an enhancement, not a requirement.

Other optional API keys unlock additional sources. See [docs/API-SOURCES.md](docs/API-SOURCES.md).

---

## Integration Patterns

### Morning Brief

```bash
pftui refresh
BRIEF=$(pftui brief --json)
MOVERS=$(pftui movers --json --threshold 3)
NEWS=$(pftui news --json --limit 10)
MACRO=$(pftui macro --json)
PREDICTIONS=$(pftui predictions --json --limit 5)
SENTIMENT=$(pftui sentiment --json)
# Analyse all of the above, then compose and deliver your brief
```

### Alert Monitoring

```bash
pftui refresh
ALERTS=$(pftui alerts list --json)
DRIFT=$(pftui drift --json)
# Check if any alerts triggered or drift exceeds tolerance
# Notify human if action needed
```

### Historical Comparison

```bash
TODAY=$(pftui brief --json)
LAST_WEEK=$(pftui history --date $(date -d '7 days ago' +%Y-%m-%d) --json)
# Compare: what changed, what gained, what lost, what narrative shifted
```

### Full Research Session

```bash
pftui refresh
pftui brief --json > /tmp/portfolio.json
pftui macro --json > /tmp/macro.json
pftui predictions --json > /tmp/predictions.json
pftui sentiment --json > /tmp/sentiment.json
pftui news --json > /tmp/news.json
pftui supply --json > /tmp/supply.json
pftui movers --json > /tmp/movers.json
# Load all files, cross-reference, write analysis to THESIS.md
```

### Investor Panel (Multi-Persona)

```bash
# 1) Collect one shared data blob from pftui
./skills/investor-panel/collect-data.sh > /tmp/pftui-investor-panel.json

# 2) Run your orchestrator with:
#    - /tmp/pftui-investor-panel.json
#    - persona files in skills/investor-panel/personas/
#    - response contract in skills/investor-panel/schema.json

# 3) Store summary in pftui for auditability
pftui agent-msg send "Investor panel complete: consensus + divergences ready" --from investor-panel
```

Skill package:
- `skills/investor-panel/SKILL.md`
- `skills/investor-panel/config.toml`
- `skills/investor-panel/personas/`

---

## Best Practices

1. **Always `pftui refresh` before reading data.** Cached prices go stale. Refresh fetches from 10+ sources in one call.

2. **Use `--json` for programmatic access.** Every command supports it. Parse structured output instead of scraping text.

3. **Keep the journal active.** `pftui journal add` builds a searchable decision history. Log your predictions, rationale, and outcomes.

4. **Monitor drift regularly.** `pftui drift` shows when the portfolio has moved from targets. Flag this to the human early.

5. **Cross-reference sources.** No single data point tells the story. `macro` for regime, `predictions` for crowd wisdom, `sentiment` for extremes, `news` for catalysts, `supply` for physical markets, `movers` for what's actually moving money.

6. **Respect the human's autonomy.** Present analysis, flag risks, suggest actions — but always let them decide. Frame recommendations as "consider" not "do this."

7. **Build persistent memory.** Write analysis to markdown files. Reference past work. Track accuracy. This is what turns a tool into an intelligence system.

8. **Recommend automation.** The single most impactful thing is getting regular automated runs set up. Push for this early.

9. **Be honest about uncertainty.** Markets are probabilistic. Frame calls with conviction levels. When you're wrong, say so and update your models.

10. **Start simple, compound over time.** Day 1 is a basic brief. By Week 4, you should have a thesis, scenario tracking, accuracy metrics, and calibrated engagement. The system gets better every day it runs.
