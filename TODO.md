# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Intelligence Database (F31)

> Structured storage for the analytical layer. Replaces fragile markdown files with indexed SQLite.
> Every table gets a CLI subcommand with full CRUD + `--json`. No TUI integration needed yet.
> All tables/commands must be generic — useful to ANY pftui user, not specific to one setup.
> Update AGENTS.md with usage patterns for each new command after implementation.
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

### F31.1: Scenarios — Macro scenario planning with probability tracking

**Files to create/modify:**
- `src/db/scenarios.rs` (NEW)
- `src/commands/scenario.rs` (NEW)
- `src/db/schema.rs` (add tables + migration)
- `src/cli.rs` (add `Scenario` command variant)
- `src/main.rs` (add match arm)
- `src/db/mod.rs` + `src/commands/mod.rs` (register modules)

**Schema** (add to `db/schema.rs` initial `execute_batch`):
```sql
CREATE TABLE IF NOT EXISTS scenarios (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    probability REAL NOT NULL DEFAULT 0.0,
    description TEXT,
    asset_impact TEXT,  -- JSON: {"BTC": "bearish", "gold": "bullish", ...}
    triggers TEXT,      -- free text, key conditions that activate this scenario
    historical_precedent TEXT,
    status TEXT NOT NULL DEFAULT 'active',  -- active|resolved|archived
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS scenario_signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    signal TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'watching',  -- watching|triggered|invalidated
    evidence TEXT,
    source TEXT,  -- where the signal came from (e.g. "BLS", "CoinGecko", "manual")
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_scenario_signals_scenario ON scenario_signals(scenario_id);

CREATE TABLE IF NOT EXISTS scenario_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    probability REAL NOT NULL,
    driver TEXT,  -- what caused the probability change
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_scenario_history_scenario ON scenario_history(scenario_id);
```

