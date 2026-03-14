use clap::{Parser, Subcommand, ValueEnum};

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Inter-agent structured message passing
    Message {
        /// Action: send, list, reply, flag, ack, ack-all, purge
        action: String,

        /// Message content (for send/reply/flag)
        value: Option<String>,

        /// Batch mode for send: repeat to enqueue multiple related messages
        #[arg(long = "batch")]
        batch: Vec<String>,

        #[arg(long)]
        id: Option<i64>,

        /// Sender (required for send/reply/flag; filter for list)
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
    },
}

#[derive(Subcommand)]
pub enum WatchlistCommand {
    /// Add symbol(s) to watchlist
    Add {
        /// Symbol to watch (e.g. AAPL, BTC, GC=F). Omit when using --bulk.
        symbol: Option<String>,
        /// Asset category (equity, crypto, forex, cash, commodity, fund). Auto-detected if omitted.
        #[arg(long)]
        category: Option<String>,
        /// Add multiple symbols at once, comma-separated (e.g. GOOG,META,AMZN,TSLA)
        #[arg(long)]
        bulk: Option<String>,
        /// Set a target entry price (e.g. 300, 55000). Creates an alert when hit.
        #[arg(long)]
        target: Option<String>,
        /// Direction for target: "below" (default, buy dip) or "above" (breakout)
        #[arg(long, default_value = "below")]
        direction: String,
    },
    /// Remove a symbol from watchlist
    Remove {
        /// Symbol to unwatch
        symbol: String,
    },
    /// List watchlist symbols with cached prices
    List {
        /// Filter to symbols within N% of their target price (e.g. 10)
        #[arg(long)]
        approaching: Option<String>,
        /// Output JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
}

#[derive(Parser)]
#[command(name = "pftui", version, about = "Terminal portfolio tracker")]
pub struct Cli {
    /// Use cached/local data only; do not attempt network refresh/backfill calls
    #[arg(long, visible_alias = "offline", global = true)]
    pub cached_only: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Agentic operations and inter-agent workflows
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },

