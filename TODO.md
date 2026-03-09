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

- [ ] **P1-BUG: `pftui structural` — "Postgres structural storage not yet implemented"** — All structural commands (metric-set, metric-list, cycle-set, cycle-list, outcome-add, outcome-list, parallel-add, parallel-list, log-add, log-list, dashboard) return this error. Postgres tables exist (`power_metrics`, `structural_cycles`, `structural_outcomes`, `structural_outcome_history`, `historical_parallels`, `structural_log`) but the Rust code has no Postgres dispatch for this module. The MACRO layer of the analytics engine is completely blocked. This is the only F31 module without Postgres support.

- [ ] **P1-BUG: `pftui analytics summary` — minimal output, no alignment score** — Currently shows only 4 one-line summaries (LOW/MEDIUM/HIGH/MACRO). Missing: prices row, alert count, triggered alert count, combined alignment score (████████░░ 80%), divergence notes, signal count. Compare current output to the spec in TODO F31.13 — it's about 20% of what was designed. The `analytics low/medium/high` subcommands are similarly sparse (just counts, no actual data).

- [ ] **P1-BUG: `pftui analytics alignment` — only shows one asset at a time** — Should show a matrix of all held + watchlist assets with per-layer signals. Currently requires `--symbol` flag and only returns one asset. The multi-asset alignment table from the F31.13 spec isn't implemented.

- [ ] **P1: `pftui regime current` — confidence 0.25 is suspiciously low** — Risk-off regime detected but confidence only 0.25 with VIX 27.85, DXY 99.12, oil $94.31. In a war with oil at $100 and VIX near 30, confidence should be much higher. The classification logic may need tuning or the weighting formula may not account for all inputs. Regime is a foundation for the LOW layer — inaccurate confidence undermines downstream analytics.

- [ ] **P1: `pftui movers` — shows "No movers" despite BTC +4.8%, Oil +6%** — price_history only has 2 entries for today (CL=F, BZ=F). The 84 symbols in price_cache aren't getting written to price_history during refresh. The `movers` command (and `eod` "TOP MOVERS" section) are non-functional until price_history is populated per-refresh cycle. Brief `1D` column also shows `—` for all positions.

- [ ] **P1: `pftui correlations compute` — "insufficient history"** — Even after refresh with 84 cached prices, correlations can't compute. Likely needs multiple days of price_history (which barely exists). Once price_history populates properly, this should self-resolve — but verify the minimum data requirement is documented.

- [ ] **P1: Prediction decode NULL crash blocks refresh completion** — `pftui refresh` exits with code 1 after "Error: unexpected null; try decoding as an `Option`" from `predictions_cache::get_last_update_postgres`. Root cause: `SELECT MAX(updated_at)` returns SQL NULL on empty table, but sqlx `query_scalar` doesn't handle `Option<Option<i64>>`. Workaround: seeded a dummy row. Proper fix: use `.flatten()` or `COALESCE(MAX(updated_at), 0)` in the query.

### Infrastructure

### Code Quality Quick Wins (audit-driven)

- [ ] [Audit][P1] **FX fallback invariant hardening** — remove implicit `strip_suffix(...).unwrap()` and return explicit error if suffix invariant fails. (`src/price/yahoo.rs`)

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
