---
description: Deep, opinionated evaluation of ONE topic — marshals pftui's full analytical arsenal (TA, historic analogs, cycles, economic data, backtesting, positioning) plus the operator's portfolio/thesis/journal and what the multi-timeframe agents have been thinking, then confirms or denies the prevailing expectations with conviction and calls out flawed reasoning. Not a report — a verdict.
argument-hint: <topic or question to evaluate> [--asset SYM] [--quick] [--no-refresh]
---

# /pftui-deepdive — opinionated, evidence-grounded topic evaluation

This is NOT `/pftui-report`. It does not survey the portfolio, does not generate a PDF,
and does not publish anything. It takes **one topic** and renders a **judgment**: it
marshals pftui's full analytical arsenal and the operator's entire strategic context,
confirms or denies the prevailing expectations, **takes a confident stance with
conviction**, and **calls out errors** in the operator's thinking or the pftui agents'
journaling — with the evidence.

The deliverable is the evaluation itself, in chat. Dense with measured numbers, honest
about uncertainty, decisive in conclusion.

## Inputs (parse from the invocation)

- **TOPIC** (the free-text body): the question/thesis/topic to evaluate, verbatim. This is
  the lens for everything. Examples: "is the BTC 4-year cycle still intact or has the major
  cycle topped?", "does gold actually outperform in rate-cutting cycles?", "is the AI capex
  boom a bubble about to pop equities?", "should we trust the desk's convergent-bull gold
  call?".
- **--asset SYM** (optional, repeatable): the asset(s) the topic centers on. If absent,
  infer from the topic (BTC/gold/SPX/etc.) and from held positions.
- **--quick** (optional): orchestrator-only, no subagent fan-out — a fast evaluation from
  the analytics + context the orchestrator gathers itself. Default is **deep** (spawn the
  focused subagents in Step 4).
- **--no-refresh** (optional): skip `pftui data refresh`.

## Hard rules

- **Never use Haiku** for any subagent (per `~/.claude/CLAUDE.md`). Opus or Sonnet only.
- pftui DB is LOCAL SQLITE at `~/Library/Application Support/pftui/pftui.db`. Use the `pftui`
  CLI and `sqlite3`. Never `psql`/`PGPASSWORD`/`/root/...`.