    /// Portfolio summary to stdout
    Summary {
        /// Group output by a field (e.g. "category")
        #[arg(long, value_enum)]
        group_by: Option<SummaryGroupBy>,

        /// Show P&L over a time period instead of total gain from cost basis
        #[arg(long, value_enum)]
        period: Option<SummaryPeriod>,

        /// Model hypothetical prices: SYMBOL:PRICE,SYMBOL:PRICE (e.g. GC=F:5500,BTC:55000)
        #[arg(long, value_name = "OVERRIDES")]
        what_if: Option<String>,

        /// Output JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },

    /// Export portfolio data (JSON exports full snapshot; CSV exports positions only)
    Export {
        #[arg(value_enum)]
        format: ExportFormat,

        /// Write output to a file instead of stdout
        #[arg(long, short)]
        output: Option<String>,
    },

    /// List all transactions
    ListTx {
        /// Show transaction notes column
        #[arg(long)]
        notes: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Add a transaction
    AddTx {
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        tx_type: Option<String>,
        #[arg(long)]
        quantity: Option<String>,
        #[arg(long)]
        price: Option<String>,
        #[arg(long, default_value = "USD")]
        currency: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },

    /// Remove a transaction by ID
    RemoveTx {
        /// Transaction ID to remove
        id: i64,
    },

    /// Run the portfolio setup wizard
    Setup,

    /// Add symbol(s) to the watchlist
    Watch {
        /// Symbol to watch (e.g. AAPL, BTC, GC=F). Omit when using --bulk.
        symbol: Option<String>,
        /// Asset category (equity, crypto, forex, cash, commodity, fund). Auto-detected if omitted.
        #[arg(long)]
        category: Option<String>,
        /// Add multiple symbols at once, comma-separated (e.g. GOOG,META,AMZN,TSLA)
        #[arg(long)]
        bulk: Option<String>,
        /// Set a target entry price (e.g. 300, 55000). Creates an alert when hit.
        #[arg(long)]
        target: Option<String>,
        /// Direction for target: "below" (default, buy dip) or "above" (breakout)
        #[arg(long, default_value = "below")]
        direction: String,
    },

    /// Remove a symbol from the watchlist
    Unwatch {
        /// Symbol to unwatch
        symbol: String,
    },

    /// Fetch and cache current prices for all held symbols without launching the TUI
    Refresh {
        /// Send OS notification for newly triggered alerts
        #[arg(long)]
        notify: bool,
    },

    /// Show data freshness status for all cached sources
    Status {
        /// Explicitly request per-source data health output
        #[arg(long)]
        data: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show active database backend details and table row counts
    #[command(name = "db-info")]
    DbInfo {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Run system diagnostics: test DB connection, API endpoints, and cache freshness
    Doctor {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// View and update pftui configuration fields
    Config {
        /// Action: list, get, set
        action: String,
        /// Field name (required for get/set)
        field: Option<String>,
        /// Field value (required for set)
        value: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Output a markdown-formatted portfolio brief for agent consumption and daily reports
    Brief {
        /// Output structured JSON (includes all available data)
        #[arg(long)]
        json: bool,
    },
    /// Show total portfolio value with gain/loss (uses cached prices)
    Value {
        /// Output JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },

    /// Display watchlist symbols with current cached prices
    Watchlist {
        #[command(subcommand)]
        action: Option<WatchlistCommand>,

        /// Filter to symbols within N% of their target price (e.g. 10)
        #[arg(long)]
        approaching: Option<String>,
        /// Output JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },

    /// Set a cash position to an exact amount (replaces existing transactions for that currency)
    SetCash {
        /// Currency symbol (e.g. USD, GBP, EUR)
        symbol: String,
        /// Amount to set (e.g. 45000, 12500.50). Use 0 to clear.
        amount: String,
    },

    /// Launch pftui with a realistic demo portfolio (your real data is untouched)
    Demo,

    /// Render the TUI as ANSI text to stdout (no interactive terminal required)
    Snapshot {
        /// Terminal width in columns (default: 120)
        #[arg(long, default_value = "120")]
        width: u16,

        /// Terminal height in rows (default: 40)
        #[arg(long, default_value = "40")]
        height: u16,

        /// Strip colors and output plain text only
        #[arg(long)]
        plain: bool,
    },

    /// Import data from a JSON snapshot file (as produced by `pftui export json`)
    Import {
        /// Path to the JSON snapshot file
        path: String,

        /// Import mode: replace wipes existing data, merge adds without deleting
        #[arg(long, value_enum, default_value = "replace")]
        mode: ImportModeArg,
    },

    /// Show portfolio value and positions as of a past date using cached price history
    History {
        /// Target date in YYYY-MM-DD format (e.g. 2026-02-28)
        #[arg(long)]
        date: String,

        /// Group output by a field (e.g. "category")
        #[arg(long, value_enum)]
        group_by: Option<SummaryGroupBy>,
    },

    /// Macro dashboard: key economic indicators, yields, commodities, currencies, and derived metrics
    Macro {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Oil dashboard: WTI/Brent, spread, RSI, and oil-geopolitics headlines
    Oil {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Crisis dashboard: oil, VIX, defense, safe havens, shipping/geopolitics context
    Crisis {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Market regime classification and history
    #[command(name = "regime")]
    Regime {
        /// Action: current, history, transitions
        action: String,

        #[arg(long)]
        limit: Option<usize>,

        #[arg(long)]
        json: bool,
    },

    /// CME FedWatch probabilities from Fed funds futures implied pricing
    Fedwatch {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Sovereign holdings tracker: CB gold (WGC), government BTC, COMEX silver
    Sovereign {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show cached economic indicators (Brave/BLS)
    Economy {
        /// Filter to a specific indicator (e.g. cpi, nfp, fed_funds_rate)
        #[arg(long)]
        indicator: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// End-of-Day summary: brief + movers + macro + sentiment combined
    Eod {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Global macro dashboard: World Bank structural data for major economies
    Global {
        /// Filter to a specific country code (e.g. USA, CHN, IND, RUS, BRA, ZAF, GBR, EUU)
        #[arg(long)]
        country: Option<String>,

        /// Filter to a specific indicator: gdp, debt, current-account, reserves
        #[arg(long)]
        indicator: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show portfolio performance: returns over time (MTD, QTD, YTD, since inception)
    Performance {
        /// Custom start date for return calculation (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,

        /// Return series grouping: daily, weekly, monthly
        #[arg(long)]
        period: Option<String>,

        /// Benchmark symbol to compare against (e.g. SPY)
        #[arg(long)]
        vs: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show BTC ETF flow data (inflows/outflows by fund)
    EtfFlows {
        /// Number of days to show (default: 1, today only)
        #[arg(long, default_value = "1")]
        days: u16,

        /// Filter to a specific fund (e.g. IBIT, FBTC, GBTC)
        #[arg(long)]
        fund: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show biggest daily movers across held + watchlist symbols
    Movers {
        /// Minimum absolute daily change % to include (default: 3)
        #[arg(long, default_value = "3")]
        threshold: String,

        /// Use overnight framing (since last close) for briefing workflows
        #[arg(long)]
        overnight: bool,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Scan positions using a simple filter DSL
    Scan {
        /// Filter expression (e.g. "allocation_pct > 10 and gain_pct < 0")
        #[arg(long)]
        filter: Option<String>,

        /// Save filter as a named query (e.g. --save "risk")
        #[arg(long)]
        save: Option<String>,

        /// Load and run a previously saved query (e.g. --load "risk")
        #[arg(long)]
        load: Option<String>,

        /// List saved scan queries
        #[arg(long)]
        list: bool,

        /// Require matching recent news items containing this keyword
        #[arg(long = "news-keyword")]
        news_keyword: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

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

        /// Inline notes for probability updates (alias of --driver)
        #[arg(long)]
        notes: Option<String>,

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
    },

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

        /// Status filter for list, or resolved status for resolve
        #[arg(long)]
        status: Option<String>,

        #[arg(long)]
        json: bool,
    },

    /// Inter-agent structured message passing
    #[command(name = "agent-msg")]
    AgentMsg {
        /// Action: send, list, reply, flag, ack, ack-all, purge
        action: String,

        /// Message content (for send/reply/flag)
        value: Option<String>,

        /// Batch mode for send: repeat to enqueue multiple related messages
        #[arg(long = "batch")]
        batch: Vec<String>,

        #[arg(long)]
        id: Option<i64>,

        /// Sender (required for send/reply/flag; filter for list)
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
    },

    /// Multi-timeframe analytics engine views
    #[command(name = "analytics")]
    Analytics {
        /// Action: signals, summary, low, medium, high, macro, alignment, divergence, digest, recap, gaps
        action: String,

        /// Macro subcommand (for `analytics macro`): metrics, compare, cycles, outcomes, parallels, log
        value: Option<String>,

        /// Optional macro argument #1 (example: `US` in `analytics macro metrics US`)
        value2: Option<String>,

        /// Optional macro argument #2 (example: `China` in `analytics macro compare US China`)
        value3: Option<String>,

        /// Symbol filter (mainly for `signals`)
        #[arg(long)]
        symbol: Option<String>,

        /// Country filter (repeatable for comparison/history views)
        #[arg(long)]
        country: Vec<String>,

        /// Metric name for macro metric updates
        #[arg(long)]
        metric: Option<String>,

        /// Numeric score for macro metric updates
        #[arg(long)]
        score: Option<f64>,

        /// Ranking value for macro metric updates
        #[arg(long)]
        rank: Option<i32>,

        /// Trend label (e.g. rising, stable, declining)
        #[arg(long)]
        trend: Option<String>,

        /// Probability value for macro outcome updates
        #[arg(long)]
        probability: Option<f64>,

        /// Stage/phase for macro cycle updates
        #[arg(long = "phase", alias = "stage")]
        phase: Option<String>,

        /// Free-text evidence for macro cycle updates
        #[arg(long)]
        evidence: Option<String>,

        /// Free-text notes
        #[arg(long)]
        notes: Option<String>,

        /// Data source / citation label
        #[arg(long)]
        source: Option<String>,

        /// Driver text for outcome updates
        #[arg(long)]
        driver: Option<String>,

        /// Impact text for structural log rows
        #[arg(long)]
        impact: Option<String>,

        /// Outcome shift text for structural log rows
        #[arg(long)]
        outcome: Option<String>,

        /// Decade filter for macro history views (e.g. 1940)
        #[arg(long)]
        decade: Option<i32>,

        /// Show composite trajectories in macro history view
        #[arg(long)]
        composite: bool,

        /// CSV file path for batch import operations
        #[arg(long)]
        file: Option<String>,

        /// Signal type filter: alignment, divergence, transition
        #[arg(long)]
        signal_type: Option<String>,

        /// Severity filter: info, notable, critical
        #[arg(long)]
        severity: Option<String>,

        /// Agent role for digest mode (e.g. low-agent, medium-agent, evening-analyst)
        #[arg(long)]
        from: Option<String>,

        /// Date filter (YYYY-MM-DD, today, yesterday) for recap mode
        #[arg(long)]
        date: Option<String>,

        /// Max results
        #[arg(long)]
        limit: Option<usize>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

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

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show prediction market odds from Polymarket and Manifold
    Predictions {
        /// Filter by category: crypto, economics, geopolitics, ai, finance, macro (supports pipe lists, e.g. geopolitics|macro). Defaults to "macro" (economics|geopolitics|crypto).
        #[arg(long)]
        category: Option<String>,

        /// Search question text/topics (e.g. "ceasefire", "Fed rate")
        #[arg(long)]
        search: Option<String>,

        /// Maximum number of markets to show (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Track your market predictions and score accuracy
    #[command(name = "predict")]
    Predict {
        /// Action: add, list, score, stats, scorecard
        action: String,

        /// Prediction claim text (for add)
        value: Option<String>,

        #[arg(long)]
        id: Option<i64>,

        #[arg(long)]
        symbol: Option<String>,

        #[arg(long)]
        conviction: Option<String>,

        /// Timeframe: low, medium, high, macro
        #[arg(long)]
        timeframe: Option<String>,

        /// Confidence score (0.0 - 1.0)
        #[arg(long)]
        confidence: Option<f64>,

        /// Source agent identifier (e.g. low-agent, evening-analyst)
        #[arg(long = "source-agent")]
        source_agent: Option<String>,

        /// Expected resolution date
        #[arg(long)]
        target_date: Option<String>,

        /// Explicit scoring criterion (e.g. "daily close above 5000")
        #[arg(long = "resolution-criteria")]
        resolution_criteria: Option<String>,

        /// Outcome: correct, partial, wrong
        #[arg(long)]
        outcome: Option<String>,

        /// Scoring notes
        #[arg(long)]
        notes: Option<String>,

        /// Lesson learned after scoring
        #[arg(long)]
        lesson: Option<String>,

        /// Filter: pending, correct, partial, wrong
        #[arg(long)]
        filter: Option<String>,

        /// Date filter for scorecard: YYYY-MM-DD, today, yesterday
        #[arg(long)]
        date: Option<String>,

        #[arg(long)]
        limit: Option<usize>,

        #[arg(long)]
        json: bool,
    },

    /// Show rolling correlation matrix for held assets and key macro anchors
    Correlations {
        /// Action: compute (default) or history
        action: Option<String>,

        /// Symbol A (for history)
        value: Option<String>,

        /// Symbol B (for history)
        value2: Option<String>,

        /// Primary window for sorting/display emphasis: 7, 30, or 90
        #[arg(long, default_value = "30")]
        window: usize,

        /// Period for snapshots/history: 7d, 30d, 90d
        #[arg(long)]
        period: Option<String>,

        /// Store computed correlations as snapshots
        #[arg(long)]
        store: bool,

        /// Maximum number of pairs to show
        #[arg(long, default_value = "15")]
        limit: usize,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show latest financial news from RSS feeds
    News {
        /// Filter by source (e.g. "Reuters", "CoinDesk", "ZeroHedge")
        #[arg(long)]
        source: Option<String>,

        /// Search title text (case-insensitive substring match)
        #[arg(long)]
        search: Option<String>,

        /// Show only news from last N hours
        #[arg(long)]
        hours: Option<i64>,

        /// Maximum number of articles to show (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show market sentiment: Fear & Greed indices + COT positioning
    Sentiment {
        /// Symbol to show COT detail (GC=F, SI=F, CL=F, BTC). Omit for overview.
        symbol: Option<String>,

        /// Show historical F&G trend over N days
        #[arg(long)]
        history: Option<usize>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show COMEX warehouse inventory (gold, silver)
    Supply {
        /// Specific metal symbol (GC=F for gold, SI=F for silver). Omit for all.
        symbol: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show upcoming economic calendar events
    Calendar {
        /// Number of days to look ahead (default: 7)
        #[arg(long, default_value = "7")]
        days: i64,

        /// Filter by impact level: high, medium, low
        #[arg(long)]
        impact: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Manage price, allocation, and indicator alerts
    Alerts {
        /// Action: add, list, remove, check, ack, rearm
        action: String,

        /// Alert rule (for add) or alert ID (for remove/ack/rearm)
        value: Option<String>,

        /// Output as JSON (for check and list)
        #[arg(long)]
        json: bool,

        /// Filter by status: armed, triggered, acknowledged (for list)
        #[arg(long)]
        status: Option<String>,
    },

    /// Manage allocation targets for positions
    Target {
        /// Action: set, list, remove
        action: String,

        /// Symbol (for set/remove)
        symbol: Option<String>,

        /// Target allocation percentage (e.g. "25", "10.5"). Accepts % suffix.
        #[arg(long)]
        target: Option<String>,

        /// Drift band percentage (default: 2%). Accepts % suffix.
        #[arg(long)]
        band: Option<String>,

        /// Output as JSON (for list)
        #[arg(long)]
        json: bool,
    },

    /// Show allocation drift vs targets
    Drift {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Suggest trades to rebalance to target allocations
    Rebalance {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Track asset conviction scores over time (-5 to +5)
    Conviction {
        /// Action: set, list, history, changes
        action: String,

        /// Symbol (for set/history) or days (for changes, default 7)
        value: Option<String>,

        /// Score -5 to +5 (negative values: prefer --score=-2)
        #[arg(long)]
        score: Option<i32>,

        /// Compatibility positional for negative score after `--` (e.g. `... -- -2`)
        #[arg(hide = true)]
        score_positional: Option<String>,

        /// Notes explaining the score
        #[arg(long)]
        notes: Option<String>,

        /// Max results for history
        #[arg(long)]
        limit: Option<usize>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Manage trade journal and decision log
    Journal {
        /// Action: add, list, search, update, remove, tags, stats
        action: String,

        /// Content text (for add) or search query (for search)
        value: Option<String>,

        /// Entry ID (for update/remove)
        #[arg(long)]
        id: Option<i64>,

        /// ISO 8601 timestamp (for add). Defaults to now.
        #[arg(long)]
        date: Option<String>,

        /// Tag: trade, thesis, prediction, reflection, alert, lesson, call
        #[arg(long)]
        tag: Option<String>,

        /// Asset symbol (e.g. GC=F, BTC)
        #[arg(long)]
        symbol: Option<String>,

        /// Conviction: high, medium, low
        #[arg(long)]
        conviction: Option<String>,

        /// Entry status: open, validated, invalidated, closed
        #[arg(long)]
        status: Option<String>,

        /// Filter by status (for list)
        #[arg(long)]
        filter_status: Option<String>,

        /// Updated content (for update)
        #[arg(long)]
        content: Option<String>,

        /// Time filter: "7d", "30d", "2026-02-24" (for list/search)
        #[arg(long)]
        since: Option<String>,

        /// Maximum number of results (for list/search)
        #[arg(long)]
        limit: Option<usize>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

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
    },

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
    },

    /// Track dividend payments, ex-dates, and trailing yield
    Dividends {
        /// Action: add, list, remove
        action: String,

        /// Symbol (for add), ID (for remove), or optional symbol filter (for list)
        value: Option<String>,

        /// Amount per share (for add)
        #[arg(long)]
        amount: Option<String>,

        /// Pay date in YYYY-MM-DD (for add)
        #[arg(long)]
        pay_date: Option<String>,

        /// Ex-dividend date in YYYY-MM-DD (for add)
        #[arg(long)]
        ex_date: Option<String>,

        /// Currency (default: USD)
        #[arg(long, default_value = "USD")]
        currency: String,

        /// Optional note
        #[arg(long)]
        notes: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Add, view, or remove position thesis annotations
    Annotate {
        /// Asset symbol (required unless using --list)
        symbol: Option<String>,

        /// Thesis text
        #[arg(long)]
        thesis: Option<String>,

        /// Invalidation criteria
        #[arg(long)]
        invalidation: Option<String>,

        /// Review date in YYYY-MM-DD
        #[arg(long)]
        review_date: Option<String>,

        /// Target price/level
        #[arg(long)]
        target: Option<String>,

        /// Show annotation for symbol
        #[arg(long)]
        show: bool,

        /// List all annotations
        #[arg(long)]
        list: bool,

        /// Remove annotation for symbol
        #[arg(long)]
        remove: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Manage named asset groups
    Group {
        /// Action: create, list, show, remove
        action: String,

        /// Group name (required for create/show/remove)
        name: Option<String>,

        /// Comma-separated symbols for create (e.g. GC=F,SI=F,BTC)
        #[arg(long)]
        symbols: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// One-time migration from legacy JOURNAL.md into SQLite journal table
    MigrateJournal {
        /// Path to source markdown journal file
        #[arg(long, default_value = "JOURNAL.md")]
        path: String,

        /// Parse and report but do not write to database
        #[arg(long)]
        dry_run: bool,

        /// Default tag for entries without explicit tag metadata
        #[arg(long)]
        default_tag: Option<String>,

        /// Default status for entries without explicit status metadata
        #[arg(long, default_value = "open")]
        default_status: String,

        /// Output summary as JSON
        #[arg(long)]
        json: bool,
    },

    /// Start the web dashboard server
    Web {
        /// Port to bind to (default: 8080)
        #[arg(long, short, default_value = "8080")]
        port: u16,

        /// Host to bind to (default: 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,

        /// Disable authentication (NOT recommended for non-localhost)
        #[arg(long)]
        no_auth: bool,
    },

    /// Show sector + defense performance (XLE/XLF/XLK + ITA/LMT/RTX/PLTR)
    Sector {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Treemap-style sector heatmap using 1D performance
    Heatmap {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show options chain for an equity symbol (Yahoo free data)
    Options {
        /// Underlying symbol (e.g. AAPL, TSLA)
        symbol: String,

        /// Expiry date in YYYY-MM-DD (default: nearest expiry)
        #[arg(long)]
        expiry: Option<String>,

        /// Number of strikes per side to show (default: 12)
        #[arg(long, default_value = "12")]
        limit: usize,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Manage named portfolios (list/current/create/switch/remove)
    Portfolio {
        /// Action: list, current, create, switch, remove
        action: String,

        /// Portfolio name (for create/switch/remove)
        name: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Run named portfolio stress scenarios
    StressTest {
        /// Scenario name (e.g. "2008 GFC", "Oil $100", "BTC 40k")
        scenario: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Run Brave web/news search for financial research queries
    Research {
        /// Research query text
        query: Option<String>,

        /// Use Brave news endpoint instead of web endpoint
        #[arg(long)]
        news: bool,

        /// Freshness window: pd (day), pw (week), pm (month), py (year)
        #[arg(long)]
        freshness: Option<String>,

        /// Number of results to return
        #[arg(long, default_value = "5")]
        count: usize,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Preset: latest Fed statements/speeches
        #[arg(long)]
        fed: bool,

        /// Preset: latest earnings for a symbol (e.g. --earnings TSLA)
        #[arg(long)]
        earnings: Option<String>,

        /// Preset: geopolitical developments
        #[arg(long)]
        geopolitics: bool,

        /// Preset: COT positioning for a symbol/asset (e.g. --cot gold)
        #[arg(long)]
        cot: Option<String>,

        /// Preset: ETF flows for an asset (e.g. --etf btc)
        #[arg(long)]
        etf: Option<String>,

        /// Preset: OPEC production/decision updates
        #[arg(long)]
        opec: bool,
    },

    /// Track structural macro cycles, power metrics, and historical parallels
    #[command(name = "structural")]
    Structural {
        /// Action: metric-set, metric-list, metric-history, cycle-set, cycle-list,
        ///         outcome-add, outcome-list, outcome-update, outcome-history,
        ///         parallel-add, parallel-list, parallel-search,
        ///         log-add, log-list, dashboard
        action: String,

        /// Value (country for metrics, cycle/outcome name, query, etc.)
        value: Option<String>,

        #[arg(long)]
        country: Option<String>,
        #[arg(long)]
        metric: Option<String>,
        #[arg(long)]
        score: Option<f64>,
        #[arg(long)]
        rank: Option<i32>,
        #[arg(long)]
        trend: Option<String>,
        #[arg(long)]
        stage: Option<String>,
        #[arg(long)]
        entered: Option<String>,
        #[arg(long)]
        probability: Option<f64>,
        #[arg(long)]
        horizon: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        parallel: Option<String>,
        #[arg(long)]
        impact: Option<String>,
        #[arg(long)]
        driver: Option<String>,
        #[arg(long)]
        period: Option<String>,
        #[arg(long)]
        event: Option<String>,
        #[arg(long)]
        parallel_to: Option<String>,
        #[arg(long)]
        similarity: Option<i32>,
        #[arg(long)]
        outcome: Option<String>,
        #[arg(long)]
        evidence: Option<String>,
        #[arg(long)]
        signals: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },

    /// Track high-timeframe structural trends and per-asset impacts
    #[command(name = "trends")]
    Trends {
        /// Action: add, list, update, evidence-add, evidence-list, impact-add, impact-list, dashboard
        /// Example: `pftui trends evidence-add --trend "AI capex wave" --date 2026-03-13 --evidence "Hyperscaler capex +35% YoY" --source "earnings call"`
        action: String,

        /// Value (trend name for add/update, or evidence text for evidence-add if `--evidence` is omitted)
        value: Option<String>,

        #[arg(long)]
        id: Option<i64>,
        #[arg(long)]
        timeframe: Option<String>,
        #[arg(long)]
        direction: Option<String>,
        #[arg(long)]
        conviction: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        asset_impact: Option<String>,
        #[arg(long)]
        key_signal: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        date: Option<String>,
        /// Evidence text (preferred for `evidence-add`; clearer than positional value)
        #[arg(long)]
        evidence: Option<String>,
        #[arg(long)]
        direction_impact: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        impact: Option<String>,
        #[arg(long)]
        mechanism: Option<String>,
        #[arg(long)]
        impact_timeframe: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, ValueEnum)]
pub enum ExportFormat {
    Csv,
    Json,
}

#[derive(Clone, ValueEnum)]
pub enum SummaryGroupBy {
    Category,
}

#[derive(Clone, ValueEnum)]
pub enum ImportModeArg {
    /// Wipe existing data and rebuild from snapshot (default)
    Replace,
    /// Add new entries without deleting existing data
    Merge,
}

/// Time period for P&L calculation.
#[derive(Clone, ValueEnum, Debug)]
pub enum SummaryPeriod {
    /// Today (since last market close)
    #[value(alias = "1d")]
    Today,
    /// One week
    #[value(name = "1w")]
    OneWeek,
    /// One month
    #[value(name = "1m")]
    OneMonth,
    /// Three months
    #[value(name = "3m")]
    ThreeMonths,
    /// One year
    #[value(name = "1y")]
    OneYear,
}

impl SummaryPeriod {
    /// Returns the number of days to subtract from today for this period.
    pub fn days_back(&self) -> i64 {
        match self {
            SummaryPeriod::Today => 1,
            SummaryPeriod::OneWeek => 7,
            SummaryPeriod::OneMonth => 30,
            SummaryPeriod::ThreeMonths => 90,
            SummaryPeriod::OneYear => 365,
        }
    }

    /// Returns a human-readable label for this period.
    pub fn label(&self) -> &'static str {
        match self {
            SummaryPeriod::Today => "today",
            SummaryPeriod::OneWeek => "1W",
            SummaryPeriod::OneMonth => "1M",
            SummaryPeriod::ThreeMonths => "3M",
            SummaryPeriod::OneYear => "1Y",
        }
    }
}
