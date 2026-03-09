# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Analytics Engine (F31)

> pftui's core differentiator: a multi-timeframe analytics engine backed by structured SQLite/Postgres.
> Four layers: LOW (hours→days), MEDIUM (weeks→months), HIGH (months→years), MACRO (years→decades).
> Each layer uses different data, updates at different frequencies, and produces different signals.
> Layers constrain downward (macro bias → themes → scenarios → signals) and signal upward.
> See `docs/ANALYTICS-ENGINE.md` for full architecture.
>
> Every table gets a CLI subcommand with full CRUD + `--json`. No TUI integration needed yet.
> All tables/commands must be generic — useful to ANY pftui user, not specific to one setup.
> Update AGENTS.md with usage patterns for each new command after implementation.
>
> **Timeframe mapping:**
> - LOW: `correlation_snapshots`, `regime_snapshots` (+ existing: price_cache, sentiment_cache, prediction_cache, calendar_events, alerts)
> - MEDIUM: `scenarios`, `thesis`, `convictions`, `research_questions`, `user_predictions`, `opportunity_cost`, `daily_notes`, `agent_messages` (+ existing: bls_cache, economic_cache, cot_cache, comex_cache)
> - HIGH: `trend_tracker`, `trend_evidence`, `trend_asset_impact`
> - MACRO: `power_metrics`, `structural_cycles`, `structural_outcomes`, `historical_parallels`, `structural_log`
> - CROSS: `analytics` CLI unifying all layers, `timeframe_signals` for alignment/divergence detection
>
> **Implementation pattern** (follow existing code exactly):
> 1. Schema: add `CREATE TABLE IF NOT EXISTS` to `db/schema.rs` initial batch (for fresh DBs)
>    AND a migration guard block below (for existing DBs): check `pragma_table_info`, `ALTER TABLE` if needed
> 2. Storage: `src/db/<module>.rs` — struct + `from_row()` + CRUD functions using `rusqlite` params
> 3. Command: `src/commands/<module>.rs` — action router calling db functions, handles `--json` via `serde_json`
> 4. CLI: `src/cli.rs` — add `Command` variant with `#[command(name = "...")]` + clap args
> 5. Router: `src/main.rs` — match arm dispatching to commands module
> 6. Module registration: add `pub mod` lines to `src/db/mod.rs` and `src/commands/mod.rs`
>
> Reference implementation: `journal` — see `src/db/journal.rs`, `src/commands/journal.rs`, cli.rs `Journal` variant, main.rs routing.
> All string args use `Option<String>`. Action is first positional `String`. Value is second positional `Option<String>`.

### F31.1: Scenarios — Macro scenario planning [MEDIUM]

_Already implemented by dev cron (scenarios table exists). Verify CLI completeness._

### F31.4: Research Questions — Open questions with evidence tracking [MEDIUM]

- [x] Implemented (`pftui question add/list/update/resolve` + `research_questions` table)

**Files:** `src/db/research_questions.rs`, `src/commands/research_question.rs`, schema/cli/main/mod updates.

Note: `src/commands/research.rs` already exists (Brave search research command). Name the new command `question` to avoid collision: `pftui question add/list/update/resolve`.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS research_questions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    question TEXT NOT NULL,
    evidence_tilt TEXT NOT NULL DEFAULT 'neutral',  -- neutral|leaning_bullish|leaning_bearish|strongly_bullish|strongly_bearish
    key_signal TEXT,      -- what to watch for
    evidence TEXT,        -- accumulated evidence notes
    first_raised TEXT NOT NULL DEFAULT (datetime('now')),
    last_updated TEXT NOT NULL DEFAULT (datetime('now')),
    status TEXT NOT NULL DEFAULT 'open',  -- open|resolved|superseded
    resolution TEXT       -- how it was resolved (filled when status changes)
);
```

**DB functions:**
- `add_question(conn, question, key_signal) -> Result<i64>`
- `list_questions(conn, status_filter: Option<&str>) -> Result<Vec<ResearchQuestion>>`
- `update_question(conn, id, tilt, evidence, key_signal) -> Result<()>` — appends to evidence, updates tilt + last_updated
- `resolve_question(conn, id, resolution, status) -> Result<()>` — sets status + resolution

**CLI variant:**
```rust
/// Track research questions and evidence
#[command(name = "question")]
Question {
    /// Action: add, list, update, resolve
    action: String,
    /// Question text (for add), or search query (for list)
    value: Option<String>,
    #[arg(long)]
    id: Option<i64>,
    /// Evidence tilt: neutral, leaning_bullish, leaning_bearish, strongly_bullish, strongly_bearish
    #[arg(long)]
    tilt: Option<String>,
    /// New evidence to append
    #[arg(long)]
    evidence: Option<String>,
    /// Key signal to watch
    #[arg(long)]
    signal: Option<String>,
    /// Resolution text (for resolve)
    #[arg(long)]
    resolution: Option<String>,
    /// Status filter for list
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    json: bool,
}
```

**Example usage:**
```bash
pftui question add "Will AI success lift or destroy the consumer economy?" --signal "White-collar layoff data, NFP composition"
pftui question update --id 1 --tilt leaning_bearish --evidence "NFP -92K, wages +3.8% = stagflation. PLTR only green tech."
pftui question resolve --id 3 --resolution "Both — controlled AND going up. Epstein angle doesn't invalidate BTC."
pftui question list --status open --json
```

---

### F31.5: User Predictions — Your calls, scored for accuracy [MEDIUM]

- [x] Implemented (`pftui predict add/list/score/stats` + `user_predictions` table)

**Files:** `src/db/user_predictions.rs`, `src/commands/predict.rs`, schema/cli/main/mod updates.

Note: distinct from `prediction_cache`/`predictions_cache` (Polymarket data). These are YOUR predictions.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS user_predictions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    claim TEXT NOT NULL,
    symbol TEXT,
    conviction TEXT NOT NULL DEFAULT 'medium',  -- high|medium|low
    target_date TEXT,     -- when you expect the prediction to resolve
    outcome TEXT NOT NULL DEFAULT 'pending',  -- pending|correct|partial|wrong
    score_notes TEXT,     -- explanation of outcome when scored
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    scored_at TEXT        -- when the prediction was scored
);
```

