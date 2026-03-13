# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Analytics Engine (F31)

F31 analytics engine is complete and shipped.
Detailed implementation checklist is archived in git history/changelog.
Current references:
- `docs/ANALYTICS-ENGINE.md`
- `AGENTS.md` (Analytics Engine chapter)
- `src/commands/analytics.rs`
- `src/db/timeframe_signals.rs`

## P1 — Feature Requests

> User-requested features and high-value improvements.

### Data & Display


### CLI Enhancements


### Infrastructure


### Code Quality


### Analytics Engine: Agent Offload (F38)

> Move mechanical data assembly out of agent token budgets and into native pftui commands.
> Principle: pftui computes, agents interpret. No agent should run raw SQL or stitch
> together data that the analytics engine already has.

### Infrastructure

### Code Quality Quick Wins (audit-driven)


### F32: Native PostgreSQL Backend (epic)

Native SQLite/Postgres parity is complete and shipped. The original migration checklist is archived in git history and changelog entries.
Current authoritative validation/signoff references:
- `docs/BACKEND-PARITY.md`
- `docs/MIGRATING.md`
- `scripts/parity_check.sh`
- `.github/workflows/ci.yml` (`postgres-parity` job)

#### P32: Backend Parity Hardening (production quality)

> F32 established native Postgres paths. P32 closes remaining production-grade parity gaps:
> performance, CI validation, and docs consistency.

---

## P2 — Nice to Have

> Future improvements. Lower priority.

### TUI Polish (batch: ~4hrs total)

### Watchlist (batch: ~2hrs total)

### Scanner (batch: ~3hrs total)

### Distribution

### Other

---

## P3 — Long Term

### F36: Investor Perspectives Panel — Multi-lens analysis via sub-agents

