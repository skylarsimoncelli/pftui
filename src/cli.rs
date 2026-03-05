use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "pftui", version, about = "Terminal portfolio tracker")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
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

        /// Show technical indicators (RSI, MACD, SMA) for each position
        #[arg(long)]
        technicals: bool,
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

    /// Output a markdown-formatted portfolio brief for agent consumption and daily reports
    Brief {
        /// Show technical indicators (RSI, MACD, SMA) for each position
        #[arg(long)]
        technicals: bool,

        /// Output structured JSON for agent consumption (includes all available data)
        #[arg(long)]
        agent: bool,
    },
    /// Show total portfolio value with gain/loss (uses cached prices)
    Value,

    /// Display watchlist symbols with current cached prices
    Watchlist {
        /// Filter to symbols within N% of their target price (e.g. 10)
        #[arg(long)]
        approaching: Option<String>,
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

    /// Show biggest daily movers across held + watchlist symbols
    Movers {
        /// Minimum absolute daily change % to include (default: 3)
        #[arg(long, default_value = "3")]
        threshold: String,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },

    /// Show prediction market odds from Polymarket and Manifold
    Predictions {
        /// Filter by category: crypto, economics, geopolitics, ai
        #[arg(long)]
        category: Option<String>,

        /// Search question text (case-insensitive substring match)
        #[arg(long)]
        search: Option<String>,

        /// Maximum number of markets to show (default: 10)
        #[arg(long, default_value = "10")]
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
