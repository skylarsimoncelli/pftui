use clap::{Parser, Subcommand, ValueEnum};

#[derive(Subcommand)]
pub enum AgentMessageCommand {
    /// Send one or more structured messages
    Send {
        /// Message content (positional text or one/more --batch values)
        value: Option<String>,

        /// Batch mode for send: repeat to enqueue multiple related messages
        #[arg(long = "batch")]
        batch: Vec<String>,

        /// Optional logical package id shared across all messages in the batch
        #[arg(long = "package-id")]
        package_id: Option<String>,

        /// Optional logical package title shared across all messages in the batch
        #[arg(long = "package-title")]
        package_title: Option<String>,

        /// Sender (required)
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

        #[arg(long)]
        json: bool,
    },
    /// List queued or historical messages
    List {
        /// Sender filter
        #[arg(long)]
        from: Option<String>,

        #[arg(long)]
        to: Option<String>,

        /// Analytics engine layer: low, medium, high, macro, cross
        #[arg(long)]
        layer: Option<String>,

        /// Show only unacknowledged
        #[arg(long)]
        unacked: bool,

        /// Time filter
        #[arg(long)]
        since: Option<String>,

        #[arg(long = "package-id")]
        package_id: Option<String>,

        #[arg(long)]
        limit: Option<usize>,

        #[arg(long)]
        json: bool,
    },
    /// Reply to an existing message
    Reply {
        /// Message content
        value: Option<String>,

        #[arg(long)]
        id: Option<i64>,

        /// Sender (required)
        #[arg(long)]
        from: Option<String>,

        #[arg(long)]
        priority: Option<String>,

        #[arg(long)]
        category: Option<String>,

        #[arg(long)]
        layer: Option<String>,

        #[arg(long)]
        json: bool,
    },
    /// Escalate an issue on an existing message
    Flag {
        /// Escalation reason
        value: Option<String>,

        #[arg(long)]
        id: Option<i64>,

        /// Explicitly mark this flag as a data-quality issue
        #[arg(long)]
        quality: bool,

        /// Sender (required)
        #[arg(long)]
        from: Option<String>,

        #[arg(long)]
        priority: Option<String>,

        #[arg(long)]
        category: Option<String>,

        #[arg(long)]
        layer: Option<String>,

        #[arg(long)]
        json: bool,
    },
    /// Acknowledge messages by ID, or all at once with --all
    ///
    /// Examples:
    ///   pftui agent message ack --id 1 --id 2    # ack specific messages
    ///   pftui agent message ack --all             # ack all pending messages
    ///   pftui agent message ack --all --to bot-x  # ack all for a specific recipient
    Ack {
        /// One or more message IDs (repeatable: --id 1 --id 2 --id 3)
        #[arg(long)]
        id: Vec<i64>,

        /// Acknowledge ALL pending messages (same as `ack-all`)
        #[arg(long, conflicts_with = "id")]
        all: bool,

        /// Filter by recipient when using --all
        #[arg(long, requires = "all")]
        to: Option<String>,

        #[arg(long)]
        json: bool,
    },
    /// Acknowledge all pending messages for a recipient (alias for `ack --all`)
    #[command(name = "ack-all")]
    AckAll {
        #[arg(long)]
        to: Option<String>,

        #[arg(long)]
        json: bool,
    },
    /// Purge old messages
    Purge {
        /// Days to retain before purge
        #[arg(long)]
        days: Option<usize>,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Inter-agent structured message passing
    Message {
        #[command(subcommand)]
        command: AgentMessageCommand,
    },
    /// Adversarial debate mechanism — structured bull/bear arguments on assets or scenarios.
    ///
    /// Start debates, add rounds with bull/bear arguments, and resolve with a summary.
    /// Designed for single-agent structured argumentation (agent plays both sides).
    ///
    /// Examples:
    ///   pftui agent debate start --topic "BTC to 200k this cycle?" --rounds 3
    ///   pftui agent debate add-round --debate-id 1 --round 1 --position bull \
    ///     --argument "Halving supply shock + ETF demand" --evidence "ETF flow data, on-chain metrics"
    ///   pftui agent debate add-round --debate-id 1 --round 1 --position bear \
    ///     --argument "Macro headwinds, DXY strength" --evidence "Fed dot plot, DXY chart"
    ///   pftui agent debate resolve --debate-id 1 --summary "Bull case stronger near-term"
    ///   pftui agent debate history --json
    ///   pftui agent debate summary --debate-id 1 --json
    Debate {
        #[command(subcommand)]
        command: AgentDebateCommand,
    },
}

#[derive(Subcommand)]
pub enum AgentDebateCommand {
    /// Start a new adversarial debate on a topic
    ///
    /// Examples:
    ///   pftui agent debate start --topic "Is gold going to 5000?" --rounds 3
    ///   pftui agent debate start --topic "US recession in 2026?" --json
    Start {
        /// Topic of the debate (asset, scenario, or macro question)
        #[arg(long)]
        topic: String,
        /// Number of argument rounds (default 3, max 10)
        #[arg(long, default_value = "3")]
        rounds: i64,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Add a bull or bear argument to a debate round
    ///
    /// Examples:
    ///   pftui agent debate add-round --debate-id 1 --round 1 --position bull \
    ///     --argument "ETF inflows accelerating" --evidence "BlackRock IBIT data"
    #[command(name = "add-round")]
    AddRound {
        /// ID of the debate to add to
        #[arg(long = "debate-id")]
        debate_id: i64,
        /// Round number (1-indexed)
        #[arg(long)]
        round: i64,
        /// Position: bull or bear
        #[arg(long)]
        position: String,
        /// The argument text
        #[arg(long)]
        argument: String,
        /// Source agent name (e.g. high-agent, evening-analysis)
        #[arg(long = "agent-source")]
        agent_source: Option<String>,
        /// Evidence references supporting the argument
        #[arg(long)]
        evidence: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Resolve (close) a debate with an optional summary
    ///
    /// Examples:
    ///   pftui agent debate resolve --debate-id 1 --summary "Bull case wins on flow data"
    Resolve {
        /// ID of the debate to resolve
        #[arg(long = "debate-id")]
        debate_id: i64,
        /// Resolution summary explaining which side prevailed and why
        #[arg(long)]
        summary: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// List past debates with optional filters
    ///
    /// Examples:
    ///   pftui agent debate history --json
    ///   pftui agent debate history --status active
    ///   pftui agent debate history --topic gold --limit 5
    History {
        /// Filter by status: active, resolved
        #[arg(long)]
        status: Option<String>,
        /// Filter by topic keyword
        #[arg(long)]
        topic: Option<String>,
        /// Maximum debates to show
        #[arg(long)]
        limit: Option<usize>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Show full debate detail with all rounds
    ///
    /// Examples:
    ///   pftui agent debate summary --debate-id 1 --json
    ///   pftui agent debate summary --json  # shows latest debate
    Summary {
        /// Debate ID (shows latest if omitted)
        #[arg(long = "debate-id")]
        debate_id: Option<i64>,
        /// Output as JSON for agent/script consumption
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

#[derive(Subcommand)]
pub enum DashboardCommand {
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
    /// Sector heatmap and sector-relative performance views
    Sector {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Treemap-style market heatmap by category and move
    Heatmap {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Global macro dashboard for country indicators
    Global {
        /// Country code (e.g., US, CN, IN). Omit to show defaults.
        country: Option<String>,
        /// Indicator key (e.g., GDP, debt_to_gdp)
        indicator: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DataCommand {
    /// Fetch and cache current prices for tracked symbols
    Refresh {
        /// Send OS notification for newly triggered alerts
        #[arg(long)]
        notify: bool,
        /// Output structured JSON metrics instead of human-readable text
        #[arg(long)]
        json: bool,
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
    /// Pre-built dashboard views
    Dashboard {
        #[command(subcommand)]
        command: DashboardCommand,
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

        /// Include sentiment score and label for each article (keyword-based)
        #[arg(long = "with-sentiment")]
        with_sentiment: bool,

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
    /// Interpret cached COT positioning using percentile and z-score context
    Cot {
        /// Optional tracked symbol (GC=F, SI=F, CL=F, BTC)
        symbol: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// CME FedWatch probabilities from Fed funds futures implied pricing
    Fedwatch {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Show cached BTC on-chain metrics from the latest refresh
    Onchain {
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
    /// Store and query slower-moving analyst consensus calls
    Consensus {
        #[command(subcommand)]
        command: ConsensusCommand,
    },
    /// Prediction intelligence: market odds (Polymarket/Manifold) and personal prediction tracking (stats, scorecard, unanswered)
    Predictions {
        #[command(subcommand)]
        command: Option<DataPredictionsCommand>,

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
    /// Show BTC ETF flow data (inflows/outflows by fund)
    #[command(name = "etf-flows")]
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
    /// Show COMEX warehouse inventory (gold, silver)
    Supply {
        /// Specific metal symbol (GC=F for gold, SI=F for silver). Omit for all.
        symbol: Option<String>,

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
    /// Consolidated closing prices for all portfolio + watchlist symbols
    #[command(alias = "quotes", after_help = "Aliases: `data quotes` also works.\n\nFor overnight futures specifically, see: pftui data futures\nFor market overview symbols, add --market flag.")]
    Prices {
        /// Include all market overview symbols (indices, commodities, crypto, forex, bonds)
        #[arg(long)]
        market: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Backfill missing OHLCV data for existing price history (re-fetches from Yahoo Finance)
    Backfill {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// EIA weekly crude oil inventory and Strategic Petroleum Reserve (SPR) levels
    #[command(name = "oil-inventory")]
    OilInventory {
        /// Number of weeks of history to fetch for context (default: 52)
        #[arg(long, default_value = "52")]
        weeks: usize,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Overnight futures prices for pre-market positioning (ES, NQ, YM, RTY, GC, SI, CL)
    #[command(after_help = "For portfolio/watchlist price quotes, see: pftui data prices (alias: data quotes)\nFor market overview prices, see: pftui data prices --market")]
    Futures {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Oil futures term structure: contango/backwardation, WTI-Brent spread, war-premium signal
    #[command(name = "oil-premium")]
    OilPremium {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Alert management lives under `analytics alerts` — this redirects there
    #[command(name = "alerts", after_help = "Alerts are managed under the analytics domain:\n\n  pftui analytics alerts list        List alert rules\n  pftui analytics alerts check       Check alerts against current data\n  pftui analytics alerts add          Add an alert rule\n  pftui analytics alerts ack          Acknowledge triggered alerts\n  pftui analytics alerts seed-defaults Seed smart-alert defaults\n\nRun `pftui analytics alerts --help` for full details.")]
    Alerts {
        #[command(subcommand)]
        command: Option<DataAlertsRedirect>,
    },
}

#[derive(Subcommand)]
pub enum DataAlertsRedirect {
    /// → Redirects to `analytics alerts check`
    Check {
        #[arg(long)]
        today: bool,
        #[arg(long)]
        json: bool,
    },
    /// → Redirects to `analytics alerts list`
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        triggered: bool,
        #[arg(long)]
        since: Option<i64>,
        #[arg(long)]
        today: bool,
        #[arg(long)]
        recent: bool,
        #[arg(long, default_value = "24")]
        recent_hours: i64,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ConsensusCommand {
    /// Add a new analyst consensus call
    Add {
        /// Research source or institution
        #[arg(long)]
        source: String,

        /// Topic key, e.g. rate_cuts or gold_target
        #[arg(long)]
        topic: String,

        /// The actual analyst call text
        #[arg(long = "call")]
        call_text: String,

        /// Date of the call in YYYY-MM-DD
        #[arg(long)]
        date: String,

        /// Output inserted row as JSON
        #[arg(long)]
        json: bool,
    },
    /// List stored analyst consensus calls
    List {
        /// Filter by topic key
        #[arg(long)]
        topic: Option<String>,

        /// Filter by research source
        #[arg(long)]
        source: Option<String>,

        /// Maximum rows to return
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DataPredictionsCommand {
    /// Show prediction market contract odds from Polymarket (tag-based macro-relevant contracts)
    #[command(after_help = "Sources: Polymarket Gamma events API (fed, economics, geopolitics, politics, bitcoin, crypto, ai tags).\n\nWhen the enriched prediction_market_contracts table is populated (via `pftui refresh`), shows contracts with exchange, event grouping, liquidity, and end dates. Falls back to legacy predictions_cache when contracts table is empty.\n\nSee also: `data predictions stats`, `data predictions scorecard`, `data predictions unanswered`, `analytics predictions`")]
    Markets {
        /// Filter by category: crypto, economics, geopolitics, ai, finance, macro
        #[arg(long)]
        category: Option<String>,

        /// Search question text/topics
        #[arg(long)]
        search: Option<String>,

        /// Maximum number of markets to show (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Prediction accuracy statistics — hit rate by conviction, timeframe, symbol, and agent
    Stats {
        /// Filter by timeframe: low, medium, high, macro
        #[arg(long)]
        timeframe: Option<String>,

        /// Filter by source agent (e.g. low-agent, medium-agent)
        #[arg(long)]
        agent: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Prediction scorecard — date-ordered scored predictions with outcomes
    Scorecard {
        /// Filter by date (YYYY-MM-DD)
        #[arg(long)]
        date: Option<String>,

        /// Maximum predictions to show
        #[arg(long)]
        limit: Option<usize>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List unanswered/pending predictions awaiting scoring
    Unanswered {
        /// Filter by timeframe: low, medium, high, macro
        #[arg(long)]
        timeframe: Option<String>,

        /// Filter by symbol
        #[arg(long)]
        symbol: Option<String>,

        /// Maximum predictions to show
        #[arg(long)]
        limit: Option<usize>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Link a prediction market contract to a pftui scenario. On each refresh, the contract's probability is auto-logged as a scenario history data point.
    #[command(
        after_help = "Maps a Polymarket contract to a pftui scenario so that every\n`pftui data refresh` automatically logs the market probability as a\ndata point in the scenario's history timeline.\n\nUse --search to find contracts by keyword (matches question and event\ntitle). Use --scenario to specify the scenario name.\n\nExample:\n  pftui data predictions map --scenario \"US Recession 2026\" --search \"recession\"\n\nTo see all mappings:\n  pftui data predictions map --list\n\nSee also: `data predictions markets`, `analytics scenario list`,\n          `analytics calibration` (F55.5)"
    )]
    Map {
        /// Scenario name to link (must match an existing scenario)
        #[arg(long)]
        scenario: Option<String>,

        /// Search query to find a contract by question/event title
        #[arg(long)]
        search: Option<String>,

        /// Specific contract_id to link (alternative to --search)
        #[arg(long)]
        contract: Option<String>,

        /// List all existing scenario-contract mappings
        #[arg(long)]
        list: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a scenario-contract mapping
    #[command(
        after_help = "Removes the link between a scenario and a prediction market contract.\nThe contract and scenario remain intact; only the mapping is deleted.\n\nUse --scenario to remove all mappings for a scenario, or provide\nboth --scenario and --contract to remove a specific mapping.\n\nSee also: `data predictions map --list`"
    )]
    Unmap {
        /// Scenario name to unlink
        #[arg(long, required = true)]
        scenario: String,

        /// Specific contract_id to unlink (if omitted, removes all mappings for this scenario)
        #[arg(long)]
        contract: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PortfolioTransactionCommand {
    /// Add a transaction
    Add {
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
    Remove {
        /// Transaction ID to remove
        id: i64,
    },
    /// List all transactions
    List {
        /// Show transaction notes column
        #[arg(long)]
        notes: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PortfolioProfilesCommand {
    /// List all portfolio profiles
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the current active portfolio profile
    Current {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a named portfolio profile
    Create {
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Switch to a named portfolio profile
    Switch {
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a named portfolio profile
    Remove {
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PortfolioTargetCommand {
    /// Set a target allocation
    Set {
        /// Symbol to target
        symbol: Option<String>,

        /// Target allocation percentage (e.g. "25", "10.5"). Accepts % suffix.
        #[arg(long)]
        target: Option<String>,

        /// Drift band percentage (default: 2%). Accepts % suffix.
        #[arg(long)]
        band: Option<String>,
    },
    /// List current target allocations
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a target allocation
    Remove {
        /// Symbol to remove
        symbol: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PortfolioOpportunityCommand {
    /// Add an opportunity-cost entry
    Add {
        /// Event description
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
        json: bool,
    },
    /// List opportunity-cost entries
    List {
        #[arg(long)]
        since: Option<String>,

        #[arg(long)]
        asset: Option<String>,

        #[arg(long)]
        limit: Option<usize>,

        #[arg(long)]
        json: bool,
    },
    /// Show opportunity-cost summary stats
    Stats {
        #[arg(long)]
        since: Option<String>,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PortfolioCommand {
    /// Portfolio summary to stdout (default when no subcommand is provided)
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
    /// Show total portfolio value with gain/loss (uses cached prices)
    Value {
        /// Output JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Output a markdown-formatted portfolio brief for agent consumption and daily reports
    Brief {
        /// Output structured JSON (includes all available data)
        #[arg(long)]
        json: bool,
    },
    /// End-of-Day summary: brief + movers + macro + sentiment combined
    Eod {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Show today's P&L per position and total (price change vs previous close)
    #[command(name = "daily-pnl")]
    DailyPnl {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Show total unrealized gain/loss across all positions with cost basis comparison
    Unrealized {
        /// Group output by asset category
        #[arg(long, value_enum)]
        group_by: Option<SummaryGroupBy>,

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
    /// Show portfolio value and positions as of a past date using cached price history
    History {
        /// Target date in YYYY-MM-DD format (e.g. 2026-02-28)
        #[arg(long)]
        date: String,

        /// Group output by a field (e.g. "category")
        #[arg(long, value_enum)]
        group_by: Option<SummaryGroupBy>,
    },
    /// Manage allocation targets for positions
    Target {
        #[command(subcommand)]
        command: PortfolioTargetCommand,
    },
    /// Quick allocation snapshot: each position's weight in the portfolio
    Allocation {
        /// Group output by a field (e.g. "category")
        #[arg(long, value_enum)]
        group_by: Option<SummaryGroupBy>,

        /// Output as JSON
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
    /// Run named portfolio stress scenarios
    #[command(name = "stress-test")]
    StressTest {
        /// Scenario name (e.g. "2008 GFC", "Oil $100", "BTC 40k")
        scenario: String,

        /// Output as JSON
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
    /// Track what your positioning saved and cost you
    Opportunity {
        #[command(subcommand)]
        command: PortfolioOpportunityCommand,
    },
    /// Manage named portfolio profiles
    Profiles {
        #[command(subcommand)]
        command: PortfolioProfilesCommand,
    },
    /// Track symbols you do not currently hold
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
    #[command(name = "set-cash")]
    SetCash {
        /// Currency symbol (e.g. USD, GBP, EUR)
        symbol: String,
        /// Amount to set (e.g. 45000, 12500.50). Use 0 to clear.
        amount: String,
    },
    /// Manage transactions
    Transaction {
        #[command(subcommand)]
        command: PortfolioTransactionCommand,
    },
    /// Connect and sync broker accounts
    Broker {
        #[command(subcommand)]
        command: PortfolioBrokerCommand,
    },
}

#[derive(Subcommand)]
pub enum PortfolioBrokerCommand {
    /// Add or update a broker connection
    Add {
        /// Broker name (trading212, ibkr, binance, kraken, coinbase, crypto-com)
        broker: crate::broker::BrokerKind,
        /// API key or access token
        #[arg(long)]
        api_key: Option<String>,
        /// API secret or private key
        #[arg(long)]
        secret: Option<String>,
        /// Optional label for this connection
        #[arg(long)]
        label: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Sync positions from a broker (or all configured brokers)
    Sync {
        /// Broker to sync (omit to sync all configured brokers)
        broker: Option<crate::broker::BrokerKind>,
        /// Show what would be synced without writing any data
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a broker connection and its synced transactions
    Remove {
        /// Broker name to remove
        broker: crate::broker::BrokerKind,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List configured broker connections
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DaemonCommand {
    /// Start the daemon (foreground — use systemd/screen/tmux to background)
    Start {
        /// Refresh interval in seconds (default: 300 = 5 minutes)
        #[arg(long, default_value = "300")]
        interval: u64,

        /// Output structured JSON log lines instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Show daemon status (reads heartbeat file)
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum UniverseCommand {
    /// List all tracked universe groups and symbols
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a symbol to a universe group
    Add {
        /// Symbol to add
        symbol: String,
        /// Group name (indices, sectors, commodities, fx, rates, crypto_majors, custom)
        #[arg(long, default_value = "custom")]
        group: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a symbol from a universe group
    Remove {
        /// Symbol to remove
        symbol: String,
        /// Group name (indices, sectors, commodities, fx, rates, crypto_majors, custom)
        #[arg(long, default_value = "custom")]
        group: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SystemCommand {
    /// Run as a background daemon: refresh data + evaluate alerts on a loop
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
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
    /// Export portfolio data (JSON exports full snapshot; CSV exports positions only)
    Export {
        #[arg(value_enum)]
        format: ExportFormat,

        /// Write output to a file instead of stdout
        #[arg(long, short)]
        output: Option<String>,
    },
    /// Import data from a JSON snapshot file (as produced by `pftui export json`)
    Import {
        /// Path to the JSON snapshot file
        path: String,

        /// Import mode: replace wipes existing data, merge adds without deleting
        #[arg(long, value_enum, default_value = "replace")]
        mode: ImportModeArg,
    },
    /// Sync a remote Postgres source into the local SQLite mirror
    Mirror {
        #[command(subcommand)]
        command: MirrorCommand,
    },
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
    /// Run the portfolio setup wizard
    Setup,
    /// Launch pftui with a realistic demo portfolio (your real data is untouched)
    Demo,
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
    /// Native iOS mobile API server controls
    Mobile {
        #[command(subcommand)]
        command: MobileCommand,
    },
    /// Manage the tracked symbol universe (indices, sectors, commodities, FX, rates, crypto, custom)
    Universe {
        #[command(subcommand)]
        command: UniverseCommand,
    },
    /// Search all CLI commands by keyword (helps agents discover features)
    Search {
        /// Search query (matches command paths and descriptions)
        query: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show US equity market hours: current phase, next open/close, agent guidance
    #[command(name = "market-hours")]
    MarketHours {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// One-time migration from legacy JOURNAL.md into SQLite journal table
    #[command(name = "migrate-journal")]
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
}

#[derive(Subcommand)]
pub enum MobileCommand {
    /// Enable the mobile API and generate TLS credentials
    Enable {
        /// Host to bind to (use 0.0.0.0 for same-WiFi phone access)
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,

        /// Port to bind to (default: 9443)
        #[arg(long, short, default_value = "9443")]
        port: u16,
    },
    /// Disable the mobile API
    Disable,
    /// Show mobile API configuration and certificate fingerprint
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage long-lived mobile API tokens
    Token {
        #[command(subcommand)]
        command: MobileTokenCommand,
    },
    /// Start the TLS mobile API server
    Serve,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum MobileTokenPermissionArg {
    Read,
    Write,
}

#[derive(Subcommand)]
pub enum MobileTokenCommand {
    /// Generate a new mobile API token and print it once
    Generate {
        /// Human-readable token label
        #[arg(long, default_value = "ios")]
        name: String,

        /// Token permission scope
        #[arg(long, value_enum, default_value = "read")]
        permission: MobileTokenPermissionArg,
    },
    /// List all configured mobile API tokens (names, permissions, dates — not hashes)
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Revoke a mobile API token by name or prefix
    Revoke {
        /// Token name or prefix to match
        prefix: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum MirrorCommand {
    /// Pull all public tables from a remote Postgres source into the local SQLite database
    Sync {
        /// Remote Postgres URL to mirror from. Defaults to mirror_source_url or current database_url.
        #[arg(long)]
        source_url: Option<String>,

        /// After syncing, switch the active backend to local SQLite and persist mirror_source_url.
        #[arg(long)]
        activate: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalEntryCommand {
    /// Add a journal entry. Content can be positional or via --content flag.
    ///
    /// Examples:
    ///   pftui journal entry add "Gold looking strong" --tag macro --symbol GC=F
    ///   pftui journal entry add --content "Fed meeting notes" --date 2026-03-27
    ///   pftui journal entry add "BTC thesis update" --conviction high --tag btc
    Add {
        /// The journal entry text (positional). Alternative: use --content flag.
        value: Option<String>,
        /// The journal entry text (named flag). Overrides positional value if both given.
        #[arg(long)]
        content: Option<String>,
        #[arg(long, help = "Entry date (YYYY-MM-DD). Defaults to today.")]
        date: Option<String>,
        #[arg(long, help = "Tag for categorization (e.g. macro, btc, trade).")]
        tag: Option<String>,
        #[arg(long, help = "Related asset symbol (e.g. BTC-USD, GC=F).")]
        symbol: Option<String>,
        #[arg(long, help = "Conviction level (e.g. high, medium, low).")]
        conviction: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List journal entries with optional filters (date, tag, symbol, status)
    List {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        filter_status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Full-text search across journal entry content
    Search {
        query: String,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Update an existing journal entry by ID (content, status)
    Update {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a journal entry by ID
    Remove {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        json: bool,
    },
    /// List all tags used across journal entries
    Tags {
        #[arg(long)]
        json: bool,
    },
    /// Journal entry statistics: counts by tag, date range, conviction distribution
    Stats {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalPredictionCommand {
    /// Add a prediction. Timeframe accepts: low, medium, high, macro (aliases: short=low, long=high).
    /// Prefer --claim flag for the prediction text; positional form kept for backwards compatibility.
    ///
    /// Examples:
    ///   pftui journal prediction add --claim "BTC above 70k" --timeframe short --confidence 0.7
    ///   pftui journal prediction add "BTC above 70k" --timeframe short --confidence 0.7
    ///   pftui journal prediction add "Gold to 3000" medium 0.8
    Add {
        /// The prediction claim text (positional, backwards-compatible)
        value: Option<String>,
        /// The prediction claim text (named flag, preferred)
        #[arg(long)]
        claim: Option<String>,
        /// Timeframe (positional shorthand, backwards-compat): low|medium|high|macro|short|long
        timeframe_pos: Option<String>,
        /// Confidence (positional shorthand): 0.0..=1.0
        confidence_pos: Option<f64>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        conviction: Option<String>,
        /// Analytics timeframe: low, medium, high, macro (aliases: short=low, long=high). Preferred over positional.
        #[arg(long)]
        timeframe: Option<String>,
        #[arg(long)]
        confidence: Option<f64>,
        #[arg(long = "source-agent")]
        source_agent: Option<String>,
        #[arg(long)]
        target_date: Option<String>,
        #[arg(long = "resolution-criteria")]
        resolution_criteria: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List predictions with optional filters (status, timeframe, symbol)
    List {
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        timeframe: Option<String>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Score a prediction outcome: correct, partial, wrong, or pending
    Score {
        /// Prediction ID (flag form)
        #[arg(long)]
        id: Option<i64>,
        /// Prediction ID (positional form)
        id_pos: Option<i64>,
        #[arg(long)]
        outcome: Option<String>,
        /// Outcome (positional form): correct|partial|wrong|pending
        outcome_pos: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        /// Notes (positional form)
        notes_pos: Option<String>,
        #[arg(long)]
        lesson: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Score multiple predictions at once. Each entry is id:outcome (e.g. 3:correct 7:wrong 12:partial)
    #[command(name = "score-batch")]
    ScoreBatch {
        /// Pairs of id:outcome (e.g. 3:correct 7:wrong 12:partial)
        #[arg(required = true, num_args = 1..)]
        entries: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Prediction accuracy statistics: hit rate by conviction, timeframe, symbol, and agent
    Stats {
        /// Filter by timeframe: low, medium, high, macro
        #[arg(long)]
        timeframe: Option<String>,

        /// Filter by source agent (e.g. low-agent, medium-agent)
        #[arg(long)]
        agent: Option<String>,

        #[arg(long)]
        json: bool,
    },
    /// Date-ordered scorecard of scored predictions with outcomes and lessons
    Scorecard {
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Auto-score pending predictions whose target_date has passed, using market price data.
    /// Only scores unambiguous price-direction predictions (e.g., "BTC above $70K by Mar 28").
    /// Complex or qualitative predictions are left as pending.
    #[command(name = "auto-score")]
    AutoScore {
        /// Preview what would be scored without writing changes
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Structured lesson extraction from wrong predictions.
    ///
    /// Lists wrong predictions with their structured lessons (miss type, root cause, signal misread).
    /// Use `lessons add` to attach a lesson to a wrong prediction.
    ///
    /// Examples:
    ///   pftui journal prediction lessons --json
    ///   pftui journal prediction lessons --miss-type timing --json
    ///   pftui journal prediction lessons add --prediction-id 42 --miss-type directional \
    ///     --what-happened "BTC dropped to 60k" --why-wrong "Ignored macro headwinds"
    Lessons {
        #[command(subcommand)]
        command: Option<JournalPredictionLessonsCommand>,
        /// Filter by miss type: directional, timing, magnitude
        #[arg(long = "miss-type")]
        miss_type: Option<String>,
        /// Maximum lessons to show
        #[arg(long)]
        limit: Option<usize>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalPredictionLessonsCommand {
    /// Add a structured lesson for a wrong prediction
    ///
    /// Examples:
    ///   pftui journal prediction lessons add --prediction-id 42 --miss-type directional \
    ///     --what-happened "BTC dropped to 60k" --why-wrong "Ignored macro headwinds" \
    ///     --signal-misread "Volume divergence was bearish"
    Add {
        /// ID of the wrong prediction to attach the lesson to
        #[arg(long = "prediction-id")]
        prediction_id: i64,
        /// Type of miss: directional, timing, or magnitude
        #[arg(long = "miss-type")]
        miss_type: String,
        /// What actually happened (market outcome)
        #[arg(long = "what-happened")]
        what_happened: String,
        /// Why the prediction was wrong — root cause analysis
        #[arg(long = "why-wrong")]
        why_wrong: String,
        /// What signal was misread or missed (optional)
        #[arg(long = "signal-misread")]
        signal_misread: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalConvictionCommand {
    /// Set conviction score for an asset (-5 bearish to +5 bullish)
    Set {
        symbol: String,
        /// Score (positional shorthand): -5..+5
        #[arg(allow_hyphen_values = true)]
        score_pos: Option<i32>,
        #[arg(long)]
        score: Option<i32>,
        #[arg(long)]
        notes: Option<String>,
        /// Notes (positional shorthand)
        notes_pos: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List current conviction scores for all tracked assets
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show conviction score history for a specific asset over time
    History {
        symbol: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Show recent conviction changes across all assets (default: last 7 days)
    Changes {
        days: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalNotesCommand {
    /// Add a date-keyed narrative note (daily research log, market observations)
    Add {
        value: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        section: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List narrative notes with optional date range filter
    List {
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Full-text search across narrative notes
    Search {
        query: String,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a narrative note by ID
    Remove {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalScenarioSignalCommand {
    /// Add a signal (evidence or trigger) linked to a scenario
    Add {
        value: String,
        #[arg(long)]
        scenario: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List scenario signals with optional scenario/status filter
    List {
        #[arg(long)]
        scenario: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Update a scenario signal's evidence or status by ID
    Update {
        #[arg(long = "signal-id")]
        signal_id: i64,
        #[arg(long)]
        evidence: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a scenario signal by ID
    Remove {
        #[arg(long = "signal-id")]
        signal_id: i64,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalScenarioCommand {
    /// Add a new macro scenario with probability, triggers, and impact assessment
    Add {
        value: String,
        #[arg(long)]
        probability: Option<f64>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        impact: Option<String>,
        #[arg(long)]
        triggers: Option<String>,
        #[arg(long)]
        precedent: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List scenarios with optional status filter (active, resolved, archived)
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Update a scenario's probability, description, triggers, or status
    Update {
        value: String,
        /// History note / driver (positional shorthand)
        note_pos: Option<String>,
        #[arg(long)]
        probability: Option<f64>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        impact: Option<String>,
        #[arg(long)]
        triggers: Option<String>,
        #[arg(long)]
        precedent: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        driver: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a scenario by name
    Remove {
        value: String,
        #[arg(long)]
        json: bool,
    },
    /// Show probability history and driver log for a scenario
    History {
        value: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Manage scenario signals (evidence and triggers)
    Signal {
        #[command(subcommand)]
        command: JournalScenarioSignalCommand,
    },
    /// Promote a hypothesis scenario to an active situation
    Promote {
        /// Scenario name
        value: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalCommand {
    /// Journal entries and decision log rows
    Entry {
        #[command(subcommand)]
        command: JournalEntryCommand,
    },
    /// Prediction tracking and scoring
    Prediction {
        #[command(subcommand)]
        command: JournalPredictionCommand,
    },
    /// Asset conviction scores over time (-5 to +5)
    Conviction {
        #[command(subcommand)]
        command: JournalConvictionCommand,
    },
    /// Date-keyed narrative notes
    Notes {
        #[command(subcommand)]
        command: JournalNotesCommand,
    },
    /// Macro scenarios and scenario signals
    Scenario {
        #[command(subcommand)]
        command: JournalScenarioCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsCorrelationsCommand {
    /// Compute rolling correlations
    Compute {
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
    /// Show stored correlation history for a pair
    History {
        /// Symbol A
        symbol_a: String,
        /// Symbol B
        symbol_b: String,
        /// Primary window for sorting/display emphasis: 7, 30, or 90
        #[arg(long, default_value = "30")]
        window: usize,
        /// Period for snapshots/history: 7d, 30d, 90d
        #[arg(long)]
        period: Option<String>,
        /// Maximum number of rows to show
        #[arg(long, default_value = "15")]
        limit: usize,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Show the latest stored correlation snapshot rows
    Latest {
        /// Period for snapshots/history: 7d, 30d, 90d
        #[arg(long)]
        period: Option<String>,
        /// Maximum number of rows to show
        #[arg(long, default_value = "25")]
        limit: usize,
        /// Enrich correlation breaks with portfolio impact analysis (requires --json)
        #[arg(long = "with-impact")]
        with_impact: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// List stored correlation snapshots (alias for `latest`)
    List {
        /// Period for snapshots/history: 7d, 30d, 90d
        #[arg(long)]
        period: Option<String>,
        /// Maximum number of rows to show
        #[arg(long, default_value = "25")]
        limit: usize,
        /// Enrich correlation breaks with portfolio impact analysis (requires --json)
        #[arg(long = "with-impact")]
        with_impact: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// List pairs with correlation breaks (short-term vs long-term divergence beyond threshold)
    Breaks {
        /// Minimum absolute delta (|corr_7d − corr_90d|) to count as a break (default: 0.30)
        #[arg(long, default_value = "0.30")]
        threshold: f64,
        /// Maximum number of break pairs to show
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Auto-create recurring technical correlation_break alerts for each detected break pair
        #[arg(long = "seed-alerts")]
        seed_alerts: bool,
        /// Cooldown in minutes for seeded alerts (default: 240)
        #[arg(long, default_value = "240")]
        cooldown: i64,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsAlertsCommand {
    /// Add an alert rule
    Add {
        /// Legacy natural-language rule form: "BTC below 55000"
        rule: Option<String>,
        /// Structured alert kind: price, allocation, indicator, technical, macro, ratio
        #[arg(long)]
        kind: Option<String>,
        /// Symbol or symbol pair (for structured alerts)
        #[arg(long)]
        symbol: Option<String>,
        /// Create a price alert from a stored level selector (support, resistance, bb_upper, bb_lower, range_52w_high, range_52w_low, swing_high, swing_low)
        #[arg(long = "from-level")]
        from_level: Option<String>,
        /// Structured smart-alert condition name
        #[arg(long)]
        condition: Option<String>,
        /// Human label for the alert
        #[arg(long)]
        label: Option<String>,
        /// Store as recurring instead of one-shot
        #[arg(long)]
        recurring: bool,
        /// Cooldown in minutes before a recurring alert can fire again
        #[arg(long, default_value_t = 0)]
        cooldown_minutes: i64,
    },
    /// List alerts
    List {
        /// Filter by status: armed, triggered, acknowledged
        #[arg(long)]
        status: Option<String>,
        /// Return triggered alert log rows instead of alert definitions
        #[arg(long)]
        triggered: bool,
        /// Only include triggered alerts from the last N hours
        #[arg(long)]
        since: Option<i64>,
        /// Only include alerts triggered since local midnight
        #[arg(long)]
        today: bool,
        /// Show recently triggered/acknowledged alerts (default: last 24h). Useful for investigation continuity across agent cycles.
        #[arg(long)]
        recent: bool,
        /// Number of hours for --recent filter (default: 24)
        #[arg(long = "recent-hours", default_value_t = 24)]
        recent_hours: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove alert by ID
    Remove { id: i64 },
    /// Check alerts against current data
    Check {
        /// Only include alerts triggered since local midnight
        #[arg(long)]
        today: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Acknowledge one or more alerts by ID
    Ack {
        /// One or more alert IDs to acknowledge
        #[arg(required = true)]
        ids: Vec<i64>,
    },
    /// Rearm alert by ID
    Rearm { id: i64 },
    /// Seed a default smart-alert set for current holdings + core macro conditions
    SeedDefaults,
    /// Triage dashboard: prioritize, group, and score all alerts by urgency
    #[command(after_help = "Groups alerts into urgency tiers:\n\n  🔴 CRITICAL  Newly triggered — needs immediate attention\n  🟠 HIGH      Previously triggered, not yet acknowledged\n  🟡 WATCH     Armed and within 5% of threshold\n  🟢 LOW       Armed but far from threshold\n\nSummary stats by kind (price/technical/macro/scenario/ratio)\nand actionability scoring.\n\nSee also: analytics alerts check, analytics alerts list")]
    Triage {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsTrendsEvidenceCommand {
    /// Add evidence to a trend
    Add {
        /// Trend ID
        #[arg(long)]
        id: Option<i64>,
        /// Evidence text
        #[arg(long)]
        evidence: Option<String>,
        /// Optional positional fallback for evidence text
        value: Option<String>,
        #[arg(long)]
        date: Option<String>,
        #[arg(long = "direction-impact")]
        direction_impact: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List trend evidence rows
    List {
        #[arg(long)]
        id: Option<i64>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsTrendsImpactCommand {
    /// Add an asset impact mapping for a trend
    Add {
        #[arg(long)]
        id: Option<i64>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        impact: Option<String>,
        #[arg(long)]
        mechanism: Option<String>,
        #[arg(long = "impact-timeframe")]
        impact_timeframe: Option<String>,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List trend impact mappings
    List {
        #[arg(long)]
        id: Option<i64>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsTrendsCommand {
    /// Add a new trend (structural force or narrative) with direction and conviction
    Add {
        value: Option<String>,
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
        #[arg(long = "asset-impact")]
        asset_impact: Option<String>,
        #[arg(long = "key-signal")]
        key_signal: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List tracked trends with optional filters (timeframe, direction, conviction, category)
    List {
        #[arg(long)]
        timeframe: Option<String>,
        #[arg(long)]
        direction: Option<String>,
        #[arg(long)]
        conviction: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Update a trend's direction, conviction, status, or description
    Update {
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
        #[arg(long = "asset-impact")]
        asset_impact: Option<String>,
        #[arg(long = "key-signal")]
        key_signal: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Trend dashboard: consolidated view of all active trends by timeframe and direction
    Dashboard {
        #[arg(long)]
        json: bool,
    },
    /// Manage trend evidence (supporting/conflicting data points)
    Evidence {
        #[command(subcommand)]
        command: AnalyticsTrendsEvidenceCommand,
    },
    /// Manage trend-to-asset impact mappings (which assets a trend affects and how)
    Impact {
        #[command(subcommand)]
        command: AnalyticsTrendsImpactCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsMacroCyclesHistoryCommand {
    /// Add a historical cycle data point (e.g. US trade score for the 1940s)
    Add {
        #[arg(long)]
        country: String,
        #[arg(long, visible_alias = "metric")]
        determinant: String,
        #[arg(long, visible_alias = "decade")]
        year: i32,
        #[arg(long)]
        score: f64,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List historical cycle data with optional country/metric/decade filters
    List {
        #[arg(long = "country")]
        countries: Vec<String>,
        #[arg(long, visible_alias = "metric")]
        determinant: Option<String>,
        #[arg(long, visible_alias = "decade")]
        year: Option<i32>,
        #[arg(long)]
        composite: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsMacroCyclesCommand {
    /// Show current power metrics and cycle phases for all tracked countries
    Current {
        /// Filter by country code (e.g. US, CN)
        country: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Historical cycle data: add and list power metric scores by country/decade
    History {
        #[command(subcommand)]
        command: AnalyticsMacroCyclesHistoryCommand,
    },
    /// Update a country's cycle phase with evidence notes
    Update {
        name: String,
        #[arg(long)]
        phase: String,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        evidence: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsMacroRegimeCommand {
    /// Show the current market regime classification with confidence and drivers
    Current {
        #[arg(long)]
        json: bool,
    },
    /// Set the market regime (risk-on, risk-off, crisis, etc.) with confidence and drivers
    Set {
        regime: String,
        #[arg(long)]
        confidence: Option<f64>,
        #[arg(long)]
        drivers: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show regime classification history over time
    History {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Show regime transitions: when the regime changed and what triggered it
    Transitions {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsMacroCommand {
    /// Show macro power metrics for a country (education, trade, military, innovation, etc.)
    Metrics {
        country: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Side-by-side comparison of two countries' macro power metrics
    Compare {
        left: Option<String>,
        right: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Long-term empire/power cycles: current phases, historical data, updates
    Cycles {
        #[command(subcommand)]
        command: Option<AnalyticsMacroCyclesCommand>,
        #[arg(long)]
        json: bool,
    },
    /// Scenario-to-outcome mapping: what happens to assets under each macro scenario
    Outcomes {
        #[arg(long)]
        json: bool,
    },
    /// Historical parallels: match current conditions to past macro regimes
    Parallels {
        #[arg(long)]
        json: bool,
    },
    /// Macro analysis log: timestamped agent observations and regime notes
    Log {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Market regime classification: current state, set, history, transitions
    Regime {
        #[command(subcommand)]
        command: AnalyticsMacroRegimeCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsScenarioSignalCommand {
    /// Add a signal (evidence or trigger) linked to a scenario
    Add {
        value: String,
        #[arg(long)]
        scenario: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List scenario signals with optional scenario/status filter
    List {
        #[arg(long)]
        scenario: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Update a scenario signal's evidence or status by ID
    Update {
        #[arg(long = "signal-id")]
        signal_id: i64,
        #[arg(long)]
        evidence: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a scenario signal by ID
    Remove {
        #[arg(long = "signal-id")]
        signal_id: i64,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SituationBranchCommand {
    /// Add a branch (sub-outcome) to an active situation
    Add {
        /// Situation name
        #[arg(long)]
        situation: String,
        /// Branch name
        value: String,
        #[arg(long)]
        probability: Option<f64>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List branches for a situation
    List {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        json: bool,
    },
    /// Update a branch probability or status
    Update {
        /// Branch ID
        id: i64,
        #[arg(long)]
        probability: Option<f64>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SituationImpactCommand {
    /// Add an asset impact to a situation
    Add {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        direction: String,
        #[arg(long, default_value = "primary")]
        tier: String,
        #[arg(long)]
        mechanism: Option<String>,
        #[arg(long)]
        parent: Option<i64>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List impact chains for a situation
    List {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        tree: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SituationIndicatorCommand {
    /// Add a mechanical data indicator to a situation
    Add {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        operator: String,
        #[arg(long)]
        threshold: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        impact: Option<i64>,
        #[arg(long, default_value = "close")]
        metric: String,
        #[arg(long)]
        json: bool,
    },
    /// List indicators for a situation
    List {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SituationUpdateCommand {
    /// Log a structured event/update for a situation
    Log {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        headline: String,
        #[arg(long)]
        detail: Option<String>,
        #[arg(long, default_value = "normal")]
        severity: String,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        source_agent: Option<String>,
        #[arg(long)]
        next_decision: Option<String>,
        #[arg(long)]
        next_decision_at: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List event updates for a situation
    List {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SituationCommand {
    /// Show the Situation Room dashboard (regime + active situations)
    Dashboard {
        #[arg(long)]
        json: bool,
    },
    /// Cross-situation matrix: all active situations with branches, indicators, impacts, and overlap
    Matrix {
        /// Filter to situations affecting a specific symbol
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List all active situations with summary counts
    List {
        #[arg(long)]
        phase: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Full composite view for one situation
    View {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        json: bool,
    },
    /// Demote an active situation back to hypothesis
    Demote {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        json: bool,
    },
    /// Resolve a situation with outcome notes
    Resolve {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        resolution: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Manage branches (sub-outcomes) of a situation
    Branch {
        #[command(subcommand)]
        command: SituationBranchCommand,
    },
    /// Manage asset impact chains
    Impact {
        #[command(subcommand)]
        command: SituationImpactCommand,
    },
    /// Manage mechanical data indicators
    Indicator {
        #[command(subcommand)]
        command: SituationIndicatorCommand,
    },
    /// Log and list structured event updates
    Update {
        #[command(subcommand)]
        command: SituationUpdateCommand,
    },
    /// Cross-situation exposure for a specific symbol
    Exposure {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsScenarioCommand {
    /// Add a new macro scenario with probability, triggers, and impact assessment
    Add {
        value: String,
        #[arg(long)]
        probability: Option<f64>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        impact: Option<String>,
        #[arg(long)]
        triggers: Option<String>,
        #[arg(long)]
        precedent: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List scenarios with optional status filter (active, resolved, archived)
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Update a scenario's probability, description, triggers, or status
    Update {
        value: String,
        /// History note / driver (positional shorthand)
        note_pos: Option<String>,
        #[arg(long)]
        probability: Option<f64>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        impact: Option<String>,
        #[arg(long)]
        triggers: Option<String>,
        #[arg(long)]
        precedent: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        driver: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a scenario by name
    Remove {
        value: String,
        #[arg(long)]
        json: bool,
    },
    /// Show probability history and driver log for a scenario
    History {
        value: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Manage scenario signals (evidence and triggers)
    Signal {
        #[command(subcommand)]
        command: AnalyticsScenarioSignalCommand,
    },
    /// Automated probability suggestions based on signal evidence
    #[command(
        after_help = "Analyzes each active scenario's signals (triggered/watching/invalidated)\nand recent probability trend to suggest whether probability should\nincrease, decrease, or hold.\n\nDesigned for agent consumption — agents can use this to inform\ntheir probability update decisions.\n\nSee also: analytics scenario list, analytics scenario signal list"
    )]
    Suggest {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Consolidated portfolio impact matrix across all scenarios and presets
    #[command(
        name = "impact-matrix",
        after_help = "Runs every active scenario (using defined impacts) AND all built-in\nstress presets through the portfolio, producing a ranked matrix of\noutcomes sorted by impact severity (worst to best).\n\nScenario impacts use direction+tier assumptions (15/8/4% for\nprimary/secondary/tertiary). Presets use fixed historical-analog shocks.\nExpected P&L is probability-weighted across active scenarios only.\n\nDesigned for agent consumption — one JSON call returns the complete\nrisk landscape.\n\nSee also: analytics impact-estimate, portfolio stress-test,\n          analytics scenario list, analytics scenario suggest"
    )]
    ImpactMatrix {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsConvictionCommand {
    /// Set conviction score for an asset (-5 bearish to +5 bullish)
    Set {
        symbol: String,
        #[arg(allow_hyphen_values = true)]
        score_pos: Option<i32>,
        #[arg(long)]
        score: Option<i32>,
        #[arg(long)]
        notes: Option<String>,
        notes_pos: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List current conviction scores for all tracked assets
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show conviction score history for a specific asset over time
    History {
        symbol: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Show recent conviction changes across all assets (default: last 7 days)
    Changes {
        days: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsViewsCommand {
    /// Set or update an analyst's view on an asset (upsert)
    ///
    /// EXAMPLES:
    ///   pftui analytics views set --analyst low --asset BTC --direction bull --conviction 3 \
    ///     --reasoning "Short-term momentum strong, breaking key resistance"
    ///   pftui analytics views set --analyst high --asset GLD --direction bull --conviction 4 \
    ///     --reasoning "Structural central bank buying" \
    ///     --evidence "WGC Q4 data, PBOC reserves" \
    ///     --blind-spots "Risk-on shift could pause buying" --json
    Set {
        /// Analyst layer: low, medium, high, macro
        #[arg(long)]
        analyst: String,
        /// Asset symbol (e.g. BTC, GLD, TSLA)
        #[arg(long)]
        asset: String,
        /// Direction: bull, bear, neutral
        #[arg(long)]
        direction: String,
        /// Conviction score: -5 (strong bear) to +5 (strong bull)
        #[arg(long, allow_hyphen_values = true)]
        conviction: i64,
        /// Why this view — reasoning summary
        #[arg(long)]
        reasoning: String,
        /// Supporting data points
        #[arg(long)]
        evidence: Option<String>,
        /// What could invalidate this view
        #[arg(long = "blind-spots")]
        blind_spots: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List current analyst views with optional filters
    ///
    /// EXAMPLES:
    ///   pftui analytics views list --json
    ///   pftui analytics views list --analyst low --json
    ///   pftui analytics views list --asset BTC --json
    List {
        /// Filter by analyst layer: low, medium, high, macro
        #[arg(long)]
        analyst: Option<String>,
        /// Filter by asset symbol
        #[arg(long)]
        asset: Option<String>,
        /// Maximum results to show
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Full cross-analyst view matrix: rows=assets, columns=analysts (LOW/MEDIUM/HIGH/MACRO)
    ///
    /// EXAMPLES:
    ///   pftui analytics views matrix --json
    Matrix {
        #[arg(long)]
        json: bool,
    },
    /// Delete an analyst's view on an asset
    ///
    /// EXAMPLES:
    ///   pftui analytics views delete --analyst low --asset BTC --json
    Delete {
        /// Analyst layer: low, medium, high, macro
        #[arg(long)]
        analyst: String,
        /// Asset symbol
        #[arg(long)]
        asset: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsDebateScoreCommand {
    /// Score a resolved debate: which side (bull/bear) was right?
    ///
    /// EXAMPLES:
    ///   pftui analytics debate-score add --debate-id 1 --winner bull --margin decisive \
    ///     --outcome "BTC reached 185k — bull case validated"
    ///   pftui analytics debate-score add --debate-id 2 --winner bear --margin marginal \
    ///     --outcome "Gold corrected 5%" --assessment "Bear timing right, bull structure right" \
    ///     --scored-by evening-analysis --json
    Add {
        /// ID of the resolved debate to score
        #[arg(long = "debate-id")]
        debate_id: i64,
        /// Which side won: bull, bear, or mixed
        #[arg(long)]
        winner: String,
        /// How decisive was the outcome: decisive, marginal, or mixed
        #[arg(long, default_value = "marginal")]
        margin: String,
        /// What actually happened — the factual outcome
        #[arg(long)]
        outcome: String,
        /// Assessment of which arguments from each side were validated/invalidated
        #[arg(long)]
        assessment: Option<String>,
        /// Agent that scored this debate
        #[arg(long = "scored-by")]
        scored_by: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List scored debates with optional filters
    ///
    /// EXAMPLES:
    ///   pftui analytics debate-score list --json
    ///   pftui analytics debate-score list --winner bull
    ///   pftui analytics debate-score list --topic gold --limit 5
    List {
        /// Filter by topic keyword
        #[arg(long)]
        topic: Option<String>,
        /// Filter by winner: bull, bear, or mixed
        #[arg(long)]
        winner: Option<String>,
        /// Maximum results to show
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Aggregate accuracy statistics: bull vs bear win rates overall and by topic
    ///
    /// EXAMPLES:
    ///   pftui analytics debate-score accuracy --json
    ///   pftui analytics debate-score accuracy --topic BTC --json
    Accuracy {
        /// Filter accuracy stats to debates matching a topic keyword
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List resolved debates that haven't been scored yet
    ///
    /// EXAMPLES:
    ///   pftui analytics debate-score unscored --json
    ///   pftui analytics debate-score unscored --limit 5
    Unscored {
        /// Maximum results to show
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsCommand {
    /// Full synthesized intelligence blob for a single asset
    Asset {
        /// Symbol to analyze (required)
        symbol: String,
        #[arg(long)]
        json: bool,
    },
    /// Technical indicators for one or all assets (RSI, MACD, SMA, Bollinger, ATR)
    Technicals {
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long, default_value = "1d")]
        timeframe: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Market structure levels: support, resistance, moving averages, swing points, 52-week range
    Levels {
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        level_type: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Technical and cross-timeframe signals (RSI extremes, MACD crosses, SMA reclaims, BB squeezes)
    Signals {
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long = "signal-type")]
        signal_type: Option<String>,
        #[arg(long)]
        severity: Option<String>,
        /// Filter signal source: "technical" (per-symbol), "timeframe" (cross-layer), or "all" (default)
        #[arg(long, default_value = "all")]
        source: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Portfolio summary: allocation breakdown, top movers, regime read, key metrics
    Summary {
        #[arg(long)]
        json: bool,
    },
    /// Situation Room — active situation monitoring and management (see also: `analytics scenario` for macro scenarios)
    Situation {
        #[command(subcommand)]
        command: Option<SituationCommand>,
        #[arg(long)]
        json: bool,
    },
    /// Change radar: what moved since last refresh (prices, scenarios, convictions, signals)
    Deltas {
        #[arg(long, default_value = "last-refresh")]
        since: String,
        #[arg(long)]
        json: bool,
    },
    /// Ranked upcoming catalysts and event pressure (earnings, Fed meetings, data releases)
    Catalysts {
        #[arg(long, default_value = "week")]
        window: String,
        #[arg(long)]
        json: bool,
    },
    /// Portfolio impact analysis: which scenarios and events affect your holdings most
    Impact {
        #[arg(long)]
        json: bool,
    },
    /// Projected P&L under each active scenario/branch probability
    ImpactEstimate {
        #[arg(long)]
        json: bool,
    },
    /// Compare pftui scenario probabilities vs prediction market consensus. Flag divergences.
    #[command(after_help = "Compares pftui scenario probabilities against prediction market\nconsensus (Polymarket contracts). Flags divergences above the threshold\n(default: 15pp).\n\nRequires scenario↔contract mappings created via:\n  pftui data predictions map --scenario \"<name>\" --search \"<query>\"\n\nExample:\n  pftui analytics calibration --json\n  pftui analytics calibration --threshold 10 --json\n\nSee also: data predictions map, analytics scenario list")]
    Calibration {
        /// Divergence threshold in percentage points (default: 15)
        #[arg(long, default_value = "15")]
        threshold: f64,
        #[arg(long)]
        json: bool,
    },
    /// Debate accuracy scoring: track which side (bull/bear) was right historically
    #[command(name = "debate-score", after_help = "Score resolved debates to track which side (bull/bear) was historically\ncorrect. Feeds into system accuracy tracking.\n\nWorkflow:\n  1. Debates are created and resolved via `agent debate`\n  2. Score resolved debates with `analytics debate-score add`\n  3. View accuracy stats with `analytics debate-score accuracy`\n  4. Find unscored debates with `analytics debate-score unscored`\n\nExamples:\n  pftui analytics debate-score add --debate-id 1 --winner bull --outcome \"BTC hit 185k\"\n  pftui analytics debate-score list --json\n  pftui analytics debate-score accuracy --topic BTC --json\n  pftui analytics debate-score unscored --json\n\nSee also: agent debate start, agent debate history, agent debate summary")]
    DebateScore {
        #[command(subcommand)]
        command: AnalyticsDebateScoreCommand,
    },
    /// Per-analyst, per-asset directional views with conviction scores (F57: Timeframe Analyst Self-Awareness)
    #[command(after_help = "Each timeframe analyst (LOW/MEDIUM/HIGH/MACRO) writes a structured\nview per asset on every run. Views include direction, conviction (-5 to +5),\nreasoning, key evidence, and blind spots.\n\nSubcommands:\n  set     — write/update an analyst's view on an asset\n  list    — list views with optional analyst/asset filters\n  matrix  — full cross-analyst view matrix\n  delete  — remove a view\n\nExamples:\n  pftui analytics views set --analyst low --asset BTC --direction bull \\\n    --conviction 3 --reasoning \"Momentum strong\" --json\n  pftui analytics views list --asset BTC --json\n  pftui analytics views matrix --json\n\nSee also: analytics alignment, analytics divergence")]
    Views {
        #[command(subcommand)]
        command: AnalyticsViewsCommand,
    },
    /// Identified opportunities: undervalued positions, scenario plays, entry points
    Opportunities {
        #[arg(long)]
        json: bool,
    },
    /// Structured analytical narrative: recap, key themes, and analytical memory
    Narrative {
        #[arg(long)]
        json: bool,
    },
    /// Cross-timeframe synthesis: alignment, divergence, tensions, and regime context
    Synthesis {
        #[arg(long)]
        json: bool,
    },
    /// LOW timeframe layer (hours to days): momentum, intraday signals, short-term positioning
    Low {
        #[arg(long)]
        json: bool,
    },
    /// MEDIUM timeframe layer (weeks to months): swing trends, sector rotation, earnings impact
    Medium {
        #[arg(long)]
        json: bool,
    },
    /// HIGH timeframe layer (months to years): secular trends, macro positioning, structural forces
    High {
        #[arg(long)]
        json: bool,
    },
    /// MACRO timeframe layer: empire cycles, world order shifts, decade-scale power metrics
    Macro {
        #[command(subcommand)]
        command: Option<AnalyticsMacroCommand>,
        #[arg(long)]
        json: bool,
    },
    /// Cross-timeframe alignment: how LOW/MEDIUM/HIGH/MACRO layers agree or conflict per asset
    Alignment {
        #[arg(long)]
        symbol: Option<String>,
        /// Compact summary grouped by consensus (counts + notable symbols)
        #[arg(long)]
        summary: bool,
        #[arg(long)]
        json: bool,
    },
    /// Cross-timeframe divergence: assets where timeframe layers disagree on direction
    Divergence {
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Unified cross-timeframe view: alignment + divergence + correlation breaks in one call
    #[command(name = "cross-timeframe", after_help = "\
EXAMPLES:
  pftui analytics cross-timeframe --json             # Full alignment + divergence + breaks
  pftui analytics cross-timeframe --resolve --json    # Add resolution analysis for divergent assets
  pftui analytics cross-timeframe --resolve --symbol BTC --json

See also: analytics alignment, analytics divergence, analytics correlations, analytics regime-transitions")]
    CrossTimeframe {
        /// Filter to a specific symbol
        #[arg(long)]
        symbol: Option<String>,
        /// Correlation break threshold (default: 0.30)
        #[arg(long, default_value = "0.30")]
        threshold: f64,
        /// Max correlation breaks to return (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Include disagreement resolution analysis: which timeframe dominates, suggested stance, resolution triggers
        #[arg(long)]
        resolve: bool,
        #[arg(long)]
        json: bool,
    },
    /// Daily digest: condensed summary of market activity and portfolio changes
    Digest {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Daily recap: structured market recap with key themes for analytical memory
    Recap {
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Weekly review: summarizes the past week's key moves, scenario shifts, prediction outcomes, conviction changes, and portfolio performance
    #[command(name = "weekly-review")]
    WeeklyReview {
        /// Number of days to cover (default: 7)
        #[arg(long, default_value = "7")]
        days: usize,
        #[arg(long)]
        json: bool,
    },
    /// Data quality gaps: OHLCV coverage, missing dates, stale symbols
    Gaps {
        /// Show per-symbol OHLCV data quality for a specific symbol
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Top movers: biggest gainers and losers across portfolio, watchlist, and market
    Movers {
        #[command(subcommand)]
        command: Option<AnalyticsMoversCommand>,
        #[arg(long, default_value = "3")]
        threshold: String,
        #[arg(long)]
        overnight: bool,
        #[arg(long)]
        json: bool,
    },
    /// Rolling correlations: compute, store, and detect correlation breaks between asset pairs
    Correlations {
        #[command(subcommand)]
        command: Option<AnalyticsCorrelationsCommand>,
        /// Output as JSON (when no subcommand given)
        #[arg(long)]
        json: bool,
    },
    /// Multi-filter scan: technical setups, news keywords, trackline breaches, saved scans
    Scan {
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        save: Option<String>,
        #[arg(long)]
        load: Option<String>,
        #[arg(long)]
        list: bool,
        #[arg(long = "news-keyword")]
        news_keyword: Option<String>,
        #[arg(long = "trackline-breaches")]
        trackline_breaches: bool,
        #[arg(long)]
        json: bool,
    },
    /// Web research: search news, Fed statements, earnings, COT data, ETF flows, geopolitics
    Research {
        query: Option<String>,
        #[arg(long)]
        news: bool,
        #[arg(long)]
        freshness: Option<String>,
        #[arg(long, default_value = "5")]
        count: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        fed: bool,
        #[arg(long)]
        earnings: Option<String>,
        #[arg(long)]
        geopolitics: bool,
        #[arg(long)]
        cot: Option<String>,
        #[arg(long)]
        etf: Option<String>,
        #[arg(long)]
        opec: bool,
    },
    /// Structural trend tracking: add, list, update, evidence, impact mappings, dashboard
    Trends {
        #[command(subcommand)]
        command: AnalyticsTrendsCommand,
    },
    /// Alert rules and monitoring (also available as `data alerts`)
    #[command(after_help = "Common workflows:\n  pftui analytics alerts check             Check all alerts against current data\n  pftui analytics alerts check --today     Check only today's triggers\n  pftui analytics alerts triage            Prioritized alert dashboard with urgency tiers\n  pftui analytics alerts list              List alert rules\n  pftui analytics alerts list --triggered  Show triggered alert log\n  pftui analytics alerts add \"BTC > 100000\" Add a custom alert rule\n  pftui analytics alerts seed-defaults     Seed smart-alert defaults for holdings\n\nAlso accessible via: pftui data alerts check, pftui data alerts list")]
    Alerts {
        #[command(subcommand)]
        command: AnalyticsAlertsCommand,
    },
    /// Macro scenario tracking: add, list, update, and manage probability scenarios
    #[command(alias = "scenarios")]
    Scenario {
        #[command(subcommand)]
        command: AnalyticsScenarioCommand,
    },
    /// Asset conviction tracking: set, list, history, and recent changes (-5 to +5 scale)
    Conviction {
        #[command(subcommand)]
        command: AnalyticsConvictionCommand,
    },
    /// Prediction intelligence: market odds and personal prediction tracking (alias for `data predictions`)
    Predictions {
        #[command(subcommand)]
        command: Option<DataPredictionsCommand>,

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
    /// Dixon Power Flow Tracker — track power shifts between FIC, MIC, and TIC
    #[command(name = "power-flow")]
    PowerFlow {
        #[command(subcommand)]
        command: AnalyticsPowerFlowCommand,
    },
    /// News sentiment analysis: keyword-based scoring and aggregation of cached news
    #[command(name = "news-sentiment")]
    NewsSentiment {
        /// Filter by news category (e.g. "crypto", "commodities", "geopolitics")
        #[arg(long)]
        category: Option<String>,

        /// Only score news from last N hours
        #[arg(long)]
        hours: Option<i64>,

        /// Maximum number of articles to score (default: 50)
        #[arg(long, default_value = "50")]
        limit: usize,

        /// Show per-article detail with keyword hits
        #[arg(long)]
        detail: bool,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Consolidated morning intelligence: situation + deltas + synthesis + scenarios + correlation breaks + alerts + news sentiment in one call
    #[command(name = "morning-brief", after_help = "Combines situation room, 24h deltas, cross-timeframe synthesis,\nactive scenario probabilities, correlation breaks, catalysts, portfolio impact,\ntriggered alerts, and news sentiment into a single payload.\n\nDesigned for morning-brief agents that previously needed 5-6 separate\nanalytics commands to assemble intelligence.\n\nSee also: analytics situation, analytics deltas, analytics synthesis")]
    MorningBrief {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
    },
    /// Consolidated evening analysis: morning-brief + narrative + opportunities + conviction changes + prediction stats + cross-timeframe resolution in one call
    #[command(name = "evening-brief", after_help = "Deep evening analysis payload for agents. Extends morning-brief with:\n  - Narrative: structured recap, key themes, analytical memory\n  - Opportunities: identified entry points, scenario plays\n  - Conviction changes: shifts over the past 7 days\n  - Prediction stats: overall accuracy scorecard\n  - Cross-timeframe resolution: divergent assets with stance guidance\n\nDesigned for the evening analyst who previously needed 20+ separate\nanalytics commands to assemble a full picture.\n\nSee also: analytics morning-brief, analytics narrative, analytics cross-timeframe")]
    EveningBrief {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
    },
    /// Regime-asset flow correlation: cross-references regime state with asset class flows to detect power structure patterns
    #[command(name = "regime-flows", after_help = "Cross-references the current market regime with asset class flows to detect\npower structure patterns automatically. Monitors key ratios (gold/oil,\ncopper/gold, BTC/gold), safe-haven vs risk flows, energy complex signals,\nand defense sector tracking.\n\nDetects patterns: safe-haven rotation, geopolitical stress, inflationary pulse,\nrisk-on breakout, deflationary signal, dollar wrecking ball, energy crisis,\nand regime divergence.\n\nSee also: analytics macro regime, analytics correlations, analytics movers themes")]
    RegimeFlows {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
    },
    /// Regime transition probability scoring: analyzes signal momentum, current state, and historical patterns to score likelihood of regime changes
    #[command(name = "regime-transitions", after_help = "Scores the probability of transitioning from the current regime to each\npossible state (risk-on, risk-off, crisis, stagflation, etc.).\n\nAnalyzes:\n  - 6 signal momentum indicators (VIX, DXY, yields, equities, gold, oil)\n  - Current regime confidence and duration\n  - Special regime triggers (crisis: VIX>30+oil>90, stagflation: gold up+equities down)\n  - Historical transition frequency and patterns\n\nEach candidate shows probability, key drivers, confirmation triggers, and\ninvalidation conditions.\n\nSee also: analytics macro regime, analytics regime-flows, analytics synthesis")]
    RegimeTransitions {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsPowerFlowCommand {
    /// Log a power flow event
    Add {
        /// What happened
        #[arg(long)]
        event: String,
        /// Source complex: FIC, MIC, or TIC
        #[arg(long)]
        source: String,
        /// Direction: gaining or losing
        #[arg(long)]
        direction: String,
        /// Target complex (optional): FIC, MIC, or TIC
        #[arg(long)]
        target: Option<String>,
        /// Market/money signal supporting this classification
        #[arg(long)]
        evidence: String,
        /// Significance of this power shift (1-5, default: 3)
        #[arg(long, default_value_t = 3)]
        magnitude: i32,
        /// Which agent logged this
        #[arg(long = "agent-source")]
        agent_source: Option<String>,
        /// Date (YYYY-MM-DD, default: today)
        #[arg(long)]
        date: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List power flow entries
    List {
        /// Filter by complex: FIC, MIC, or TIC
        #[arg(long)]
        complex: Option<String>,
        /// Filter by direction: gaining or losing
        #[arg(long)]
        direction: Option<String>,
        /// Number of days to look back (default: 7)
        #[arg(long, default_value_t = 7)]
        days: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Aggregate power balance per complex
    Balance {
        /// Number of days to aggregate (default: 30)
        #[arg(long, default_value_t = 30)]
        days: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Generate a structured FIC/MIC/TIC power assessment with trend analysis, key events, and regime detection
    #[command(after_help = "Analyzes logged power flow events to produce a comprehensive assessment:\n\n\
        • Per-complex net scores, event counts, and trend direction\n\
        • First-half vs second-half trend comparison for momentum detection\n\
        • Directed power shifts between complexes\n\
        • Key events (magnitude ≥ 4)\n\
        • Regime classification (FIC/MIC/TIC-dominant or contested)\n\
        • Regime shift detection when a complex reverses direction\n\n\
        Designed for weekly assessments by medium-timeframe analysts.\n\n\
        See also: analytics power-flow balance, analytics power-flow list, analytics regime-flows")]
    Assess {
        /// Number of days to assess (default: 7)
        #[arg(long, default_value_t = 7)]
        days: usize,
        /// Filter assessment to a single complex: FIC, MIC, or TIC
        #[arg(long)]
        complex: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsMoversCommand {
    /// Detect sector-wide themes: clusters of symbols in the same sector moving together
    Themes {
        /// Minimum % change threshold for a symbol to count as a mover (default: 2)
        #[arg(long, default_value = "2")]
        threshold: String,
        /// Minimum number of symbols moving in the same direction to form a theme (default: 2)
        #[arg(long, default_value_t = 2)]
        min_symbols: usize,
        /// Output as JSON for agent/script consumption
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
    /// Interactive command console with hierarchical navigation and tab completion
    Console,

    /// Personal research journal: entries, predictions, convictions, notes, scenarios
    Journal {
        #[command(subcommand)]
        command: Option<JournalCommand>,
    },

    /// Agentic operations and inter-agent workflows
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },

    /// Data management operations
    #[command(after_help = "Looking for alerts? Use:\n  pftui data alerts check      Check alerts against current data\n  pftui data alerts list       List alert rules\n  pftui analytics alerts triage  Prioritized alert dashboard\n  pftui analytics alerts       Full alert management (add, ack, seed-defaults)")]
    Data {
        #[command(subcommand)]
        command: DataCommand,
    },

    /// System/admin operations: config, diagnostics, import/export, setup, web
    System {
        #[command(subcommand)]
        command: SystemCommand,
    },

    /// Portfolio operations: holdings, value, targets, rebalancing, and transactions
    Portfolio {
        #[command(subcommand)]
        command: Option<PortfolioCommand>,
    },

    /// Multi-timeframe analytics engine views (includes scenario, situation, signals, synthesis)
    #[command(name = "analytics", after_help = "Key subcommands:\n  alerts     Alert rules: add, list, check, ack, seed-defaults (also: data alerts)\n  scenario   Macro scenario tracking: probabilities, triggers, history (alias: scenarios)\n  situation  Situation Room: active situations, regime, branches, indicators\n  signals    Technical and cross-timeframe signals\n  synthesis  Cross-timeframe alignment and divergence analysis")]
    Analytics {
        #[command(subcommand)]
        command: AnalyticsCommand,
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use clap::CommandFactory;
    use clap::Parser;

    fn help_text() -> Result<String> {
        let mut cmd = Cli::command();
        let mut buffer = Vec::new();
        cmd.write_long_help(&mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }

    fn subcommand_help(path: &[&str]) -> Result<String> {
        let mut cmd = Cli::command();
        for segment in path {
            cmd = cmd
                .find_subcommand_mut(segment)
                .unwrap_or_else(|| panic!("missing subcommand: {segment}"))
                .clone();
        }
        let mut buffer = Vec::new();
        cmd.write_long_help(&mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }

    #[test]
    fn top_level_help_lists_only_f42_domains() -> Result<()> {
        let help = help_text()?;
        for command in [
            "agent",
            "analytics",
            "data",
            "journal",
            "portfolio",
            "system",
        ] {
            assert!(
                help.contains(command),
                "missing top-level command: {command}"
            );
        }
        for removed in ["dashboard", "market", "portfolios", "watchlist"] {
            assert!(
                !help.contains(removed),
                "stale top-level command present: {removed}"
            );
        }
        Ok(())
    }

    #[test]
    fn parses_portfolio_profiles_path() {
        let cli =
            Cli::try_parse_from(["pftui", "portfolio", "profiles", "list", "--json"]).unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command:
                    Some(PortfolioCommand::Profiles {
                        command: PortfolioProfilesCommand::List { json },
                    }),
            }) => assert!(json),
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_portfolio_watchlist_path() {
        let cli = Cli::try_parse_from([
            "pftui",
            "portfolio",
            "watchlist",
            "add",
            "TSLA",
            "--target",
            "300",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command:
                    Some(PortfolioCommand::Watchlist {
                        action: Some(WatchlistCommand::Add { symbol, target, .. }),
                        ..
                    }),
            }) => {
                assert_eq!(symbol.as_deref(), Some("TSLA"));
                assert_eq!(target.as_deref(), Some("300"));
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_market_paths() {
        let cli = Cli::try_parse_from(["pftui", "data", "news", "--limit", "5", "--json"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::News { limit, json, .. },
            }) => {
                assert_eq!(limit, 5);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_consensus_subcommands() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "consensus",
            "add",
            "--source",
            "Goldman Sachs",
            "--topic",
            "rate_cuts",
            "--call",
            "50bp cuts in Sep+Dec 2026",
            "--date",
            "2026-03-12",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Data {
                command:
                    DataCommand::Consensus {
                        command:
                            ConsensusCommand::Add {
                                source,
                                topic,
                                call_text,
                                date,
                                json,
                            },
                    },
            }) => {
                assert_eq!(source, "Goldman Sachs");
                assert_eq!(topic, "rate_cuts");
                assert_eq!(call_text, "50bp cuts in Sep+Dec 2026");
                assert_eq!(date, "2026-03-12");
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_cot_command() {
        let cli = Cli::try_parse_from(["pftui", "data", "cot", "GC=F", "--json"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::Cot { symbol, json },
            }) => {
                assert_eq!(symbol.as_deref(), Some("GC=F"));
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_oil_inventory_command() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "oil-inventory", "--weeks", "12", "--json"])
                .unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::OilInventory { weeks, json },
            }) => {
                assert_eq!(weeks, 12);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_oil_inventory_defaults() {
        let cli = Cli::try_parse_from(["pftui", "data", "oil-inventory"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::OilInventory { weeks, json },
            }) => {
                assert_eq!(weeks, 52);
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_oil_premium_json() {
        let cli = Cli::try_parse_from(["pftui", "data", "oil-premium", "--json"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::OilPremium { json },
            }) => {
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_oil_premium_defaults() {
        let cli = Cli::try_parse_from(["pftui", "data", "oil-premium"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::OilPremium { json },
            }) => {
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_futures_json() {
        let cli = Cli::try_parse_from(["pftui", "data", "futures", "--json"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::Futures { json },
            }) => {
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_futures_defaults() {
        let cli = Cli::try_parse_from(["pftui", "data", "futures"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::Futures { json },
            }) => {
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_backfill_command() {
        let cli = Cli::try_parse_from(["pftui", "data", "backfill", "--json"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::Backfill { json },
            }) => {
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_backfill_no_flags() {
        let cli = Cli::try_parse_from(["pftui", "data", "backfill"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::Backfill { json },
            }) => {
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_onchain_command() {
        let cli = Cli::try_parse_from(["pftui", "data", "onchain", "--json"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command: DataCommand::Onchain { json },
            }) => {
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_agent_message_subcommands() {
        let cli = Cli::try_parse_from([
            "pftui", "agent", "message", "ack-all", "--to", "agent-b", "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Agent {
                command:
                    AgentCommand::Message {
                        command: AgentMessageCommand::AckAll { to, json },
                    },
            }) => {
                assert_eq!(to.as_deref(), Some("agent-b"));
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_agent_message_flag_quality_alias() {
        let cli = Cli::try_parse_from([
            "pftui",
            "agent",
            "message",
            "flag",
            "--id",
            "7",
            "--from",
            "agent-b",
            "--quality",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Agent {
                command:
                    AgentCommand::Message {
                        command:
                            AgentMessageCommand::Flag {
                                id,
                                from,
                                quality,
                                json,
                                ..
                            },
                    },
            }) => {
                assert_eq!(id, Some(7));
                assert_eq!(from.as_deref(), Some("agent-b"));
                assert!(quality);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_agent_message_ack_all_flag() {
        // `ack --all` should be equivalent to `ack-all`
        let cli = Cli::try_parse_from([
            "pftui", "agent", "message", "ack", "--all", "--to", "agent-b", "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Agent {
                command:
                    AgentCommand::Message {
                        command:
                            AgentMessageCommand::Ack {
                                id,
                                all,
                                to,
                                json,
                            },
                    },
            }) => {
                assert!(id.is_empty());
                assert!(all);
                assert_eq!(to.as_deref(), Some("agent-b"));
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_agent_message_ack_all_flag_no_to() {
        // `ack --all` without --to should also work
        let cli =
            Cli::try_parse_from(["pftui", "agent", "message", "ack", "--all", "--json"]).unwrap();
        match cli.command {
            Some(Command::Agent {
                command:
                    AgentCommand::Message {
                        command:
                            AgentMessageCommand::Ack {
                                id,
                                all,
                                to,
                                json,
                            },
                    },
            }) => {
                assert!(id.is_empty());
                assert!(all);
                assert!(to.is_none());
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn ack_id_conflicts_with_all_flag() {
        // --id and --all should conflict
        let result = Cli::try_parse_from([
            "pftui", "agent", "message", "ack", "--id", "1", "--all",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn removed_top_level_namespaces_fail_to_parse() {
        for argv in [
            ["pftui", "watchlist", "list"].as_slice(),
            ["pftui", "market", "news"].as_slice(),
            ["pftui", "portfolios", "list"].as_slice(),
            ["pftui", "dashboard", "macro"].as_slice(),
        ] {
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "unexpectedly parsed: {argv:?}"
            );
        }
    }

    #[test]
    fn help_snapshots_cover_critical_f42_subtrees() -> Result<()> {
        let portfolio_help = subcommand_help(&["portfolio"])?;
        assert!(portfolio_help.contains("profiles"));
        assert!(portfolio_help.contains("watchlist"));
        assert!(!portfolio_help.contains("portfolios"));

        let data_help = subcommand_help(&["data"])?;
        for command in [
            "dashboard",
            "news",
            "sentiment",
            "calendar",
            "cot",
            "fedwatch",
            "onchain",
            "economy",
            "consensus",
            "predictions",
            "options",
            "etf-flows",
            "supply",
            "sovereign",
            "oil-inventory",
            "backfill",
        ] {
            assert!(
                data_help.contains(command),
                "missing data subtree command: {command}"
            );
        }

        let agent_help = subcommand_help(&["agent"])?;
        assert!(agent_help.contains("message"));
        assert!(!agent_help.contains("journal"));

        let journal_help = subcommand_help(&["journal"])?;
        for command in ["entry", "prediction", "conviction", "notes", "scenario"] {
            assert!(journal_help.contains(command));
        }

        let message_help = subcommand_help(&["agent", "message"])?;
        for command in ["send", "list", "reply", "flag", "ack", "ack-all", "purge"] {
            assert!(
                message_help.contains(command),
                "missing agent message command: {command}"
            );
        }
        Ok(())
    }

    #[test]
    fn parse_prediction_score_positional_syntax() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "score",
            "51",
            "correct",
            "quick note",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Score {
                            id,
                            id_pos,
                            outcome,
                            outcome_pos,
                            notes,
                            notes_pos,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction score command");
        };

        assert_eq!(id, None);
        assert_eq!(id_pos, Some(51));
        assert_eq!(outcome, None);
        assert_eq!(outcome_pos.as_deref(), Some("correct"));
        assert_eq!(notes, None);
        assert_eq!(notes_pos.as_deref(), Some("quick note"));
    }

    #[test]
    fn parse_prediction_add_timeframe_positional_syntax() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "btc breakout",
            "macro",
            "0.8",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            value,
                            timeframe_pos,
                            confidence_pos,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value.as_deref(), Some("btc breakout"));
        assert_eq!(timeframe_pos.as_deref(), Some("macro"));
        assert_eq!(confidence_pos, Some(0.8));
    }

    #[test]
    fn parse_prediction_score_batch_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "score-batch",
            "3:correct",
            "7:wrong",
            "12:partial",
            "--json",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command: JournalPredictionCommand::ScoreBatch { entries, json },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction score-batch command");
        };

        assert_eq!(entries, vec!["3:correct", "7:wrong", "12:partial"]);
        assert!(json);
    }

    #[test]
    fn parse_prediction_score_batch_single_entry() {
        let cli =
            Cli::try_parse_from(["pftui", "journal", "prediction", "score-batch", "5:correct"])
                .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command: JournalPredictionCommand::ScoreBatch { entries, json },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction score-batch command");
        };

        assert_eq!(entries, vec!["5:correct"]);
        assert!(!json);
    }

    #[test]
    fn parse_conviction_set_negative_score_positional_syntax() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "conviction",
            "set",
            "BTC",
            "-2",
            "setup weakening",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Conviction {
                    command:
                        JournalConvictionCommand::Set {
                            symbol,
                            score_pos,
                            notes_pos,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal conviction set command");
        };

        assert_eq!(symbol, "BTC");
        assert_eq!(score_pos, Some(-2));
        assert_eq!(notes_pos.as_deref(), Some("setup weakening"));
    }

    #[test]
    fn parse_scenario_update_notes_positional_syntax() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "scenario",
            "update",
            "Hard Landing",
            "labor rolling over",
            "--probability",
            "65",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Scenario {
                    command:
                        JournalScenarioCommand::Update {
                            value,
                            note_pos,
                            probability,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal scenario update command");
        };

        assert_eq!(value, "Hard Landing");
        assert_eq!(note_pos.as_deref(), Some("labor rolling over"));
        assert_eq!(probability, Some(65.0));
    }

    #[test]
    fn parse_analytics_calibration_default() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "calibration", "--json"]).unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Calibration { threshold, json },
        }) = cli.command
        else {
            panic!("expected analytics calibration command");
        };
        assert!((threshold - 15.0).abs() < f64::EPSILON);
        assert!(json);
    }

    #[test]
    fn parse_analytics_calibration_custom_threshold() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "calibration",
            "--threshold",
            "10",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Calibration { threshold, json },
        }) = cli.command
        else {
            panic!("expected analytics calibration command");
        };
        assert!((threshold - 10.0).abs() < f64::EPSILON);
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_set() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "views",
            "set",
            "--analyst",
            "low",
            "--asset",
            "BTC",
            "--direction",
            "bull",
            "--conviction",
            "3",
            "--reasoning",
            "Short-term momentum strong",
            "--evidence",
            "RSI 62, MACD cross",
            "--blind-spots",
            "Whale selling risk",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::Set {
                            analyst,
                            asset,
                            direction,
                            conviction,
                            reasoning,
                            evidence,
                            blind_spots,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views set command");
        };
        assert_eq!(analyst, "low");
        assert_eq!(asset, "BTC");
        assert_eq!(direction, "bull");
        assert_eq!(conviction, 3);
        assert_eq!(reasoning, "Short-term momentum strong");
        assert_eq!(evidence.as_deref(), Some("RSI 62, MACD cross"));
        assert_eq!(blind_spots.as_deref(), Some("Whale selling risk"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_list() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "views",
            "list",
            "--analyst",
            "high",
            "--asset",
            "GLD",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::List {
                            analyst,
                            asset,
                            json,
                            ..
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views list command");
        };
        assert_eq!(analyst.as_deref(), Some("high"));
        assert_eq!(asset.as_deref(), Some("GLD"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_matrix() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "views",
            "matrix",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command: AnalyticsViewsCommand::Matrix { json },
                },
        }) = cli.command
        else {
            panic!("expected analytics views matrix command");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_delete() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "views",
            "delete",
            "--analyst",
            "medium",
            "--asset",
            "TSLA",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::Delete {
                            analyst,
                            asset,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views delete command");
        };
        assert_eq!(analyst, "medium");
        assert_eq!(asset, "TSLA");
        assert!(json);
    }

    #[test]
    fn parse_analytics_debate_score_add() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "debate-score",
            "add",
            "--debate-id",
            "1",
            "--winner",
            "bull",
            "--margin",
            "decisive",
            "--outcome",
            "BTC hit 185k",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::DebateScore {
                    command:
                        AnalyticsDebateScoreCommand::Add {
                            debate_id,
                            winner,
                            margin,
                            outcome,
                            json,
                            ..
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics debate-score add command");
        };
        assert_eq!(debate_id, 1);
        assert_eq!(winner, "bull");
        assert_eq!(margin, "decisive");
        assert_eq!(outcome, "BTC hit 185k");
        assert!(json);
    }

    #[test]
    fn parse_analytics_debate_score_list() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "debate-score",
            "list",
            "--winner",
            "bear",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::DebateScore {
                    command:
                        AnalyticsDebateScoreCommand::List {
                            winner, json, ..
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics debate-score list command");
        };
        assert_eq!(winner.as_deref(), Some("bear"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_debate_score_accuracy() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "debate-score",
            "accuracy",
            "--topic",
            "BTC",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::DebateScore {
                    command:
                        AnalyticsDebateScoreCommand::Accuracy { topic, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics debate-score accuracy command");
        };
        assert_eq!(topic.as_deref(), Some("BTC"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_debate_score_unscored() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "debate-score",
            "unscored",
            "--limit",
            "5",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::DebateScore {
                    command:
                        AnalyticsDebateScoreCommand::Unscored { limit, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics debate-score unscored command");
        };
        assert_eq!(limit, Some(5));
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_list_json() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "scenario", "list", "--json"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command:
                        AnalyticsScenarioCommand::List {
                            status,
                            limit,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario list command");
        };

        assert_eq!(status, None);
        assert_eq!(limit, None);
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_add() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "add",
            "Hard Landing",
            "--probability",
            "45.0",
            "--description",
            "Recession scenario",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command:
                        AnalyticsScenarioCommand::Add {
                            value,
                            probability,
                            description,
                            json,
                            ..
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario add command");
        };

        assert_eq!(value, "Hard Landing");
        assert_eq!(probability, Some(45.0));
        assert_eq!(description.as_deref(), Some("Recession scenario"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_update() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "update",
            "Hard Landing",
            "labor rolling over",
            "--probability",
            "65.0",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command:
                        AnalyticsScenarioCommand::Update {
                            value,
                            note_pos,
                            probability,
                            json,
                            ..
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario update command");
        };

        assert_eq!(value, "Hard Landing");
        assert_eq!(note_pos.as_deref(), Some("labor rolling over"));
        assert_eq!(probability, Some(65.0));
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_remove() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "remove",
            "Hard Landing",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::Remove { value, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario remove command");
        };

        assert_eq!(value, "Hard Landing");
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_history() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "history",
            "Hard Landing",
            "--limit",
            "10",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::History { value, limit, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario history command");
        };

        assert_eq!(value, "Hard Landing");
        assert_eq!(limit, Some(10));
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_signal_add() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "signal",
            "add",
            "ISM below 45",
            "--scenario",
            "Hard Landing",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command:
                        AnalyticsScenarioCommand::Signal {
                            command:
                                AnalyticsScenarioSignalCommand::Add {
                                    value,
                                    scenario,
                                    json,
                                    ..
                                },
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario signal add command");
        };

        assert_eq!(value, "ISM below 45");
        assert_eq!(scenario.as_deref(), Some("Hard Landing"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_signal_list() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "signal",
            "list",
            "--scenario",
            "Hard Landing",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command:
                        AnalyticsScenarioCommand::Signal {
                            command: AnalyticsScenarioSignalCommand::List { scenario, json, .. },
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario signal list command");
        };

        assert_eq!(scenario.as_deref(), Some("Hard Landing"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_suggest() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "scenario", "suggest", "--json"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::Suggest { json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario suggest command");
        };

        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_suggest_no_json() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "scenario", "suggest"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::Suggest { json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario suggest command");
        };

        assert!(!json);
    }

    #[test]
    fn parse_analytics_scenario_impact_matrix_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "impact-matrix",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::ImpactMatrix { json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario impact-matrix command");
        };

        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_impact_matrix_no_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "impact-matrix",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::ImpactMatrix { json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario impact-matrix command");
        };

        assert!(!json);
    }

    #[test]
    fn parse_analytics_conviction_set_positional_syntax() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "conviction",
            "set",
            "BTC",
            "-2",
            "setup weakening",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Conviction {
                    command:
                        AnalyticsConvictionCommand::Set {
                            symbol,
                            score_pos,
                            score,
                            notes,
                            notes_pos,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics conviction set command");
        };

        assert_eq!(symbol, "BTC");
        assert_eq!(score_pos, Some(-2));
        assert_eq!(score, None);
        assert_eq!(notes, None);
        assert_eq!(notes_pos.as_deref(), Some("setup weakening"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_regime_set_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "regime",
            "set",
            "risk-off",
            "--confidence",
            "0.8",
            "--drivers",
            "manual override",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Regime {
                            command:
                                AnalyticsMacroRegimeCommand::Set {
                                    regime,
                                    confidence,
                                    drivers,
                                    json,
                                },
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro regime set command");
        };

        assert_eq!(regime, "risk-off");
        assert_eq!(confidence, Some(0.8));
        assert_eq!(drivers.as_deref(), Some("manual override"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_cycles_current_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "cycles",
            "current",
            "US",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Cycles {
                            command:
                                Some(AnalyticsMacroCyclesCommand::Current { country, json }),
                            ..
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro cycles current command");
        };

        assert_eq!(country.as_deref(), Some("US"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_cycles_current_no_country() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "cycles",
            "current",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Cycles {
                            command:
                                Some(AnalyticsMacroCyclesCommand::Current { country, json }),
                            ..
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro cycles current command");
        };

        assert!(country.is_none());
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_cycles_history_add_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "cycles",
            "history",
            "add",
            "--country",
            "US",
            "--determinant",
            "education",
            "--year",
            "1950",
            "--score",
            "9",
            "--notes",
            "GI Bill boom",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Cycles {
                            command:
                                Some(AnalyticsMacroCyclesCommand::History {
                                    command:
                                        AnalyticsMacroCyclesHistoryCommand::Add {
                                            country,
                                            determinant,
                                            year,
                                            score,
                                            notes,
                                            json,
                                            ..
                                        },
                                }),
                            ..
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro cycles history add command");
        };

        assert_eq!(country, "US");
        assert_eq!(determinant, "education");
        assert_eq!(year, 1950);
        assert_eq!(score, 9.0);
        assert_eq!(notes.as_deref(), Some("GI Bill boom"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_cycles_history_list_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "cycles",
            "history",
            "list",
            "--country",
            "US",
            "--determinant",
            "military",
            "--year",
            "1940",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Cycles {
                            command:
                                Some(AnalyticsMacroCyclesCommand::History {
                                    command:
                                        AnalyticsMacroCyclesHistoryCommand::List {
                                            countries,
                                            determinant,
                                            year,
                                            json,
                                            ..
                                        },
                                }),
                            ..
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro cycles history list command");
        };

        assert_eq!(countries, vec!["US".to_string()]);
        assert_eq!(determinant.as_deref(), Some("military"));
        assert_eq!(year, Some(1940));
        assert!(json);
    }

    #[test]
    fn parse_analytics_technicals_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "technicals",
            "--symbol",
            "AAPL",
            "--timeframe",
            "1d",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Technicals {
                    symbol,
                    timeframe,
                    json,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics technicals command");
        };

        assert_eq!(symbol.as_deref(), Some("AAPL"));
        assert_eq!(timeframe, "1d");
        assert!(json);
    }

    #[test]
    fn parse_analytics_levels_command() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "levels", "--symbol", "BTC", "--json"])
                .unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Levels { symbol, json, .. },
        }) = cli.command
        else {
            panic!("expected analytics levels command");
        };

        assert_eq!(symbol.as_deref(), Some("BTC"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_levels_with_type_filter() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "levels",
            "--level-type",
            "support",
            "--limit",
            "10",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Levels {
                    symbol,
                    level_type,
                    limit,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected analytics levels command");
        };

        assert_eq!(symbol, None);
        assert_eq!(level_type.as_deref(), Some("support"));
        assert_eq!(limit, Some(10));
        assert!(json);
    }

    #[test]
    fn parse_analytics_asset_command() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "asset", "BTC-USD", "--json"]).unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Asset { symbol, json },
        }) = cli.command
        else {
            panic!("expected analytics asset command");
        };

        assert_eq!(symbol, "BTC-USD");
        assert!(json);
    }

    #[test]
    fn parse_analytics_asset_command_no_json() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "asset", "GC=F"]).unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Asset { symbol, json },
        }) = cli.command
        else {
            panic!("expected analytics asset command");
        };

        assert_eq!(symbol, "GC=F");
        assert!(!json);
    }

    #[test]
    fn parse_analytics_narrative_command() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "narrative", "--json"]).unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Narrative { json },
        }) = cli.command
        else {
            panic!("expected analytics narrative command");
        };

        assert!(json);
    }

    #[test]
    fn parse_analytics_signals_technical_source() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "signals",
            "--source",
            "technical",
            "--symbol",
            "BTC",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Signals {
                    symbol,
                    source,
                    json,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics signals command");
        };

        assert_eq!(symbol.as_deref(), Some("BTC"));
        assert_eq!(source, "technical");
        assert!(json);
    }

    #[test]
    fn parse_analytics_signals_default_source_is_all() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "signals", "--json"]).unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Signals { source, json, .. },
        }) = cli.command
        else {
            panic!("expected analytics signals command");
        };

        assert_eq!(source, "all");
        assert!(json);
    }

    #[test]
    fn parse_analytics_gaps_with_symbol() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "gaps", "--symbol", "AAPL", "--json"])
            .unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Gaps { symbol, json },
        }) = cli.command
        else {
            panic!("expected analytics gaps command");
        };

        assert_eq!(symbol, Some("AAPL".to_string()));
        assert!(json);
    }

    #[test]
    fn parse_analytics_gaps_without_symbol() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "gaps", "--json"]).unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Gaps { symbol, json },
        }) = cli.command
        else {
            panic!("expected analytics gaps command");
        };

        assert_eq!(symbol, None);
        assert!(json);
    }

    #[test]
    fn parse_prediction_add_timeframe_flag_syntax() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "BTC above 70k",
            "--timeframe",
            "low",
            "--confidence",
            "0.7",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            value,
                            timeframe_pos,
                            confidence_pos,
                            timeframe,
                            confidence,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value.as_deref(), Some("BTC above 70k"));
        assert_eq!(timeframe_pos, None);
        assert_eq!(confidence_pos, None);
        assert_eq!(timeframe.as_deref(), Some("low"));
        assert_eq!(confidence, Some(0.7));
    }

    #[test]
    fn parse_prediction_add_timeframe_alias_short() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "BTC above 70k",
            "--timeframe",
            "short",
            "--confidence",
            "0.7",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            value,
                            timeframe,
                            confidence,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value.as_deref(), Some("BTC above 70k"));
        assert_eq!(timeframe.as_deref(), Some("short"));
        assert_eq!(confidence, Some(0.7));
    }

    #[test]
    fn parse_prediction_add_timeframe_alias_long_positional() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "Gold to 5000",
            "long",
            "0.6",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            value,
                            timeframe_pos,
                            confidence_pos,
                            timeframe,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value.as_deref(), Some("Gold to 5000"));
        assert_eq!(timeframe_pos.as_deref(), Some("long"));
        assert_eq!(confidence_pos, Some(0.6));
        assert_eq!(timeframe, None);
    }

    #[test]
    fn parse_prediction_add_flag_wins_over_positional() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "BTC above 70k",
            "macro",
            "0.9",
            "--timeframe",
            "short",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            value,
                            timeframe_pos,
                            timeframe,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value.as_deref(), Some("BTC above 70k"));
        // Both are captured; the dispatch in main.rs uses flag first
        assert_eq!(timeframe_pos.as_deref(), Some("macro"));
        assert_eq!(timeframe.as_deref(), Some("short"));
    }

    #[test]
    fn parse_prediction_add_full_flag_syntax_with_all_options() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "BTC above 70k",
            "--timeframe",
            "short",
            "--confidence",
            "0.7",
            "--symbol",
            "BTC",
            "--source-agent",
            "evening-analyst",
            "--json",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            value,
                            timeframe,
                            confidence,
                            symbol,
                            source_agent,
                            json,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value.as_deref(), Some("BTC above 70k"));
        assert_eq!(timeframe.as_deref(), Some("short"));
        assert_eq!(confidence, Some(0.7));
        assert_eq!(symbol.as_deref(), Some("BTC"));
        assert_eq!(source_agent.as_deref(), Some("evening-analyst"));
        assert!(json);
    }

    #[test]
    fn parse_prediction_add_claim_flag() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "--claim",
            "BTC above 100k",
            "--timeframe",
            "low",
            "--confidence",
            "0.8",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            value,
                            claim,
                            timeframe,
                            confidence,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value, None);
        assert_eq!(claim.as_deref(), Some("BTC above 100k"));
        assert_eq!(timeframe.as_deref(), Some("low"));
        assert_eq!(confidence, Some(0.8));
    }

    #[test]
    fn parse_prediction_add_claim_overrides_positional() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "positional text",
            "--claim",
            "flag text",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add { value, claim, .. },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value.as_deref(), Some("positional text"));
        assert_eq!(claim.as_deref(), Some("flag text"));
        // main.rs resolves claim.or(value), so --claim wins
    }

    #[test]
    fn parse_prediction_add_no_value_parses() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "--timeframe",
            "low",
        ])
        .expect("cli should parse with no value/claim");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add { value, claim, .. },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value, None);
        assert_eq!(claim, None);
        // main.rs will return an error when neither is provided
    }

    #[test]
    fn parse_prediction_add_claim_only_with_all_flags() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "--claim",
            "Gold above 3500 by Q2",
            "--timeframe",
            "high",
            "--confidence",
            "0.6",
            "--symbol",
            "GC=F",
            "--source-agent",
            "low-agent",
            "--target-date",
            "2026-06-30",
            "--json",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            value,
                            claim,
                            timeframe,
                            confidence,
                            symbol,
                            source_agent,
                            target_date,
                            json,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(value, None);
        assert_eq!(claim.as_deref(), Some("Gold above 3500 by Q2"));
        assert_eq!(timeframe.as_deref(), Some("high"));
        assert_eq!(confidence, Some(0.6));
        assert_eq!(symbol.as_deref(), Some("GC=F"));
        assert_eq!(source_agent.as_deref(), Some("low-agent"));
        assert_eq!(target_date.as_deref(), Some("2026-06-30"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_conviction_list_json() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "conviction", "list", "--json"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Conviction {
                    command: AnalyticsConvictionCommand::List { json },
                },
        }) = cli.command
        else {
            panic!("expected analytics conviction list command");
        };

        assert!(json);
    }

    #[test]
    fn parse_analytics_conviction_history() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "conviction",
            "history",
            "BTC",
            "--limit",
            "10",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Conviction {
                    command:
                        AnalyticsConvictionCommand::History {
                            symbol,
                            limit,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics conviction history command");
        };

        assert_eq!(symbol, "BTC");
        assert_eq!(limit, Some(10));
        assert!(json);
    }

    #[test]
    fn parse_analytics_conviction_changes() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "conviction",
            "changes",
            "14",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Conviction {
                    command: AnalyticsConvictionCommand::Changes { days, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics conviction changes command");
        };

        assert_eq!(days.as_deref(), Some("14"));
        assert!(json);
    }

    #[test]
    fn test_correlations_bare_json_flag() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "correlations", "--json"]).unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Correlations { command, json } = command else {
            panic!("expected correlations");
        };
        assert!(json);
        assert!(command.is_none());
    }

    #[test]
    fn test_correlations_list_subcommand() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "correlations",
            "list",
            "--json",
            "--limit",
            "10",
        ])
        .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Correlations { command, json: _ } = command else {
            panic!("expected correlations");
        };
        let Some(AnalyticsCorrelationsCommand::List { period, limit, json, .. }) = command else {
            panic!("expected List subcommand");
        };
        assert!(json);
        assert_eq!(limit, 10);
        assert!(period.is_none());
    }

    #[test]
    fn test_correlations_list_with_period() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "correlations",
            "list",
            "--period",
            "7d",
        ])
        .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Correlations { command, json: _ } = command else {
            panic!("expected correlations");
        };
        let Some(AnalyticsCorrelationsCommand::List { period, limit, json, .. }) = command else {
            panic!("expected List subcommand");
        };
        assert!(!json);
        assert_eq!(limit, 25);
        assert_eq!(period.as_deref(), Some("7d"));
    }

    #[test]
    fn test_analytics_predictions_bare() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "predictions", "--json"]).unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Predictions {
            command: subcmd,
            category,
            search,
            limit,
            json,
        } = command
        else {
            panic!("expected predictions command");
        };
        assert!(subcmd.is_none());
        assert!(json);
        assert!(category.is_none());
        assert!(search.is_none());
        assert_eq!(limit, 10);
    }

    #[test]
    fn test_analytics_predictions_with_filters() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "predictions",
            "--category",
            "crypto",
            "--search",
            "bitcoin",
            "--limit",
            "5",
        ])
        .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Predictions {
            command: _,
            category,
            search,
            limit,
            json,
        } = command
        else {
            panic!("expected predictions command");
        };
        assert!(!json);
        assert_eq!(category.as_deref(), Some("crypto"));
        assert_eq!(search.as_deref(), Some("bitcoin"));
        assert_eq!(limit, 5);
    }

    #[test]
    fn parse_data_predictions_markets_with_search() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "markets",
            "--search",
            "Fed rate",
            "--category",
            "economics",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        let Some(DataPredictionsCommand::Markets {
            search,
            category,
            json,
            ..
        }) = subcmd
        else {
            panic!("expected markets subcommand");
        };
        assert_eq!(search.as_deref(), Some("Fed rate"));
        assert_eq!(category.as_deref(), Some("economics"));
        assert!(json);
    }

    #[test]
    fn test_predictions_stats_subcommand() {
        // data predictions stats --json
        let cli =
            Cli::try_parse_from(["pftui", "data", "predictions", "stats", "--json"]).unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        assert!(matches!(
            subcmd,
            Some(DataPredictionsCommand::Stats { json: true, .. })
        ));

        // analytics predictions stats --json
        let cli2 =
            Cli::try_parse_from(["pftui", "analytics", "predictions", "stats", "--json"])
                .unwrap();
        let Some(Command::Analytics { command: cmd2 }) = cli2.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Predictions { command: subcmd2, .. } = cmd2 else {
            panic!("expected predictions command");
        };
        assert!(matches!(
            subcmd2,
            Some(DataPredictionsCommand::Stats { json: true, .. })
        ));

        // data predictions stats --timeframe low --agent low-agent --json
        let cli3 = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "stats",
            "--timeframe",
            "low",
            "--agent",
            "low-agent",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command: cmd3 }) = cli3.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd3, .. } = cmd3 else {
            panic!("expected predictions command");
        };
        match subcmd3 {
            Some(DataPredictionsCommand::Stats {
                timeframe,
                agent,
                json,
            }) => {
                assert_eq!(timeframe.as_deref(), Some("low"));
                assert_eq!(agent.as_deref(), Some("low-agent"));
                assert!(json);
            }
            _ => panic!("expected stats subcommand with filters"),
        }

        // data predictions stats (no filters)
        let cli4 =
            Cli::try_parse_from(["pftui", "data", "predictions", "stats"]).unwrap();
        let Some(Command::Data { command: cmd4 }) = cli4.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd4, .. } = cmd4 else {
            panic!("expected predictions command");
        };
        match subcmd4 {
            Some(DataPredictionsCommand::Stats {
                timeframe,
                agent,
                json,
            }) => {
                assert!(timeframe.is_none());
                assert!(agent.is_none());
                assert!(!json);
            }
            _ => panic!("expected stats subcommand"),
        }
    }

    #[test]
    fn test_predictions_scorecard_subcommand() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "scorecard",
            "--date",
            "2026-03-25",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Scorecard { date, limit, json }) => {
                assert_eq!(date.as_deref(), Some("2026-03-25"));
                assert!(limit.is_none());
                assert!(json);
            }
            _ => panic!("expected scorecard subcommand"),
        }
    }

    #[test]
    fn test_predictions_unanswered_subcommand() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "unanswered",
            "--timeframe",
            "medium",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Unanswered {
                timeframe,
                symbol,
                limit,
                json,
            }) => {
                assert_eq!(timeframe.as_deref(), Some("medium"));
                assert!(symbol.is_none());
                assert!(limit.is_none());
                assert!(json);
            }
            _ => panic!("expected unanswered subcommand"),
        }
    }

    #[test]
    fn test_journal_prediction_stats_filters() {
        // journal prediction stats --timeframe low --agent low-agent --json
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "stats",
            "--timeframe",
            "low",
            "--agent",
            "low-agent",
            "--json",
        ])
        .unwrap();
        let Some(Command::Journal {
            command: Some(JournalCommand::Prediction { command }),
        }) = cli.command
        else {
            panic!("expected journal prediction command");
        };
        match command {
            JournalPredictionCommand::Stats {
                timeframe,
                agent,
                json,
            } => {
                assert_eq!(timeframe.as_deref(), Some("low"));
                assert_eq!(agent.as_deref(), Some("low-agent"));
                assert!(json);
            }
            _ => panic!("expected stats subcommand"),
        }
    }

    #[test]
    fn test_predictions_markets_subcommand() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "markets",
            "--category",
            "crypto",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Markets {
                category,
                search,
                limit,
                json,
            }) => {
                assert_eq!(category.as_deref(), Some("crypto"));
                assert!(search.is_none());
                assert_eq!(limit, 10);
                assert!(json);
            }
            _ => panic!("expected markets subcommand"),
        }
    }

    #[test]
    fn parse_data_predictions_map_list() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "map",
            "--list",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Map { list, json, .. }) => {
                assert!(list);
                assert!(json);
            }
            _ => panic!("expected map subcommand"),
        }
    }

    #[test]
    fn parse_data_predictions_map_with_scenario_and_search() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "map",
            "--scenario",
            "US Recession 2026",
            "--search",
            "recession",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Map {
                scenario,
                search,
                contract,
                list,
                json,
            }) => {
                assert_eq!(scenario.as_deref(), Some("US Recession 2026"));
                assert_eq!(search.as_deref(), Some("recession"));
                assert!(contract.is_none());
                assert!(!list);
                assert!(!json);
            }
            _ => panic!("expected map subcommand"),
        }
    }

    #[test]
    fn parse_data_predictions_map_with_contract_id() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "map",
            "--scenario",
            "Fed Cut April",
            "--contract",
            "0xabc123",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Map {
                scenario,
                contract,
                json,
                ..
            }) => {
                assert_eq!(scenario.as_deref(), Some("Fed Cut April"));
                assert_eq!(contract.as_deref(), Some("0xabc123"));
                assert!(json);
            }
            _ => panic!("expected map subcommand"),
        }
    }

    #[test]
    fn parse_data_predictions_unmap() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "unmap",
            "--scenario",
            "US Recession 2026",
            "--contract",
            "0xdef456",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Unmap {
                scenario,
                contract,
                json,
            }) => {
                assert_eq!(scenario, "US Recession 2026");
                assert_eq!(contract.as_deref(), Some("0xdef456"));
                assert!(!json);
            }
            _ => panic!("expected unmap subcommand"),
        }
    }

    #[test]
    fn parse_data_predictions_unmap_all() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "unmap",
            "--scenario",
            "Iran Strike",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions { command: subcmd, .. } = command else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Unmap {
                scenario,
                contract,
                ..
            }) => {
                assert_eq!(scenario, "Iran Strike");
                assert!(contract.is_none());
            }
            _ => panic!("expected unmap subcommand"),
        }
    }

    #[test]
    fn parse_analytics_movers_themes_subcommand() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "movers",
            "themes",
            "--threshold",
            "2",
            "--min-symbols",
            "3",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Movers { command, .. } = command else {
            panic!("expected Movers");
        };
        let Some(AnalyticsMoversCommand::Themes {
            threshold,
            min_symbols,
            json,
        }) = command
        else {
            panic!("expected Themes subcommand");
        };
        assert_eq!(threshold, "2");
        assert_eq!(min_symbols, 3);
        assert!(json);
    }

    #[test]
    fn parse_analytics_movers_bare_still_works() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "movers", "--threshold", "5", "--json"])
                .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Movers {
            command,
            threshold,
            json,
            ..
        } = command
        else {
            panic!("expected Movers");
        };
        assert!(command.is_none());
        assert_eq!(threshold, "5");
        assert!(json);
    }

    #[test]
    fn parse_alignment_summary_flag() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "alignment", "--summary", "--json"])
                .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alignment {
            symbol,
            summary,
            json,
        } = command
        else {
            panic!("expected Alignment");
        };
        assert!(summary);
        assert!(json);
        assert!(symbol.is_none());
    }

    #[test]
    fn parse_alignment_bare_no_summary() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "alignment", "--json"]).unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alignment {
            summary, json, ..
        } = command
        else {
            panic!("expected Alignment");
        };
        assert!(!summary);
        assert!(json);
    }

    #[test]
    fn parse_news_sentiment_defaults() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "news-sentiment", "--json"]).unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::NewsSentiment {
            category,
            hours,
            limit,
            detail,
            json,
        } = command
        else {
            panic!("expected NewsSentiment");
        };
        assert!(category.is_none());
        assert!(hours.is_none());
        assert_eq!(limit, 50);
        assert!(!detail);
        assert!(json);
    }

    #[test]
    fn parse_news_sentiment_with_filters() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "news-sentiment",
            "--category",
            "crypto",
            "--hours",
            "6",
            "--detail",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::NewsSentiment {
            category,
            hours,
            detail,
            json,
            ..
        } = command
        else {
            panic!("expected NewsSentiment");
        };
        assert_eq!(category.as_deref(), Some("crypto"));
        assert_eq!(hours, Some(6));
        assert!(detail);
        assert!(json);
    }

    #[test]
    fn parse_data_news_with_sentiment() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "news",
            "--with-sentiment",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            with_sentiment,
            json,
            ..
        } = command
        else {
            panic!("expected News");
        };
        assert!(with_sentiment);
        assert!(json);
    }

    #[test]
    fn parse_analytics_morning_brief_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "morning-brief", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::MorningBrief { json } = command else {
            panic!("expected MorningBrief");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_morning_brief_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "morning-brief"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::MorningBrief { json } = command else {
            panic!("expected MorningBrief");
        };
        assert!(!json);
    }

    #[test]
    fn parse_analytics_evening_brief_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "evening-brief", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::EveningBrief { json } = command else {
            panic!("expected EveningBrief");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_evening_brief_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "evening-brief"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::EveningBrief { json } = command else {
            panic!("expected EveningBrief");
        };
        assert!(!json);
    }

    #[test]
    fn parse_analytics_regime_flows_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "regime-flows", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::RegimeFlows { json } = command else {
            panic!("expected RegimeFlows");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_regime_flows_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "regime-flows"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::RegimeFlows { json } = command else {
            panic!("expected RegimeFlows");
        };
        assert!(!json);
    }

    #[test]
    fn parse_analytics_regime_transitions_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "regime-transitions", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::RegimeTransitions { json } = command else {
            panic!("expected RegimeTransitions");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_regime_transitions_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "regime-transitions"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::RegimeTransitions { json } = command else {
            panic!("expected RegimeTransitions");
        };
        assert!(!json);
    }

    #[test]
    fn parse_power_flow_assess_defaults() {
        let cli = Cli::parse_from(["pftui", "analytics", "power-flow", "assess"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::PowerFlow { command } = command else {
            panic!("expected PowerFlow");
        };
        let AnalyticsPowerFlowCommand::Assess {
            days,
            complex,
            json,
        } = command
        else {
            panic!("expected Assess");
        };
        assert_eq!(days, 7);
        assert!(complex.is_none());
        assert!(!json);
    }

    #[test]
    fn parse_power_flow_assess_all_flags() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "power-flow",
            "assess",
            "--days",
            "14",
            "--complex",
            "FIC",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::PowerFlow { command } = command else {
            panic!("expected PowerFlow");
        };
        let AnalyticsPowerFlowCommand::Assess {
            days,
            complex,
            json,
        } = command
        else {
            panic!("expected Assess");
        };
        assert_eq!(days, 14);
        assert_eq!(complex.as_deref(), Some("FIC"));
        assert!(json);
    }

    #[test]
    fn journal_entry_add_positional_value() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "entry",
            "add",
            "Gold looking strong",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        let JournalCommand::Entry { command: entry_cmd } = command.unwrap() else {
            panic!("expected Entry");
        };
        let JournalEntryCommand::Add { value, content, .. } = entry_cmd else {
            panic!("expected Add");
        };
        assert_eq!(value.as_deref(), Some("Gold looking strong"));
        assert!(content.is_none());
    }

    #[test]
    fn journal_entry_add_content_flag() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "entry",
            "add",
            "--content",
            "Fed meeting notes",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        let JournalCommand::Entry { command: entry_cmd } = command.unwrap() else {
            panic!("expected Entry");
        };
        let JournalEntryCommand::Add { value, content, .. } = entry_cmd else {
            panic!("expected Add");
        };
        assert!(value.is_none());
        assert_eq!(content.as_deref(), Some("Fed meeting notes"));
    }

    #[test]
    fn journal_entry_add_content_overrides_positional() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "entry",
            "add",
            "positional text",
            "--content",
            "flag text wins",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        let JournalCommand::Entry { command: entry_cmd } = command.unwrap() else {
            panic!("expected Entry");
        };
        let JournalEntryCommand::Add { value, content, .. } = entry_cmd else {
            panic!("expected Add");
        };
        // Both present — main.rs resolves content.or(value), so content wins
        assert_eq!(value.as_deref(), Some("positional text"));
        assert_eq!(content.as_deref(), Some("flag text wins"));
    }

    #[test]
    fn journal_entry_add_no_value_no_content_parses() {
        // Clap allows this since value is now optional; main.rs handles the error
        let cli = Cli::try_parse_from(["pftui", "journal", "entry", "add", "--tag", "macro"])
            .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        let JournalCommand::Entry { command: entry_cmd } = command.unwrap() else {
            panic!("expected Entry");
        };
        let JournalEntryCommand::Add { value, content, .. } = entry_cmd else {
            panic!("expected Add");
        };
        assert!(value.is_none());
        assert!(content.is_none());
    }

    #[test]
    fn journal_entry_add_help_shows_content_flag() -> Result<()> {
        let help = subcommand_help(&["journal", "entry", "add"])?;
        assert!(help.contains("--content"), "help should show --content flag");
        assert!(
            help.contains("--date"),
            "help should show --date flag"
        );
        assert!(
            help.contains("YYYY-MM-DD"),
            "help should describe date format"
        );
        Ok(())
    }

    #[test]
    fn parse_data_prices_market_flag() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "prices", "--market", "--json"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices { market, json } = command else {
            panic!("expected Prices");
        };
        assert!(market);
        assert!(json);
    }

    #[test]
    fn parse_data_prices_no_market_flag() {
        let cli = Cli::try_parse_from(["pftui", "data", "prices", "--json"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices { market, json } = command else {
            panic!("expected Prices");
        };
        assert!(!market);
        assert!(json);
    }

    #[test]
    fn parse_data_quotes_alias_resolves_to_prices() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "quotes", "--json"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices { market, json } = command else {
            panic!("expected Prices via quotes alias");
        };
        assert!(!market);
        assert!(json);
    }

    #[test]
    fn parse_data_quotes_alias_with_market_flag() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "quotes", "--market", "--json"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices { market, json } = command else {
            panic!("expected Prices via quotes alias");
        };
        assert!(market);
        assert!(json);
    }

    #[test]
    fn parse_data_quotes_alias_no_flags() {
        let cli = Cli::try_parse_from(["pftui", "data", "quotes"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices { market, json } = command else {
            panic!("expected Prices via quotes alias");
        };
        assert!(!market);
        assert!(!json);
    }

    #[test]
    fn parse_analytics_alerts_triage_json() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "alerts", "triage", "--json"]).unwrap();
        let Command::Analytics { command } = cli.command.unwrap() else {
            panic!("expected Analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected Alerts");
        };
        let AnalyticsAlertsCommand::Triage { json } = command else {
            panic!("expected Triage");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_alerts_triage_no_json() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "alerts", "triage"]).unwrap();
        let Command::Analytics { command } = cli.command.unwrap() else {
            panic!("expected Analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected Alerts");
        };
        let AnalyticsAlertsCommand::Triage { json } = command else {
            panic!("expected Triage");
        };
        assert!(!json);
    }

    #[test]
    fn parse_analytics_cross_timeframe_resolve_flag() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "cross-timeframe",
            "--resolve",
            "--json",
        ])
        .unwrap();
        let Command::Analytics { command } = cli.command.unwrap() else {
            panic!("expected Analytics");
        };
        let AnalyticsCommand::CrossTimeframe {
            symbol,
            threshold: _,
            limit: _,
            resolve,
            json,
        } = command
        else {
            panic!("expected CrossTimeframe");
        };
        assert!(resolve);
        assert!(json);
        assert!(symbol.is_none());
    }

    #[test]
    fn parse_analytics_cross_timeframe_no_resolve() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "cross-timeframe", "--json"]).unwrap();
        let Command::Analytics { command } = cli.command.unwrap() else {
            panic!("expected Analytics");
        };
        let AnalyticsCommand::CrossTimeframe {
            resolve, json, ..
        } = command
        else {
            panic!("expected CrossTimeframe");
        };
        assert!(!resolve);
        assert!(json);
    }

    #[test]
    fn parse_analytics_cross_timeframe_resolve_with_symbol() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "cross-timeframe",
            "--resolve",
            "--symbol",
            "BTC",
            "--json",
        ])
        .unwrap();
        let Command::Analytics { command } = cli.command.unwrap() else {
            panic!("expected Analytics");
        };
        let AnalyticsCommand::CrossTimeframe {
            symbol, resolve, ..
        } = command
        else {
            panic!("expected CrossTimeframe");
        };
        assert!(resolve);
        assert_eq!(symbol.as_deref(), Some("BTC"));
    }

    #[test]
    fn parse_journal_prediction_lessons_list() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "lessons",
            "--json",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        match command {
            Some(JournalCommand::Prediction { command }) => {
                match command {
                    JournalPredictionCommand::Lessons { command, json, .. } => {
                        assert!(command.is_none());
                        assert!(json);
                    }
                    _ => panic!("expected Lessons"),
                }
            }
            _ => panic!("expected Prediction"),
        }
    }

    #[test]
    fn parse_journal_prediction_lessons_with_miss_type() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "lessons",
            "--miss-type",
            "timing",
            "--limit",
            "5",
            "--json",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        match command {
            Some(JournalCommand::Prediction { command }) => {
                match command {
                    JournalPredictionCommand::Lessons { miss_type, limit, json, .. } => {
                        assert_eq!(miss_type.as_deref(), Some("timing"));
                        assert_eq!(limit, Some(5));
                        assert!(json);
                    }
                    _ => panic!("expected Lessons"),
                }
            }
            _ => panic!("expected Prediction"),
        }
    }

    #[test]
    fn parse_journal_prediction_lessons_add() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "lessons",
            "add",
            "--prediction-id",
            "42",
            "--miss-type",
            "directional",
            "--what-happened",
            "BTC dropped to 60k",
            "--why-wrong",
            "Ignored macro headwinds",
            "--signal-misread",
            "Volume divergence was bearish",
            "--json",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        match command {
            Some(JournalCommand::Prediction { command }) => {
                match command {
                    JournalPredictionCommand::Lessons { command, .. } => {
                        match command {
                            Some(JournalPredictionLessonsCommand::Add {
                                prediction_id,
                                miss_type,
                                what_happened,
                                why_wrong,
                                signal_misread,
                                json,
                            }) => {
                                assert_eq!(prediction_id, 42);
                                assert_eq!(miss_type, "directional");
                                assert_eq!(what_happened, "BTC dropped to 60k");
                                assert_eq!(why_wrong, "Ignored macro headwinds");
                                assert_eq!(signal_misread.as_deref(), Some("Volume divergence was bearish"));
                                assert!(json);
                            }
                            _ => panic!("expected Add"),
                        }
                    }
                    _ => panic!("expected Lessons"),
                }
            }
            _ => panic!("expected Prediction"),
        }
    }
}
