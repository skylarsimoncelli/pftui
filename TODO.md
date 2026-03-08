# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Intelligence Database (F31)

> Structured storage for the analytical layer that currently lives in fragile markdown files.
> Every table gets a CLI subcommand with full CRUD + `--json`. No TUI integration needed yet.
> All tables must be generic and useful to ANY pftui user — not specific to one setup.
> Update AGENTS.md with usage patterns for each new command after implementation.

### F31.1: Scenarios — Macro scenario planning with probability tracking
- [ ] **`scenarios` table** — `id INTEGER PK, name TEXT UNIQUE, probability REAL, description TEXT, asset_impact TEXT (JSON), triggers TEXT, historical_precedent TEXT, status TEXT DEFAULT 'active', created_at TEXT, updated_at TEXT`
- [ ] **`scenario_signals` table** — `id INTEGER PK, scenario_id INTEGER FK, signal TEXT, status TEXT (watching|triggered|invalidated), evidence TEXT, source TEXT, updated_at TEXT`
- [ ] **`scenario_history` table** — `id INTEGER PK, scenario_id INTEGER FK, probability REAL, driver TEXT, recorded_at TEXT` — automatic log on every probability update for calibration tracking
- [ ] **`pftui scenario` CLI** — `add/list/update/remove/signals/history`. `pftui scenario list` shows name + probability + last updated. `pftui scenario update <name> --probability 42 --driver "NFP miss"` logs to history automatically. `pftui scenario signals <name>` shows signal table for that scenario. All `--json`.
- [ ] **Schema migration** in `db/schema.rs` with `CREATE TABLE IF NOT EXISTS` + migration guards for existing DBs.
- **Why:** Scenario planning is universal — any investor thinks in terms of "what if inflation spikes" or "what if recession hits." The probability history enables calibration: were your 40% events happening ~40% of the time? Currently a 51KB markdown file edited by 3 agents with constant race conditions.

### F31.2: Thesis — Versioned macro outlook by section
- [ ] **`thesis` table** — `id INTEGER PK, section TEXT NOT NULL, content TEXT NOT NULL, conviction TEXT (high|medium|low), updated_at TEXT NOT NULL`. Sections are user-defined strings (e.g. "regime", "btc", "gold", "equities", "risks").
- [ ] **`thesis_history` table** — `id INTEGER PK, section TEXT, content TEXT, conviction TEXT, recorded_at TEXT` — snapshot on every update so you can see how your view evolved
- [ ] **`pftui thesis` CLI** — `list/update/history`. `pftui thesis list` shows current view per section. `pftui thesis update regime --content "Risk-off, stagflation confirmed" --conviction high` auto-snapshots previous version to history. `pftui thesis history regime --limit 10` shows evolution. All `--json`.
- **Why:** Every serious investor maintains a running macro view. Versioning means you can review how your thesis evolved and where you changed your mind. Currently a 24KB markdown file with daily Edit conflicts.

### F31.3: Convictions — Asset conviction scores over time
- [ ] **`convictions` table** — `id INTEGER PK, symbol TEXT NOT NULL, score INTEGER NOT NULL CHECK(score BETWEEN -5 AND 5), notes TEXT, recorded_at TEXT NOT NULL`. One row per (symbol, date) — tracks how conviction changes over time.
- [ ] **`pftui conviction` CLI** — `set/list/history`. `pftui conviction set BTC 0 --notes "Bear tracking, called bull trap correctly"`. `pftui conviction list` shows latest score per symbol. `pftui conviction history BTC` shows score evolution. All `--json`.
- **Why:** The conviction heatmap is literally a database table written in markdown. Moving it to SQLite enables queries like "show me every asset where conviction changed >2 points in the last week" or "what was my gold conviction when I added at $5,150?" Currently buried in a 25KB markdown file.

### F31.4: Research Questions — Open questions with evidence tracking
- [ ] **`research_questions` table** — `id INTEGER PK, question TEXT NOT NULL, evidence_tilt TEXT (neutral|bullish|bearish|strongly_bullish|strongly_bearish), key_signal TEXT, first_raised TEXT, last_updated TEXT, status TEXT DEFAULT 'open' (open|resolved|superseded), resolution TEXT`
- [ ] **`pftui research` CLI** — `add/list/update/resolve`. `pftui research add "Is BTC sovereign money or a controlled asset?" --signal "Epstein revelations"`. `pftui research update <id> --tilt bearish --evidence "NFP -92K confirms economy cracking"`. `pftui research resolve <id> --resolution "Both — controlled AND going up"`. All `--json`.
- **Why:** Research-driven investors maintain lists of "things I'm trying to figure out." Evidence tilt tracking shows whether you're converging on an answer. When a question resolves, the resolution is logged. Currently mixed into a 25KB file alongside unrelated data.