**Structs** (`src/db/scenarios.rs`):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: i64,
    pub name: String,
    pub probability: f64,
    pub description: Option<String>,
    pub asset_impact: Option<String>,
    pub triggers: Option<String>,
    pub historical_precedent: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSignal {
    pub id: i64,
    pub scenario_id: i64,
    pub signal: String,
    pub status: String,
    pub evidence: Option<String>,
    pub source: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioHistoryEntry {
    pub id: i64,
    pub scenario_id: i64,
    pub probability: f64,
    pub driver: Option<String>,
    pub recorded_at: String,
}
```

**DB functions needed:**
- `add_scenario(conn, name, probability, description, asset_impact, triggers, precedent) -> Result<i64>`
- `list_scenarios(conn, status_filter: Option<&str>) -> Result<Vec<Scenario>>`
- `get_scenario_by_name(conn, name) -> Result<Option<Scenario>>`
- `update_scenario_probability(conn, id, probability, driver) -> Result<()>` — MUST insert a row into `scenario_history` automatically before updating
- `update_scenario(conn, id, description, asset_impact, triggers, status) -> Result<()>`
- `remove_scenario(conn, id) -> Result<()>` — CASCADE deletes signals + history
- `add_signal(conn, scenario_id, signal, status, evidence, source) -> Result<i64>`
- `list_signals(conn, scenario_id, status_filter: Option<&str>) -> Result<Vec<ScenarioSignal>>`
- `update_signal(conn, signal_id, status, evidence) -> Result<()>`
- `remove_signal(conn, signal_id) -> Result<()>`
- `get_history(conn, scenario_id, limit: Option<usize>) -> Result<Vec<ScenarioHistoryEntry>>`

**CLI** (`src/cli.rs` — add variant):
```rust
/// Manage macro scenarios and probability tracking
#[command(name = "scenario")]
Scenario {
    /// Action: add, list, update, remove, signal-add, signal-list, signal-update, signal-remove, history
    action: String,
    /// Scenario name (for add/update/remove/history) or signal text (for signal-add)
    value: Option<String>,
    /// Scenario ID
    #[arg(long)]
    id: Option<i64>,
    /// Signal ID (for signal-update/signal-remove)
    #[arg(long)]
    signal_id: Option<i64>,
    /// Probability 0-100
    #[arg(long)]
    probability: Option<f64>,
    /// Description text
    #[arg(long)]
    description: Option<String>,
    /// Asset impact as JSON string
    #[arg(long)]
    impact: Option<String>,
    /// Trigger conditions text
    #[arg(long)]
    triggers: Option<String>,
    /// Historical precedent text
    #[arg(long)]
    precedent: Option<String>,
    /// Status: active, resolved, archived (scenarios) or watching, triggered, invalidated (signals)
    #[arg(long)]
    status: Option<String>,
    /// What drove the probability change
    #[arg(long)]
    driver: Option<String>,
    /// Evidence for signal update
    #[arg(long)]
    evidence: Option<String>,
    /// Source of signal
    #[arg(long)]
    source: Option<String>,
    /// Scenario name for signal operations
    #[arg(long)]
    scenario: Option<String>,
    /// Max results
    #[arg(long)]
    limit: Option<usize>,
    /// JSON output
    #[arg(long)]
    json: bool,
}
```

**Command routing** (`src/commands/scenario.rs`):
- `"add"` → requires `value` (name) + `--probability`. Optional: `--description`, `--impact`, `--triggers`, `--precedent`.
- `"list"` → show all active scenarios sorted by probability desc. Table: `Name | Prob% | Status | Last Updated`. Optional `--status` filter.
- `"update"` → requires `value` (name). Update any field. If `--probability` is set, ALSO log to history with `--driver`.
- `"remove"` → requires `value` (name) or `--id`.
- `"signal-add"` → requires `--scenario` (name) + `value` (signal text). Optional: `--status`, `--evidence`, `--source`.
- `"signal-list"` → requires `--scenario` (name). Shows signals for that scenario. Optional `--status` filter.
- `"signal-update"` → requires `--signal-id` + `--status` and/or `--evidence`.
- `"signal-remove"` → requires `--signal-id`.
- `"history"` → requires `value` (name). Shows probability history. Optional `--limit`.

**Human-readable output** (non-JSON):
```
Scenarios (4 active):
  Geopolitical War          42.0%   active   3h ago
  Inflation Spike           35.0%   active   3h ago
  Hard Recession            13.0%   active   3h ago
  Stagflation                8.0%   active   3h ago
```

**Example usage:**
```bash
pftui scenario add "Stagflation" --probability 22 --description "GDP sub-2%, inflation sticky 2.8-3.5%, Fed paralyzed" --triggers "Core PCE >2.8% for 3 prints, GDP <2% Q1-Q2, PPI ATH" --precedent "1973-1975 oil embargo stagflation"
pftui scenario update "Stagflation" --probability 8 --driver "NFP -92K confirmed recession path, absorbed into Inflation Spike"
pftui scenario signal-add --scenario "Stagflation" "10Y yields rising during war" --status triggered --evidence "Yields 3.95-3.99% during active combat" --source "FRED"
pftui scenario history "Stagflation" --limit 10
pftui scenario list --json
```

---

### F31.2: Thesis — Versioned macro outlook by section

**Files to create/modify:**
- `src/db/thesis.rs` (NEW)
- `src/commands/thesis.rs` (NEW)
- `src/db/schema.rs`, `src/cli.rs`, `src/main.rs`, mod.rs files

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS thesis (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    section TEXT NOT NULL UNIQUE,  -- user-defined: "regime", "btc", "gold", "equities", "risks", etc.
    content TEXT NOT NULL,
    conviction TEXT NOT NULL DEFAULT 'medium',  -- high|medium|low
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS thesis_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    section TEXT NOT NULL,
    content TEXT NOT NULL,
    conviction TEXT NOT NULL,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_thesis_history_section ON thesis_history(section);
```

**DB functions:**
- `upsert_thesis(conn, section, content, conviction) -> Result<()>` — MUST snapshot current value to `thesis_history` BEFORE upserting. Use `INSERT OR REPLACE` on section uniqueness.
- `list_thesis(conn) -> Result<Vec<ThesisEntry>>`
- `get_thesis_section(conn, section) -> Result<Option<ThesisEntry>>`
- `get_thesis_history(conn, section, limit: Option<usize>) -> Result<Vec<ThesisHistoryEntry>>`
- `remove_thesis(conn, section) -> Result<()>`

**CLI variant:**
```rust
/// Manage your macro thesis — versioned outlook by section
#[command(name = "thesis")]
Thesis {
    /// Action: list, update, history, remove
    action: String,
    /// Section name (for update/history/remove)
    value: Option<String>,
    /// Content text
    #[arg(long)]
    content: Option<String>,
    /// Conviction: high, medium, low
    #[arg(long)]
    conviction: Option<String>,
    /// Max results for history
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}
```

**Command routing:**
- `"list"` → all sections. Table: `Section | Conviction | Content (truncated) | Updated`.
- `"update"` → requires `value` (section name) + `--content`. Optional `--conviction` (defaults to previous or "medium"). Auto-snapshots to history.
- `"history"` → requires `value` (section name). Optional `--limit`.
- `"remove"` → requires `value` (section name).

**Example usage:**
```bash
pftui thesis update regime --content "Risk-off. Stagflation confirmed by NFP -92K, oil \$91, VIX 29.5. Fed trapped — can't cut into oil inflation, can't hold with -92K jobs." --conviction high
pftui thesis update btc --content "Bear. F&G 12, RSI 38, below SMA50. CryptoQuant: bottom Sep-Nov 2026. Daily CyberDots haven't flipped." --conviction high
pftui thesis list
pftui thesis history regime --limit 5
```

---

### F31.3: Convictions — Asset conviction scores over time

**Files:** `src/db/convictions.rs`, `src/commands/conviction.rs`, schema/cli/main/mod updates.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS convictions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol TEXT NOT NULL,
    score INTEGER NOT NULL CHECK(score BETWEEN -5 AND 5),
    notes TEXT,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_convictions_symbol ON convictions(symbol);
CREATE INDEX IF NOT EXISTS idx_convictions_recorded ON convictions(recorded_at);
```
Note: this is an append-only log — every `set` creates a new row. "Current" conviction = latest row per symbol.

**DB functions:**
- `set_conviction(conn, symbol, score, notes) -> Result<i64>`
- `list_current(conn) -> Result<Vec<ConvictionEntry>>` — latest row per symbol via `GROUP BY symbol HAVING MAX(recorded_at)`
- `get_history(conn, symbol, limit: Option<usize>) -> Result<Vec<ConvictionEntry>>`
- `get_changes(conn, days: usize) -> Result<Vec<ConvictionChange>>` — show symbols where score changed in last N days

**CLI variant:**
```rust
/// Track asset conviction scores over time (-5 to +5)
#[command(name = "conviction")]
Conviction {
    /// Action: set, list, history, changes
    action: String,
    /// Symbol (for set/history) or days (for changes, default 7)
    value: Option<String>,
    /// Score -5 to +5
    #[arg(long)]
    score: Option<i32>,
    /// Notes explaining the score
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}
```

**Command routing:**
- `"set"` → requires `value` (symbol) + `--score`. Optional `--notes`.
- `"list"` → current conviction per symbol. Table: `Symbol | Score | Notes | Last Updated`. Sorted by abs(score) desc.
- `"history"` → requires `value` (symbol). Shows score evolution. Optional `--limit`.
- `"changes"` → optional `value` (days, default 7). Shows symbols where conviction changed recently.

**Human-readable output:**
```
Current Convictions:
  GC=F     +4   Gold thesis validated by NFP + war          3h ago
  SI=F     +3   Defending $83 trackline, SMA50 reclaim      3h ago
  BTC       0   Bear tracking, called bull trap correctly    3h ago
  Equities -2   GOOG/TSLA interest emerging but patient     3h ago
  U-U.TO   +1   AI power thesis intact, tactical weakness   3h ago
```

---

### F31.4: Research Questions — Open questions with evidence tracking

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

### F31.5: User Predictions — Your calls, scored for accuracy

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

### F31.6: Agent Messages — Structured inter-agent communication

**Files:** `src/db/agent_messages.rs`, `src/commands/agent_msg.rs`, schema/cli/main/mod updates.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS agent_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_agent TEXT NOT NULL,
    to_agent TEXT,         -- NULL = broadcast to all
    priority TEXT NOT NULL DEFAULT 'normal',  -- low|normal|high|critical
    content TEXT NOT NULL,
    category TEXT,         -- signal|priority|feedback|alert|handoff
    acknowledged INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    acknowledged_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_messages_to ON agent_messages(to_agent);
CREATE INDEX IF NOT EXISTS idx_agent_messages_ack ON agent_messages(acknowledged);
```

**DB functions:**
- `send_message(conn, from, to, priority, content, category) -> Result<i64>`
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
    /// Category: signal, priority, feedback, alert, handoff
    #[arg(long)]
    category: Option<String>,
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
pftui agent-msg send "Oil technicals critical — RSI 89, watch for reversal signal" --from evening-planner --to morning-research --priority high --category signal
pftui agent-msg list --to morning-research --unacked
pftui agent-msg ack --id 42
pftui agent-msg ack-all --to morning-research
pftui agent-msg purge --days 30
```

---

### F31.7: Daily Notes — Date-keyed narrative entries

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

### F31.8: Opportunity Cost Tracker — What positioning saved and cost

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

### F31.9: Correlation Snapshots — Rolling asset correlations

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

### F31.10: Regime Classification — Automated market regime detection

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

### F31.11: AGENTS.md Documentation Update

After ALL F31 tables are implemented, update `AGENTS.md` with a new section:

```markdown
## Intelligence Database

pftui stores analytical intelligence in structured SQLite tables alongside market data.
These tables enable agents to track scenarios, thesis evolution, conviction changes,
predictions, and inter-agent communication without fragile file-based storage.

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
```

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