- This is a PRIVATE evaluation — portfolio context is in scope and stays local. Nothing is
  committed or published. (Do NOT trigger the report skill's publish/PR/privacy-audit path.)
- **Accuracy still binds**: every specific number you cite must come from a pftui query, an
  analytics command, or a cited external source — never invented. Distinguish measured fact
  from judgment explicitly.
- **Conviction is required, but so is honesty.** Take a clear stance; assign a confidence;
  but respect every honesty caveat the engine emits (thin analog samples, single-regime
  backtests, small-N cycles, CIs that straddle zero). A confident stance built on
  acknowledged-uncertain evidence is the goal — not false precision, and not mush.

## Pipeline

### Step 0: Setup + recompile

```bash
cd ~/pftui
git log -1 --format='%h %s'
cargo build --release 2>&1 | tail -5
test -x target/release/pftui && cp target/release/pftui "$(which pftui)" 2>/dev/null
pftui --version
DATE_ISO=$(date +%Y-%m-%d)
DB="$HOME/Library/Application Support/pftui/pftui.db"
```

Capture TOPIC verbatim to `/tmp/pftui-deepdive-topic-$DATE_ISO.txt`. Identify the focus
asset(s): parse the topic; if none explicit, default to the held basket + any asset the
topic clearly concerns. Resolve aliases (BTC→BTC-USD for deep series, etc.).

### Step 1: Refresh (unless --no-refresh)

```bash
pftui data refresh 2>&1 | tail -20
```

### Step 2: INGEST FULL STRATEGIC CONTEXT (the "full comprehension" requirement)

Pull everything needed to understand (a) what the operator believes, (b) what the agents
have been thinking, (c) the portfolio, and (d) the topic's history. Read these before any
judgment.

**(a) What the operator believes — thesis + journal:**
```bash
# Mandatory framework + any topic-relevant thesis sections
sqlite3 "$DB" "SELECT section, content FROM thesis WHERE section IN
  ('first-principles','user-profile','decision-frameworks','blind-spots','security-rules',
   'cycle-frameworks','btc-cycle-framework','positioning','structural') ORDER BY section"
# plus topic-specific thesis (btc/gold/silver/dollar/oil/uranium/equities) as relevant
# Operator's own journal + tagged beliefs/intents/facts (60d)
sqlite3 -json "$DB" "SELECT date, section, content FROM daily_notes
  WHERE author='skylar' AND date >= date('now','-60 days') ORDER BY date DESC"
```

**(b) What the agents have been thinking — views, notes, predictions, expectations:**
```bash
# Convergence + every layer's current view on the focus assets
pftui analytics views convergence-all --json
sqlite3 -json "$DB" "SELECT analyst, asset, direction, conviction, reasoning, blind_spots
  FROM analyst_views WHERE updated_at >= datetime('now','-30 days') ORDER BY asset, analyst"
# Recent analyst + synthesis + adversary + antithesis notes (30d)
sqlite3 -json "$DB" "SELECT date, author, substr(content,1,600) AS content FROM daily_notes
  WHERE author LIKE 'analyst-%' AND date >= date('now','-30 days') ORDER BY date DESC LIMIT 120"
# Open predictions + scenario ledger + convictions + misalignments
sqlite3 -json "$DB" "SELECT id, claim, symbol, conviction, timeframe, confidence, target_date
  FROM user_predictions WHERE outcome='pending' ORDER BY created_at DESC LIMIT 80"
pftui journal scenario list --json
pftui journal conviction list --json
pftui research misalignments --json        # which layers are on probation — discount them
# The most recent report's synthesis (the desk's current thesis on record)
sqlite3 -json "$DB" "SELECT content FROM daily_notes WHERE author='analyst-synthesis'
  AND content LIKE '[synthesis-%' ORDER BY date DESC LIMIT 12"
```

**(c) Portfolio:**
```bash
sqlite3 "$DB" "WITH net AS (SELECT symbol, SUM(CASE WHEN tx_type IN ('buy','transfer_in')
  THEN quantity WHEN tx_type IN ('sell','transfer_out') THEN -quantity ELSE 0 END) AS qty
  FROM transactions GROUP BY symbol) SELECT symbol, qty FROM net WHERE qty>0 ORDER BY symbol"
# + the operator-accumulation-plan / posture from the tagged journal notes above
```

State, before analysis: **"Here is what the operator and the desk currently expect about
this topic"** — a crisp summary of the prevailing view you must now test.

### Step 3: RUN THE FULL ANALYTICAL ARSENAL on the topic

Use pftui to its full potential, targeted at the focus asset(s). Run whatever is relevant;
at minimum, reach for each of these families and cite the numbers:

```bash
# TECHNICAL ANALYSIS
pftui analytics technicals --symbols <SYM> --json
pftui analytics technicals structure <SYM> --timeframe daily --json
pftui analytics technicals structure <SYM> --timeframe weekly --json

# HISTORIC PARALLELS (the Environment Engine — docs/ENVIRONMENT-ENGINE.md)
pftui analytics environment current --json                       # macro state + regime quad
pftui analytics analog --asset <SYM> --horizon 90 --json         # closest historic environments + forward returns
pftui analytics analog --asset <SYM> --horizon 180 --json
pftui analytics positioning --asset <SYM> --json                 # fused analog+regime+cycle stance + honesty note

# MARKET CYCLES
pftui analytics cycles clock --asset <SYM> --json                # halving/Loukas/major-vs-4yr (BTC), cycle position (gold)
pftui analytics cycles analyze <SYM> --json

# ECONOMIC DATA
pftui data economy --json
pftui data fedwatch --json
pftui analytics real-rates differentials --json
pftui data real-yields show --since 90d --json

# BACKTESTING — TEST THE TOPIC'S SPECIFIC CLAIM, don't assert folklore
#   express the claim as a strategy and measure it (results carry DSR + bootstrap CI):
pftui analytics strategy segment --asset <SYM> --when "<the claim as a boolean expr>" --json
pftui analytics strategy compare --asset <SYM> --when "<regime A>" --when-label A --vs "<regime B>" --vs-label B --json
pftui analytics strategy backtest --asset <SYM> --entry "<entry expr>" --exit "hold 90d" --json

# SIGNAL EXPECTANCY + PARALLELS CATALOG
pftui research expectancy --asset <SYM> --json
pftui research events --signal <id> --asset <SYM> --json
```

The point of Step 3 is to **replace opinion with measurement** wherever the topic makes a
testable claim. If the topic says "X usually leads to Y", build the backtest and report the
measured number + its honesty stats. If the topic is about cycle position, read the cycle
clock + analog regime. If it's about positioning, read `analytics positioning` and interrogate
its drivers.

### Step 4 (deep mode only): Spawn focused subagents — SAME analysts, topic-pointed

Send ONE message with parallel Agent calls (Opus). Reuse the report analysts, but each is
pointed at THE TOPIC, not at writing per-asset views:

- **The 4 timeframe analysts** (`agents/routines/{low,medium,high,macro}-timeframe-analyst.md`
  as background): each answers the topic FROM ITS HORIZON, citing the Step-3 measurements +
  the relevant DB context. Low = does the near-term tape confirm/deny; Medium = rate/business
  cycle; High = structural/supercycle; Macro = the big cycle + the measured analogs/positioning.
- **The adversary** (`agents/report-prompts/phase2a-adversary.md`): build the strongest case
  AGAINST the conclusion that is emerging, using only the same data.
- **External research** (`agents/report-prompts/phase2c-external-research.md`, WEB): the outside
  view on the topic — where does the desk's read sit vs the consensus, and is the consensus
  itself the trade or the trap.

Give every subagent: the TOPIC, the focus assets, the ingested context (Step 2 highlights),
and the Step-3 measured outputs (paths to the JSON, or inline). Instruct them to return a
focused, evidence-cited evaluation (not DB writes), and to **flag any place the operator's
beliefs or the desk's journaled views contradict the measured evidence**.

### Step 5: SYNTHESIZE THE VERDICT (orchestrator — this is the deliverable)

You write the evaluation. Structure it for decisiveness, not balance:

1. **The topic, sharpened.** One or two sentences: the precise question being judged.
2. **The prevailing expectation.** What the operator believes (from thesis/journal) and what
   the desk currently holds (from convergence/views/synthesis) on this topic — stated plainly
   so it can be confirmed or denied.
3. **The measured evidence.** The Step-3 numbers that bear on it: analog forward-return
   distributions (with n, CI, regime), backtested claims (with DSR / bootstrap CI), cycle
   position, TA structure, expectancy, economic prints. Lead with measurement. Flag every
   honesty caveat (single-regime, thin-N, CI-straddles-zero) inline — do not hide them.
4. **VERDICT — confirm or deny, with conviction.** Take the stance. Assign a confidence
   (and say what drives it up or down). Do not hedge into mush; a well-supported "the desk is
   right, and here is why with numbers" or "this expectation is wrong, and here is the
   contradicting measurement" is the product.
5. **Where the thinking is WRONG.** Explicitly: name the errors you found in the operator's
   beliefs (journal/thesis) AND in the agents' journaled views — each with the specific
   evidence that contradicts it. Be direct. This is the highest-value section; do not soften
   it. (If a layer is on probation per `research misalignments`, say its view is mechanically
   discounted and why.)
6. **What would flip the verdict.** The falsifiable observable(s) that would change your mind —
   a level, a print, a scored prediction. Make it markable.
7. **Portfolio implication.** Tie it to the actual book and the accumulation plan: what this
   verdict means for the held positions and the dry-powder deployment, concretely.

### Step 6: Capture the verdict (optional but recommended)

Write one journal note so the judgment feeds back into the loop and can be scored later:
```bash
pftui journal notes add --section analysis --author analyst-deepdive \
  "[deepdive $DATE_ISO] <topic>: VERDICT <confirm/deny> (<confidence>). Key measured evidence:
   <...>. Errors called out — operator: <...>; desk: <...>. Flips if: <...>."
```
If the verdict implies a falsifiable market call, offer to log it as a scored prediction
(`pftui journal prediction add --source-agent analyst-deepdive --falsify "..."`) so the
deepdive's own track record accrues.

## Voice & stance

- **Confident and decisive.** Reach a judgment and own it. This is a verdict, not a survey.
- **Adversarial toward groupthink.** Actively hunt for where the operator and the desk are
  wrong. "Here is the measurement that contradicts what you/the desk believe" is the most
  valuable thing this skill produces — lead with it when you find it.
- **Measurement over narrative.** Every load-bearing claim is a number from pftui or a cited
  source. Folklore ("X usually happens") is replaced by the backtest of X, with its CI.
- **Honest about uncertainty without being timid.** State the caveats, then still take the
  stance the weight of evidence supports.

## Canonical author

Writes (when it captures a verdict) use author **`analyst-deepdive`** — a measurement/judgment
layer, never a convergence-voting layer. (Registered in AGENTS.md / CLAUDE.md author tables.)

## What this skill does NOT do

- No PDF, no commit, no PR, no website, no publish — it is a private evaluation.
- No full per-asset portfolio survey (that's `/pftui-report`).
- No privacy-audit publish gate (nothing leaves the machine) — but cited-number accuracy still
  binds.
- It does not write analyst_views or move scenario probabilities (it judges them, it doesn't
  vote). The only writes are the optional `analyst-deepdive` verdict note + an optional scored
  prediction.
