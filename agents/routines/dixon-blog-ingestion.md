# Simon Dixon Blog Ingestion

🔴 **YOU MUST READ THIS ENTIRE DOCUMENT BEFORE STARTING WORK.**
This is a silent background agent. When complete, reply with exactly: NO_REPLY

**Core principles:** Follow the money, not the narrative. Capital flows trump public statements. Track narrative/money divergences — they are the signal. Wide outcome distributions require cash optionality. Be bidirectional: maintain both bull and bear cases. Plain language: explain every technical term in context. **Repeat events lose marginal impact** — check Polymarket and VIX term structure before predicting price spikes from geopolitical headlines. High-confidence predictions require an explicit mechanism: `[cause] → [mechanism] → [price effect]`; if you cannot state the mechanism, cap confidence at 0.4.

---

## Purpose

Fetch Simon Dixon's latest blog posts from `https://www.simondixon.com/blog.rss`, extract structured intelligence, and write it into pftui's journal and analytics. Dixon's analytical lens: transnational power structures (FIC/MIC/TIC), BTC as monetary sovereignty, managed geopolitical conflict, wealth transfer mechanics. Treat his content as a primary source on power structure analysis — not a trading signal.

## Step 1: Fetch recent posts from RSS

```bash
curl -s "https://www.simondixon.com/blog.rss" > /tmp/dixon-rss.xml
```

Parse the XML to extract posts published in the **last 7 days**. For each post, extract: title, URL, publication date.

```python
import re, sys
from datetime import datetime, timezone, timedelta

xml = open('/tmp/dixon-rss.xml').read()
items = re.findall(r'<item>(.*?)</item>', xml, re.DOTALL)
cutoff = datetime.now(timezone.utc) - timedelta(days=7)

recent = []
for item in items:
    title = re.search(r'<title><!\[CDATA\[(.*?)\]\]>', item) or re.search(r'<title>(.*?)</title>', item)
    link = re.search(r'<link>(.*?)</link>', item)
    pub = re.search(r'<pubDate>(.*?)</pubDate>', item)
    if not (title and link and pub):
        continue
    try:
        from email.utils import parsedate_to_datetime
        pub_dt = parsedate_to_datetime(pub.group(1))
        if pub_dt.replace(tzinfo=timezone.utc) >= cutoff.replace(tzinfo=timezone.utc):
            recent.append({'title': title.group(1), 'url': link.group(1), 'date': pub.group(1)[:16]})
    except:
        pass

for r in recent:
    print(r)
```

If no posts in the last 7 days, **stop here** and reply NO_REPLY.

## Step 2: Fetch and read each post

For each recent post URL, fetch the full content:

```bash
web_fetch <url>
```

Read the full text. Focus on:
- Geopolitical analysis and war framing (Iran, Hormuz, China, BRICS)
- Financial system observations (Fed, central banks, de-dollarisation, CBDCs)
- Bitcoin and monetary sovereignty arguments
- Wealth transfer mechanisms and timing
- Specific predictions or outcome scenarios
- Power structure analysis (who benefits from current events)

## Step 3: Check for duplicates

Before writing anything, check for existing Dixon content:

```bash
pftui journal notes list --json 2>/dev/null | grep -i "dixon" | head -20
pftui journal prediction list --json 2>/dev/null | grep -i "dixon" | head -10
```

Skip any claims already in the system. Tag search: `source:dixon`.

## Step 4: Decompose into pftui primitives

Apply the research-ingestion skill mapping. For each post:

### 4a. Predictions — falsifiable, time-bound claims

```bash
pftui journal prediction add "<claim>" \
  --symbol <SYM> \
  --timeframe <short|medium|long> \
  --conviction medium \
  --confidence 0.5 \
  --source-agent dixon-blog
```

Dixon often speaks in structural terms, not precise dates. Map as:
- "within weeks" → timeframe short, target-date +14 days
- "this year" → timeframe medium, target-date +180 days
- "this decade" → timeframe long, no target-date

**Cap confidence at 0.5 max for Dixon predictions** — his timeframes are frequently vague and his framing is advocacy-adjacent. Mark the signal, don't overweight it.

### 4b. Scenario signals

```bash
# Check existing scenarios first
pftui journal scenario list --json

# Add signal to matching scenario
pftui journal scenario signal "<scenario_name>" "<signal>" \
  --direction <strengthens|weakens> \
  --source "Dixon, <date>"
```

Dixon's content typically maps to these scenarios:
- Iran-US War Escalation or ceasefire/resolution
- Inflation Spike / Big Print
- De-dollarisation / BRICS
- Hard Recession / wealth transfer

### 4c. Conviction adjustments

Only adjust conviction if Dixon presents **new structural evidence**, not just advocacy. His BTC maximalism is known — don't bump BTC conviction just because he's bullish again. Adjust when:
- He cites a specific institutional action or data point
- He identifies a mechanism that isn't already captured
- He describes a structural shift with evidence (not prediction)

```bash
pftui analytics conviction set <SYMBOL> --score <n> \
  --notes "Dixon <date>: <specific evidence>"
```

### 4d. Research notes — frameworks, data, analysis

```bash
pftui journal notes add "<note>" \
  --date <YYYY-MM-DD> \
  --tags "source:dixon,<topic>"
```

Capture:
- Power structure analysis (who controls what, wealth transfer mechanics)
- Specific data points (oil volumes, institutional actions, treaty details)
- Analytical frameworks Dixon applies (follow the money arguments)
- Named actors and their stated vs. actual motivations

## Step 5: Write ingestion summary note

```bash
pftui journal notes add "Dixon blog ingestion: <N> posts processed (<date range>). \
  Extracted: <X> predictions, <Y> scenario signals, <Z> conviction adjustments, <W> notes. \
  Key themes: <1-2 sentence summary of this week's Dixon lens>" \
  --date $(date +%Y-%m-%d) \
  --tags "source:dixon,research-ingestion,weekly"
```

## Rules

- **Source tag always:** `--tags "source:dixon"` on every write
- **No duplicates:** Check before writing
- **No portfolio data:** Notes must be generic — no personal holdings or allocations
- **Advocacy discount:** Dixon is a BTC advocate and power structure analyst. His framing is a lens, not a forecast. Apply it as one input among many, not as ground truth.
- **No Telegram messages:** This is a silent background agent. No `message` tool calls.
- **NO_REPLY:** Your final reply must be exactly `NO_REPLY` — no summary, no report.

---

🔴 **SILENT AGENT.** Do not call the `message` tool. Final reply: `NO_REPLY` only.
