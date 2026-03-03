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

    /// Add a symbol to the watchlist
    Watch {
        /// Symbol to watch (e.g. AAPL, BTC, GC=F)
        symbol: String,
        /// Asset category (equity, crypto, forex, cash, commodity, fund). Auto-detected if omitted.
        #[arg(long)]
        category: Option<String>,
    },

    /// Remove a symbol from the watchlist
    Unwatch {
        /// Symbol to unwatch
        symbol: String,
    },

    /// Fetch and cache current prices for all held symbols without launching the TUI
    Refresh,

    /// Output a markdown-formatted portfolio brief for agent consumption and daily reports
    Brief,
    /// Show total portfolio value with gain/loss (uses cached prices)
    Value,

    /// Display watchlist symbols with current cached prices
    Watchlist,

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
