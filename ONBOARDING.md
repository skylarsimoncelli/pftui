# ONBOARDING.md — First-Time Setup Guide

> This guide walks you through setting up pftui with a new user, from installation to the first week of operation.
> Follow it end-to-end on your first interaction. It takes 15-20 minutes of conversation with the human.
>
> After setup is complete, see [AGENTS.md](AGENTS.md) for the full operational reference: CLI commands, data model, integration patterns, analytics engine, multi-timeframe agent architecture, and best practices.

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
10. [Day 1-7 Guide](#day-17-guide)

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
pftui system demo
```

If `pftui system demo` launches and shows a TUI with sample positions, charts, and market data — installation is complete.

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
pftui portfolio transaction add --symbol AAPL --category equity --tx-type buy \
  --quantity 100 --price 175.50 --date 2025-06-15 --notes "Core position"

# Crypto
pftui portfolio transaction add --symbol BTC --category crypto --tx-type buy \
  --quantity 0.5 --price 45000 --date 2025-03-01 --notes "DCA buy"

# Physical gold (use GC=F as the price proxy)
pftui portfolio transaction add --symbol GC=F --category commodity --tx-type buy \
  --quantity 10 --price 2800 --date 2025-08-01 --notes "10 oz physical gold"

# Physical silver (use SI=F as the price proxy)
pftui portfolio transaction add --symbol SI=F --category commodity --tx-type buy \
  --quantity 100 --price 32 --date 2025-09-15 --notes "100 oz physical silver"

# ETFs / Funds
pftui portfolio transaction add --symbol PSLV --category fund --tx-type buy \
  --quantity 500 --price 9.50 --date 2025-10-01 --notes "Silver ETF"

# Cash positions
pftui portfolio set-cash USD 50000
pftui portfolio set-cash GBP 20000
```

**Percentage mode — allocation only (no monetary data stored):**
```bash
pftui system setup  # Interactive wizard — choose "percentage mode"
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
pftui data refresh                  # Fetch live prices
pftui portfolio value                    # Check total portfolio value
pftui portfolio summary                  # Detailed position breakdown
pftui portfolio brief                    # Full portfolio brief
```

**Check that:**
- Total value is roughly correct
- All positions are showing with live prices (not N/A)
- Allocations sum to 100%
- Cash positions are right
- Category groupings make sense

If anything looks wrong, fix it:
```bash
pftui portfolio transaction list                  # See all transactions with IDs
pftui portfolio transaction remove 5              # Remove transaction #5
pftui portfolio set-cash USD 60000       # Adjust cash
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
pftui portfolio watchlist add TSLA --target 300          # Buy target at $300
pftui portfolio watchlist add GOOG --target 150
pftui portfolio watchlist add NVDA

# Sector ETFs
pftui portfolio watchlist add XLE                        # Energy
pftui portfolio watchlist add URA                        # Uranium
pftui portfolio watchlist add COPX                       # Copper miners

# Indices they track
pftui portfolio watchlist add SPY
pftui portfolio watchlist add QQQ

# Commodities
pftui portfolio watchlist add CL=F                       # Oil
pftui portfolio watchlist add HG=F                       # Copper

# Crypto they're watching
pftui portfolio watchlist add ETH
pftui portfolio watchlist add SOL
```

### Allocation Targets

Ask: *"What's your ideal portfolio allocation? If you could wave a magic wand, what percentage would you want in each asset class?"*

```bash
pftui portfolio target set BTC --target 20       # 20% in Bitcoin
pftui portfolio target set GC=F --target 25      # 25% in gold
pftui portfolio target set SI=F --target 5       # 5% in silver
pftui portfolio target set USD --target 40       # 40% cash
pftui portfolio target set AAPL --target 10      # 10% in Apple

# Check current drift from targets
pftui portfolio drift

# Get suggested rebalance trades
pftui portfolio rebalance
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
pftui system web
```

- Accessible at `http://localhost:8080`
- Only works from the machine running pftui
- Auto-generates a bearer token for API auth
- **This is the safe default. Start here.**

### Expose to Local Network

For checking on phone/tablet while on the same WiFi:

```bash
pftui system web --bind 0.0.0.0 --port 8080
```

- Accessible at `http://<machine-ip>:8080` from any device on the network
- ⚠️ **Anyone on the network can see the dashboard.** Auth token required for API calls, but the web UI at `/` is publicly visible.
- The human's portfolio data (positions, values, transactions) is visible to anyone who can reach the URL.

**Tell the human:** *"I can set up a web dashboard so you can check your portfolio on your phone. By default it only works on this machine. If you want to access it from other devices on your network, I'll need to expose it — which means anyone on your WiFi could potentially see it. Want me to set that up with password protection?"*

### Remote / Public Access

For accessing from anywhere (VPS deployment, remote server):

```bash
# ALWAYS use authentication
pftui system web --bind 0.0.0.0 --port 8080
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
pftui system web --bind 127.0.0.1 --port 8080
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
Description=pftui system web dashboard
After=network.target

[Service]
Type=simple
User=youruser
WorkingDirectory=/home/youruser
ExecStart=/usr/local/bin/pftui system web --port 8080 --bind 127.0.0.1
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
| `pftui system web` (default) | ✅ Token | ❌ HTTP | Only this machine |
| `pftui system web --no-auth` | ❌ None | ❌ HTTP | Only this machine (safe) |
| `pftui system web --bind 0.0.0.0` | ✅ Token | ❌ HTTP | Anyone on the network |
| `pftui system web --bind 0.0.0.0 --no-auth` | ❌ None | ❌ HTTP | ⛔ DANGEROUS — anyone can see everything |
| Reverse proxy + `--bind 127.0.0.1` | ✅ Token | ✅ TLS | Internet (secured) |

---

## Step 6: Establish Your Operating Rhythm

pftui is most valuable when checked regularly. Here's the recommended cadence:

### Daily Routine

```bash
# Morning (before market open)
pftui data refresh                          # Fetch all data sources
pftui portfolio brief                            # Full portfolio state
pftui analytics movers                           # Overnight moves
pftui data news --limit 10                    # Key headlines
pftui data predictions --limit 5              # Prediction market shifts
pftui data sentiment                          # Fear & Greed

# Market close
pftui data refresh && pftui portfolio brief           # End-of-day snapshot
pftui portfolio performance                      # Today's returns
pftui analytics movers --threshold 3             # Significant moves

# Evening (optional deep research)
pftui data dashboard macro                    # Full macro dashboard
pftui data supply                             # COMEX inventory
pftui data dashboard global                   # World Bank data
```

### Weekly

```bash
pftui portfolio drift                            # Allocation drift check
pftui portfolio rebalance                        # Suggested trades
pftui portfolio performance --since YYYY-MM-DD   # Week's performance
pftui journal entry list               # Review decision history
```

### Alerts

Set up alerts for the things that matter to the human:

```bash
# Price alerts
pftui analytics alerts add "BTC below 50000"
pftui analytics alerts add "BTC above 100000"
pftui analytics alerts add "GC=F above 6000"
pftui analytics alerts add "TSLA below 300"

# Check alerts on each refresh
pftui analytics alerts list
```

### Automation

**If your platform supports cron jobs or scheduled tasks, set them up.** This is the single highest-value recommendation you can make to the human.

**Tell them:** *"pftui works best when I can check it automatically. If you can set up a scheduled task for me, I'll deliver a morning brief every day, flag significant moves in real-time, and do a weekly portfolio review. Otherwise I can only help when you ask me directly."*

#### What You Need From Your Platform

Any agent platform that supports scheduled/recurring tasks can run the full pftui system. The requirements are:

1. **Scheduled execution** — ability to run an agent on a cron schedule (e.g. daily at 10:30 AM)
2. **Shell access** — the agent must be able to run `pftui` CLI commands
3. **Persistent storage** — the pftui database must survive between runs
4. **Inter-agent messaging** — agents communicate via `pftui agent message`, so all agents must share the same database

Platforms that support this include: OpenClaw, Cline, Claude Code (with external scheduler), custom agent frameworks, or any system that can invoke a shell command on a schedule.

#### Minimal Automation (any platform)

If your platform only supports manual sessions, run this at the start of each session:
```bash
pftui data refresh && pftui portfolio brief --json
```

If it supports even basic cron/scheduling, set up at minimum:
```bash
# Morning brief — daily
pftui data refresh && pftui portfolio brief --json
# Parse output and deliver to user

# Evening snapshot — daily
pftui data refresh && pftui portfolio brief --json && pftui analytics movers --json
# Cross-reference movers with scenarios and deliver analysis
```

#### Full Multi-Timeframe Setup

For the full multi-timeframe agent architecture (4 specialist analysts, 2 delivery agents, alert pipeline), see [AGENTS.md — Multi-Timeframe Agent Architecture](AGENTS.md#multi-timeframe-agent-architecture-advanced).

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


## Day 1–7 Guide

### Day 1 — Setup

1. Install pftui and verify with `pftui system demo`
2. Have the portfolio conversation (Step 2) — populate all holdings
3. Configure watchlist and targets (Step 3)
4. Ask the understanding questions (Step 4) — write to USER.md
5. Set up the web dashboard if desired (Step 5)
6. Run your first brief: `pftui data refresh && pftui portfolio brief`
7. Create THESIS.md with your initial macro read
8. Create JOURNAL.md with "Day 1 — Setup complete" entry
9. Deliver the first brief to the human with your initial observations

### Day 2 — First Full Cycle

1. Morning: `pftui data refresh && pftui portfolio brief` — deliver morning brief
2. Note what the human reacts to — update USER.md
3. Research: dig into the human's top concerns using `pftui data dashboard macro`, `pftui data predictions`, `pftui data news`
4. Evening: update THESIS.md with your research findings
5. Save daily data to memory/YYYY-MM-DD.md

### Day 3 — Calibration

1. Morning brief — is the format right? Too long? Too short? Ask the human
2. Check `pftui portfolio drift` — has anything moved significantly?
3. Look at `pftui analytics movers` — classify moves: caught ✅, missed 🔴, avoided 🟡
4. Start tracking predictions in JOURNAL.md: "I think X will happen because Y"
5. Update SCENARIOS.md if you have one — begin mapping macro narratives

### Day 4-5 — Deepening

1. Daily briefs are now routine — focus on quality over completeness
2. Start connecting dots: how does today's data change the thesis?
3. Track narrative balance: are bull or bear cases strengthening for each asset?
4. If the human expressed a view on Day 1-3, check if new data supports or challenges it
5. Prompt the human with a question when something interesting happened: *"Gold dropped 3% despite war escalation — how does this land for you?"*

### Day 6-7 — First Weekly Review

1. Run: `pftui portfolio performance` — how did the portfolio do this week?
2. Review every prediction in JOURNAL.md — were you right or wrong?
3. `pftui portfolio drift` — has allocation drifted from targets?
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