**DB functions:**
- `add_prediction(conn, claim, symbol, conviction, target_date) -> Result<i64>`
- `list_predictions(conn, outcome_filter: Option<&str>, symbol: Option<&str>, limit: Option<usize>) -> Result<Vec<UserPrediction>>`
- `score_prediction(conn, id, outcome, notes) -> Result<()>` — sets outcome + score_notes + scored_at
- `get_stats(conn) -> Result<PredictionStats>` — compute hit rate by conviction level and by symbol

**Stats struct:**
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct PredictionStats {
    pub total: usize,
    pub scored: usize,
    pub pending: usize,
    pub correct: usize,
    pub partial: usize,
    pub wrong: usize,
    pub hit_rate_pct: f64,  // (correct + 0.5*partial) / scored * 100
    pub by_conviction: HashMap<String, ConvictionStats>,  // high/medium/low breakdown
    pub by_symbol: HashMap<String, ConvictionStats>,      // per-asset breakdown
}
```

**CLI variant:**
```rust
/// Track your market predictions and score accuracy
#[command(name = "predict")]
Predict {
    /// Action: add, list, score, stats
    action: String,
    /// Prediction claim text (for add)
    value: Option<String>,
    #[arg(long)]
    id: Option<i64>,
    #[arg(long)]
    symbol: Option<String>,
    #[arg(long)]
    conviction: Option<String>,
    /// Expected resolution date
    #[arg(long)]
    target_date: Option<String>,
    /// Outcome: correct, partial, wrong
    #[arg(long)]
    outcome: Option<String>,
    /// Scoring notes
    #[arg(long)]
    notes: Option<String>,
    /// Filter: pending, correct, partial, wrong
    #[arg(long)]
    filter: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}
```

**Example usage:**
```bash
pftui predict add "BTC hits 50k by Oct 2026" --symbol BTC --conviction high --target-date 2026-10-31
pftui predict add "Oil above 100 within 3 weeks" --symbol CL=F --conviction medium --target-date 2026-03-28
pftui predict score --id 1 --outcome correct --notes "Hit \$48k on Oct 15"
pftui predict stats --json
pftui predict list --filter pending
```

---

### F31.6: Agent Messages — Structured inter-agent communication [CROSS]

- [x] Implemented (`pftui agent-msg send/list/ack/ack-all/purge` + `agent_messages` table)

**Files:** `src/db/agent_messages.rs`, `src/commands/agent_msg.rs`, schema/cli/main/mod updates.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS agent_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_agent TEXT NOT NULL,
    to_agent TEXT,         -- NULL = broadcast to all
    priority TEXT NOT NULL DEFAULT 'normal',  -- low|normal|high|critical
    content TEXT NOT NULL,
    category TEXT,         -- signal|feedback|alert|handoff|escalation
    layer TEXT,            -- low|medium|high|macro|cross (analytics engine layer)
    acknowledged INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    acknowledged_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_messages_to ON agent_messages(to_agent);
CREATE INDEX IF NOT EXISTS idx_agent_messages_ack ON agent_messages(acknowledged);
```

**DB functions:**
- `send_message(conn, from, to, priority, content, category, layer) -> Result<i64>`
- `list_messages(conn, to: Option<&str>, unacked_only: bool, since: Option<&str>, limit: Option<usize>) -> Result<Vec<AgentMessage>>`
- `acknowledge(conn, id) -> Result<()>` — sets acknowledged=1 + acknowledged_at
- `acknowledge_all(conn, to: &str) -> Result<usize>` — ack all for a recipient
- `purge_old(conn, days: usize) -> Result<usize>` — delete acknowledged messages older than N days

**CLI variant:**
```rust
/// Inter-agent structured message passing
#[command(name = "agent-msg")]
AgentMsg {
    /// Action: send, list, ack, ack-all, purge
    action: String,
    /// Message content (for send)
    value: Option<String>,
    #[arg(long)]
    id: Option<i64>,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    to: Option<String>,
    /// Priority: low, normal, high, critical
    #[arg(long)]
    priority: Option<String>,
    /// Category: signal, feedback, alert, handoff, escalation
    #[arg(long)]
    category: Option<String>,
    /// Analytics engine layer: low, medium, high, macro, cross
    #[arg(long)]
    layer: Option<String>,
    /// Show only unacknowledged
    #[arg(long)]
    unacked: bool,
    /// Time filter
    #[arg(long)]
    since: Option<String>,
    /// Days for purge
    #[arg(long)]
    days: Option<usize>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}
```

**Example usage:**
```bash
# LOW → MEDIUM: intraday signal escalated to scenario analysis
pftui agent-msg send "BTC-SPX correlation broke from 0.8 to 0.3 — potential regime shift" --from low-refresh --to evening-planner --priority high --category escalation --layer low

# MEDIUM → HIGH: economic data confirming a structural trend
pftui agent-msg send "NFP -92K confirms white-collar displacement accelerating" --from evening-planner --to structural-analyst --priority normal --category signal --layer medium

# HIGH → MACRO: trend shift with structural implications
pftui agent-msg send "BRICS payment system processing $2B/day — reserve currency transition accelerating" --from structural-analyst --to macro-analyst --priority high --category escalation --layer high

# MACRO → MEDIUM: structural context constraining scenario analysis
pftui agent-msg send "Stage 6 confirmed — weight war/disorder scenarios higher" --from macro-analyst --to evening-planner --priority normal --category feedback --layer macro

# Cross-layer broadcast
pftui agent-msg send "FOMC decision in 2 hours — all layers expect volatility" --from morning-research --priority critical --category alert --layer cross

# Query by layer
pftui agent-msg list --layer low --unacked
pftui agent-msg list --layer medium --since 2026-03-01
pftui agent-msg ack --id 42
pftui agent-msg purge --days 30
```

**Layer escalation pattern:**
Signals flow UP through layers (Low→Medium→High→Macro) via `--category escalation`.
Context flows DOWN through layers (Macro→High→Medium→Low) via `--category feedback`.
This creates the bidirectional intelligence loop where intraday data informs structural
analysis and structural context constrains intraday interpretation.

---

### F31.7: Daily Notes — Date-keyed narrative entries [CROSS]

- [x] Implemented (`pftui notes add/list/search/remove` + `daily_notes` table)