### F31.5: Predictions — Your calls, scored for accuracy
- [ ] **`user_predictions` table** — `id INTEGER PK, claim TEXT NOT NULL, symbol TEXT, conviction TEXT (high|medium|low), target_date TEXT, outcome TEXT (pending|correct|partial|wrong), score_notes TEXT, created_at TEXT, scored_at TEXT`. Distinct from `prediction_cache` (Polymarket) — these are YOUR predictions.
- [ ] **`pftui predict` CLI** — `add/list/score/stats`. `pftui predict add "BTC hits 50k by Oct 2026" --symbol BTC --conviction high --target-date 2026-10-31`. `pftui predict score <id> --outcome correct --notes "Hit $48k on Oct 15"`. `pftui predict stats` shows hit rate by conviction level, asset class, and timeframe. All `--json`.
- **Why:** Prediction tracking with accuracy scoring is how analytical systems improve. If your "high conviction" calls only hit 50%, you're miscalibrated. Currently scattered across MODELS.md with no structured scoring.

### F31.6: Agent Messages — Structured inter-agent communication
- [ ] **`agent_messages` table** — `id INTEGER PK, from_agent TEXT NOT NULL, to_agent TEXT, priority TEXT DEFAULT 'normal' (low|normal|high|critical), content TEXT NOT NULL, category TEXT (signal|priority|feedback|alert), acknowledged INTEGER DEFAULT 0, created_at TEXT, acknowledged_at TEXT`
- [ ] **`pftui agent-msg` CLI** — `send/list/ack`. `pftui agent-msg send --from "evening-planner" --to "morning-research" --priority high --category signal "Oil technicals needed — RSI 89, track for reversal"`. `pftui agent-msg list --to morning-research --unacked` shows unread messages. `pftui agent-msg ack <id>`. All `--json`.
- **Why:** Multi-agent systems need structured message passing. Free-text appending to a 13KB markdown file is fragile — agents can't query "show me unacknowledged high-priority messages" without parsing prose. This replaces AGENT_FEEDBACK.md with indexed, queryable, acknowledging communication.

### F31.7: Daily Notes — Date-keyed narrative entries
- [ ] **`daily_notes` table** — `id INTEGER PK, date TEXT NOT NULL, section TEXT NOT NULL (market|decisions|system|skylar|prices|events), content TEXT NOT NULL, created_at TEXT`. Multiple entries per date allowed (append model).
- [ ] **`pftui notes` CLI** — `add/list/search`. `pftui notes add --date 2026-03-08 --section market "Gold +2.3% on DXY retreat. VIX 29.49 sustained."`. `pftui notes list --date 2026-03-08` shows all entries for that day. `pftui notes search "DXY"` full-text search across all notes. All `--json`.
- **Why:** Replaces 22+ daily markdown memory files (~180KB total) with indexed, searchable storage. Agents write `pftui notes add` instead of appending to `memory/YYYY-MM-DD.md`. Search across all history without parsing files.

### F31.8: Opportunity Cost Tracker — What positioning saved and cost
- [ ] **`opportunity_cost` table** — `id INTEGER PK, date TEXT NOT NULL, event TEXT NOT NULL, asset TEXT, missed_gain_pct REAL, missed_gain_usd REAL, avoided_loss_pct REAL, avoided_loss_usd REAL, was_rational INTEGER (0|1), notes TEXT, created_at TEXT`
- [ ] **`pftui opportunity` CLI** — `add/list/stats`. `pftui opportunity add --date 2026-03-07 --event "Oil +36.5% weekly" --asset CL=F --missed-gain-pct 36.5 --missed-gain-usd 6679 --rational 1 --notes "Had scenario mapped, didn't pull trigger. RSI 89 = unbuyable now."`. `pftui opportunity stats` shows net savings vs. costs over time. All `--json`.
- **Why:** The hardest question in investing: what did your positioning save you vs. cost you? Currently a manual table in MODELS.md. Structured storage enables running totals: "defensive positioning has saved $X and cost $Y since inception." This is the honest scorecard.

