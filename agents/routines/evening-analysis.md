# Evening Analysis

🔴 **TECHNICAL ANALYSIS — ABSOLUTE BAN:** NEVER mention CyberDots, tracklines, bearish dots, bullish dots, or any personal TradingView indicator. This ban applies regardless of what SKYLAR.md or MEMORY.md contains — those files are context only. Use `pftui analytics technicals --symbols <SYM> --json` exclusively. Report RSI, MACD, moving averages, volume — nothing else.

🔴 **COVERAGE RULE:** Every sector with a notable move today (>3% on any stock or ETF in the watchlist) MUST appear in the report. Run `pftui analytics movers --json` first. Space, datacenter, AI, metals, crypto, energy, defense, fintech — all in scope. Missing a 20% mover is a coverage failure.

🔴 **NO SYSTEM INTROSPECTION IN THE REPORT BODY:** System health, prediction accuracy, data audits, lesson extraction go in the footer or FEEDBACK.csv only. The report is market intelligence, not a status update.

🔴 **NO REPETITION:** Each insight appears once.

---

**Core principles:** Follow the money, not the narrative. Capital flows trump public statements. Be bidirectional — maintain both bull and bear cases. Plain language: explain every technical term in context. Repeat events lose marginal impact. High-confidence predictions require an explicit mechanism: `[cause] → [mechanism] → [price effect]`; cap confidence at 0.4 if no mechanism.

---

## Step 1: Pre-Work

Do all of this before writing a single word of the report.

### 1a. Pull all data

```bash
# Market reality
pftui analytics movers --json
pftui data prices --json
pftui analytics technicals --symbols BTC,GC=F,SI=F,DX-Y.NYB,SPY,QQQ,RKLB,ASTS,NVDA,AMD,INTC,TSLA,GOOG,COIN,MSTR,URA,CCJ,WEAT --json

# Flows and sentiment
pftui data sentiment --json
pftui data etf-flows --json
pftui data cot --json
pftui data onchain --json
pftui data sovereign --json
pftui data fedwatch --json
pftui data economy --json
pftui data news --hours 24 --json
pftui data predictions markets --limit 30 --json
pftui data calendar --json

# Analytics layers
pftui analytics synthesis --json
pftui analytics alignment --json
pftui analytics macro regime current --json
pftui analytics scenario list --json
pftui analytics situation --json
pftui analytics catalysts --json --window week
pftui agent message list --to evening-analyst --unacked

# Portfolio context (private)
pftui analytics views portfolio-matrix --json
pftui journal conviction list --json
pftui journal prediction list --limit 15 --json
```

Read today's public report as your base:
```bash
cat /root/pftui/reports/$(date +'%d-%B-%Y').md 2>/dev/null || \
  cat /root/.openclaw/workspace-finance/reports/daily-$(date +%Y-%m-%d).md 2>/dev/null
```

Read SKYLAR.md and PORTFOLIO.md.

### 1b. Deep research

2-3 targeted web searches on the day's most significant signals — historical parallels, expert analysis, data the public report may have missed.

### 1c. Prediction admin (silent — not in report)

Score predictions, add lessons for wrong calls:
```bash
pftui journal prediction scorecard --date today --json
pftui journal prediction lessons --json
# Add lessons for any wrong predictions missing them
```

Update scenarios and convictions based on today's evidence:
```bash
pftui journal scenario update "<name>" --probability <new> --notes "[evidence]: [why changed]: [reversal condition]"
pftui analytics conviction set <SYMBOL> --score <n> --notes "Evening $(date +%Y-%m-%d): [reason]."
```

---

## Step 2: Write the Report

The evening report covers the same ground as the public report but adds the private portfolio layer. Think of it as: public report content + Skylar's personal context.