**Files:** `src/db/daily_notes.rs`, `src/commands/notes.rs`, schema/cli/main/mod updates.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS daily_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT NOT NULL,   -- YYYY-MM-DD
    section TEXT NOT NULL DEFAULT 'general',  -- market|decisions|system|analysis|events|general
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_daily_notes_date ON daily_notes(date);
CREATE INDEX IF NOT EXISTS idx_daily_notes_section ON daily_notes(section);
```

**DB functions:**
- `add_note(conn, date, section, content) -> Result<i64>`
- `list_notes(conn, date: Option<&str>, section: Option<&str>, limit: Option<usize>) -> Result<Vec<DailyNote>>`
- `search_notes(conn, query: &str, since: Option<&str>, limit: Option<usize>) -> Result<Vec<DailyNote>>` — `WHERE content LIKE '%query%'`
- `remove_note(conn, id) -> Result<()>`

**CLI variant:**
```rust
/// Date-keyed research notes and narrative entries
#[command(name = "notes")]
Notes {
    /// Action: add, list, search, remove
    action: String,
    /// Content (for add) or search query (for search)
    value: Option<String>,
    #[arg(long)]
    id: Option<i64>,
    /// Date YYYY-MM-DD (defaults to today for add)
    #[arg(long)]
    date: Option<String>,
    /// Section: market, decisions, system, analysis, events, general
    #[arg(long)]
    section: Option<String>,
    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}
```

**Example usage:**
```bash
pftui notes add "Gold +2.3% on DXY retreat from 99.3 to 98.86. War premium building. NFP -92K triggered safe haven bid." --section market --date 2026-03-07
pftui notes list --date 2026-03-07
pftui notes search "DXY" --since 2026-03-01
```

---

### F31.8: Opportunity Cost Tracker — What positioning saved and cost [MEDIUM]

- [x] Implemented (`pftui opportunity add/list/stats` + `opportunity_cost` table)

**Files:** `src/db/opportunity_cost.rs`, `src/commands/opportunity.rs`, schema/cli/main/mod updates.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS opportunity_cost (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT NOT NULL,
    event TEXT NOT NULL,
    asset TEXT,
    missed_gain_pct REAL,     -- what we missed (positive number)
    missed_gain_usd REAL,
    avoided_loss_pct REAL,    -- what we avoided (positive number)
    avoided_loss_usd REAL,
    was_rational INTEGER NOT NULL DEFAULT 1,  -- 1 = rational miss, 0 = mistake
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**DB functions:**
- `add_entry(conn, date, event, asset, missed_gain_pct, missed_gain_usd, avoided_loss_pct, avoided_loss_usd, was_rational, notes) -> Result<i64>` — all numeric fields are `Option`
- `list_entries(conn, since: Option<&str>, asset: Option<&str>, limit: Option<usize>) -> Result<Vec<OpportunityCostEntry>>`
- `get_stats(conn, since: Option<&str>) -> Result<OpCostStats>` — totals: sum missed gains, sum avoided losses, net, rational miss count

**Stats struct:**
```rust
pub struct OpCostStats {
    pub total_entries: usize,
    pub total_missed_usd: f64,
    pub total_avoided_usd: f64,
    pub net_usd: f64,  // avoided - missed (positive = positioning helped)
    pub rational_misses: usize,
    pub mistakes: usize,
}
```

**CLI variant:**
```rust
/// Track what your positioning saved and cost you
#[command(name = "opportunity")]
Opportunity {
    /// Action: add, list, stats
    action: String,
    /// Event description (for add)
    value: Option<String>,
    #[arg(long)]
    date: Option<String>,
    #[arg(long)]
    asset: Option<String>,
    #[arg(long)]
    missed_gain_pct: Option<f64>,
    #[arg(long)]
    missed_gain_usd: Option<f64>,
    #[arg(long)]
    avoided_loss_pct: Option<f64>,
    #[arg(long)]
    avoided_loss_usd: Option<f64>,
    /// Was this a rational decision? (true/false, default true)
    #[arg(long)]
    rational: Option<bool>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}
```

---

### F31.9: Correlation Snapshots — Rolling asset correlations [LOW]

- [x] Implemented (`correlation_snapshots` table, refresh snapshot pipeline, `pftui correlations history`, `--store`)

**Files:** `src/db/correlation_snapshots.rs`, `src/commands/correlations.rs` (EXISTS — extend it), schema update.

Note: `src/commands/correlations.rs` already exists and computes correlations from `price_history`. Extend it to STORE snapshots and show history.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS correlation_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_a TEXT NOT NULL,
    symbol_b TEXT NOT NULL,
    correlation REAL NOT NULL,
    period TEXT NOT NULL DEFAULT '30d',  -- 7d|30d|90d
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_corr_snap_pair ON correlation_snapshots(symbol_a, symbol_b);
CREATE INDEX IF NOT EXISTS idx_corr_snap_date ON correlation_snapshots(recorded_at);
```

**Integration with refresh pipeline:**
In `src/commands/refresh.rs`, after prices are fetched and `price_history` is updated, compute Pearson correlations for configurable pairs from `price_history` table (need ≥7 days of data). Store results in `correlation_snapshots`. Default pairs to compute (if both symbols have price history): held positions × macro symbols (SPY, DXY, GC=F, CL=F, ^VIX).

**DB functions:**
- `store_snapshot(conn, symbol_a, symbol_b, correlation, period) -> Result<i64>`
- `list_current(conn, period: Option<&str>) -> Result<Vec<CorrelationSnapshot>>` — latest per pair
- `get_history(conn, symbol_a, symbol_b, period: Option<&str>, limit: Option<usize>) -> Result<Vec<CorrelationSnapshot>>`

**Extend existing `pftui correlations` CLI:**
- Current behavior (compute live from price_history) becomes default
- Add `--store` flag to save current computation as snapshot
- Add `"history"` action: `pftui correlations history BTC SPY --period 30d --limit 30`

---

### F31.10: Regime Classification — Automated market regime detection [LOW]

- [x] Implemented (`regime_snapshots` table, refresh-time classification, `pftui regime current/history/transitions`)

**Files:** `src/db/regime_snapshots.rs`, `src/commands/regime.rs`, schema/cli/main/mod updates.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS regime_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    regime TEXT NOT NULL,  -- risk-on|risk-off|stagflation|crisis|transition
    confidence REAL,       -- 0.0-1.0
    drivers TEXT,          -- JSON array of strings: ["VIX >25", "oil RSI >80", ...]
    vix REAL,
    dxy REAL,
    yield_10y REAL,
    oil REAL,
    gold REAL,
    btc REAL,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**Classification rules** (implement in `src/commands/regime.rs`):
