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
    Summary,

    /// Export portfolio data
    Export {
        #[arg(value_enum)]
        format: ExportFormat,
    },

    /// List all transactions
    ListTx,

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
}

#[derive(Clone, ValueEnum)]
pub enum ExportFormat {
    Csv,
    Json,
}