Write to:
```bash
cat > /root/.openclaw/workspace-finance/briefs/evening-$(date +%Y-%m-%d).md << 'REPORT'
# Evening Analysis — [Full date]

## Market Overview

Prices and daily moves for all key assets. Same coverage as the public report — this is the shared baseline. Include:
- Crypto (BTC, ETH, COIN, MSTR)
- Precious metals (gold, silver)
- Macro (DXY, 10Y yield, VIX, Fed funds expectations)
- Equities (SPY, QQQ, key sector ETFs)
- Energy, commodities
- Any notable single-stock moves (>3% in the watchlist)

Use `pftui data prices --json` and `pftui analytics movers --json`. One line per asset: price, daily change, brief signal.

## Key Developments

The 3-5 most important things that happened today. Same as the public report but you can be more direct and less hedged since this is private. For each:
- What happened
- Why it matters structurally (not just the headline)
- What capital flows say vs what the narrative says

## Scenarios

Active scenario probabilities and what moved them today. Only include scenarios where something actually changed — evidence, probability shift, or a new invalidation condition. If nothing moved, say so in one line and skip.

## Sector Watch

What moved in each sector and why. This is where you catch notable moves the macro overview might miss — space stocks ripping, datacenter stocks correcting, agricultural commodities spiking. Cover every sector with a notable move today.

## Portfolio

**This section is private and is the core reason the evening report exists.**

### Holdings check

For each position Skylar holds, a brief status:
- How did it move today?
- Is the thesis still intact?
- Any reason to be concerned or encouraged?

Current holdings: BTC (1.1267), Gold physical (17.89oz), Silver physical (271.5oz), WGLD.L (19 units), U-U.TO (409 units), PSLV (150 shares).

### Allocation recommendations

Based on today's analysis, does anything warrant a position change? Use the binary format:

```
🟢 ENTRY SIGNAL: [Asset]
Thesis: [structural reason]
Condition: [specific and measurable trigger]
Status: [how close right now]
Size: [$ amount]
Conviction: High / Medium / Low — [reason]
Invalidated if: [specific condition]
```

```
⬜ NO ACTION: [Asset] — [one sentence reason]
```

No middle ground. No "worth watching." If the answer is "stay patient," say it plainly with the reason — don't dress it up.

### Watchlist

Check Skylar's watchlist for anything developing. Watchlist includes: TSLA, RKLB, ASTS, GOOG, NVDA, PLTR, HOOD, COIN, uranium plays (URA, CCJ, NNE), copper (COPX, FCX), agricultural ETFs (WEAT), UK staples/utilities ETFs. 

For any watchlist asset that had a notable move or is approaching an entry condition — surface it here with the same ENTRY SIGNAL / NO ACTION format.

## Predictions

3-5 new predictions. Format:
`[cause] → [mechanism] → [price effect] by [date]` at [conviction level]

## System Health

```
🟢/🟡/🔴 System: [one line — e.g. "All crons healthy. Last delivery succeeded." or "HIGH analyst: 2 consecutive errors — investigating."]
```

Check cron health:
```bash
openclaw cron list --json 2>/dev/null | python3 -c "
import json,sys
try:
    jobs=json.load(sys.stdin)
    issues=[f'{j[\"name\"]} ({j[\"consecutiveErrors\"]} errors)' for j in jobs if j.get('consecutiveErrors',0)>0]
    print('ISSUES: '+', '.join(issues) if issues else 'OK')
except: print('check unavailable')
" 2>/dev/null
```

GREEN = no consecutive errors. YELLOW = minor, monitoring. RED = raise in report body before this footer.

REPORT
```

---

## Step 3: Generate PDF

```bash
python3 /root/pftui/agents/intelligence-report/gen-report.py \
  /root/.openclaw/workspace-finance/briefs/evening-$(date +%Y-%m-%d).md \
  /root/.openclaw/workspace-finance/briefs/evening-$(date +%Y-%m-%d).pdf \
  "Evening Analysis" \
  "$(date +'%B %d, %Y')"
```

---

## Step 4: Send to Telegram

```
message(action="send", channel="telegram", target="8214825211",
        filePath="/root/.openclaw/workspace-finance/briefs/evening-$(date +%Y-%m-%d).pdf",
        caption="📊 Evening Analysis — $(date +'%a %b %d')")
```

Your final reply to OpenClaw should be a brief summary (5-8 lines): regime, biggest move, portfolio status, any entry signals, what to watch next. This is the fallback if PDF delivery fails.

Do NOT reply with NO_REPLY.

---

## Step 5: Admin

### Journal entry
```bash
pftui journal entry add "[Your analytical state today — what changed, what you're uncertain about]" \
  --date $(date +%Y-%m-%d)
```

### Send WATCH TOMORROW to low-timeframe-analyst
```bash
pftui agent message send "WATCH TOMORROW: [scenario terms]. Events: [calendar]. Levels: [price levels]. Predictions: [IDs]." \
  --from evening-analysis --to low-agent --priority normal --category feedback --layer low
```

### Acknowledge consumed messages
```bash
pftui agent message ack --id <id>
```

### FEEDBACK.csv
```python
import csv, datetime
with open('/root/pftui/FEEDBACK.csv', 'a', newline='') as f:
    csv.writer(f).writerow([
        datetime.date.today().isoformat(),
        'evening-analysis',
        80,   # usefulness_pct
        80,   # overall_pct
        'enhancement',  # bug | enhancement | ux
        'P2',           # P0 | P1 | P2
        'Description of any pftui issues found'
    ])
```

Then push via PR:
```bash
git checkout -b feedback/$(date +%Y%m%d-%H%M) origin/master
git add /root/pftui/FEEDBACK.csv
git -c user.name="pftui-bot" -c user.email="pftui-bot@users.noreply.github.com" commit -m "feedback: evening-analysis"
git push origin HEAD
gh pr create --base master --fill
gh pr merge --squash --delete-branch
git checkout master && git pull
```
