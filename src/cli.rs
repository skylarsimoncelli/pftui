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
    Prices {
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
    /// Oil futures term structure: contango/backwardation, WTI-Brent spread, war-premium signal
    #[command(name = "oil-premium")]
    OilPremium {
        /// Output as JSON for agent/script consumption
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
    Add {
        value: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        conviction: Option<String>,
        #[arg(long)]
        json: bool,
    },
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
    Search {
        query: String,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
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
    Remove {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        json: bool,
    },
    Tags {
        #[arg(long)]
        json: bool,
    },
    Stats {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalPredictionCommand {
    /// Add a prediction. Timeframe accepts: low, medium, high, macro (aliases: short=low, long=high).
    /// Prefer --timeframe flag; positional shorthand kept for backwards compatibility.
    ///
    /// Examples:
    ///   pftui journal prediction add "BTC above 70k" --timeframe short --confidence 0.7
    ///   pftui journal prediction add "Gold to 3000" medium 0.8
    Add {
        /// The prediction claim text
        value: String,
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
    Stats {
        #[arg(long)]
        json: bool,
    },
    Scorecard {
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalConvictionCommand {
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
    List {
        #[arg(long)]
        json: bool,
    },
    History {
        symbol: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Changes {
        days: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalNotesCommand {
    Add {
        value: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        section: Option<String>,
        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Search {
        query: String,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Remove {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalScenarioSignalCommand {
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
    Remove {
        #[arg(long = "signal-id")]
        signal_id: i64,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum JournalScenarioCommand {
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
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
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
    Remove {
        value: String,
        #[arg(long)]
        json: bool,
    },
    History {
        value: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
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
        /// Structured alert kind: price, allocation, indicator, technical, macro
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
    Dashboard {
        #[arg(long)]
        json: bool,
    },
    Evidence {
        #[command(subcommand)]
        command: AnalyticsTrendsEvidenceCommand,
    },
    Impact {
        #[command(subcommand)]
        command: AnalyticsTrendsImpactCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsMacroCyclesHistoryCommand {
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
    History {
        #[command(subcommand)]
        command: AnalyticsMacroCyclesHistoryCommand,
    },
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
    Current {
        #[arg(long)]
        json: bool,
    },
    Set {
        regime: String,
        #[arg(long)]
        confidence: Option<f64>,
        #[arg(long)]
        drivers: Option<String>,
        #[arg(long)]
        json: bool,
    },
    History {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Transitions {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsMacroCommand {
    Metrics {
        country: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Compare {
        left: Option<String>,
        right: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Cycles {
        #[command(subcommand)]
        command: Option<AnalyticsMacroCyclesCommand>,
        #[arg(long)]
        json: bool,
    },
    Outcomes {
        #[arg(long)]
        json: bool,
    },
    Parallels {
        #[arg(long)]
        json: bool,
    },
    Log {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Regime {
        #[command(subcommand)]
        command: AnalyticsMacroRegimeCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsScenarioSignalCommand {
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
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
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
    Remove {
        value: String,
        #[arg(long)]
        json: bool,
    },
    History {
        value: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Signal {
        #[command(subcommand)]
        command: AnalyticsScenarioSignalCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsConvictionCommand {
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
    List {
        #[arg(long)]
        json: bool,
    },
    History {
        symbol: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Changes {
        days: Option<String>,
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
    Summary {
        #[arg(long)]
        json: bool,
    },
    /// Situation Room — active situation monitoring and management
    Situation {
        #[command(subcommand)]
        command: Option<SituationCommand>,
        #[arg(long)]
        json: bool,
    },
    Deltas {
        #[arg(long, default_value = "last-refresh")]
        since: String,
        #[arg(long)]
        json: bool,
    },
    Catalysts {
        #[arg(long, default_value = "week")]
        window: String,
        #[arg(long)]
        json: bool,
    },
    Impact {
        #[arg(long)]
        json: bool,
    },
    /// Projected P&L under each active scenario/branch probability
    ImpactEstimate {
        #[arg(long)]
        json: bool,
    },
    Opportunities {
        #[arg(long)]
        json: bool,
    },
    Narrative {
        #[arg(long)]
        json: bool,
    },
    Synthesis {
        #[arg(long)]
        json: bool,
    },
    Low {
        #[arg(long)]
        json: bool,
    },
    Medium {
        #[arg(long)]
        json: bool,
    },
    High {
        #[arg(long)]
        json: bool,
    },
    Macro {
        #[command(subcommand)]
        command: Option<AnalyticsMacroCommand>,
        #[arg(long)]
        json: bool,
    },
    Alignment {
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Divergence {
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Digest {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
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
    Gaps {
        /// Show per-symbol OHLCV data quality for a specific symbol
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Movers {
        #[arg(long, default_value = "3")]
        threshold: String,
        #[arg(long)]
        overnight: bool,
        #[arg(long)]
        json: bool,
    },
    Correlations {
        #[command(subcommand)]
        command: Option<AnalyticsCorrelationsCommand>,
    },
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
    Trends {
        #[command(subcommand)]
        command: AnalyticsTrendsCommand,
    },
    Alerts {
        #[command(subcommand)]
        command: AnalyticsAlertsCommand,
    },
    Scenario {
        #[command(subcommand)]
        command: AnalyticsScenarioCommand,
    },
    Conviction {
        #[command(subcommand)]
        command: AnalyticsConvictionCommand,
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

    /// Multi-timeframe analytics engine views
    #[command(name = "analytics")]
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

        assert_eq!(value, "btc breakout");
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

        assert_eq!(value, "BTC above 70k");
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

        assert_eq!(value, "BTC above 70k");
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

        assert_eq!(value, "Gold to 5000");
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

        assert_eq!(value, "BTC above 70k");
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

        assert_eq!(value, "BTC above 70k");
        assert_eq!(timeframe.as_deref(), Some("short"));
        assert_eq!(confidence, Some(0.7));
        assert_eq!(symbol.as_deref(), Some("BTC"));
        assert_eq!(source_agent.as_deref(), Some("evening-analyst"));
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
}