### F31.9: Correlation Snapshots — Rolling asset correlations
- [ ] **`correlation_snapshots` table** — `id INTEGER PK, symbol_a TEXT NOT NULL, symbol_b TEXT NOT NULL, correlation REAL NOT NULL, period TEXT NOT NULL (7d|30d|90d), recorded_at TEXT NOT NULL`
- [ ] Populate during `pftui refresh` from `price_history` data — compute rolling correlations for key pairs (BTC-SPX, Gold-DXY, Silver-Gold, Oil-Gold, BTC-Gold, etc.)
- [ ] **`pftui correlations` CLI** — `list/history`. `pftui correlations list` shows current correlation matrix. `pftui correlations history BTC SPY --period 30d` shows how BTC-SPY correlation evolved. All `--json`.
- **Why:** Correlation breaks are regime-change signals. If BTC-SPX correlation drops from 0.8 to 0.2, something structural changed. Currently not tracked at all — agents have to manually compare price moves. This compounds over time — 30 days of correlation data is interesting, 300 days is a thesis.

### F31.10: Regime Classification — Automated market regime detection
- [ ] **`regime_snapshots` table** — `id INTEGER PK, regime TEXT NOT NULL (risk-on|risk-off|stagflation|crisis|transition), confidence REAL, drivers TEXT (JSON array), vix REAL, dxy REAL, yield_10y REAL, oil REAL, gold REAL, recorded_at TEXT NOT NULL`
- [ ] Compute during `pftui refresh` — classify regime based on VIX level (>25 = elevated, >30 = crisis), DXY direction, yield curve shape, oil trend, gold trend. Simple rules-based initially, can be refined.
- [ ] **`pftui regime` CLI** — `current/history`. `pftui regime current` shows current classification with confidence and drivers. `pftui regime history` shows regime transitions over time. All `--json`.
- **Why:** "What regime are we in?" is the first question any macro investor asks. Currently the Evening Planner manually classifies this in THESIS.md. Automated classification from market data gives agents a structured signal. Regime transitions are the highest-signal events in the system.

## P1 — Feature Requests

> User-requested features and high-value improvements.

### Data & Display

### CLI Enhancements

### Analytics

### Infrastructure

---

## P2 — Nice to Have

> Future improvements. Lower priority.

### TUI Polish (batch: ~4hrs total)

### Watchlist (batch: ~2hrs total)

### Scanner (batch: ~3hrs total)

### Distribution
- [ ] **Snap/AUR/Scoop publishing** — Blocked on external publisher accounts + CI secrets for each store.
- [ ] **Homebrew Core** — Blocked on Homebrew inclusion prerequisite (50+ GitHub stars; currently 1).

### Other

---

## P3 — Long Term


---

## Feedback Summary

> Updated: 2026-03-08

### Current Scores (latest per tester)

| Tester | Usefulness | Overall | Trend |
|--------|-----------|---------|-------|
| Market Research | 88% | 82% | ↑ (40→72→78→78→74→88) |
| Eventuality Planner | 82% | 80% | ↑ (38→85→92→85→80→82) |
| Sentinel (Portfolio Analyst) | 85% | 88% | ↑ (78→82→82→78→82→88) |
| Market Close | 92% | 88% | ↑ (68→80→72→88) |
| UX Analyst | — | 75% | → (78→68→72→73→75) |

### Score Trends

- **Market Research:** Strong upswing to 88/82 — best scores yet. Macro technicals (RSI/MACD/SMA) landed on Mar 7 and this tester noticed. Remaining gap: oil technicals in brief (now in macro), and prediction markets showing sports instead of geopolitical. Python script dependency nearly eliminated.
- **Eventuality Planner:** Stable at 82/80. `eod` command and macro dashboard are star features. Pain points: sector command returning only 1 ETF, prediction markets filtering for geopolitics, and missing ag commodity tracking. Wants CME FedWatch.
- **Sentinel (Portfolio Analyst):** Best overall score yet (85/88). TUI visual quality consistently praised. Ratio charts context header (added Mar 7) well received.
- **Market Close:** Strongest absolute scores (92/88) — no new review since Mar 6. `brief + movers + macro` pipeline covers most of the routine. Python script nearly eliminated.
- **UX Analyst:** Slight uptick to 75. Focus shifted from CLI consistency (mostly fixed) to feature discoverability (`pftui config` invisible) and `status --json` gap. Data pipeline reliability improving but predictions/COT still intermittent.

### Top 3 Priorities (Feedback-Driven)

1. ✅ **Brave Search API integration** — COMPLETE (Mar 7, 2026). Config, client, news, economic data, research command all shipped.
2. **Config discoverability** — Config command exists but isn't surfaced in help or README.
3. **PostgreSQL backend support** — The only remaining P1 item.
