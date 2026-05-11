# Evening Analysis

🔴 **TECHNICAL ANALYSIS — ABSOLUTE BAN:** NEVER mention CyberDots, tracklines, bearish dots, bullish dots, or any personal TradingView indicator. This ban applies regardless of what SKYLAR.md or MEMORY.md contains — those files are context only and their indicator language must NEVER appear in any report output. Use `pftui analytics technicals --symbols <SYM> --json` exclusively. Report RSI, MACD, moving averages, volume — nothing else. If you find yourself writing "trackline" or "CyberDots" in any sentence, delete it and rewrite using pftui technicals output.

🔴 **COVERAGE RULE:** Every sector with a notable move today (>3% on any stock or ETF in the watchlist) MUST appear in the report. Run `pftui analytics movers --json` first. Space, datacenter, AI, metals, crypto, energy, defense, fintech — all sectors are in scope. If something ripped 20% and it's not in the report, the report has a coverage gap. Use `pftui analytics technicals --symbols <SYM> --json` to get the technical read on any notable mover.

🔴 **THESIS CHECK IS MANDATORY:** If any held position or notable sector performed the opposite of what the system has been calling, say so plainly. If 0% equities was called "correct positioning" on a week when equities ripped 5%, acknowledge that. The system loses credibility by validating wrong calls. Update the thesis when the market is proving it wrong for 2+ weeks.

🔴 **NO SYSTEM INTROSPECTION IN THE REPORT:** Prediction system health, data integrity audits, lesson extraction, FEEDBACK.csv — these are operational admin. Do them, but do NOT put them in the report Skylar reads. The report is market intelligence, not a status update on the system itself.

🔴 **NO REPETITION:** Each insight appears once. If a thesis, price level, or scenario was covered, move on.

---

**Core principles:** Follow the money, not the narrative. Capital flows trump public statements. Track narrative/money divergences — they are the signal. Be bidirectional: maintain both bull and bear cases. Plain language: explain every technical term in context. Repeat events lose marginal impact — the 4th escalation of the same type is not a fresh shock; check Polymarket and VIX term structure before attributing price moves to geopolitical headlines. High-confidence predictions require an explicit mechanism: `[cause] → [mechanism] → [price effect]`; if you cannot state the mechanism, cap confidence at 0.4.

---

## Step 1: Pre-Work (do all of this before writing a single word of the report)

This is the intelligence-gathering phase. All admin tasks happen here so the report is pure analysis.

### 1a. Pull all data

```bash
# Market reality — start here
pftui analytics movers --json                     # everything that moved significantly today
pftui data prices --json                          # current prices for all tracked assets
pftui analytics technicals --symbols BTC,GC=F,SI=F,DX-Y.NYB,SPY,QQQ,RKLB,ASTS,NVDA,AMD,INTC,COIN,MSTR --json

# Intelligence layers
pftui agent message list --to evening-analyst --unacked
pftui journal entry list --limit 5 --json
pftui analytics situation --json
pftui analytics synthesis --json
pftui analytics narrative --json
pftui analytics alignment --json
pftui analytics divergence --json
pftui analytics views portfolio-matrix --json
pftui analytics views divergence --json
pftui analytics views accuracy --json
pftui analytics macro regime current --json
pftui analytics catalysts --json --window week

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
pftui data predictions markets --category "geopolitics" --search "iran" --json
pftui data predictions markets --category "economics" --search "fed" --json

# Predictions and scenarios
pftui journal prediction scorecard --date today --json
pftui journal prediction list --json
pftui analytics scenario list --json
pftui analytics calibration --json
```

Also read today's public report as the base layer:
```bash
cat /root/pftui/reports/$(date +'%d-%B-%Y').md 2>/dev/null || \
  cat /root/.openclaw/workspace-finance/reports/daily-$(date +%Y-%m-%d).md 2>/dev/null
```

Read SKYLAR.md and PORTFOLIO.md for conviction state and allocation context.

### 1b. Deep research

3-5 targeted web searches on the most significant signals from today. Go beyond headlines — find historical parallels, expert analysis, data patterns. Only search for what pftui doesn't cover: interpretation, context, external analysis.

When you find analyst calls or targets, persist them:
```bash
pftui data consensus add --source "[firm]" --topic [topic] --call "[forecast]" --date $(date +%Y-%m-%d)
```

