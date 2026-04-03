---
name: research-ingestion
description: Fetch external research (YouTube transcripts, Substack articles, podcast notes, analyst reports) and decompose into pftui journal entries — predictions, scenario signals, conviction adjustments, and dated notes. Use when ingesting research from a named source into the pftui system. Triggers on "ingest", "process transcripts", "absorb research", "pull from [source]", or "what did [person] say about [topic]".
---

# Research Ingestion

Fetch research from an external source, extract structured intelligence, and write it into pftui's existing data model. No new tables needed — everything maps to existing journal commands.

## Mapping: Research → pftui

| Research contains... | pftui command | Example |
|---|---|---|
| Time-bound prediction | `journal prediction add` | "Oil hits $190 if Hormuz stays closed through May" |
| Scenario evidence | `journal scenario signal` | "Dixon: Iran war is managed theater" → iran-war scenario |
| Asset conviction argument | `journal conviction set` | "BlackRock capturing BTC supply" → BTC conviction reasoning |
| Factual claim or data point | `journal notes add` | "Hormuz at 70% capacity, 20M bbl/day at risk" |
| Analytical framework | `journal notes add` with tag | "Three Industrial Complexes (MIC/FIC/TIC)" |

## Workflow

### Step 1: Fetch the source material

Determine source type and fetch accordingly:

**YouTube channel** — Use `scripts/fetch-youtube.py`:
```bash
python3 scripts/fetch-youtube.py "<channel_name>" --since 14d --out /tmp/transcripts/
```
Falls back through: youtube-transcript-api → yt-dlp auto-subs → author's blog/website → topic summaries from web_search.

**Substack/Blog** — Use web_fetch on each article URL.

**Podcast** — Search for show notes or transcript services.

**Manual file** — Read from provided path.

### Step 2: Process each document

For each document, read the full text and extract:

1. **Predictions** — Any time-bound, falsifiable claim about future events or prices.
   ```bash
   pftui journal prediction add "<prediction text>" \
     --asset <SYMBOL> --timeframe <short|medium|long> \
     --confidence <1-10> --source "<Author, Date>"
   ```

2. **Scenario signals** — Evidence that strengthens or weakens an existing scenario.
   ```bash
   # List existing scenarios first
   pftui journal scenario list --json
   # Add signal to matching scenario
   pftui journal scenario signal "<scenario_name>" "<signal description>" \
     --direction <strengthens|weakens> --source "<Author, Date>"
   ```

3. **Conviction adjustments** — Strong arguments for or against a specific asset.
   Only adjust if the argument materially changes the analytical picture.
   ```bash
   pftui journal conviction set <SYMBOL> <-5 to +5> \
     --reasoning "<argument summary>" --source "<Author, Date>"
   ```

4. **Research notes** — Key claims, frameworks, data points, and analytical models.
   ```bash
   pftui journal notes add "<note text>" \
     --date <YYYY-MM-DD> --tags "source:<author>,<topic1>,<topic2>"
   ```

### Step 3: Write summary

After processing all documents, write a summary note:
```bash
pftui journal notes add "Research ingestion: <source>, <N> documents, <date range>. \
  Extracted: <X> predictions, <Y> scenario signals, <Z> conviction adjustments, <W> notes. \
  Key themes: <themes>" \
  --date $(date +%Y-%m-%d) --tags "source:<author>,research-ingestion"
```

### Step 4: Report to user

Reply with:
- How many documents processed
- How many items extracted per category (predictions, scenarios, convictions, notes)
- Key themes identified across all documents
- Any documents that couldn't be fetched or processed

## Rules

- **Source attribution:** Every pftui write must include `--source "Author, Date"` or equivalent.
- **Don't duplicate:** Before adding a prediction, check `pftui journal prediction list --json` for similar existing predictions.
- **Don't inflate:** Only extract genuine predictions, not speculation framed as certainty. If the source says "could" or "might", set confidence low (1-3).
- **Scenario matching:** Use `pftui journal scenario list --json` to find existing scenarios. Don't create new ones — map to what exists or note unmatched themes.
- **No portfolio data in notes:** Keep all notes generic. No personal holdings or allocations.
- **Tag consistently:** Always include `source:<author_lastname>` tag for retrieval.