> Inspired by [virattt/ai-hedge-fund](https://github.com/virattt/ai-hedge-fund).
> pftui provides the data engine; investor perspectives are pure agent orchestration.
> Each "investor agent" receives the same analytics engine data but interprets it
> through a fundamentally different investment philosophy, producing independent
> bull/bear/neutral signals with confidence and reasoning.
>
> **Key difference from ai-hedge-fund:** Their project uses a financial API for
> per-stock fundamentals (P/E, FCF, balance sheet). We feed MACRO data — scenarios,
> regime, trends, structural cycles, convictions, correlations — from pftui's
> four-timeframe analytics engine. This makes our version a MACRO hedge fund panel,
> not a stock-picker panel. The question isn't "should I buy AAPL" — it's "how
> should I position across asset classes given the current macro environment."

**Implementation: OpenClaw skill + sub-agent orchestration (no Rust changes)**

**Architecture:**
```
pftui analytics summary --json  ─┐
pftui analytics low --json       │
pftui analytics medium --json    ├─→ Data blob (JSON)
pftui analytics high --json      │
pftui analytics macro --json     │
pftui brief --json               │
pftui conviction list --json    ─┘
         │
         ▼
┌─────────────────────────────────────────────────┐
│  Orchestrator (OpenClaw skill or cron)           │
│  Spawns N sub-agents, each with:                 │
│  - Investor persona system prompt                │
│  - Same data blob                                │
│  - Structured output schema (signal + reasoning) │
│  Collects all responses, builds consensus view   │
└─────────────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────┐
│  Output: Investor Panel      │
│  - Per-investor signal       │
│  - Consensus / divergence    │
│  - Stored via pftui agent-msg│
│  - Optional: Telegram brief  │
└──────────────────────────────┘
```

**Investor Roster:**

Two categories: **Named Legends** (educational, study their philosophy) and
**Generic Archetypes** (practical, dial in a style without a specific name).
Users can enable/disable any persona. Ship with all, default to a curated subset.

**Named Legends (prominent investors):**

| Investor | Philosophy | Lens on data |
|----------|-----------|-------------|
| Ray Dalio | All-weather, risk parity, big cycles | Our MACRO layer IS his framework. Empire transitions, reserve currency. |
| Stanley Druckenmiller | Macro, asymmetric bets, liquidity | Closest to Skylar's style. Patient, conviction-driven, huge when right. |
| George Soros | Reflexivity, regime change, currencies | BRICS, DXY, war premium. "Markets influence the fundamentals they price." |
| Michael Burry | Deep contrarian, short bias, systemic risk | G2 scenario, "everyone is wrong" thesis. Always looking for what breaks. |
| Jim Rogers | Commodities supercycle, emerging markets | Commodity trends, agricultural inflation, gold/silver, BRICS. |
| Warren Buffett | Quality companies, margin of safety, cash | Cash as weapon (Berkshire $300B+). "Be fearful when others are greedy." |
| Cathie Wood | Innovation disruption, 5-year horizon | Counter-view on AI/tech. TSLA/RKLB/genomics. "Bad news is good news." |
| Peter Lynch | Ten-baggers in everyday businesses | Ground-truth consumer economy. What's selling, what's dying. |
| Jesse Livermore | Tape reading, market psychology, momentum | Pure price action. "The market is never wrong, opinions often are." |
| John Templeton | Global contrarian, buy maximum pessimism | "Bull markets are born on pessimism." Emerging market opportunities. |
| Howard Marks | Risk assessment, market cycles, second-level thinking | Cycle positioning. "You can't predict, you can prepare." |
| Paul Tudor Jones | Macro trading, inflation hedging, technical | Gold thesis, inflation protection, 200-day MA as regime signal. |
| Carl Icahn | Activist, corporate governance, unlocking value | Undervalued assets held back by bad management. Restructuring plays. |
| Mark Mobius | Emerging markets, frontier, geopolitical risk | BRICS investment thesis, non-US opportunities, political risk pricing. |
| Kyle Bass | Sovereign debt, currency crises, geopolitical | USD/debt sustainability, Japan/China macro risks, war economics. |

**Generic Archetypes (style-based, no specific person):**

| Archetype | Description | Use case |
|-----------|------------|----------|
| The Momentum Trader | Trend following, relative strength, breakout entry | "What's working and how long does it keep working?" |
| The Value Hunter | Deep discount, mean reversion, patience | "What's cheap relative to intrinsic value right now?" |
| The Risk Paritist | Equal risk across asset classes, volatility targeting | "How should I weight assets so no single risk dominates?" |
| The Yield Seeker | Income focus, dividends, real yields, carry trades | "Where's the best risk-adjusted income stream?" |
| The Macro Tourist | Central bank watching, liquidity flows, positioning data | "Where is the liquidity going and who's positioned wrong?" |
| The Doomsday Prepper | Tail risk, black swans, insurance, hard assets | "What's the worst case and am I protected?" |
| The Techno-Optimist | Innovation, disruption, exponential growth curves | "What's the world going to look like in 10 years?" |
| The Commodity Bull | Supply/demand, cycle theory, hard asset conviction | "What's physically scarce and getting scarcer?" |
| The Bond Vigilante | Yield curve, credit spreads, sovereign risk, duration | "What is the bond market telling us that equities are ignoring?" |
| The Quant | Correlations, mean reversion, factor exposure, statistics | "What does the data say with no narrative overlay?" |

Users can also create custom personas — just drop a markdown file in `personas/`.
The persona file format is standardized: philosophy, decision framework,
known biases, what they look for in data, what they ignore, famous quotes.

**Structured Output Schema (per investor):**
```json
{
  "investor": "stanley_druckenmiller",
  "overall_signal": "bearish",
  "confidence": 78,
  "positioning": {
    "cash": { "signal": "bullish", "weight": "overweight", "reasoning": "Optionality in chaos" },
    "gold": { "signal": "bullish", "weight": "overweight", "reasoning": "Stagflation + war premium" },
    "btc": { "signal": "bearish", "weight": "underweight", "reasoning": "Risk asset in risk-off" },
    "equities": { "signal": "bearish", "weight": "avoid", "reasoning": "Margin compression from oil" },
    "oil": { "signal": "neutral", "weight": "tactical", "reasoning": "War premium, watch ceasefire" }
  },
  "key_insight": "The asymmetric bet is gold — every scenario except risk-on rally is bullish.",
  "what_would_change_my_mind": "BTC holding $72k post-FOMC for 5+ days = risk-on confirmed"
}
```

**Data Collection (single shell script or skill step):**
```bash
#!/bin/bash
# Collect analytics engine data for investor panel
DATA=$(cat <<EOF
{
  "summary": $(pftui analytics summary --json 2>/dev/null),
  "low": $(pftui analytics low --json 2>/dev/null),
  "medium": $(pftui analytics medium --json 2>/dev/null),
  "high": $(pftui analytics high --json 2>/dev/null),
  "macro": $(pftui analytics macro --json 2>/dev/null),
  "brief": $(pftui brief --json 2>/dev/null),
  "convictions": $(pftui conviction list --json 2>/dev/null),
  "scenarios": $(pftui scenario list --json 2>/dev/null),
  "trends": $(pftui trends list --json 2>/dev/null),
  "predictions": $(pftui predict list --json 2>/dev/null),
  "regime": $(pftui regime current --json 2>/dev/null)
}
EOF
)
echo "$DATA"
```

**Skill Files:**
```
skills/investor-panel/
├── SKILL.md                        # Orchestrator instructions
├── collect-data.sh                 # Gathers pftui --json output
├── schema.json                     # Structured output format
├── personas/
│   ├── legends/
│   │   ├── ray_dalio.md
│   │   ├── stanley_druckenmiller.md
│   │   ├── george_soros.md
│   │   ├── michael_burry.md
│   │   ├── jim_rogers.md
│   │   ├── warren_buffett.md
│   │   ├── cathie_wood.md
│   │   ├── peter_lynch.md
│   │   ├── jesse_livermore.md
│   │   ├── john_templeton.md
│   │   ├── howard_marks.md
│   │   ├── paul_tudor_jones.md
│   │   ├── carl_icahn.md
│   │   ├── mark_mobius.md
│   │   └── kyle_bass.md
│   ├── archetypes/
│   │   ├── momentum_trader.md
│   │   ├── value_hunter.md
│   │   ├── risk_paritist.md
│   │   ├── yield_seeker.md
│   │   ├── macro_tourist.md
│   │   ├── doomsday_prepper.md
│   │   ├── techno_optimist.md
│   │   ├── commodity_bull.md
│   │   ├── bond_vigilante.md
│   │   └── quant.md
│   └── custom/                     # User-created personas (gitignored)
│       └── .gitkeep
└── config.toml                     # Which personas to run (default subset)
```

**Persona File Format (standardized):**
```markdown
# [Name or Archetype]

## Philosophy
[2-3 paragraphs on core investment beliefs]

## Decision Framework
[How they evaluate opportunities — what metrics, what signals, what sequence]

## Known Biases
[What they tend to overweight, underweight, or ignore entirely]

## What They Look For In Data
[Specific fields from the analytics engine they'd focus on]

## What They Ignore
[Noise they'd filter out]

## Historical Precedent
[How they've acted in similar macro environments — wars, stagflation, rate cuts]

## Famous Quotes
[3-5 quotes that capture their philosophy, used as grounding anchors]

## Output Emphasis
[What their response should focus on — positioning, timing, risk, opportunity]
```

**Execution Model:**
- Cron-driven (weekly, or on-demand via `/panel` command)
- Orchestrator spawns 8 sub-agents in parallel via `sessions_spawn`
- Each gets: investor persona prompt + full data blob + output schema
- Orchestrator collects responses, computes consensus, stores via `pftui agent-msg`
- Optional: Telegram delivery with consensus summary + notable divergences

**Consensus Computation:**
- Count bull/bear/neutral per asset class across all 8 investors
- Flag "strong consensus" (6+/8 agree) and "divergence" (4/4 split)
- The most valuable output is DIVERGENCE — when Buffett says buy and Burry says sell, that's the conversation worth having

**Example Output (Telegram):**
```
🎯 INVESTOR PANEL — Mar 9, 2026

CONSENSUS:
  Gold:     ████████ 8/8 BULLISH (strongest signal)
  Cash:     ██████░░ 6/8 BULLISH (Buffett, Druckenmiller lead)
  Equities: ██████░░ 6/8 BEARISH (Wood dissents — AI thesis)
  BTC:      ████░░░░ 4/8 SPLIT (Soros bearish, Wood bullish)
  Oil:      ███░░░░░ 3/8 mixed (Rogers bullish, most neutral)

NOTABLE DIVERGENCE:
  🔴 Burry vs 🟢 Dalio on BTC:
    Burry: "BTC is a risk asset in a risk-off world. $40k."
    Dalio: "BTC serves as neutral reserve in multipolar transition."

TOP INSIGHT (Druckenmiller):
  "The asymmetric bet is gold — every scenario except risk-on
  rally is bullish. That's 95% of probability space."
```

**Why this works as a pftui feature (not just our private agent):**
- Any pftui user with an AI agent can use this skill
- The data collection script uses only `pftui` CLI commands
- Persona files are open source, customizable, and educational
- Users can add their own investor personas or remove ones they don't care about
- The `--json` output from every pftui command is the API surface
- Positions pftui as "the data engine that powers AI investment analysis"

**Dependencies:**
- F31 analytics engine complete (especially `--json` on all commands)
- OpenClaw sub-agent spawning (sessions_spawn)
- Persona prompt engineering (the hard part — each investor needs 2-3 pages of philosophy, decision criteria, and known biases)

**NOT in scope:**
- No per-stock fundamental analysis (no Financial Datasets API)
- No trade execution or order generation
- No backtesting (different problem)
- No real-time data (uses pftui cached data from last refresh)

---

## Integration Optimiser Recommendations

> From Integration Optimiser cron — integration gaps between AI agents and pftui

- [ ] [P0] Market Close cron: Change `--section eod` to `--section market` (eod is invalid section)
- [ ] [P1] Morning Research: Move pftui write-back commands BEFORE Telegram send to ensure execution under timeout pressure
- [ ] [P1] Morning Research: Add explicit "WRITE TO PFTUI BEFORE SENDING BRIEF" instruction to prompt
- [ ] [P1] Market Close: Ensure `pftui agent-msg send --from market-close --to evening-planner` executes for notable moves
- [ ] [P1] Morning Research should use `pftui predict add` for every specific market call to build prediction track record
- [ ] [P2] Add MODELS.md Edit guidance header following SCENARIOS.md pattern to prevent agent edit failures

---

### Integration Optimiser Recommendations (2026-03-11)

- **P1: alignment scoring algorithm** — Current alignment score (5.6%) is too basic. Need per-asset alignment score (0-100) that weights: conviction score, trend direction, regime state, scenario probability impact. This is the deployment signal tracker — needs to be the best feature in pftui.
- **P2: prediction resolution criteria** — Add `resolution_criteria` column to `user_predictions` so auto-scoring knows exactly what to check (e.g., "daily close above $5,000" vs "intraday touch of $5,000").
- **P2: scan query keyword matching** — `pftui scan` currently only filters on portfolio metrics (gain_pct, allocation_pct). Add news keyword scanning: `pftui scan --news-keyword "FOMC" --save fomc-watch` that triggers when news_cache contains matching items.

### Prediction Framework Enhancement (2026-03-11)

**For dev cron (pftui code changes):**
- **P2: `pftui predict` needs resolution_criteria field** — Add column and CLI flag so predictions have explicit criteria (e.g., "daily close above $5,000" vs "intraday touch")
- **P1: Alignment scoring algorithm upgrade** — Current 5.6% alignment score is too basic. Need per-asset alignment score (0-100) weighting: conviction score, trend direction, regime state, scenario probability. This IS the deployment signal tracker. Must be pftui's best feature.
- **P2: `pftui scan --news-keyword` flag** — Scan news_cache for keyword matches. `pftui scan --news-keyword "FOMC" --save fomc-watch` triggers when matching news appears. Enables data processors to catch breaking news without web_search.

### Sentinel Integration Tasks (2026-03-11)
**For finance sentinel to complete:**
- **Integration Optimiser should monitor prediction volume per tier** — Flag if Pre-Market makes 0 predictions (should be 2-5), if Evening makes 0 MEDIUM predictions (should be 3-5), etc. Pipeline health includes prediction discipline.
- **Integration Optimiser should track prediction accuracy trends** — Weekly accuracy trend per timeframe. Is the system getting smarter? If LOW accuracy is declining, flag which agent's predictions are dragging it down.
- **Morning Intelligence should surface accuracy trend in weekly handoff** — "Our LOW prediction accuracy this week was 62%, up from 55% last week. Main improvement: better oil cause-effect reasoning after Tuesday's wrong call."
- **Evening Analyst should build a "cause-effect model library"** — When a cause-effect prediction is confirmed (e.g., "CB buying dominates DXY for gold"), log it as a validated model in notes. Future predictions can reference validated models for higher confidence.
- **Data processors need scenario keyword list from Evening Analyst** — Evening should send a "WATCH TOMORROW" agent-msg listing specific keywords/events to flag, so data processors scan for the right things.
- **Weekly Review should audit prediction lessons** — Are lessons being written? Are they specific or generic? Are agents actually changing behavior based on lessons? Flag if lessons are templated.

### F38 Routine Integration (Sentinel, post-dev-cron)

> After dev cron ships each F38 item, update the corresponding agent routine in
> `agents/routines/` to use the new command instead of the manual workaround.
> Changes go to the repo; crons pick up automatically via raw GitHub fetch.


---

## Feedback Summary

> Last reviewed: 2026-03-13

### Current Scores (latest per tester)

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Morning Market Research | 15 | 30 | Mar 9 | ↓ (API hang — fixed in v0.7.0+) |
| Evening Eventuality Planner | 35 | 55 | Mar 9 | ↓ (DB hang — fixed in v0.7.0+) |
| Market Close | 60 | 72 | Mar 9 | ↓ (TIMESTAMPTZ panic + movers bug — fixed) |
| Sentinel Main TUI | 75 | 72 | Mar 10 | ↓ (TUI display corruption) |
| Integration Optimiser | 75 | 70 | Mar 11 | → (first review, scenario bug fixed) |
| UX Analyst | 75 | — | Mar 8 | → |
| Medium Agent | — | — | Mar 13 | → (enhancement only, no scores) |
| Sentinel Evening | — | — | Mar 13 | → (enhancement only, no scores) |
| Low-timeframe Analyst | — | — | Mar 12 | → (enhancement only, no scores) |
| High-timeframe Analyst | — | — | Mar 12 | → (enhancement only, no scores) |
| Morning Brief Agent | — | — | Mar 12 | → (enhancement only, no scores) |
| Evening Analysis | — | — | Mar 12 | → (enhancement only, no scores) |

### Score Analysis

No new scored reviews since Mar 9-11. All three main testers (Morning 15/30, Evening 35/55, Market Close 60/72) bottomed out during the API hang / DB crash / TIMESTAMPTZ panic incidents, all of which are fixed. The Mar 12-13 feedback consists entirely of enhancement requests from analytics pipeline agents — this is a healthy sign that the tool is stable enough for daily use and agents are now requesting workflow refinements rather than reporting crashes.

Notable: both Mar 13 feedback items (medium-agent, sentinel-evening) request `scenario update --notes` support, which was **already shipped on Mar 12** (analytics gaps commit). These agents may be running against a stale binary. Scores should recover significantly once post-fix reviews come in.

### Top 3 Priorities Based on Feedback

1. **Data source reliability** (P1) — 8/10 sources stale, price_history writes stopped. Must stabilize before next review cycle or scores won't recover. Multiple testers affected.
2. **Clippy warnings blocking release** (P1) — 4 clippy errors (`if_same_then_else`, `too_many_arguments`) prevent clean CI. Must fix before cutting v0.10.0.
3. **Brief movers scope** (P2) — Morning brief agent wants market movers outside portfolio in `brief --json` output. High-value for deployment opportunity tracking.

### Release Blocklist

v0.10.0 is ready feature-wise (12 commits since v0.9.0 with significant features) and tests pass (1197/1197). **Blocked by:** 4 clippy warnings-as-errors in `src/commands/analytics.rs` and `src/db/user_predictions.rs`. Fix these and ship.

### Homebrew Core

GitHub stars: 0. Need 50+ before submitting to homebrew-core.