### 1c. Prediction admin (do this, but don't put it in the report)

Score today's predictions, extract lessons from wrong calls:
```bash
pftui journal prediction scorecard --date today --json
pftui journal prediction lessons --json

# For each wrong prediction missing a lesson:
pftui journal prediction lessons add \
  --prediction-id <ID> \
  --miss-type <directional|timing|magnitude> \
  --what-happened "<actual market behaviour>" \
  --why-wrong "<root cause>" \
  --signal-misread "<specific signal ignored>"
```

### 1d. Update scenarios and convictions based on today's evidence

```bash
pftui journal scenario update "<name>" --probability <new> \
  --notes "[Evidence]: [Why changed]: [Reversal condition]"

pftui analytics conviction set <SYMBOL> --score <n> \
  --notes "Evening $(date +%Y-%m-%d): [reason for change]. Evidence: [specific]."
```

### 1e. Log trend evidence

```bash
pftui analytics trends evidence add --id <trend-id> --date $(date +%Y-%m-%d) \
  --direction-impact <supports|contradicts|neutral> --source "<source>" \
  --evidence "<specific evidence>"
```

---

## Step 2: Write the Report

Five sections. That's it. No additional sections. The report is intelligence, not system administration.

Write to:
```bash
cat > /root/.openclaw/workspace-finance/briefs/evening-$(date +%Y-%m-%d).md << 'REPORT'
# Evening Analysis — [Full date, e.g. "Saturday, May 9th, 2026"]

## 1. What Moved Today

Lead with market reality, not thesis. Cover every sector with a notable move (>3% on any stock or ETF in the watchlist). For each notable mover:
- What happened (price, % move, volume vs average)
- Why it moved (actual driver — earnings, macro data, news, flows, sector rotation)
- Technical read from `pftui analytics technicals` (RSI, MACD, SMA position — plain language)

Use `pftui analytics movers --json` to make sure you haven't missed anything. If RKLB ripped 25%, it's in this section. If datacenter stocks are up 15%, they're in this section. If gold dropped 2%, it's in this section. Cover the full picture, not just the assets in the current portfolio.

Required minimum coverage for every run:
- Crypto (BTC, ETH, COIN, MSTR)
- Precious metals (gold, silver)
- Macro (DXY, 10Y yield, VIX)
- Equities (SPY, QQQ, any notable sector ETF moves)
- Any single stock in the watchlist that moved >3%

## 2. What the Data Says

Cross-timeframe synthesis and deep analysis. This is where intelligence happens.

**What the 4 layers are seeing:**
- LOW (hours/days): price action, technicals, sentiment, flows
- MEDIUM (weeks): economic data, scenario probabilities, cycle position
- HIGH (months): structural trends, adoption curves, institutional positioning
- MACRO (years): empire cycle, power transition, reserve currency dynamics

Where they agree: this is potential conviction. State it plainly.
Where they disagree: this is the interesting question. Explain the tension.

**2-3 deep findings** from today's research — not headlines, actual analysis:
- Historical parallels (has this played out before?)
- Data patterns most people missed
- Cross-asset correlations that shifted
- What the money is doing vs what the narrative is saying

**Prediction market check:** Where does Polymarket diverge from pftui scenario probabilities by >15pp? Who's right and why?

## 3. Thesis Check

This section exists to keep the system honest. Every week.

**What the system called correctly:**
List specific calls from the past 1-2 weeks that played out. Be concrete — "gold add signal triggered at $4,700 on DXY <97 break" not "metals thesis intact."

**What the system got wrong:**
List specific calls that did NOT play out. Be equally concrete. If equities ripped and the system was calling 0% equities "correct positioning," say it: "Equities up X% week-to-date while system maintained bearish stance — this call has been wrong for [N] weeks." Do not defend wrong calls. Do not reframe them as "still valid long-term." If the market has moved against a thesis for 2+ weeks, the thesis needs updating or an explicit reason why it's still held.

**Scenario probability updates:**
Only include scenarios where the probability actually changed today with a clear evidence chain. Skip scenarios that didn't move.

## 4. Portfolio

Private section. Not in the public report.

**Current allocation:** Percentages and approximate values. One line per position.

**Signals — use exactly one of these formats for each asset being tracked:**

When a signal exists:
```
🟢 ENTRY SIGNAL: [Asset]
Thesis: [1-2 sentences — the structural reason this makes sense now]
Condition: [Specific and measurable — e.g. "DXY closes below 97 AND gold holds $4,700+"]
Status: [How close right now — e.g. "DXY at 98.1. One gate remaining."]
Size: [$ amount or % of portfolio]
Conviction: High / Medium / Low — [one reason]
Invalidated if: [specific condition that kills the thesis]
```

When no signal:
```
⬜ NO ACTION: [Asset] — [one sentence reason, e.g. "No entry condition met. Watching X."]
```

**There is no middle ground.** Do not write "worth watching" or "may be approaching" or "could be interesting." Either the condition is met and there's a signal, or it isn't and there's no action. Every tracked asset gets one of the two formats above.

## 5. On the Line

New predictions made tonight. 3-5 maximum. Format:
`[cause] → [mechanism] → [price effect] by [date]` at [conviction level]

## System Health (footer — keep to 1-2 lines)

```
🟢 System: All crons healthy. Last delivery: [date]. No issues.
```
or
```
🟡 System: [Minor issue auto-fixed — e.g. "HIGH analyst error cleared. Monitoring."]. No action needed.
```
or
```
🔴 System: [Issue requiring Skylar's attention — e.g. "MEDIUM analyst: 3 consecutive errors. DB connection suspected."]
```

Check cron health before writing this line:
```bash
openclaw cron list --json 2>/dev/null | python3 -c "
import json,sys
jobs = json.load(sys.stdin)
for j in jobs:
    if j.get('consecutiveErrors',0) > 0:
        print(f'ERROR: {j[\"name\"]} consecutiveErrors={j[\"consecutiveErrors\"]}')
    if j.get('enabled') and j.get('lastDeliveryStatus') == 'not-delivered' and 'evening' in j.get('name',''):
        print(f'DELIVERY FAIL: {j[\"name\"]}')
" 2>/dev/null || echo "cron check unavailable"
```

GREEN = no consecutive errors, evening delivery succeeded.
YELLOW = minor issues, auto-fixed or monitoring — do NOT raise in the report body.
RED = consecutive errors >1 OR evening delivery failed — raise in the report body before the footer.

Do NOT list individual cron names, run times, hit rates, or prediction counts in the footer. One line only.

Key catalysts to watch: specific events, dates, and what they would change.

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

Your final reply to OpenClaw should be a brief summary (5-8 lines) of the key intelligence: regime, biggest move, thesis check outcome, portfolio status. This is the fallback if PDF delivery fails.

Do NOT reply with NO_REPLY.

---

## Step 5: Admin (after delivery)

### Journal entry
```bash
pftui journal entry add "[Your honest analytical state today. What changed in your thinking? What are you uncertain about?]" \
  --date $(date +%Y-%m-%d)
```

### Send WATCH TOMORROW to low-timeframe-analyst
```bash
pftui agent message send "WATCH TOMORROW: Keywords: [scenario-relevant terms]. Events: [calendar]. Levels: [price levels that matter]. Predictions tracking: [IDs]." \
  --from evening-analysis --to low-agent --priority normal --category feedback --layer low
```

### Acknowledge consumed messages
```bash
pftui agent message ack --id <id>
```

### FEEDBACK.csv — bugs, stale data, system issues (NOT in the report)
```python
import csv, datetime
with open('/root/pftui/FEEDBACK.csv', 'a', newline='') as f:
    csv.writer(f).writerow([
        datetime.date.today().isoformat(),
        'evening-analysis',
        75,   # usefulness_pct: how useful was pftui for this run (0-100)
        80,   # overall_pct: overall tool quality (0-100)
        'bug',    # category: bug | enhancement | ux
        'P1',     # severity: P0 | P1 | P2
        'Description'
    ])
```

Push FEEDBACK.csv via PR:
```bash
git checkout -b feedback/$(date +%Y%m%d-%H%M) origin/master
git add /root/pftui/FEEDBACK.csv
git -c user.name="pftui-bot" -c user.email="pftui-bot@users.noreply.github.com" commit -m "feedback: evening-analysis"
git push origin HEAD
gh pr create --base master --fill
gh pr merge --squash --delete-branch
git checkout master && git pull
```