```
crisis:      VIX > 30 AND oil > 90
stagflation: VIX > 25 AND oil > 80 AND gold trending up AND equities trending down
risk-off:    VIX > 25 OR (DXY rising AND gold rising AND equities falling)
risk-on:     VIX < 20 AND equities trending up AND DXY stable/falling
transition:  doesn't clearly match any above
```
Read VIX, DXY, oil, gold, BTC from `price_cache`. Read 10Y yield from `economic_cache` or `price_cache` (^TNX). Confidence = how many sub-conditions match within the classification.

**Integration with refresh:** Compute regime classification at end of `pftui refresh`. Store snapshot. Only store if regime changed OR once per day (avoid duplicate rows).

**DB functions:**
- `store_regime(conn, regime, confidence, drivers_json, vix, dxy, yield_10y, oil, gold, btc) -> Result<i64>`
- `get_current(conn) -> Result<Option<RegimeSnapshot>>`
- `get_history(conn, limit: Option<usize>) -> Result<Vec<RegimeSnapshot>>`
- `get_transitions(conn, limit: Option<usize>) -> Result<Vec<RegimeSnapshot>>` — only rows where regime differs from previous

**CLI variant:**
```rust
/// Market regime classification and history
#[command(name = "regime")]
Regime {
    /// Action: current, history, transitions
    action: String,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}
```

**Human-readable output:**
```
Current Regime: RISK-OFF (confidence: 0.85)
  Drivers: VIX 29.5 (>25), Gold trending up, Equities trending down, Oil RSI 89
  VIX: 29.5 | DXY: 98.86 | 10Y: 4.13% | Oil: $91.27 | Gold: $5,181 | BTC: $67,164
  Since: 2026-03-01 (8 days)
```

---



### F31.12: High-Timeframe Trends — Trend tracking [HIGH]

- [x] Implemented (`pftui trends` add/list/update/evidence/impact/dashboard + trend tables)

The only missing analytics layer. LOW, MEDIUM, and MACRO are covered by F31.1-F31.11.
HIGH-timeframe tracks multi-quarter structural trends (AI, energy, demographics, politics).

**Files:** `src/db/trends.rs` (NEW), `src/commands/trends.rs` (NEW), schema/cli/main/mod updates.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS trend_tracker (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    timeframe TEXT NOT NULL DEFAULT 'high',  -- high|macro
    direction TEXT NOT NULL DEFAULT 'neutral',  -- accelerating|stable|decelerating|reversing
    conviction TEXT NOT NULL DEFAULT 'medium',  -- high|medium|low
    category TEXT,  -- ai|energy|demographics|politics|trade|technology|regulation
    description TEXT,
    asset_impact TEXT,  -- JSON: {"NVDA": "bullish", "XLK": "bullish"}
    key_signal TEXT,  -- what would change direction
    status TEXT NOT NULL DEFAULT 'active',  -- active|paused|resolved
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS trend_evidence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trend_id INTEGER NOT NULL REFERENCES trend_tracker(id) ON DELETE CASCADE,
    date TEXT NOT NULL,
    evidence TEXT NOT NULL,
    direction_impact TEXT,  -- strengthens|weakens|neutral
    source TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_trend_evidence_trend ON trend_evidence(trend_id);

CREATE TABLE IF NOT EXISTS trend_asset_impact (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trend_id INTEGER NOT NULL REFERENCES trend_tracker(id) ON DELETE CASCADE,
    symbol TEXT NOT NULL,
    impact TEXT NOT NULL,  -- bullish|bearish|neutral
    mechanism TEXT,  -- HOW the trend affects this asset
    timeframe TEXT,  -- when the impact materialises
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_trend_asset_trend ON trend_asset_impact(trend_id);
```

**DB functions:**
- `add_trend(conn, name, timeframe, direction, conviction, category, description, asset_impact, key_signal)` → `Result<i64>`
- `list_trends(conn, status: Option<&str>, category: Option<&str>)` → `Result<Vec<Trend>>`
- `update_trend(conn, name, direction, conviction, description, key_signal, status)` → `Result<()>`
- `add_evidence(conn, trend_id, date, evidence, direction_impact, source)` → `Result<i64>`
- `list_evidence(conn, trend_id, limit: Option<usize>)` → `Result<Vec<TrendEvidence>>`
- `add_asset_impact(conn, trend_id, symbol, impact, mechanism, timeframe)` → `Result<i64>`
- `list_asset_impacts(conn, trend_id)` → `Result<Vec<TrendAssetImpact>>`
- `get_impacts_for_symbol(conn, symbol)` → `Result<Vec<(Trend, TrendAssetImpact)>>` — which trends affect this asset

**CLI:** `pftui trends add/list/update/evidence-add/evidence-list/impact-add/impact-list/dashboard`

**Example usage:**
```bash
pftui trends add "AI White-Collar Displacement" --category ai --direction accelerating --conviction medium --description "AI success destroys consumer economy through white-collar displacement spiral" --signal "NFP composition shift: white-collar losses exceeding blue-collar"
pftui trends evidence-add --trend "AI White-Collar Displacement" --date 2026-03-07 "NFP -92K with wages +3.8% = stagflation" --impact strengthens --source "BLS"
pftui trends impact-add --trend "AI White-Collar Displacement" --symbol PLTR --impact bullish --mechanism "Defense-AI convergence, government contracts immune to consumer spending"
pftui trends dashboard --json
```

---

### F31.13: Analytics Engine CLI — Multi-timeframe dashboards [CROSS]

- [x] Implemented (`pftui analytics summary/low/medium/high/macro/alignment/signals`)

The unified view across all four analytics layers. Reads from all F31 tables + existing data tables. No new storage — pure presentation and cross-referencing.

**Files:** `src/commands/analytics.rs` (NEW), cli/main/mod updates.

**CLI variant:**
```rust
/// Multi-timeframe analytics engine
#[command(name = "analytics")]
Analytics {
    /// View: summary, low, medium, high, macro, alignment
    action: String,
    /// Symbol filter for alignment view
    #[arg(long)]
    symbol: Option<String>,
    #[arg(long)]
    json: bool,
}
```

**Actions:**
- `"summary"` — 4-layer combined view: regime + top scenario + top trend + structural cycle + alignment score. Pulls from: `regime_snapshots`, `scenarios`, `trend_tracker`, `structural_cycles`, `convictions`.
- `"low"` — expanded LOW layer: prices, sentiment, calendar, correlations, regime, alerts. Pulls from: `price_cache`, `sentiment_cache`, `prediction_cache`, `calendar_events`, `correlation_snapshots`, `regime_snapshots`, `alerts`.
- `"medium"` — expanded MEDIUM layer: scenarios with probabilities, thesis sections, convictions, open predictions. Pulls from: `scenarios`, `thesis`, `convictions`, `research_questions`, `user_predictions`, `bls_cache`, `cot_cache`, `comex_cache`.
- `"high"` — expanded HIGH layer: active trends with direction, latest evidence, asset impacts. Pulls from: `trend_tracker`, `trend_evidence`, `trend_asset_impact`.
- `"macro"` — expanded MACRO layer: structural dashboard. Pulls from: `power_metrics`, `structural_cycles`, `structural_outcomes`, `historical_parallels`, `structural_log`.
- `"alignment"` — per-asset matrix showing what each timeframe says. Computed from: regime per-asset signal + scenario asset_impact + trend asset_impact + structural outcome asset_implications. No new storage needed.

**Human-readable `analytics summary` output:**
```
Analytics Engine — Multi-Timeframe Intelligence
════════════════════════════════════════════════════════════════

LOW (hours → days)                        Updated: 2h ago
  Regime: RISK-OFF (0.85) │ F&G: 🔴 10 │ VIX: 29.5 ⚠️
  Alerts: 3 triggered │ Movers: 7 > 3%

MEDIUM (weeks → months)                   Updated: 8h ago
  Top Scenario: Geopolitical War (42%) ↑
  Thesis: Stagflation (HIGH) │ Gold +4, BTC 0, Equities -2

HIGH (months → years)                     Updated: 1w ago
  ▲ AI Displacement, Nuclear Renaissance, BRICS De-Dollar
  ▼ Space Commercialisation (funding tightening)

MACRO (years → decades)                   Updated: 1w ago
  Stage 5→6 │ USD reserve declining │ Gradual Decline 45%

ALIGNMENT: ████████░░ 80% — Gold: all 4 agree bullish
```

**Human-readable `analytics alignment` output:**
```
  Symbol │ Low    │ Medium │ High   │ Macro  │ Consensus
  ───────┼────────┼────────┼────────┼────────┼──────────
  GC=F   │ ▲ Bull │ ▲ Bull │ ▲ Bull │ ▲ Bull │ STRONG BUY
  BTC    │ ▼ Bear │ ▼ Bear │ → Flat │ → Flat │ AVOID
  SPY    │ ▼ Bear │ ▼ Bear │ ▼ Bear │ ▼ Bear │ STRONG AVOID
```

Alignment logic: query each layer's latest signal per asset. 4/4 = STRONG. 3/4 = directional. Split = MIXED. Computed, not stored.

---

### F31.14: Cross-Timeframe Signal Detection [CROSS]

Automated detection of alignment and divergence across timeframes. Future — after F31.13 is working.

- [x] **`timeframe_signals` table** — `id, signal_type (alignment|divergence|transition), layers TEXT (JSON), assets TEXT, description TEXT, severity TEXT (info|notable|critical), detected_at TEXT`
- [x] Compute during `pftui refresh`: compare regime, scenario probabilities, trend directions, structural outcomes. Log when layers agree or diverge on a specific asset.
- [x] **`pftui analytics signals`** — show active cross-timeframe signals. `--json`.
- [x] Integrate with `pftui brief --agent` — include top cross-timeframe signal in agent blob.

---

### F31.15: Documentation & Product Updates [CROSS]

After F31.1-F31.14 are implemented:

- [x] **README.md** — new "Analytics Engine" section. Multi-timeframe diagram. Position pftui as an analytics platform.
- [x] **Website `index.html`** — Analytics Engine section with 4-layer diagram. Terminal demo scene: `pftui analytics summary`. Comparison table row: "Multi-Timeframe Analytics."
- [x] **AGENTS.md** — "Analytics Engine" chapter: which commands per timeframe, what signals to look for, how to update each layer.
- [x] **PRODUCT-VISION.md** — center vision on the analytics engine as core differentiator.
- [x] **Marketing line:** "The only retail tool with a multi-timeframe analytics engine. From intraday volatility to decade-long empire cycles."

---

### F31.16: AGENTS.md Documentation Update [CROSS]

After ALL F31 tables are implemented, update `AGENTS.md` with a new section:

- [x] Implemented (Analytics Engine chapter added to AGENTS.md)

```markdown
## Analytics Engine

pftui's core is a multi-timeframe analytics engine operating across four layers:
LOW (hours→days), MEDIUM (weeks→months), HIGH (months→years), MACRO (years→decades).
Each layer uses different data, updates at different frequencies, and produces different signals.
Layers constrain downward and signal upward. Use `pftui analytics summary` for the combined view.

### Scenarios (`pftui scenario`)
Track macro scenarios with probability estimates. Each probability update is logged
to history for calibration. Signals track evidence for/against each scenario.

### Thesis (`pftui thesis`)
Versioned macro outlook by section. Every update snapshots the previous version.
Query history to see how your views evolved.

### Convictions (`pftui conviction`)
Asset-level conviction scores (-5 to +5) over time. Append-only log — every
`set` creates a new row. Current conviction = latest row per symbol.

### Research Questions (`pftui question`)
Open questions with evidence tilt tracking. Update evidence as data arrives.
Resolve when you have an answer.

### Predictions (`pftui predict`)
YOUR calls, scored for accuracy. Track hit rate by conviction level to
calibrate your confidence.

### Agent Messages (`pftui agent-msg`)
Structured message passing between agents. Priority levels, categories,
and acknowledgment tracking. Replaces free-text file appending.

### Daily Notes (`pftui notes`)
Date-keyed narrative entries. Multiple entries per day with section tags.
Full-text search across all history.

### Opportunity Cost (`pftui opportunity`)
Track what positioning saved and cost. Net scorecard of rational vs.
irrational misses.

### Correlations (`pftui correlations`)
Rolling correlation snapshots computed during refresh. Track correlation
evolution between asset pairs. Correlation breaks = regime change signals.

### Regime (`pftui regime`)
Automated market regime classification. Computed from VIX, DXY, yields,
oil, and gold during refresh. Tracks regime transitions over time.

### Structural Cycles (`pftui structural`) [MACRO]
Long-cycle macro intelligence — multi-decade empire cycles, reserve currency
transitions, and power metrics. Use `pftui structural dashboard` for the combined view.

### Trends (`pftui trends`) [HIGH]
Multi-quarter structural trends — AI disruption, energy transition, geopolitical
shifts. Track direction, evidence, and per-asset impact. Use `pftui trends dashboard`.

### Analytics (`pftui analytics`) [CROSS]
Unified multi-timeframe view. `summary` shows all 4 layers. `alignment` shows
per-asset consensus across timeframes. `low/medium/high/macro` expand each layer.
```

## P1 — Feature Requests

> User-requested features and high-value improvements.

### Data & Display
- [x] [Feedback] **Fix Brent crude data** — Added Brent (`BZ=F`) to shared economy symbol set so `refresh` always fetches/caches it, preventing macro dashboard `---` gaps and stabilizing WTI-Brent spread availability. (Completed 2026-03-09)

### CLI Enhancements

- [x] [Feedback] **Filter prediction markets by category** — `pftui predictions --category` now supports `finance`/`macro` aliases and pipe-separated filters (e.g. `geopolitics|finance|macro`) to focus out sports/entertainment noise. (Completed 2026-03-09)
- [x] [Feedback] **Oil technicals in macro dashboard** — `pftui macro` now backfills WTI/Brent history on-demand (unless `--cached-only`) so RSI/MACD/SMA render reliably for oil rows. (Completed 2026-03-09)

### Analytics







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

**Completed (P32 complete)**
- P32.1 Docs parity sweep
- P32.2 Postgres CI job
- P32.3 Runtime strategy cleanup (complete: all `src/db` Postgres paths now use shared `pg_runtime`)
- P32.4 Postgres schema type upgrades (hot path columns)
- P32.5 Pooling config
- P32.6 Setup/backend switch validation
- P32.7 Runtime cleanup completion (batches A-G complete)
- P32.8 Postgres CI expansion (includes parity suite + acceptance script run)
- P32.9 Parity acceptance suite (`scripts/parity_check.sh`)
- P32.10 Final parity signoff docs (`docs/BACKEND-PARITY.md`)

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

### F38: "Data Aggregation Engine" — Product definition + README + Docs + Website

> The missing product layer. The Analytics Engine interprets data, but something has to
> COLLECT it first. The Data Aggregation Engine is the foundation: 10+ data sources,
> local caching, pre-processing (RSI, MACD, SMA, Bollinger, correlations, regime
> classification), and normalization into a unified schema. One `pftui refresh` and
> your database has everything. This needs to be named, documented, and positioned
> as the first pillar of the product stack.
>
> **The full product stack:**
> ```
>                  ┌──────────────────────────┐
>                  │        AI Layer           │  Agents, routines, investor panel
>                  ├──────────────────────────┤
>                  │    Analytics Engine       │  4-timeframe intelligence
>                  ├──────────────────────────┤
>                  │  Data Aggregation Engine  │  10+ sources, pre-processing, compute
>                  └────────────┬─────────────┘
>                               │ read/write
>                  ┌────────────▼─────────────┐
>                  │      Your Database        │  SQLite / Postgres
>                  │   (shared state layer)    │  The single source of truth
>                  └──────────────────────────┘
> ```
> The Database is NOT a passive layer between aggregation and analytics — it's the
> **shared state layer** that ALL other layers read from and write to. The aggregation
> engine writes raw data + pre-computed technicals. The analytics engine writes
> scenarios, convictions, regime classifications. The AI layer writes agent messages,
> notes, predictions. Every layer's output is another layer's input, and the database
> is the meeting point.
>
> The Data Aggregation Engine also performs **significant compute** against raw data —
> technical analysis (RSI, MACD, SMA, Bollinger), correlation matrices, regime
> classification, trend change detection, probability shifts. This pre-processing
> reduces the analytical burden on the Analytics Engine, which can focus on
> higher-order interpretation (scenario weighting, cross-timeframe alignment,
> structural cycle positioning) rather than re-deriving technicals from scratch.

**What the Data Aggregation Engine does:**

1. **Source Collection** — pulls from 10+ APIs and feeds in one `pftui refresh`:

| Source | Data Type | Update Cadence | Key Required |
|--------|-----------|----------------|-------------|
| Yahoo Finance | Equities, ETFs, forex, crypto, commodities (OHLCV) | Real-time | No |
| CoinGecko | Crypto prices, market cap, 24h volume | Real-time | No |
| Polymarket | Prediction market probabilities | 15-min | No |
| CFTC Socrata | Commitments of Traders positioning | Weekly | No |
| Alternative.me | Crypto Fear & Greed Index | Daily | No |
| BLS API v1 | CPI, unemployment, NFP, wages (101 series) | Monthly | No |
| World Bank | GDP, debt/GDP, reserves (8 economies, 160 indicators) | Quarterly | No |
| CME Group | COMEX gold/silver warehouse inventory | Daily | No |
| Blockchair | BTC on-chain data, ETF flows | Real-time | No |
| RSS Feeds | Reuters, CoinDesk, Bloomberg, Kitco, CNBC | 10-min | No |
| Brave Search | News, economic data, research queries | On-demand | Optional (free tier) |
| CME FedWatch | Fed funds futures implied rate probabilities | Daily | No |

2. **Pre-Processing & Technical Analysis** — computed on cached data, not fetched:

| Computation | What it produces | Used by |
|-------------|-----------------|---------|
| RSI (14-period) | Overbought/oversold per symbol | LOW layer, movers, alerts |
| SMA (20, 50) | Trend direction, support/resistance | LOW layer, watchlist |
| MACD (12/26/9) | Momentum, crossover signals | LOW layer, macro dashboard |
| Bollinger Bands | Volatility envelope | LOW layer |
| Price history (daily OHLCV) | Historical price series | Correlations, charts, backtesting |
| Correlation matrix | Rolling cross-asset correlations | LOW layer, regime detection |
| Regime classification | Risk-on/risk-off/transition with confidence | LOW layer, analytics |
| FX normalization | Multi-currency cost basis → base currency | Portfolio value, P&L |
| Change detection | 1D, 1W, 1M change % per symbol | Movers, alerts, brief |
| Alert evaluation | Price/allocation threshold scanning | Alert triggers |

3. **Normalization** — all data lands in a unified schema:
- Prices → `price_cache` (symbol, price, currency, fetched_at, source)
- History → `price_history` (symbol, date, close, source, volume)
- Sentiment → `sentiment_cache` (index, value, label, fetched_at)
- Economic → `bls_cache`, `economic_data`, `worldbank_cache`
- Positioning → `cot_cache`, `comex_cache`, `onchain_cache`
- Events → `calendar_events`, `news_cache`
- Predictions → `predictions_cache`

4. **Staleness Tracking** — `pftui status` shows freshness per source:
```bash
$ pftui status
Source           Last Fetch         Records  Status
────────────────────────────────────────────────────
Prices           2m ago             84       ✓ Fresh
Predictions      15m ago            4        ✓ Fresh
News             10m ago            116      ✓ Fresh
COT              3d ago             4        ✓ Current
Sentiment        2m ago             2        ✓ Fresh
Calendar         2m ago             3        ✓ Fresh
BLS              2m ago             101      ✓ Fresh
World Bank       2m ago             160      ✓ Fresh
COMEX            failed             0        ✗ Error
On-chain         2m ago             1        ✓ Fresh
```

**Commands that ARE the Data Aggregation Engine:**
- `pftui refresh` — the single command that triggers the entire pipeline
- `pftui status` — data freshness dashboard
- `pftui doctor` — connectivity diagnostics for all sources
- `pftui config set brave_api_key <key>` — unlock additional sources

**The aggregation engine doesn't just collect — it computes.** When `pftui refresh` runs,
it doesn't just cache raw prices. It computes RSI, MACD, SMA across all symbols. It runs
correlation matrices across held assets. It classifies the market regime (risk-on/risk-off)
with a confidence score. It detects which alerts are triggered, which movers crossed
thresholds, which prediction market probabilities shifted. By the time the Analytics Engine
reads from the database, the heavy numerical work is already done. The Analytics Engine's
job is interpretation and cross-referencing — "what does RSI 89 on oil MEAN given the
current war scenario?" — not "calculate RSI from 14 days of closes."

**The database as shared state:** The aggregation engine writes price_cache, sentiment_cache,
cot_cache. The analytics engine writes scenarios, thesis, convictions, regime_snapshots.
The AI layer writes agent_messages, daily_notes, user_predictions. Every layer's output
becomes queryable state for every other layer. An agent reads `pftui regime current` (written
by aggregation's classifier), combines it with `pftui scenario list` (written by an evening
planner agent), and writes `pftui conviction set GC=F --score 4` (consumed by the analytics
alignment view). The database is the meeting point — not a pipe between layers.

**Key product differentiator:** Most tools show you data from ONE source. TradingView shows
charts (one source). Yahoo shows prices (one source). Bloomberg aggregates but costs $25k/yr.
pftui aggregates 10+ free sources into one local database with one command, pre-processes
technicals and classifications, and makes it all available to both human and AI operators
through a unified CLI. The aggregation layer is invisible when it works — but it's the
foundation everything else depends on.

**Documentation structure (same pattern as Analytics Engine and AI Layer):**

`docs/DATA-AGGREGATION.md` — Full dedicated documentation page. Covers:
- Overview: what the aggregation engine does and why local caching matters
- Source catalog: every API, what it provides, update cadence, key requirements
- Pre-processing pipeline: every computation performed on raw data
- Schema reference: every cache table with column descriptions
- Staleness management: how freshness is tracked, max-age thresholds
- Extension: how to add new data sources (for contributors)
- Brave Search integration: what it unlocks beyond free sources
- Troubleshooting: common failures (rate limits, API changes)

README section — high-level product overview (~30-40 lines). "Data Aggregation Engine"
positioned BEFORE "Your Database" in the flow. One `pftui refresh` pulls 10+ sources.
Show the source table. Link to `docs/DATA-AGGREGATION.md`.

Website section — "Data Aggregation Engine — 10+ Sources, One Command"
- Visual showing the data flow: APIs → pftui refresh → local database
- Source logos or icons
- Terminal demo showing `pftui refresh` output with ✓ checks
- Emphasize: no API keys for core sources, free forever

**Files to update:**

---

### F37: "AI Layer" — README + Website section for agent capabilities

> pftui's agent integration is a major differentiator that isn't documented anywhere
> user-facing. The README mentions "agent-native" but doesn't explain what that means
> in practice. The website has no section on it. This is a standalone product feature
> that deserves prominent placement — alongside "Your Database" and "Analytics Engine."

**The AI Layer is three capabilities:**

**1. Bootstrapping & Bidirectional Communication**
- `pftui brief --json` gives agents a complete portfolio snapshot in one call
- `pftui agent-msg` enables structured inter-agent message passing with priorities, categories, layers
- `pftui conviction`, `pftui notes`, `pftui predict` — agents write observations, humans review and override
- Every command supports `--json` — the CLI IS the API
- Agents and humans operate on the same data, same tool, same database. Not separate systems.
- The human sets conviction, the agent tracks evidence. The agent proposes scenarios, the human adjusts probabilities. Bidirectional by design.

**2. Scheduled Routines & Reports**
- Daily briefs, market close summaries, weekly reviews — all generated from pftui data
- Cron-driven: morning research pulls `pftui refresh` + `pftui analytics summary`, evening planner updates scenarios
- Multi-agent feedback loops: morning agent passes signals to evening agent via `pftui agent-msg`, evening agent guides tomorrow's priorities
- Alert system: `pftui alerts` triggers notifications on price/allocation thresholds
- No external APIs required — all intelligence derived from pftui's own database

**3. Investor Perspectives Panel (F36)**
- Feed analytics engine data to sub-agents prompted as famous investors or investment styles
- 15 named legends (Dalio, Druckenmiller, Buffett, etc.) + 10 generic archetypes (Momentum Trader, Doomsday Prepper, etc.)
- Each interprets the same data through a different philosophy
- Consensus and divergence detection — the disagreements are the most valuable output
- Custom personas: drop a markdown file, add an investor

**README section (add after "Analytics Engine" or "Your Database"):**

```markdown
## AI Layer

pftui is designed to be operated by AI agents alongside humans.
Every command outputs `--json`. Every table is read/write via CLI.
The result: your AI agent and you operate on the **same data,
same tool, same database** — not separate systems stitched together.

### What agents can do with pftui

**Daily Intelligence Loop**
Your agent runs `pftui refresh` at market open, reads `pftui analytics summary`,
writes observations to `pftui notes`, updates `pftui scenario` probabilities,
and delivers a brief to your phone. You reply with your read. The agent logs
your conviction via `pftui conviction set`. Tomorrow's brief incorporates
your feedback. The loop compounds.

**Inter-Agent Communication**
Multiple agents coordinate via `pftui agent-msg` — a structured message bus
with priorities, categories, and analytics engine layer tags. A low-timeframe
agent detects a correlation break, escalates to the medium-timeframe agent,
which investigates whether it's a scenario shift. Signals flow up, context
flows down.

**Investor Perspectives Panel**
Feed your analytics engine data to sub-agents prompted as Warren Buffett,
Stanley Druckenmiller, Michael Burry, or 22 other investor personas.
Each interprets your portfolio through a different philosophy. When 7 of 8
agree on gold but split on BTC — that's signal. Custom personas: just add
a markdown file.

**Your agent setup (example with OpenClaw):**
\`\`\`bash
# Morning cron: refresh data, generate brief
pftui refresh
pftui brief --json | openclaw agent send --stdin

# Agent writes back:
pftui conviction set GC=F --score 4 --notes "War premium + BRICS"
pftui scenario update "Inflation Spike" --probability 38
pftui agent-msg send "Gold alignment: all 4 layers bullish" \\
  --from morning-agent --priority high --layer cross
\`\`\`

No vendor lock-in. Any agent framework that can call CLI commands can
operate pftui. OpenClaw, LangChain, AutoGPT, Claude Code, a bash script.
The database is yours. The intelligence compounds.
```

**Website section (new card/section after Analytics Engine):**
- Hero: "AI Layer — Your Agent's Operating System"
- 3-column layout: Bidirectional Comms | Scheduled Routines | Investor Panel
- Terminal demo scene showing agent writing conviction + reading brief
- Comparison table row: "AI Agent Integration" — pftui ✓, Bloomberg ✗, TradingView ✗, Yahoo ✗
- CTA: "See AGENTS.md for the full agent operator guide"

**Documentation structure (same pattern as Analytics Engine):**

`docs/AI-LAYER.md` — Full dedicated documentation page. Covers:
- Overview: what the AI Layer is and why pftui is different from every other tool
- Bidirectional Communication: how agents and humans share the same data/tool/DB.
  Every `--json` command, the agent-msg bus, conviction/notes/predict as shared state.
  Include example workflow: agent writes observation → human reviews → agent incorporates
  feedback → loop compounds.
- Scheduled Routines: cron-driven intelligence loops. Morning brief → market close →
  evening analysis → weekly review. Multi-agent feedback loops via agent-msg with
  layer escalation. Include example cron schedule and what each routine does.
- Investor Perspectives Panel (F36): sub-agents as famous investors/styles.
  15 legends + 10 archetypes + custom. Data collection via pftui --json.
  Consensus/divergence as the primary output. Include example panel output.
- Integration Patterns: how to connect pftui to OpenClaw, LangChain, Claude Code,
  or any agent framework. The CLI IS the API — no SDK needed.
- Quick Start: 5-command example showing agent setup end-to-end.

README "AI Layer" section — high-level product overview (~30-40 lines). Covers the
three capabilities in 2-3 sentences each. Links to `docs/AI-LAYER.md` for full details.
Positioned after "Analytics Engine" section.

Website "AI Layer" section — visual product feature section. 3-card layout.
Terminal demo scene. Comparison table row.

**Files to update:**

---

## Feedback Summary

> Updated: 2026-03-09

### Current Scores (latest per tester)

| Tester | Usefulness | Overall | Trend |
|--------|-----------|---------|-------|
| Morning Market Research | 88% | 82% | ↑ (25→65→82→78→82→88) |
| Evening Eventuality Planner | 35% | 55% | ↓ (20→88→92→85→80→82→35) |
| Sentinel (Portfolio Analyst) | 85% | 78% | → (75→85→85→78→85→85) |
| Market Close | 92% | 88% | ↑ (72→82→78→92) |
| UX Analyst | — | 75% | → (78→68→72→73→75) |

### Score Trends

- **Morning Market Research:** Steady at 88/82 — best scores since launch. Macro technicals (RSI/MACD/SMA) landed on Mar 7. Remaining gaps: oil technicals in macro, prediction markets showing sports instead of geopolitical, ag commodity tracking. Python script nearly eliminated.
- **Evening Eventuality Planner:** ⚠️ CRASHED to 35/55 on Mar 9. ALL commands hung indefinitely with Postgres backend — zero functionality. Previous session (Mar 8) also hit SQLite migration blocker (0/15). Reliability is the #1 issue. When working (Mar 5-7), scores were 82-92. The tool's feature set is strong but backend stability is destroying trust.
- **Sentinel (Portfolio Analyst):** Stable at 85/78. TUI visual quality consistently praised. Day P&L dollar column still the most requested missing feature. Correlation grid and ratio charts well received.
- **Market Close:** Strongest absolute scores (92/88) — no new review since Mar 6. `brief + movers + macro` pipeline covers most of the routine. Python script dependency eliminated for closing data.
- **UX Analyst:** Holding at 75. Focus on feature discoverability (`pftui config` invisible) and data pipeline reliability. `--json` consistency improving but `status --json` still missing.

### Top 3 Priorities (Feedback-Driven)

1. ✅ **DB connection timeout + diagnostics** — timeout shipped; `pftui doctor` command added for proactive health checks.
2. ✅ **Broken data-source fixes** — sector coverage, Brent availability, USD/JPY daily-change sanity checks, and prediction market category filtering shipped.
3. ✅ **Oil technicals in macro** — RSI/MACD/SMA now reliably available for WTI/Brent via on-demand history backfill in `pftui macro`.
