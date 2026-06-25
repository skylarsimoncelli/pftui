use std::path::PathBuf;

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

        /// Category: signal, feedback, alert, handoff, escalation, decision-card
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
    #[command(
        after_help = "Examples:\n  pftui agent message ack --id 12 --id 13\n  pftui agent message ack --all\n  pftui agent message ack --all --to morning-brief\n\n`--to` expects a recipient agent name, not a message ID or thread ID.\nUse it only with `--all` when you want to bulk-ack the queue for one recipient."
    )]
    Ack {
        /// One or more message IDs (repeatable: --id 1 --id 2 --id 3)
        #[arg(long)]
        id: Vec<i64>,

        /// Acknowledge ALL pending messages (same as `ack-all`)
        #[arg(long, conflicts_with = "id")]
        all: bool,

        /// Recipient agent name to bulk-ack when using --all (for example: --to morning-brief)
        #[arg(long, requires = "all")]
        to: Option<String>,

        #[arg(long)]
        json: bool,
    },
    /// Acknowledge all pending messages for a recipient (alias for `ack --all`)
    #[command(
        after_help = "Example:\n  pftui agent message ack-all --to morning-brief\n\n`--to` expects a recipient agent name."
    )]
    #[command(name = "ack-all")]
    AckAll {
        /// Recipient agent name whose pending queue should be acknowledged
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
    #[command(
        after_help = "Sources: prices, predictions, fedwatch, news_rss, news_brave, cot,\n         sentiment, calendar, economy, fred, bls, worldbank, comex,\n         onchain, analytics, alerts, cleanup.\n\nExamples:\n  pftui data refresh --only prices              # price data only\n  pftui data refresh --only prices,news_rss     # prices + RSS news\n  pftui data refresh --skip worldbank,bls,cot   # skip slow sources\n  pftui data refresh --stale                    # only stale/empty status-tracked feeds\n  pftui data refresh --timeout 90 --json        # return partial JSON if the run exceeds 90s\n  pftui data refresh --accept-outlier BTC-USD   # admit a genuine >20% d/d gap past the price guard\n\n--only, --skip, and --stale are mutually exclusive.\n\nPrice-ingest guard: closes that move >20% day-over-day are SUSPECT and are\nrejected unless corroborated by a wired secondary source (BTC: mempool.space\n/ CoinGecko; GC=F: GeckoTerminal XAUT) within 5%, or admitted explicitly via\n--accept-outlier. Failed fetches never stamp a stale cached price onto\ntoday's date. Retro-scan stored history with: pftui data prices audit"
    )]
    Refresh {
        /// Send OS notification for newly triggered alerts
        #[arg(long)]
        notify: bool,
        /// Output structured JSON metrics instead of human-readable text
        #[arg(long)]
        json: bool,
        /// Stop after N seconds and return partial refresh results
        #[arg(long)]
        timeout: Option<u64>,
        /// Run only these sources (comma-separated). Mutually exclusive with --skip.
        #[arg(long, conflicts_with_all = ["skip", "stale"], value_delimiter = ',')]
        only: Vec<String>,
        /// Skip these sources (comma-separated). Mutually exclusive with --only.
        #[arg(long, conflicts_with_all = ["only", "stale"], value_delimiter = ',')]
        skip: Vec<String>,
        /// Refresh only feeds currently marked stale/empty by `data status`.
        #[arg(long, conflicts_with_all = ["only", "skip"])]
        stale: bool,
        /// Admit a >20% day-over-day price print for SYM past the ingest
        /// plausibility guard (genuine crash/halt gaps only). Repeatable or
        /// comma-separated.
        #[arg(long = "accept-outlier", value_name = "SYM", value_delimiter = ',')]
        accept_outlier: Vec<String>,
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
    /// Canonical-series registry: per-series freshness vs SLA
    Series {
        #[command(subcommand)]
        command: DataSeriesCommand,
    },
    /// One deterministic market-context line from latest closes (for stamping notes/entries)
    #[command(
        name = "snapshot-line",
        after_help = "Format: <YYYY-MM-DD> | SPX <close> | BTC <close> | GOLD <close> |\nSILVER <close> | DXY <close> | VIX <close>\n\nBuilt from the latest cached closes (BTC falls back to the deep BTC-USD\nseries; a series with no history is omitted, never invented). Journal\nwriters use `--stamp` on `journal notes add` / `journal entry add` to\nprepend this line, so every note self-contextualizes for retro-scoring\nand post-mortems.\n\nExamples:\n  pftui data snapshot-line\n  pftui data snapshot-line --json\n  pftui journal notes add \"...\" --author analyst-low --stamp"
    )]
    SnapshotLine {
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
        #[command(subcommand)]
        command: Option<DataNewsCommand>,

        /// Filter by source (e.g. "Reuters", "CoinDesk", "ZeroHedge")
        #[arg(long)]
        source: Option<String>,

        /// Search title text (case-insensitive substring match)
        #[arg(long)]
        search: Option<String>,

        /// Show only news from last N hours
        #[arg(long)]
        hours: Option<i64>,

        /// Fetch fresh headlines now instead of reading only from the cached news table
        #[arg(long, alias = "today")]
        breaking: bool,

        /// Comma-separated independence classes: independent,wire,restatement,rumor,unknown
        #[arg(long = "filter-independence")]
        filter_independence: Option<String>,

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
    /// Show Fear & Greed indices with cached history
    FearGreed {
        /// Include up to N daily history points per index
        #[arg(long)]
        history: Option<u32>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Manage economic/geopolitical calendar events
    ///
    /// Without a subcommand, lists upcoming events (same as `calendar list`).
    /// Use `add`/`remove` subcommands to manage custom catalysts (geopolitical deadlines,
    /// trade events, etc.) that flow into the analytics catalysts ranking system.
    ///
    /// Examples:
    ///   pftui data calendar --json                  # list (default)
    ///   pftui data calendar list --days 14 --impact high --json
    ///   pftui data calendar add --date 2026-04-06 --name "Iran Hormuz Strait Deadline" --impact high --type geopolitical
    ///   pftui data calendar remove --date 2026-04-06 --name "Iran Hormuz Strait Deadline"
    Calendar {
        #[command(subcommand)]
        command: Option<CalendarCommand>,

        /// Number of days to look ahead (default: 7)
        #[arg(long, default_value = "7")]
        days: i64,

        /// Filter by impact level: high, medium, low
        #[arg(long)]
        impact: Option<String>,

        /// Filter by event type: economic, earnings, geopolitical
        #[arg(long = "type")]
        event_type: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Interpret cached COT positioning using percentile and z-score context
    Cot {
        /// Optional tracked symbol (GC=F, SI=F, CL=F, BTC)
        symbol: Option<String>,

        /// Fetch fresh CFTC data now instead of relying on cached reports
        #[arg(long = "force-refresh")]
        force_refresh: bool,

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

        /// Curated geopolitics relevance filter: keyword-matched contracts only, excluding contracts resolving >12 months out, already past resolution, or with zero 24h volume
        #[arg(long, conflicts_with = "category")]
        geo: bool,

        /// Maximum number of markets to show (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Options chain viewer + GEX (gamma exposure) ingestion (Yahoo free data)
    #[command(
        after_help = "Subcommands:\n  refresh   Fetch + persist chain snapshots and compute GEX summaries\n  show      Display the most recent cached chain (Yahoo free data viewer)"
    )]
    Options {
        #[command(subcommand)]
        command: DataOptionsCommand,
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
    #[command(
        alias = "quotes",
        after_help = "Aliases: `data quotes` also works.\n\nFor overnight futures specifically, see: pftui data futures\nFor market overview symbols, add --market flag.\nUse --auto-refresh to automatically refresh stale (>2h) prices before returning.\nRetro-scan stored history for corrupt prints: pftui data prices audit"
    )]
    Prices {
        #[command(subcommand)]
        command: Option<DataPricesCommand>,
        /// Include all market overview symbols (indices, commodities, crypto, forex, bonds)
        #[arg(long)]
        market: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
        /// Automatically refresh prices if cache is stale (>2h old)
        #[arg(long)]
        auto_refresh: bool,
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
    #[command(
        after_help = "For portfolio/watchlist price quotes, see: pftui data prices (alias: data quotes)\nFor market overview prices, see: pftui data prices --market"
    )]
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
    #[command(
        name = "alerts",
        after_help = "Alerts are managed under the analytics domain:\n\n  pftui analytics alerts list        List alert rules\n  pftui analytics alerts check       Check alerts against current data\n  pftui analytics alerts add          Add an alert rule\n  pftui analytics alerts ack          Acknowledge triggered alerts\n  pftui analytics alerts seed-defaults Seed smart-alert defaults\n\nRun `pftui analytics alerts --help` for full details."
    )]
    Alerts {
        #[command(subcommand)]
        command: Option<DataAlertsRedirect>,
    },
    /// Real-yields curve: US TIPS, breakevens, G10 sovereign 10Y (FRED)
    #[command(name = "real-yields")]
    RealYields {
        #[command(subcommand)]
        command: DataRealYieldsCommand,
    },
    /// DB-wide false-value audit: per-table signature checks over stored series and ledgers (read-only)
    #[command(
        after_help = "Read-only umbrella over per-table signature checks, each carrying\nper-table judgment (April-2020 negative oil is REAL; near-zero ^IRX yields\nare REAL — neither is condemned):\n\n  price_history        spike-and-revert scan + cross-population bimodality\n                       (two close bands >10x apart — the equity-collision\n                       signature) + exact-placeholder runs (>=5 identical\n                       closes to 4dp on FX/commodity symbols)\n  economic_data        plausible-range violations (quarantined=0 anomalies)\n  sentiment_history    0-100 range + duplicate (date, index_type)\n  cot_cache            negative position counts, net != long - short\n  onchain_cache        all-zero runs >=5 per metric (incl. etf_flow_*)\n  forecast_scores /    realized/forward returns outside +/-95% (non-crypto)\n  signal_expectancy /  or +/-99.9% (crypto) — fat-finger detection in our\n  recommendations      own ledgers\n  portfolio_snapshots  day-over-day total_value jumps >30% (severity info —\n                       flow events are real; deliberate operator backfill\n                       rows with cash_value=0 are excluded)\n  scenario_history     active-scenario probability book sums outside\n                       [60, 110] per recorded date + single-scenario moves\n                       >15pp between consecutive records (pre-2026-06-10\n                       ledger discipline: info — expected; on/after: suspect)\n  transactions         buy/sell fill price >15% from the nearest session\n                       close, nonpositive quantities, orphaned paired_tx_id\n                       (always suspect — operator-entered, never auto-fixed;\n                       output is row id + symbol + date + deviation ONLY)\n\nSeverity: info (real but notable) | suspect (likely false value) |\ncorrupt (provably wrong). Output lists row KEYS only, never values from\nthe operator's portfolio tables.\n\nRead-only by design — repair stays manual:\n  pftui data decontaminate --symbol SYM   # purge poisoned L2 derived rows\n  pftui data prices audit                 # price-only spike-revert detail\n\nExamples:\n  pftui data audit\n  pftui data audit --table price_history --json"
    )]
    Audit {
        /// Limit to one table's checks (e.g. price_history, economic_data)
        #[arg(long)]
        table: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Purge L2 derived rows computed from a corrupt L1 price series (dry-run by default)
    #[command(
        after_help = "When price_history is repaired after a corruption incident, the L2 rows\ncomputed FROM the corrupt closes do not self-heal: technical_snapshots /\ncorrelation_snapshots are stamped per refresh run, so poisoned historical\nrows persist forever. This deletes them for one symbol.\n\nScope (per-symbol L2 only): technical_snapshots, correlation_snapshots\n(either side of the pair), technical_levels, technical_signals,\nsignal_expectancy. Excluded by design: timeframe_signals, regime_*,\nportfolio/position_snapshots (cross-asset aggregates / operator history —\npartial deletion would skew them).\n\nHonesty note: deleted HISTORICAL rows do not regrow on refresh (snapshots\nonly accumulate going forward); signal_expectancy alone fully rebuilds via\n`pftui research backtest`. Downstream readers tolerate the gap.\n\nDry-run is the default — counts only. `--confirm` executes inside a\ntransaction and writes a journal-note audit trail (author system,\nsection system).\n\nExamples:\n  pftui data decontaminate --symbol BTC --before 2026-06-12\n  pftui data decontaminate --symbol JPY=X --before 2026-06-12 --confirm"
    )]
    Decontaminate {
        /// Symbol whose derived rows to purge (exact match; run once per symbol/alias)
        #[arg(long)]
        symbol: String,
        /// Only purge rows computed before this YYYY-MM-DD date (default: all)
        #[arg(long)]
        before: Option<String>,
        /// Explicit dry run (the default behavior; kept for scripting clarity)
        #[arg(long, conflicts_with = "confirm")]
        dry_run: bool,
        /// Execute the deletes (otherwise this is a dry run printing counts)
        #[arg(long)]
        confirm: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Capital flow tracking: ETF creation/redemption, 13F flows, crypto exchange flows (F59 scaffold)
    #[command(
        after_help = "Capital-flows provider scaffold. Real ETF/13F data requires a paid\nprovider; the default `noop` provider returns zero flows so the\nschema, CLI, and DB plumbing stay in place. Select a provider via\nthe `PFTUI_FLOWS_PROVIDER` env var (`noop`, `etf_com_csv`, `sec_edgar_13f`).\n\nExamples:\n  pftui data flows refresh --json\n  pftui data flows refresh --asset SPY --json\n  pftui data flows show --since 30d --json\n  pftui data flows show --asset BTC --json"
    )]
    Flows {
        #[command(subcommand)]
        command: DataFlowsCommand,
    },
}

#[derive(Subcommand)]
pub enum DataFlowsCommand {
    /// Refresh capital flows from the configured provider (env: PFTUI_FLOWS_PROVIDER)
    Refresh {
        /// Optional asset filter (e.g. SPY, BTC)
        #[arg(long)]
        asset: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Show cached capital flow rows
    Show {
        /// Optional asset filter (e.g. SPY, BTC)
        #[arg(long)]
        asset: Option<String>,
        /// Lookback window: NNd, NNw, NNm, or YYYY-MM-DD
        #[arg(long)]
        since: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DataOptionsCommand {
    /// Fetch the live options chain from Yahoo and persist a snapshot + GEX summary
    Refresh {
        /// Single symbol to refresh (default: SPY, QQQ, GLD, SLV with --all or no symbol)
        #[arg(long)]
        symbol: Option<String>,
        /// Refresh the full default symbol list (SPY, QQQ, GLD, SLV)
        #[arg(long)]
        all: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Display the most recent cached options chain (latest fetched_at)
    Show {
        /// Underlying symbol (required)
        #[arg(long)]
        symbol: String,
        /// Number of strikes per side to show (default: 12)
        #[arg(long, default_value = "12")]
        limit: usize,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Live viewer: fetch from Yahoo without persisting (legacy interactive chain view)
    View {
        /// Underlying symbol (required)
        #[arg(long)]
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
}

#[derive(Subcommand)]
pub enum DataRealYieldsCommand {
    /// Fetch the configured FRED real-yield series and persist them
    Refresh {
        /// Number of days of history to fetch (default: 90)
        #[arg(long, default_value = "90")]
        days: u32,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Read cached real-yield rows, optionally filtered by series and window
    Show {
        /// Filter to a single FRED series id (e.g. DFII10, T10YIE)
        #[arg(long)]
        series: Option<String>,
        /// Window expressed as NNd/NNw/NNm or YYYY-MM-DD (default: 30d)
        #[arg(long, default_value = "30d")]
        since: String,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DataNewsCommand {
    /// Inspect or reset RSS feed health
    Feeds {
        #[command(subcommand)]
        command: DataNewsFeedsCommand,
    },
    /// Manage news source tier mappings
    Sources {
        #[command(subcommand)]
        command: DataNewsSourcesCommand,
    },
    /// Manage news-topic to prediction-market bindings
    Topics {
        #[command(subcommand)]
        command: DataNewsTopicsCommand,
    },
}

#[derive(Subcommand)]
pub enum DataNewsFeedsCommand {
    /// List RSS feed health
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Reset a disabled/degraded RSS feed back to active
    Reset {
        /// Feed ID, usually the feed name shown by `feeds list`
        feed_id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DataNewsSourcesCommand {
    /// List source domain tier mappings
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List inferred source domains that need explicit classification
    Unclassified {
        /// Lookback window, e.g. 7d
        #[arg(long, default_value = "7d")]
        since: String,
        /// Minimum article count in the lookback window
        #[arg(long = "min-articles", default_value_t = 1)]
        min_articles: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Summarize source tier coverage and top domains
    Stats {
        /// Lookback window, e.g. 7d
        #[arg(long, default_value = "7d")]
        since: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Set or update a source domain tier
    Set {
        /// Source domain, e.g. reuters.com
        domain: String,
        /// Tier 1-4 (1 primary wire, 4 unverified/blog)
        #[arg(long)]
        tier: i64,
        /// Optional operator note
        #[arg(long)]
        notes: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a source domain tier mapping
    Remove {
        /// Source domain to remove
        domain: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DataNewsTopicsCommand {
    /// List topic-to-market bindings used to annotate news JSON
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Set or update a topic-to-market binding
    Set {
        /// Topic key, e.g. iran-hormuz or fed-policy
        topic: String,
        /// Primary prediction-market contract ID
        #[arg(long = "primary-market-id")]
        primary_market_id: String,
        /// Secondary prediction-market contract ID
        #[arg(long = "secondary-market-id")]
        secondary_market_id: Option<String>,
        /// Optional operator note
        #[arg(long)]
        notes: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a topic-to-market binding
    Remove {
        /// Topic key to remove
        topic: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DataAlertsRedirect {
    /// → Redirects to `analytics alerts check`
    Check {
        #[arg(long)]
        today: bool,
        #[arg(long = "newly-triggered")]
        newly_triggered: bool,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        condition: Option<String>,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        status: Option<String>,
        /// Filter by urgency tier: critical, high, watch, low
        #[arg(long)]
        urgency: Option<String>,
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
pub enum CalendarCommand {
    /// List upcoming calendar events (default behavior)
    ///
    /// Shows economic releases, earnings, and geopolitical catalysts.
    List {
        /// Number of days to look ahead (default: 7)
        #[arg(long, default_value = "7")]
        days: i64,

        /// Filter by impact level: high, medium, low
        #[arg(long)]
        impact: Option<String>,

        /// Filter by event type: economic, earnings, geopolitical
        #[arg(long = "type")]
        event_type: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Add a custom calendar event (geopolitical deadline, trade event, etc.)
    ///
    /// Custom events flow into `analytics catalysts` ranking alongside economic releases.
    ///
    /// Examples:
    ///   pftui data calendar add --date 2026-04-06 --name "Iran Hormuz Strait Deadline" --impact high --type geopolitical
    ///   pftui data calendar add --date 2026-05-01 --name "BRICS Summit" --impact medium --type geopolitical
    ///   pftui data calendar add --date 2026-04-15 --name "AAPL Earnings" --impact high --type earnings --symbol AAPL
    Add {
        /// Event date in YYYY-MM-DD format
        #[arg(long)]
        date: String,

        /// Event name/description
        #[arg(long)]
        name: String,

        /// Impact level: high, medium, low
        #[arg(long, default_value = "high")]
        impact: String,

        /// Event type: economic, earnings, geopolitical
        #[arg(long = "type", default_value = "geopolitical")]
        event_type: String,

        /// Associated symbol (e.g. AAPL for earnings)
        #[arg(long)]
        symbol: Option<String>,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Remove a calendar event by date and name
    ///
    /// Examples:
    ///   pftui data calendar remove --date 2026-04-06 --name "Iran Hormuz Strait Deadline"
    Remove {
        /// Event date in YYYY-MM-DD format
        #[arg(long)]
        date: String,

        /// Event name (must match exactly)
        #[arg(long)]
        name: String,

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
#[allow(clippy::large_enum_variant)] // Add carries the full journal-add discipline flag set
pub enum DataPredictionsCommand {
    /// Show prediction market contract odds from Polymarket (tag-based macro-relevant contracts)
    #[command(
        after_help = "Sources: Polymarket Gamma events API (fed, economics, geopolitics, politics, bitcoin, crypto, ai tags).\n\nWhen the enriched prediction_market_contracts table is populated (via `pftui refresh`), shows contracts with exchange, event grouping, liquidity, and end dates. Falls back to legacy predictions_cache when contracts table is empty.\n\nSee also: `data predictions stats`, `data predictions scorecard`, `data predictions unanswered`, `analytics predictions`"
    )]
    Markets {
        /// Filter by category: crypto, economics, geopolitics, ai, finance, macro
        #[arg(long)]
        category: Option<String>,

        /// Search question text/topics
        #[arg(long)]
        search: Option<String>,

        /// Curated geopolitics relevance filter: keyword-matched contracts only, excluding contracts resolving >12 months out, already past resolution, or with zero 24h volume
        #[arg(long, conflicts_with = "category")]
        geo: bool,

        /// Maximum number of markets to show (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Prediction accuracy statistics — hit rate by conviction, timeframe, symbol, and agent
    Stats {
        /// Filter by timeframe: low, medium, high, macro, macro-checkpoint
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

        /// Annotate wrong predictions with structured-lesson coverage
        #[arg(long = "lesson-coverage")]
        lesson_coverage: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List unanswered/pending predictions awaiting scoring
    Unanswered {
        /// Filter by timeframe: low, medium, high, macro, macro-checkpoint
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
        after_help = "Maps a Polymarket contract to a pftui scenario so that every\n`pftui data refresh` automatically logs the market probability as a\ndata point in the scenario's history timeline.\n\nUse --search to find contracts by keyword (matches question and event\ntitle). Use --scenario to specify the scenario name.\n\nUse --auto-suggest to scan every active scenario and emit the top 3\nmapping candidates per scenario, scored by keyword overlap and category\nfit. Combine with --scenario to restrict the scan to one scenario.\n\nUse --contract-id (alias of --contract) to map a specific contract.\n\nExamples:\n  pftui data predictions map --auto-suggest\n  pftui data predictions map --auto-suggest --scenario \"US Recession 2026\"\n  pftui data predictions map --scenario \"US Recession 2026\" --contract-id 0xabc...\n  pftui data predictions map --scenario \"US Recession 2026\" --search \"recession\"\n\nTo see all mappings:\n  pftui data predictions map --list\n\nSee also: `data predictions markets`, `data predictions suggest-mappings`,\n          `analytics scenario list`, `analytics calibration` (F55.5)"
    )]
    Map {
        /// Scenario name to link (must match an existing scenario)
        #[arg(long)]
        scenario: Option<String>,

        /// Search query to find a contract by question/event title
        #[arg(long)]
        search: Option<String>,

        /// Specific contract_id to link (alternative to --search). Alias: --contract-id
        #[arg(long, visible_alias = "contract-id")]
        contract: Option<String>,

        /// List all existing scenario-contract mappings
        #[arg(long)]
        list: bool,

        /// Auto-suggest top 3 mapping candidates per active scenario, scored by keyword overlap with Polymarket contract titles. Combine with --scenario to restrict to one scenario.
        #[arg(long = "auto-suggest")]
        auto_suggest: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Suggest high-relevance unmapped contracts for active scenarios
    #[command(
        name = "suggest-mappings",
        after_help = "Surfaces high-liquidity prediction market contracts that appear relevant\nto active scenarios but are not mapped yet.\n\nExamples:\n  pftui data predictions suggest-mappings\n  pftui data predictions suggest-mappings --scenario \"US Recession 2026\" --limit 3 --json\n\nSee also: `data predictions map`, `analytics calibration`, `analytics scenario list`"
    )]
    SuggestMappings {
        /// Restrict suggestions to one scenario name
        #[arg(long)]
        scenario: Option<String>,

        /// Maximum suggested contracts per scenario
        #[arg(long, default_value_t = 5)]
        limit: usize,

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
    /// Add a personal prediction (convenience alias for `journal prediction add`)
    #[command(
        after_help = "Creates a personal prediction in the journal database.\nThis is a convenience alias — identical to `pftui journal prediction add`,\nincluding the falsifiability discipline: omitting --falsify caps confidence\nat 0.3, the calibration-derived confidence clamp applies, and\n--override-confidence-cap requires --cap-rationale.\n\nTimeframe accepts: low, medium, high, macro, macro-checkpoint (aliases: short=low, long=high).\nConviction accepts: high, medium, low.\nUse either `--source-agent` or the shorter alias `--agent`.\nUse `--lessons` to record which structured lesson IDs informed the call.\nLOW analyst calls are capped at 5/hour unless `--override-cap` is passed.\nMACRO-CHECKPOINT predictions must embed `[thesis=<slug>]` in the claim;\nscoring a checkpoint Wrong fires a parent-thesis re-eval message to synthesis.\n\nExamples:\n  pftui analytics predictions add --claim \"BTC above 100k by June\" --timeframe medium --symbol BTC-USD --lessons 218,240\n  pftui data predictions add --claim \"Gold breaks 3000\" --timeframe high --conviction high --agent medium-agent\n  pftui analytics predictions add --claim \"VIX spikes above 30\" --timeframe low --confidence 0.8\n  pftui analytics predictions add --claim \"[thesis=de-dollarisation] CB gold > 800t by 2026-09-28\" --timeframe macro-checkpoint --agent analyst-macro --target-date 2026-09-28\n\nSee also: `journal prediction add`, `analytics predictions stats`,\n          `analytics predictions scorecard`, `analytics backtest`"
    )]
    Add {
        /// The prediction claim text
        #[arg(long, required = true)]
        claim: String,

        /// Asset symbol (e.g. BTC-USD, GC=F, TSLA)
        #[arg(long)]
        symbol: Option<String>,

        /// Conviction band: low, medium, or high.
        #[arg(long)]
        conviction: Option<String>,

        /// Analytics timeframe: low, medium, high, macro, macro-checkpoint (aliases: short=low, long=high)
        #[arg(long)]
        timeframe: Option<String>,

        /// Confidence score: 0.0 to 1.0
        #[arg(long)]
        confidence: Option<f64>,

        /// Source agent name (e.g. low-timeframe, evening-analyst)
        #[arg(long = "source-agent", visible_alias = "agent")]
        source_agent: Option<String>,

        /// Fixed news topic for source-accuracy attribution
        #[arg(long)]
        topic: Option<String>,

        /// News article ID from `pftui data news --json` when this prediction is derived from one article
        #[arg(long = "source-article-id")]
        source_article_id: Option<i64>,

        /// Target date for evaluation (YYYY-MM-DD)
        #[arg(long)]
        target_date: Option<String>,

        /// Criteria for determining if the prediction was correct
        #[arg(long = "resolution-criteria")]
        resolution_criteria: Option<String>,

        /// Comma-separated structured lesson IDs that informed this prediction
        #[arg(long)]
        lessons: Option<String>,

        /// Allow LOW analyst predictions beyond the soft 5-per-hour cap
        #[arg(long = "override-cap")]
        override_cap: bool,

        /// Skip the auto-preflight check before save (same semantics as
        /// `journal prediction add --skip-preflight`)
        #[arg(long = "skip-preflight")]
        skip_preflight: bool,

        /// Accept a blocking preflight finding and commit anyway
        #[arg(long = "accept-preflight")]
        accept_preflight: bool,

        /// Append a one-line serialized preflight block to the prediction's
        /// resolution_criteria
        #[arg(long = "inline")]
        inline: bool,

        /// Override the auto-preflight abort threshold (0..=100, default 50)
        #[arg(long = "preflight-threshold")]
        preflight_threshold: Option<u32>,

        /// Analyst layer for the calibration_adjustments lookup
        /// ("low" | "medium" | "high" | "macro"). Defaults to the resolved timeframe.
        #[arg(long = "layer")]
        layer: Option<String>,

        /// Compose and persist a write-time adversary view linked to the new
        /// prediction (same semantics as `journal prediction add --with-adversary`)
        #[arg(long = "with-adversary")]
        with_adversary: bool,

        /// Machine-scoreable success condition. Deterministic grammar:
        /// "<SYMBOL> <close|closes|stays|prints> <above|below|between|in-range|in-band>
        /// <value> [<value2>] by <YYYY-MM-DD>". Omitting --falsify (or a
        /// parse failure) caps confidence at 0.3 (unfalsifiable prediction).
        #[arg(long)]
        falsify: Option<String>,

        /// Bypass the calibration-derived confidence clamp. Requires --cap-rationale.
        #[arg(long = "override-confidence-cap")]
        override_confidence_cap: bool,

        /// Why the calibration confidence clamp does not apply (required
        /// with --override-confidence-cap)
        #[arg(long = "cap-rationale")]
        cap_rationale: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[allow(clippy::large_enum_variant)]
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
        /// Cash currency to debit/credit for paired cash leg
        #[arg(long = "cash-currency", default_value = "USD")]
        cash_currency: String,
        /// Do not insert the paired cash leg for buy/sell transactions
        #[arg(long = "no-auto-cash")]
        no_auto_cash: bool,
        /// Validate and preview the transaction impact without writing to the database
        #[arg(long)]
        dry_run: bool,
        /// Output JSON
        #[arg(long)]
        json: bool,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Remove a transaction by ID
    Remove {
        /// Transaction ID to remove
        id: i64,
        /// Remove only this transaction and leave its paired leg unlinked
        #[arg(long)]
        unpaired: bool,
        /// Preview the removal impact without deleting anything
        #[arg(long)]
        dry_run: bool,
        /// Output JSON
        #[arg(long)]
        json: bool,
    },
    /// List all transactions
    List {
        /// Show transaction notes column
        #[arg(long)]
        notes: bool,

        /// Show paired transaction ID column
        #[arg(long)]
        paired: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Heuristically pair unpaired pre-deployment transactions.
    ///
    /// For each unpaired buy on a non-cash symbol, find the closest USD
    /// sell within ±2 days and ±10% notional. Idempotent — only proposes
    /// pairs where BOTH legs currently have `paired_tx_id = NULL`.
    ///
    /// EXAMPLES:
    ///   pftui portfolio transaction repair-pairs --dry-run --json
    ///   pftui portfolio transaction repair-pairs --confirm
    ///   pftui portfolio transaction repair-pairs --skip 17 --confirm
    #[command(name = "repair-pairs")]
    RepairPairs {
        /// Preview proposed pairs without mutating the database (default).
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Apply proposed pairs to the database.
        #[arg(long)]
        confirm: bool,
        /// Exclude a specific transaction id from pairing (repeatable).
        #[arg(long = "skip")]
        skip: Vec<i64>,
        /// Maximum day delta for candidate sell (default: 2).
        #[arg(long = "max-days", default_value_t = 2)]
        max_days: i64,
        /// Maximum notional delta percentage (default: 10.0).
        #[arg(long = "max-notional-pct", default_value_t = 10.0)]
        max_notional_pct: f64,
        /// Output JSON
        #[arg(long)]
        json: bool,
    },
    /// Import a Delta tracker CSV export (full trade + fiat-flow history).
    ///
    /// The export is treated as the ground-truth ledger for the window it
    /// covers. SYNC-BASE-HOLDINGS fiat rows become native paired cash legs
    /// of their same-timestamp trade; trades without a sync partner import
    /// with no cash leg (the export's own DEPOSIT/WITHDRAW rows carry the
    /// funding); plain DEPOSIT/WITHDRAW rows become external transfer_in /
    /// transfer_out flows on the fiat symbol (USD/GBP), with same-window
    /// opposite-direction USD/GBP pairs annotated as fx conversions.
    /// Pre-existing hand-entered rows are reconciled against the CSV and
    /// classified SUPERSEDED (deleted on apply), KEPT, or CONFLICT.
    ///
    /// Idempotent: imported rows carry a [delta:<key>] notes marker;
    /// re-runs skip rows already present. DRY-RUN BY DEFAULT; --apply
    /// backs up the DB (full + transactions JSON) before any mutation and
    /// writes a journal-note audit trail (author system, section system).
    ///
    /// EXAMPLES:
    ///   pftui portfolio transaction import-delta export.csv --dry-run
    ///   pftui portfolio transaction import-delta export.csv --apply --json
    #[command(name = "import-delta")]
    ImportDelta {
        /// Path to the Delta export CSV
        csv: String,
        /// Preview the import without writing (default)
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Apply the import (backs up the database first)
        #[arg(long)]
        apply: bool,
        /// Output JSON
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

        /// Target floor percentage (e.g. "22", "10.5"). Accepts % suffix.
        #[arg(long)]
        floor: Option<String>,

        /// Target ceiling percentage (e.g. "30", "12.5"). Accepts % suffix.
        #[arg(long)]
        ceiling: Option<String>,

        /// Legacy midpoint allocation percentage. Accepts % suffix.
        #[arg(long)]
        target: Option<String>,

        /// Legacy drift band percentage (default: 2%). Accepts % suffix.
        #[arg(long, alias = "drift-band")]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ReportChartFormat {
    Svg,
    Png,
    Ascii,
    Html,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ReportBuildMode {
    Public,
    Private,
    Both,
}

#[derive(Subcommand)]
pub enum ReportBuildCommand {
    /// Assemble the daily intelligence report markdown from the populated DB
    #[command(
        name = "daily",
        after_help = "Assembles the daily intelligence report markdown by calling the registered\nsection renderers in canonical order. `--mode public` writes the public\nnewsletter; `--mode private` writes the operator decision document; `--mode\nboth` (default) writes both to their own destinations.\n\n`--dry-run` prints the section plan, data availability, output paths, and\nprivacy-audit status without writing files.\n\nDefault destinations:\n  public  ~/pftui/reports/daily-<DATE>.md\n  private /tmp/pftui-private-<DATE>.md\n\nExamples:\n  pftui report build daily\n  pftui report build daily --mode public --dry-run\n  pftui report build daily --mode both --date 2026-06-02 --out-dir /tmp/run\n  pftui report build daily --mode private --out-dir /tmp/private --json"
    )]
    Daily {
        /// Which report(s) to assemble
        #[arg(long, value_enum, default_value = "both")]
        mode: ReportBuildMode,

        /// Report date (YYYY-MM-DD). Defaults to today's UTC date.
        #[arg(long, value_name = "DATE")]
        date: Option<String>,

        /// Override BOTH output directories at once (public and private)
        #[arg(long, value_name = "DIR")]
        out_dir: Option<PathBuf>,

        /// Print the section plan, data availability, output paths, and
        /// privacy-audit status without writing anything
        #[arg(long)]
        dry_run: bool,

        /// Emit the dry-run plan / write outcome as JSON for agent consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ReportCommand {
    /// Render report chart primitives from JSON or canonical database queries
    #[command(
        name = "chart",
        after_help = "Available charts:\n  stacked-bar               Portfolio allocation stacked bar\n  prob-bar                  Scenario probability bar with 7-day ghost and delta\n  drift-bar                 Allocation drift bar with target tick and tolerance band\n  what-changed-strip        Since-last-report delta pill strip\n  open-predictions-table    Open prediction due-date table (HTML-native)\n  outlook-arrows            Days/weeks/months direction and conviction arrows\n  factor-exposure           Portfolio exposure bars by scenario factor\n  conviction-grid           Multi-timeframe analyst conviction grid\n  mismatch-card             Skylar-vs-analyst view mismatch card (HTML-native)\n  decision-card             Operator decision question card (HTML-native)\n  regime-quadrant           Growth-vs-inflation macro regime quadrant\n  conviction-trajectory     Per-asset analyst conviction sparkline\n  analyst-convergence-card  Per-asset analyst convergence evidence card (HTML-native)\n  calibration-reliability   Prediction reliability by layer and conviction band\n\nExamples:\n  pftui report chart stacked-bar --from-db portfolio --out allocation.svg\n  pftui report chart prob-bar --from-db \"Inflation Spike\" --format svg\n  pftui report chart drift-bar --from-db BTC --format svg\n  pftui report chart what-changed-strip --from-json deltas.json --json\n  pftui report chart open-predictions-table --from-db pending --format html --json\n  pftui report chart outlook-arrows --from-db BTC --json\n  pftui report chart factor-exposure --from-json factors.json --json\n  pftui report chart conviction-grid --from-db all --json\n  pftui report chart mismatch-card --from-json mismatch.json --format html --json\n  pftui report chart decision-card --from-json decision.json --format html --json\n  pftui report chart regime-quadrant --from-json regime.json --json\n  pftui report chart conviction-trajectory --from-db BTC --json\n  pftui report chart analyst-convergence-card --from-db \"Gold 30d\" --format html --json\n  pftui report chart calibration-reliability --from-db 90d --json\n  pftui report chart stacked-bar --from-json segments.json --format png --out allocation.png\n  pftui report chart prob-bar --from-json scenario.json --json"
    )]
    Chart {
        /// Chart name: stacked-bar, prob-bar, drift-bar, what-changed-strip, open-predictions-table, outlook-arrows, factor-exposure, conviction-grid, mismatch-card, decision-card, regime-quadrant, conviction-trajectory, analyst-convergence-card, or calibration-reliability
        chart_name: String,

        /// Render from a canonical DB query. stacked-bar accepts portfolio; prob-bar accepts a scenario name; drift-bar, outlook-arrows, conviction-grid, conviction-trajectory, and analyst-convergence-card accept a symbol; conviction-grid also accepts all/views; conviction-trajectory accepts an optional window token like "BTC 30d"; analyst-convergence-card accepts an optional since token like "Gold 30d"; calibration-reliability accepts a window token like "90d"; open-predictions-table accepts pending/open or a limit. factor-exposure, mismatch-card, decision-card, and regime-quadrant are JSON-only.
        #[arg(long = "from-db", value_name = "QUERY")]
        from_db: Option<String>,

        /// Render from a JSON file matching the chart input schema
        #[arg(long = "from-json", value_name = "FILE")]
        from_json: Option<PathBuf>,

        /// Write rendered output to a file instead of stdout
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,

        /// Output format
        #[arg(long, value_enum, default_value = "svg")]
        format: ReportChartFormat,

        /// Output command metadata as JSON
        #[arg(long)]
        json: bool,
    },

    /// Build assembled report markdown from registered section renderers
    #[command(
        name = "build",
        after_help = "Subcommands:\n  daily  Assemble the daily intelligence report (public/private/both)"
    )]
    Build {
        #[command(subcommand)]
        command: ReportBuildCommand,
    },
}

#[derive(Subcommand)]
pub enum PortfolioCommand {
    /// Consolidated portfolio snapshot: allocation, value, daily P&L, and unrealized gain in one call
    #[command(alias = "snapshot")]
    Status {
        /// Output JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
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
    /// Show current drawdown, max MTD/YTD drawdowns, and latest move decomposition
    Drawdown {
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
    #[command(
        name = "stress-test",
        after_help = "Run a named stress scenario against your portfolio to see the\nimpact. Use --list-scenarios to discover available preset names\nand active user-defined scenarios.\n\nBuilt-in presets: Oil $100, BTC 40k, Gold $6000, 2008 GFC,\n1973 Oil Crisis. Active scenarios from `analytics scenario list`\nare also available.\n\nSee also: analytics impact-matrix, analytics scenario list"
    )]
    StressTest {
        /// Scenario name (e.g. "2008 GFC", "Oil $100", "BTC 40k")
        scenario: Option<String>,

        /// List all available scenario names (built-in presets + active scenarios)
        #[arg(long)]
        list_scenarios: bool,

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
    #[command(
        name = "set-cash",
        after_help = "Destructive: this replaces every existing transaction for the currency with one new cash row.\nIf more than one row would be discarded, pass --confirm to apply the replace.\nUse --dry-run to preview the discarded rows without mutating the database.\n\nExamples:\n  pftui portfolio set-cash USD 45000 --dry-run\n  pftui portfolio set-cash USD 45000 --confirm\n  pftui portfolio set-cash USD 0 --confirm --json"
    )]
    SetCash {
        /// Currency symbol (e.g. USD, GBP, EUR)
        symbol: String,
        /// Amount to set (e.g. 45000, 12500.50). Use 0 to clear.
        amount: String,
        /// Apply replacement when more than one existing transaction would be discarded
        #[arg(long)]
        confirm: bool,
        /// Preview discarded transactions without mutating the database
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
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
pub enum DataSeriesCommand {
    /// Per-series last datapoint, age, and staleness vs its freshness SLA
    ///
    /// Reads the `series_registry` table (the L1 series catalog): each row
    /// names where a canonical series physically lives and the SLA it must
    /// meet. Glyphs: ok=within SLA, STALE=past SLA, DEAD?=past 2x SLA or no
    /// data at all (also surfaced by `pftui system doctor`).
    ///
    /// EXAMPLES:
    ///   pftui data series status
    ///   pftui data series status --json
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DataPricesCommand {
    /// Retro-scan price_history for suspect spike-and-revert prints (read-only)
    ///
    /// Flags bars whose close jumped >20% day-over-day AND reverted >15% on
    /// the next bar — the spike-and-revert signature of a corrupt print.
    /// Genuine crashes persist on the following bars and are NOT flagged.
    ///
    /// Read-only by design: auto-deleting historical rows from the canonical
    /// L1 series is more dangerous than reporting them (a genuine event
    /// misclassified would silently destroy real history every downstream
    /// engine trusts). Repair stays a manual, operator-reviewed DELETE.
    ///
    /// EXAMPLES:
    ///   pftui data prices audit
    ///   pftui data prices audit --symbol BTC-USD --json
    Audit {
        /// Limit the scan to one symbol (default: every symbol in price_history)
        #[arg(long)]
        symbol: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SystemCommand {
    /// Run as a background daemon (legacy — not required; the system runs via Claude Code + `data refresh`, whose tail fires all recurring scoring/snapshot/alert mechanisms)
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
    /// Back up the database (or export one table as JSON) to ~/pftui-archives
    ///
    /// Whole-DB mode uses SQLite `VACUUM INTO` — an atomic, consistent,
    /// compacted copy. Table mode (`--table X`) exports every row of one
    /// table as a JSON document. Archives always land OUTSIDE the repo.
    ///
    /// EXAMPLES:
    ///   pftui system archive-db
    ///   pftui system archive-db --out /Volumes/backup/pftui-20260611.db
    ///   pftui system archive-db --table journal
    #[command(name = "archive-db")]
    ArchiveDb {
        /// Destination path (default: ~/pftui-archives/pftui-backup-<timestamp>.db
        /// or <table>-<timestamp>.json in table mode)
        #[arg(long)]
        out: Option<String>,
        /// Export a single table as JSON instead of backing up the whole DB
        #[arg(long)]
        table: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Verify or repair SQLite schema drift before normal startup migrations run
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
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
    #[command(
        after_help = "Renders any view to text via an off-screen buffer — useful for docs,\nvisual review, and CI snapshots without an interactive terminal.\n\n--demo renders a self-contained SYNTHETIC portfolio (built in a temp dir,\nnever your real DB) so output is reproducible and safe to share.\n\nViews: positions, transactions, markets, economy, watchlist, analytics,\nnews, journal, risk-dashboard. --subtab selects a sub-tab (Risk Dashboard:\n0=Risk grid, 1=Basket, 2=Cycle, 3=Diversification).\n\nExamples:\n  pftui system snapshot --demo --view risk-dashboard --subtab 3 --plain\n  pftui system snapshot --demo --view positions --width 160 --height 50\n  pftui system snapshot --view analytics   # your real data, local only"
    )]
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

        /// View to render (positions, analytics, risk-dashboard, …; default: home tab)
        #[arg(long)]
        view: Option<String>,

        /// Sub-tab index within the view (Risk Dashboard: 0–3)
        #[arg(long)]
        subtab: Option<u8>,

        /// Render a self-contained synthetic demo portfolio (never touches your real DB)
        #[arg(long)]
        demo: bool,
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
    /// Report row counts for every enrichment table vs an expected minimum,
    /// surfacing "shipped but unpopulated" tables loudly.
    ///
    /// EXAMPLES:
    ///   pftui system data-coverage
    ///   pftui system data-coverage --json
    #[command(name = "data-coverage")]
    DataCoverage {
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
pub enum SchemaCommand {
    /// Verify the database schema against a freshly migrated pftui schema
    Verify {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Repair safe missing-table, missing-column, and missing-index drift
    Repair {
        /// Show planned SQL without mutating the database
        #[arg(long)]
        dry_run: bool,
        /// Apply the planned schema repair statements
        #[arg(long)]
        confirm: bool,
        /// Output as JSON
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
    ///   pftui journal entry add "Iran update" --tags iran,oil,geopolitical
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
        #[arg(long, action = clap::ArgAction::Append, help = "Tag for categorization (repeatable, e.g. --tag macro --tag btc).")]
        tag: Vec<String>,
        #[arg(long, help = "Comma-separated tags (e.g. macro,btc,trade).")]
        tags: Option<String>,
        #[arg(long, help = "Related asset symbol (e.g. BTC-USD, GC=F).")]
        symbol: Option<String>,
        #[arg(long, help = "Conviction level (e.g. high, medium, low).")]
        conviction: Option<String>,
        #[arg(
            long,
            help = "Entry author (e.g. skylar, analyst-low, analyst-medium, analyst-high, analyst-macro, analyst-evening, analyst-morning). Defaults to 'system'."
        )]
        author: Option<String>,
        #[arg(
            long,
            help = "Prepend the market snapshot line (see `pftui data snapshot-line`) so the entry self-contextualizes."
        )]
        stamp: bool,
        #[arg(long)]
        json: bool,
    },
    /// List journal entries with optional filters (date, tag, symbol, status, author)
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
        #[arg(long, help = "Filter by author (e.g. skylar, analyst-low).")]
        author: Option<String>,
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
#[allow(clippy::large_enum_variant)]
pub enum JournalPredictionCommand {
    /// Add a prediction. Timeframe accepts: low, medium, high, macro, macro-checkpoint (aliases: short=low, long=high).
    /// LOW analyst calls are capped at 5/hour unless --override-cap is passed.
    /// Prefer --claim flag for the prediction text; positional form kept for backwards compatibility.
    ///
    /// MACRO-CHECKPOINT predictions are short-horizon (≈90-day) falsifiable sub-claims attached
    /// to a multi-year MACRO thesis. The claim MUST embed `[thesis=<slug>]` so a failed
    /// checkpoint can surface a parent-thesis re-evaluation message to synthesis.
    ///
    /// Examples:
    ///   pftui journal prediction add --claim "BTC above 70k" --timeframe short --confidence 0.7
    ///   pftui journal prediction add "BTC above 70k" --timeframe short --confidence 0.7
    ///   pftui journal prediction add "Gold to 3000" medium 0.8
    ///   pftui journal prediction add --claim "[thesis=stage-6] By 2026-09-28 DXY above 95" \
    ///     --timeframe macro-checkpoint --target-date 2026-09-28 --agent analyst-macro
    Add {
        /// The prediction claim text (positional, backwards-compatible)
        value: Option<String>,
        /// The prediction claim text (named flag, preferred)
        #[arg(long)]
        claim: Option<String>,
        /// Timeframe (positional shorthand, backwards-compat): low|medium|high|macro|macro-checkpoint|short|long
        timeframe_pos: Option<String>,
        /// Confidence (positional shorthand): 0.0..=1.0
        confidence_pos: Option<f64>,
        #[arg(long)]
        symbol: Option<String>,
        /// Conviction band for calibration lookup: low, medium, or high.
        #[arg(long)]
        conviction: Option<String>,
        /// Analytics timeframe: low, medium, high, macro (aliases: short=low, long=high). Preferred over positional.
        #[arg(long)]
        timeframe: Option<String>,
        #[arg(long)]
        confidence: Option<f64>,
        #[arg(long = "source-agent", visible_alias = "agent")]
        source_agent: Option<String>,
        /// Fixed news topic for source-accuracy attribution: fed, inflation, geopolitics, commodities, crypto, equities, other
        #[arg(long)]
        topic: Option<String>,
        /// News article ID from `pftui data news --json` when this prediction is derived from one article
        #[arg(long = "source-article-id")]
        source_article_id: Option<i64>,
        #[arg(long)]
        target_date: Option<String>,
        #[arg(long = "resolution-criteria")]
        resolution_criteria: Option<String>,
        /// Comma-separated structured lesson IDs that informed this prediction
        #[arg(long)]
        lessons: Option<String>,
        /// Allow LOW analyst predictions beyond the soft 5-per-hour cap
        #[arg(long = "override-cap")]
        override_cap: bool,
        /// Skip the auto-preflight check before save. By default `add` runs
        /// `prediction preflight` and aborts when the score meets the
        /// abort threshold (default 50). Pass --skip-preflight to bypass
        /// entirely; pass --accept-preflight to acknowledge a blocking
        /// score and commit anyway.
        #[arg(long = "skip-preflight")]
        skip_preflight: bool,
        /// Accept a blocking preflight finding and commit the prediction
        /// regardless. The preflight findings are still recorded when
        /// `--inline` is also passed.
        #[arg(long = "accept-preflight")]
        accept_preflight: bool,
        /// Append a one-line serialized preflight block to the prediction's
        /// `resolution_criteria` so the substrate it was checked against is
        /// a permanent part of the record.
        #[arg(long = "inline")]
        inline: bool,
        /// Override the auto-preflight abort threshold (0..=100, default 50).
        #[arg(long = "preflight-threshold")]
        preflight_threshold: Option<u32>,
        /// Analyst layer for the calibration_adjustments lookup
        /// ("low" | "medium" | "high" | "macro"). Defaults to the resolved
        /// timeframe when not provided.
        #[arg(long = "layer")]
        layer: Option<String>,
        /// Compose a write-time adversary view from the substrate
        /// (anti-pattern reasoning_fragments, top-3 lessons from the
        /// highest co-failing cluster, derived falsification triggers),
        /// persist it to `adversary_views` linked to the new prediction
        /// id, and append a compact `[adversary] ...` summary line to the
        /// prediction's resolution_criteria. Companion to --inline.
        #[arg(long = "with-adversary")]
        with_adversary: bool,
        /// Machine-scoreable success condition for this prediction — the
        /// condition that, if met, scores the prediction CORRECT.
        /// Deterministic grammar (no LLM):
        /// "<SYMBOL> <close|closes|stays|prints> <above|below|between|in-range|in-band>
        /// <value> [<value2>] by <YYYY-MM-DD>".
        /// close-*/prints-* = at least one daily close beyond the threshold
        /// inside the window (prints-* uses daily closes; intraday data is
        /// unavailable). stays-* = every daily close inside the window must
        /// satisfy the condition (scoreable only after the window ends).
        /// Examples: --falsify "BTC close below 50000 by 2026-09-30",
        /// --falsify "BTC stays in-range 45000 85000 by 2026-12-31".
        /// A parse failure records an unstructured (non-auto-scoreable)
        /// rule and caps confidence at 0.3; omitting --falsify entirely
        /// also caps confidence at 0.3 (unfalsifiable prediction).
        #[arg(long)]
        falsify: Option<String>,
        /// Bypass the calibration-derived confidence clamp. Requires
        /// --cap-rationale; the rationale is appended to the prediction's
        /// resolution_criteria as "[cap-override: <text>]".
        #[arg(long = "override-confidence-cap")]
        override_confidence_cap: bool,
        /// Why the calibration confidence clamp does not apply to this
        /// prediction (required with --override-confidence-cap).
        #[arg(long = "cap-rationale")]
        cap_rationale: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Compose a write-time adversary "case against" the supplied draft
    /// prediction. Classifies the claim into a `cluster_key`, then returns
    /// the anti-pattern `reasoning_fragments` reachable from the cluster,
    /// the top-3 lessons of the highest co-failing cluster, and derived
    /// falsification triggers. No live LLM call.
    ///
    /// Examples:
    ///   pftui journal prediction adversary --claim "SPY gamma pin at 700" \
    ///     --symbol SPY --timeframe low --conviction medium --layer low --json
    ///   pftui journal prediction adversary --claim "Gold above 4500 by July" \
    ///     --symbol GLD --timeframe medium --conviction high --json
    Adversary {
        /// The draft prediction claim text
        #[arg(long)]
        claim: String,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        timeframe: Option<String>,
        /// Conviction band: low, medium, or high.
        #[arg(long)]
        conviction: Option<String>,
        /// Analyst layer for symmetry with `preflight`. Carried through to
        /// the persisted adversary view but does not change the composition
        /// today.
        #[arg(long)]
        layer: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Pre-flight check: classify the draft prediction into a cluster and
    /// surface the substrate's view (calibration_adjustments, applicable
    /// reasoning_fragments, top-3 similar past predictions, top co-failing
    /// cluster, scenario_prediction_links distribution, most-similar
    /// falsification rule, and a 0..=100 preflight_score).
    ///
    /// Examples:
    ///   pftui journal prediction preflight --claim "Gold above 4500 by July 15" \
    ///     --symbol GLD --timeframe medium --conviction high --layer low --json
    ///   pftui journal prediction preflight --claim "SPY 700 gamma pin" --inline
    Preflight {
        /// The draft prediction claim text
        #[arg(long)]
        claim: String,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        timeframe: Option<String>,
        /// Conviction band for calibration lookup: low, medium, or high.
        #[arg(long)]
        conviction: Option<String>,
        /// Analyst layer for the calibration_adjustments lookup
        /// ("low" | "medium" | "high" | "macro"). Defaults to --timeframe.
        #[arg(long)]
        layer: Option<String>,
        /// News topic for the calibration_adjustments lookup
        /// ("fed","inflation","geopolitics","commodities","crypto","equities",
        /// "other"). When omitted, falls back to a cluster-derived topic.
        #[arg(long)]
        topic: Option<String>,
        /// Render a one-line preflight summary suitable for embedding into
        /// a prediction's reasoning. Implies --json off.
        #[arg(long)]
        inline: bool,
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
        /// Filter by timeframe: low, medium, high, macro, macro-checkpoint
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
        #[arg(long = "lesson-coverage")]
        lesson_coverage: bool,
        #[arg(long)]
        json: bool,
    },
    /// Auto-score predictions mechanically from structured falsification rules
    /// and price_history daily closes. A rule encodes the claim's SUCCESS
    /// CONDITION: close-*/prints-* rules score CORRECT on the first qualifying
    /// daily close inside the window and WRONG once the window expires without
    /// one; stays-* rules score WRONG on the first violating close and CORRECT
    /// only after the window expires clean. Already-scored predictions are
    /// never overwritten (use --force to allow). Also runs automatically as a
    /// tail step of `pftui data refresh`.
    #[command(name = "auto-score", alias = "autoscore", visible_alias = "score-auto")]
    AutoScore {
        /// Only evaluate rules whose eval_date_end is on or after this YYYY-MM-DD date
        #[arg(long)]
        since: Option<String>,
        /// Preview what would be scored without writing changes
        #[arg(long)]
        dry_run: bool,
        /// Minimum parser confidence required for scoring
        #[arg(long = "confidence-floor", default_value = "medium")]
        confidence_floor: PredictionConfidenceFloorArg,
        /// Allow autoscore to overwrite already-scored predictions
        #[arg(long)]
        force: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Audit legacy LLM-scored prediction outcomes against the mechanical
    /// falsification scorer. Re-derives every scored outcome (correct/partial/
    /// wrong) that lacks `auto-scored:` provenance by evaluating its
    /// falsification rule (stored row, or the claim/resolution_criteria
    /// re-parsed through the falsify grammar) against price_history daily
    /// closes, then reports agreement, the by-layer breakdown, the
    /// recorded-vs-mechanical confusion matrix, and every disagreement with
    /// the deciding bar's date+close. `recorded=correct but mechanically
    /// wrong` is the generosity measure; the reverse is harshness.
    ///
    /// Dry by default. --apply-high-confidence flips disagreeing outcomes
    /// ONLY where the rule parsed at high confidence, the deciding close is
    /// more than 1% from the threshold, and the price series was unaffected by the
    /// corruption-repair windows (BTC equity-ticker 2025-03-20→2026-02-27 +
    /// 2026-06-11 stale stamp, FX placeholder series, frozen agri feeds).
    /// Every flip APPENDS provenance to score_notes — the original outcome
    /// is preserved in the note. After applying, rebuild the calibration
    /// matrix: `pftui analytics calibration-matrix rebuild --since 365`.
    #[command(name = "rescore-audit")]
    RescoreAudit {
        /// Apply gated outcome corrections (see command help for the gates)
        #[arg(long = "apply-high-confidence")]
        apply_high_confidence: bool,
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
    ///   pftui journal prediction lessons --unresolved --json
    ///   pftui journal prediction lessons --miss-type timing --json
    ///   pftui journal prediction lessons add --prediction-id 42 --miss-type directional \
    ///     --what-happened "BTC dropped to 60k" --why-wrong "Ignored macro headwinds"
    Lessons {
        #[command(subcommand)]
        command: Option<JournalPredictionLessonsCommand>,
        /// Filter by miss type: directional, timing, magnitude
        #[arg(long = "miss-type")]
        miss_type: Option<String>,
        /// Show only wrong predictions that still have no structured lesson
        #[arg(long)]
        unresolved: bool,
        /// Maximum lessons to show
        #[arg(long)]
        limit: Option<usize>,
        /// Include retired/superseded lessons in the output (default: active only).
        /// The analyst lesson book renders active lessons only so retired
        /// rows do not crowd the context window; pass this flag to surface
        /// the full history for review.
        #[arg(long = "include-retired")]
        include_retired: bool,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, ValueEnum)]
pub enum PredictionConfidenceFloorArg {
    Medium,
    High,
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
    /// Add many lessons from a JSON file in one run
    ///
    /// File format: JSON array of objects with fields:
    ///   prediction_id, miss_type, what_happened, why_wrong, signal_misread(optional)
    Bulk {
        /// Path to a JSON file containing lesson entries
        #[arg(long)]
        input: Option<String>,
        /// Generate stub lesson templates from unresolved wrong predictions
        #[arg(long = "auto-stub")]
        auto_stub: bool,
        /// Skip predictions that already have structured lessons
        #[arg(long)]
        unresolved: bool,
        /// Preview what would be written without changing the database
        #[arg(long)]
        dry_run: bool,
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
        #[arg(
            long,
            help = "Note author (e.g. skylar, analyst-low, analyst-medium, analyst-high, analyst-macro, analyst-evening, analyst-morning, analyst-brief). Defaults to 'system'."
        )]
        author: Option<String>,
        #[arg(
            long,
            help = "Prepend the market snapshot line (see `pftui data snapshot-line`) so the note self-contextualizes."
        )]
        stamp: bool,
        #[arg(long)]
        json: bool,
    },
    /// List narrative notes with optional filters (since, author)
    List {
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, help = "Filter by author (e.g. skylar, analyst-low).")]
        author: Option<String>,
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
    /// Surface repeated note clusters ("you have written this note 9 times")
    ///
    /// Clusters an author's recent notes by mutual character-trigram Jaccard
    /// similarity ≥ 0.85 and prints the top repeated clusters (count,
    /// first/last date, 100-char excerpt). Repetition is the signal that a
    /// conclusion should be consolidated into the thesis table instead of
    /// being re-derived every run.
    ///
    /// EXAMPLES:
    ///   pftui journal notes repetition --author analyst-medium --json
    ///   pftui journal notes repetition --days 60
    Repetition {
        /// Restrict to one author (e.g. analyst-low). Omitting it clusters
        /// every author's notes separately (clusters never span authors).
        #[arg(long)]
        author: Option<String>,
        /// Lookback window in days
        #[arg(long, default_value = "30")]
        days: i64,
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
    #[command(
        after_help = "Probability updates are ledger-disciplined:\n  --evidence is REQUIRED for any --probability change\n  --proposer identifies the updating layer (default: synthesis)\n  cumulative |Δprobability| per scenario per day is capped at 5pp;\n    a hard data print bypasses the cap via --hard-print \"<event>\"\n  a same-day update by a DIFFERENT proposer requires --override-conflict\n\nExamples:\n  pftui journal scenario update \"Inflation Resurgence\" --probability 30 \\\n    --evidence \"CPI 2026-06-10 printed 2.4% vs 2.6% expected\" --proposer analyst-medium\n  pftui journal scenario update \"Inflation Resurgence\" --probability 38 \\\n    --evidence \"hot CPI + 5y5y breakevens +20bp\" --hard-print \"CPI 2026-06-10 print\""
    )]
    Update {
        #[arg(required_unless_present = "id")]
        value: Option<String>,
        #[arg(long)]
        id: Option<i64>,
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
        /// Evidence behind a probability move (REQUIRED with --probability)
        #[arg(long)]
        evidence: Option<String>,
        /// Layer proposing this update (default: synthesis)
        #[arg(long)]
        proposer: Option<String>,
        /// Hard data print justifying a daily-delta-cap (5pp) bypass
        #[arg(long = "hard-print")]
        hard_print: Option<String>,
        /// Acknowledge a same-day update by a different proposer
        #[arg(long = "override-conflict")]
        override_conflict: bool,
        #[arg(long)]
        json: bool,
    },
    /// Set a scenario's reference-class base rate (anchors probability vs history)
    #[command(
        name = "set-base-rate",
        after_help = "Records the reference-class frequency a scenario probability should be\nanchored against. `scenario list` then shows the deviation\n(probability − base_rate).\n\nExample:\n  pftui journal scenario set-base-rate \"US Equities Up Year\" --rate 70 \\\n    --reference \"US equities up-years frequency 1950-2025 ~70%\""
    )]
    SetBaseRate {
        /// Scenario name
        value: String,
        /// Base rate percentage (0-100)
        #[arg(long)]
        rate: f64,
        /// Reference class description (e.g. "US recessions per decade 1950-2025")
        #[arg(long)]
        reference: String,
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
    /// Probability timeline: all active scenarios' probability evolution over time
    Timeline {
        /// Lookback window in days (default: all history)
        #[arg(long)]
        days: Option<u32>,
        /// Output as JSON for agent/script consumption
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
    /// Operator replies — structured per-decision responses to reports
    Replies {
        #[command(subcommand)]
        command: JournalRepliesCommand,
    },
}

#[derive(Subcommand)]
pub enum JournalRepliesCommand {
    /// List operator replies
    List {
        #[arg(long = "report-date")]
        report_date: Option<String>,
        #[arg(long)]
        asset: Option<String>,
        #[arg(long = "decision-type")]
        decision_type: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Add a new operator reply
    Add {
        #[arg(long = "report-date")]
        report_date: String,
        #[arg(long = "reply-date")]
        reply_date: Option<String>,
        #[arg(long)]
        asset: Option<String>,
        #[arg(long = "decision-type")]
        decision_type: String,
        #[arg(long = "response-class")]
        response_class: String,
        #[arg(long = "conviction-implied")]
        conviction_implied: Option<String>,
        #[arg(long = "horizon")]
        horizon: Option<String>,
        #[arg(long = "reasoning")]
        reasoning: Option<String>,
        #[arg(long = "raw-content")]
        raw_content: String,
        #[arg(long = "journal-id")]
        journal_id: Option<i64>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsBasketCommand {
    /// Compute risk-aware portfolio weights for a basket of assets
    Weights {
        /// Comma-separated assets (alias or ticker), e.g. "BTC,gold,SPY"
        #[arg(long)]
        assets: String,
        /// Allocation scheme: equal | inverse-vol | risk-parity | downside-risk-parity
        #[arg(long, default_value = "risk-parity")]
        method: String,
        /// Lookback window in trading days (0 = all common history)
        #[arg(long, default_value = "0")]
        lookback: usize,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
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
        /// Filter by severity: severe (|Δ|≥0.70), moderate (≥0.50), minor (<0.50)
        #[arg(long)]
        severity: Option<String>,
        /// Auto-create recurring technical correlation_break alerts for each detected break pair
        #[arg(long = "seed-alerts")]
        seed_alerts: bool,
        /// Cooldown in minutes for seeded alerts (default: 240)
        #[arg(long, default_value = "240")]
        cooldown: i64,
        /// Enrich breaks with historical context: trend direction, duration, and recent snapshots
        #[arg(long)]
        verbose: bool,
        /// Number of historical snapshots per break pair when --verbose (default: 7)
        #[arg(long, default_value = "7")]
        history_depth: usize,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsAlertsCommand {
    /// Add an alert rule
    #[command(
        after_help = "Cycle-bottom signal conditions (evaluated mechanically on `data refresh`):\n  Confluence threshold — fires when met/7 reaches a target on a timeframe:\n    pftui analytics alerts add --kind technical --symbol BTC-USD \\\n      --condition cycle_bottom_monthly_4\n  Single criterion — fires when one named composite criterion becomes met:\n    pftui analytics alerts add --kind technical --symbol BTC-USD \\\n      --condition cycle_criterion_weekly_trend_line_reclaimed\n  Single component — fires when one atomic subcondition becomes met:\n    pftui analytics alerts add --kind technical --symbol BTC-USD \\\n      --condition cycle_component_monthly_erf_turned_up\n\n  Criterion keys: momentum_turning_up, momentum_above_price, dss_bottoming,\n    roofing_confirming_up, volatility_bands_bullish, reversal_dots,\n    trend_line_reclaimed.\n  Component keys: rsi_ma_turned_up, rsi_ma_cross_above_rsi, dss_turned_up,\n    dss_cross_above_trigger, dss_oversold, erf_bottom_zone, erf_turned_up,\n    erf_positive, cyberbands_bullish, cyberdots_bullish, cyberline_reclaim,\n    pi_cycle_bottom.\n\nCycle-TOP signal conditions (symmetric mirror — evaluated on `data refresh`):\n  Confluence threshold:  --condition cycle_top_monthly_4\n  Single criterion:      --condition cycle_top_criterion_weekly_trend_line_lost\n  Single component:      --condition cycle_top_component_monthly_erf_turned_down\n\n  Criterion keys: momentum_turning_down, momentum_below_price, dss_topping,\n    roofing_confirming_down, volatility_bands_bearish, reversal_dots_bearish,\n    trend_line_lost.\n  Component keys: rsi_ma_turned_down, rsi_ma_cross_below_rsi, dss_turned_down,\n    dss_cross_below_trigger, dss_overbought, erf_top_zone, erf_turned_down,\n    erf_negative, cyberbands_bearish, cyberdots_bearish, cyberline_lost,\n    pi_cycle_top.\n\n  Timeframes: daily | weekly | monthly.\n  One-shot by default (fires once per transition; re-arm to re-enable). Add\n  --recurring --cooldown-minutes N to auto-rearm with a cooldown floor."
    )]
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
        /// Emit the created alert as a JSON envelope (incl. the load-bearing id)
        #[arg(long)]
        json: bool,
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
        /// Only show newly triggered alerts (filters out armed/acknowledged/previously triggered)
        #[arg(long = "newly-triggered")]
        newly_triggered: bool,
        /// Filter by alert kind: price, technical, macro, allocation, indicator, ratio
        #[arg(long)]
        kind: Option<String>,
        /// Filter by condition name (e.g. correlation_break, correlation_regime_break, scenario_probability_shift)
        #[arg(long)]
        condition: Option<String>,
        /// Filter by symbol or symbol pair (e.g. BTC-USD, BTC-USD:GC=F)
        #[arg(long)]
        symbol: Option<String>,
        /// Filter by alert status: armed, triggered, acknowledged
        #[arg(long)]
        status: Option<String>,
        /// Filter by urgency tier: critical, high, watch, low
        #[arg(long)]
        urgency: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Acknowledge one or more alerts by ID, or bulk-ack by filter
    #[command(
        after_help = "Acknowledge by ID:\n  pftui analytics alerts ack 1 2 3\n\nBulk-acknowledge all triggered:\n  pftui analytics alerts ack --all-triggered\n\nBulk-acknowledge with filters:\n  pftui analytics alerts ack --all-triggered --condition correlation_break\n  pftui analytics alerts ack --all-triggered --kind macro\n  pftui analytics alerts ack --all-triggered --symbol GC=F\n  pftui analytics alerts ack --all-triggered --kind price --symbol BTC --json"
    )]
    Ack {
        /// One or more alert IDs to acknowledge
        #[arg(conflicts_with_all = ["all_triggered", "ack_condition", "ack_kind", "ack_symbol"])]
        ids: Vec<i64>,

        /// Acknowledge ALL triggered alerts (optionally filtered by --condition/--kind/--symbol)
        #[arg(long = "all-triggered", id = "all_triggered")]
        all_triggered: bool,

        /// Filter bulk-ack by condition (e.g. correlation_break, price_above_sma200)
        #[arg(long = "condition", id = "ack_condition", requires = "all_triggered")]
        condition: Option<String>,

        /// Filter bulk-ack by alert kind (price, technical, macro, scenario, ratio)
        #[arg(long = "kind", id = "ack_kind", requires = "all_triggered")]
        kind: Option<String>,

        /// Filter bulk-ack by symbol (e.g. GC=F, BTC)
        #[arg(long = "symbol", id = "ack_symbol", requires = "all_triggered")]
        symbol: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Rearm alert by ID
    Rearm { id: i64 },
    /// Seed a default smart-alert set for current holdings + core macro conditions
    SeedDefaults,
    /// Triage dashboard: prioritize, group, and score all alerts by urgency
    #[command(
        after_help = "Groups alerts into urgency tiers:\n\n  🔴 CRITICAL  Newly triggered — needs immediate attention\n  🟠 HIGH      Previously triggered, not yet acknowledged\n  🟡 WATCH     Armed and within 5% of threshold\n  🟢 LOW       Armed but far from threshold\n\nSummary stats by kind (price/technical/macro/scenario/ratio)\nand actionability scoring.\n\nSee also: analytics alerts check, analytics alerts list"
    )]
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
        /// Include recent evidence and asset impacts inline (enriched output for faster synthesis)
        #[arg(long)]
        verbose: bool,
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
    /// Set the market regime with confidence and drivers.
    ///
    /// Valid regime labels: risk-on, risk-off, crisis, stagflation,
    /// transitioning, deflation, reflation, goldilocks
    ///
    /// Accepted aliases: transition -> transitioning; risk_on / risk off
    ///
    /// Examples:
    ///   pftui analytics macro regime set risk-on --confidence 0.8 --drivers "VIX compressed, S&P ATH"
    ///   pftui analytics macro regime set stagflation --confidence 0.7
    ///   pftui analytics macro regime set transitioning
    Set {
        /// Regime label. Valid values: risk-on, risk-off, crisis, stagflation,
        /// transitioning, deflation, reflation, goldilocks
        regime: String,
        #[arg(long, help = "Conviction score 0.0–1.0 (default: auto-classified)")]
        confidence: Option<f64>,
        #[arg(
            long,
            help = "Comma-separated list of key drivers (e.g. 'VIX compressed, S&P ATH')"
        )]
        drivers: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show regime classification history over time
    History {
        #[arg(long)]
        limit: Option<usize>,
        /// Filter: only show snapshots on or after this date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// Filter: only show snapshots on or before this date (YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show regime transitions: when the regime changed and what triggered it
    Transitions {
        #[arg(long)]
        limit: Option<usize>,
        /// Filter: only show transitions on or after this date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// Filter: only show transitions on or before this date (YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Summary statistics: time spent in each regime, transition counts, durations
    Summary {
        /// Filter: only include snapshots on or after this date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// Filter: only include snapshots on or before this date (YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Temporarily override the macro regime classification with manual expiry.
    ///
    /// Use on fast-moving event days (ceasefire, rate decision, earnings surprise) when
    /// the automated signal-based classification lags reality by hours. The override
    /// auto-expires after the specified duration so normal classification resumes.
    ///
    /// Examples:
    ///   pftui analytics macro regime override risk-on --reason "Iran ceasefire April 17" --expires 4h
    ///   pftui analytics macro regime override stagflation --reason "hot CPI + war premium" --expires 24h
    ///   pftui analytics macro regime override crisis --reason "naval blockade VIX spike" --expires 2h
    ///
    /// Duration format: 30m, 4h, 12h, 24h, 2d
    /// To cancel an active override: pftui analytics macro regime override --clear
    #[command(name = "override")]
    Override {
        /// Regime label. Valid values: risk-on, risk-off, crisis, stagflation,
        /// transitioning, deflation, reflation, goldilocks
        regime: Option<String>,
        /// Human-readable reason for the override (logged for audit trail)
        #[arg(long)]
        reason: Option<String>,
        /// How long the override lasts before auto-expiry (e.g. 4h, 30m, 24h, 2d)
        #[arg(long, default_value = "4h")]
        expires: String,
        /// Cancel (clear) any active regime override and resume normal classification
        #[arg(long)]
        clear: bool,
        #[arg(long)]
        json: bool,
    },
    /// Confidence trend: how regime confidence has evolved over time with direction and stability
    #[command(
        name = "confidence-trend",
        after_help = "Shows how regime confidence has evolved over time. Computes a moving average\n(default 5-point) to smooth noise, identifies the trend direction\n(strengthening, weakening, stable), and calculates stability metrics.\n\nUseful for detecting whether the current regime is consolidating or about to\ntransition. A declining confidence trend often precedes regime changes.\n\nExamples:\n  pftui analytics macro regime confidence-trend --json\n  pftui analytics macro regime confidence-trend --window 10 --from 2026-03-01\n  pftui analytics macro regime confidence-trend --limit 50\n\nSee also: analytics macro regime history, analytics regime-transitions"
    )]
    ConfidenceTrend {
        /// Number of recent snapshots to include (default: all in range)
        #[arg(long)]
        limit: Option<usize>,
        /// Moving average window size (default: 5)
        #[arg(long, default_value = "5")]
        window: usize,
        /// Filter: only include snapshots on or after this date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// Filter: only include snapshots on or before this date (YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsMacroLogCommand {
    /// Add a structured macro log entry
    Add {
        /// Development text (positional). Alternative: use --development.
        value: Option<String>,
        /// Development text (named flag). Overrides positional value if both given.
        #[arg(long)]
        development: Option<String>,
        /// Date for the log entry (YYYY-MM-DD). Defaults to today.
        #[arg(long)]
        date: Option<String>,
        /// How this development changes cycle interpretation.
        #[arg(long = "cycle-impact", visible_alias = "impact")]
        cycle_impact: Option<String>,
        /// How this development changes macro outcome probabilities.
        #[arg(long = "outcome-shift", visible_alias = "outcome")]
        outcome_shift: Option<String>,
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
    #[command(
        after_help = "This command is read-only: it shows current macro outcome probabilities.\n\nTo change a scenario probability, use:\n  pftui journal scenario update \"Scenario Name\" --probability 65\n  pftui journal scenario update --id 42 --probability 65\n\nSee also: analytics scenario list, journal scenario history"
    )]
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
        #[command(subcommand)]
        command: Option<AnalyticsMacroLogCommand>,
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
pub enum AnalyticsRealRatesCommand {
    /// Compute US-vs-G10 sovereign 10Y differentials from cached real-yield rows
    Differentials {
        /// Window expressed as NNd/NNw/NNm or YYYY-MM-DD (default: 7d)
        #[arg(long, default_value = "7d")]
        since: String,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
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
    #[command(
        after_help = "Valid severity values:\n  low       Minor update; watch but no immediate action.\n  normal    Default event/update severity.\n  elevated  Important development; likely follow-up needed.\n  critical  Urgent development with immediate portfolio or scenario impact.\n\nExample:\n  pftui analytics situation update log --situation \"Iran Escalation\" --headline \"Brent above 95\" --severity elevated"
    )]
    Log {
        #[arg(long)]
        situation: String,
        #[arg(long)]
        headline: String,
        #[arg(long)]
        detail: Option<String>,
        #[arg(long, default_value = "normal", value_parser = ["low", "normal", "elevated", "critical"])]
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
    /// Auto-populate timeframe scores from existing regime, scenario, trend, and cycle data.
    /// Derives LOW/MEDIUM/HIGH/MACRO scores so the situation engine returns non-empty results
    /// without requiring manual setup. Safe to call repeatedly — upserts on each run.
    Populate {
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
    #[command(
        after_help = "Probability updates are ledger-disciplined:\n  --evidence is REQUIRED for any --probability change\n  --proposer identifies the updating layer (default: synthesis)\n  cumulative |Δprobability| per scenario per day is capped at 5pp;\n    a hard data print bypasses the cap via --hard-print \"<event>\"\n  a same-day update by a DIFFERENT proposer requires --override-conflict"
    )]
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
        /// Evidence behind a probability move (REQUIRED with --probability)
        #[arg(long)]
        evidence: Option<String>,
        /// Layer proposing this update (default: synthesis)
        #[arg(long)]
        proposer: Option<String>,
        /// Hard data print justifying a daily-delta-cap (5pp) bypass
        #[arg(long = "hard-print")]
        hard_print: Option<String>,
        /// Acknowledge a same-day update by a different proposer
        #[arg(long = "override-conflict")]
        override_conflict: bool,
        #[arg(long)]
        json: bool,
    },
    /// Set a scenario's reference-class base rate (anchors probability vs history)
    #[command(name = "set-base-rate")]
    SetBaseRate {
        /// Scenario name
        value: String,
        /// Base rate percentage (0-100)
        #[arg(long)]
        rate: f64,
        /// Reference class description (e.g. "US recessions per decade 1950-2025")
        #[arg(long)]
        reference: String,
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
    /// Detect high-impact news/catalyst clusters that may warrant new scenarios
    #[command(
        after_help = "Scans recent news sentiment plus upcoming catalysts to suggest new macro\nscenarios before they are added manually.\n\nThis command is suggestion-only. It never writes scenarios automatically.\nUse the emitted `journal scenario add ...` command after review.\n\nExamples:\n  pftui analytics scenario detect\n  pftui analytics scenario detect --hours 48 --limit 5 --json\n\nSee also: analytics catalysts, analytics news-sentiment, journal scenario add"
    )]
    Detect {
        /// Lookback window for recent news items (default: 72h)
        #[arg(long, default_value = "72")]
        hours: i64,
        /// Maximum suggestions to return
        #[arg(long, default_value = "5")]
        limit: usize,
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
    /// Probability timeline: all active scenarios' probability evolution over time
    #[command(
        after_help = "Shows how each active scenario's probability has evolved over time,\nwith daily-deduplicated data points and net change.\n\nDesigned for agent consumption — one JSON call returns the complete\nprobability landscape across all scenarios.\n\nUse --days to limit the lookback window.\n\nExample:\n  pftui analytics scenario timeline --days 14 --json\n\nSee also: analytics scenario history, analytics scenario list"
    )]
    Timeline {
        /// Lookback window in days (default: all history)
        #[arg(long)]
        days: Option<u32>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsEpistemicsCommand {
    /// Record (upsert) a run's epistemic-health metrics for a date
    #[command(
        after_help = "Upserts the run_health row for --date. Metrics the orchestrator\ncomputes are passed as flags; metrics Rust can derive itself are\ncomputed automatically when omitted:\n  blind_divergence      from same-day analyst_views (blind vs canonical mean)\n  scenario_delta_total  from today's scenario_updates probability ledger\n\nExample:\n  pftui analytics epistemics record --date 2026-06-10 \\\n    --agreement 0.72 --panel-dispersion 6.4 --fallback-warnings 2 \\\n    --audit-pass-rate 0.92 --agents 14 --notes \"full both-mode run\""
    )]
    Record {
        /// Run date (YYYY-MM-DD)
        #[arg(long)]
        date: String,
        /// Share of voices agreeing with the operator stance (0-1)
        #[arg(long)]
        agreement: Option<f64>,
        /// Mean |house conviction − blind conviction| across held assets (derived from analyst_views when omitted)
        #[arg(long = "blind-divergence")]
        blind_divergence: Option<f64>,
        /// Stddev of panel persona confidences
        #[arg(long = "panel-dispersion")]
        panel_dispersion: Option<f64>,
        /// Share of this run's notes that are novel (0-1)
        #[arg(long)]
        novelty: Option<f64>,
        /// Count of empty-state fallbacks hit during the run
        #[arg(long = "fallback-warnings")]
        fallback_warnings: Option<i64>,
        /// Sum of |Δprobability| across scenarios today (derived from scenario_updates when omitted)
        #[arg(long = "scenario-delta-total")]
        scenario_delta_total: Option<f64>,
        /// Accuracy-audit claims_passed/claims_total (0-1)
        #[arg(long = "audit-pass-rate")]
        audit_pass_rate: Option<f64>,
        /// Number of agents spawned during the run
        #[arg(long)]
        agents: Option<i64>,
        /// Free-form run notes
        #[arg(long)]
        notes: Option<String>,
        /// Max |Pearson r| between any canonical layer's conviction trajectory and the matching held asset's closes (derived from analyst_view_history × price_history over 90d when omitted)
        #[arg(long = "conviction-price-corr")]
        conviction_price_corr: Option<f64>,
        /// Overall scored direction-hit rate 0-1 (derived from the trailing 30d of forecast_scores when omitted)
        #[arg(long = "forecast-hit-rate")]
        forecast_hit_rate: Option<f64>,
        /// Count of active forecast misalignments (derived from forecast_misalignments when omitted)
        #[arg(long = "active-misalignments")]
        active_misalignments: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Show one run's health row with threshold flags
    Show {
        /// Run date (YYYY-MM-DD); defaults to the latest recorded run
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Trend table across recorded runs
    History {
        /// Maximum rows (newest first; default 30)
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// House-vs-rival scoreboard: scored-prediction hit rates by source agent
    #[command(
        after_help = "Compares scored user_predictions (outcome != pending) grouped by\nsource_agent — the antithesis rival ledger vs the canonical analyst-*\nlayers. Renders an accrual notice while the antithesis layer still has\nonly pending predictions."
    )]
    Rivalry {
        #[arg(long)]
        json: bool,
    },
    /// Conviction-vs-price correlation per (canonical layer × held asset)
    #[command(
        name = "conviction-price",
        after_help = "Pearson correlation between each canonical layer's signed conviction\ntrajectory (analyst_view_history, bear counts negative) and the asset's\ncloses on matching dates over the window. Needs ≥6 paired observations\nper pair; |r| > 0.6 flags \"momentum dressed as structure\" (standing\nrule 15: conviction must not track price — if conviction is just a\nlagged price chart, it adds no information).\n\nThe max |r| across pairs is what `epistemics record` self-derives into\nrun_health.conviction_price_corr when --conviction-price-corr is omitted.\n\nExamples:\n  pftui analytics epistemics conviction-price --json\n  pftui analytics epistemics conviction-price --days 60 --asset GC=F"
    )]
    ConvictionPrice {
        /// Trailing window in days (default 90)
        #[arg(long, default_value_t = 90)]
        days: i64,
        /// Restrict to one asset (default: every held asset)
        #[arg(long)]
        asset: Option<String>,
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
        ///
        /// Accepted both as `--reasoning <text>` and the longer alias
        /// `--reasoning-summary <text>` used by routine prompts.
        #[arg(long, alias = "reasoning-summary")]
        reasoning: String,
        /// Supporting data points
        ///
        /// Accepted both as `--evidence <text>` and the alias `--key-evidence <text>`.
        #[arg(long, alias = "key-evidence")]
        evidence: Option<String>,
        /// What could invalidate this view
        #[arg(long = "blind-spots")]
        blind_spots: Option<String>,
        /// Suggested allocation bias relative to long-run target weight.
        /// Valid: overweight, slight-overweight, at-target, slight-underweight, underweight.
        #[arg(long = "allocation-bias")]
        allocation_bias: Option<String>,
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
    /// Portfolio-aware view matrix: includes all held + watchlisted assets alongside viewed assets
    ///
    /// Shows every asset the user holds or watches, plus any asset with existing analyst views.
    /// Assets without views show '—' for that analyst column, surfacing coverage gaps.
    ///
    /// EXAMPLES:
    ///   pftui analytics views portfolio-matrix --json
    PortfolioMatrix {
        #[arg(long)]
        json: bool,
    },
    /// Show how analyst views on an asset have evolved over time
    ///
    /// Displays the chronological history of every view update for the given asset.
    /// Use --analyst to filter to a single timeframe layer. Tracks conviction drift
    /// and direction flip points.
    ///
    /// EXAMPLES:
    ///   pftui analytics views history --asset BTC --json
    ///   pftui analytics views history --asset GLD --analyst high --json
    ///   pftui analytics views history --asset BTC --limit 20 --json
    History {
        /// Asset symbol to show history for (required)
        #[arg(long)]
        asset: String,
        /// Filter by analyst layer: low, medium, high, macro
        #[arg(long)]
        analyst: Option<String>,
        /// Maximum entries to show (default: all)
        #[arg(long)]
        limit: Option<usize>,
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
    /// Surface assets where analysts strongly disagree — ranked by divergence magnitude
    ///
    /// Finds assets where the gap between the most bullish and most bearish analyst
    /// conviction scores is largest. These are the interesting signals: LOW says bear -3
    /// but HIGH says bull +4 means the timeframes are seeing different things.
    ///
    /// EXAMPLES:
    ///   pftui analytics views divergence --json
    ///   pftui analytics views divergence --min-spread 3 --json
    ///   pftui analytics views divergence --asset BTC --json
    ///   pftui analytics views divergence --layer high --json
    ///   pftui analytics views divergence --limit 5 --json
    Divergence {
        /// Minimum conviction spread to include (default: 2)
        #[arg(long = "min-spread", default_value = "2")]
        min_spread: i64,
        /// Filter to a specific asset
        #[arg(long)]
        asset: Option<String>,
        /// Filter to divergences where one extreme is this analyst layer: low, medium, high, macro
        #[arg(long = "layer")]
        layer: Option<String>,
        /// Maximum results to show
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Deterministic convergence aggregation across analyst views for a single asset
    ///
    /// Aggregates all analyst views for one asset within the lookback window,
    /// computes summary statistics (n_views, avg/min/max conviction, divergence,
    /// allocation-bias distribution), and assigns a deterministic `summary`
    /// classification (insufficient-views / divergent / neutral-with-divergence /
    /// strong-convergent-bull / convergent-bull / convergent-neutral /
    /// convergent-bear / strong-convergent-bear).
    ///
    /// EXAMPLES:
    ///   pftui analytics views convergence --asset GC=F --json
    ///   pftui analytics views convergence --asset BTC --since 48h --json
    Convergence {
        /// Asset symbol (e.g. BTC, GC=F)
        #[arg(long)]
        asset: String,
        /// Lookback window for views (24h, 7d, 2w, 1m). Default: 7d.
        #[arg(long, default_value = "7d")]
        since: String,
        #[arg(long)]
        json: bool,
    },
    /// Convergence aggregation for every asset with views in the window
    ///
    /// EXAMPLES:
    ///   pftui analytics views convergence-all --json
    ///   pftui analytics views convergence-all --since 7d --json
    #[command(name = "convergence-all")]
    ConvergenceAll {
        /// Lookback window for views (24h, 7d, 2w, 1m). Default: 7d.
        #[arg(long, default_value = "7d")]
        since: String,
        #[arg(long)]
        json: bool,
    },
    /// Per-analyst accuracy: how often each timeframe's directional calls are correct
    ///
    /// Compares historical analyst views against actual price movements.
    /// Each analyst's calls are evaluated over a timeframe-appropriate window:
    /// LOW=3 days, MEDIUM=14 days, HIGH=30 days, MACRO=90 days.
    /// Bull calls that see price rise are correct; bear calls that see price fall are correct.
    /// Neutral calls are skipped. Only calls whose evaluation window has fully elapsed are scored.
    ///
    /// EXAMPLES:
    ///   pftui analytics views accuracy --json
    ///   pftui analytics views accuracy --analyst low --json
    ///   pftui analytics views accuracy --asset BTC --json
    Accuracy {
        /// Filter to a specific analyst layer: low, medium, high, macro
        #[arg(long)]
        analyst: Option<String>,
        /// Filter to a specific asset symbol
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Stale-view detector: held assets whose layer view is old AND whose price has moved
    ///
    /// For each held asset (net positive transactions) and each canonical
    /// layer (low/medium/high/macro): flags the layer's latest view when it
    /// is older than --days AND the asset's price has moved more than
    /// --move-pct percent since the view's updated_at (per price_history).
    /// "View may be stale: evidence moved, conviction didn't."
    ///
    /// EXAMPLES:
    ///   pftui analytics views stale --json
    ///   pftui analytics views stale --days 14 --move-pct 5
    Stale {
        /// Age threshold in days (a view younger than this is never stale)
        #[arg(long, default_value = "21")]
        days: i64,
        /// Price-move threshold in percent since the view was written
        #[arg(long = "move-pct", default_value = "10")]
        move_pct: f64,
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
pub enum AnalyticsNewsSourcesCommand {
    /// Per-source prediction accuracy, optionally focused by domain/topic
    ///
    /// EXAMPLES:
    ///   pftui analytics news-sources accuracy --json
    ///   pftui analytics news-sources accuracy --domain bloomberg.com --topic fed --json
    Accuracy {
        /// Source domain to inspect, e.g. bloomberg.com
        #[arg(long)]
        domain: Option<String>,
        /// Fixed topic or alias, e.g. fed, inflation, iran, btc
        #[arg(long)]
        topic: Option<String>,
        /// Restrict to predictions scored in the last N days
        #[arg(long = "window-days")]
        window_days: Option<i64>,
        /// Emit an explicit notice that historical predictions before the
        /// `source_article_id` column landed are NOT retroactively attributed
        /// to a source. The accuracy ledger populates forward from feature
        /// deployment only.
        #[arg(long = "include-pre-deployment")]
        include_pre_deployment: bool,
        #[arg(long)]
        json: bool,
    },
    /// Rank sources by historical hit rate for one topic
    ///
    /// EXAMPLES:
    ///   pftui analytics news-sources rank --topic iran --json
    ///   pftui analytics news-sources rank --topic fed --limit 10
    Rank {
        /// Fixed topic or alias, e.g. fed, inflation, iran, btc
        #[arg(long)]
        topic: Option<String>,
        /// Trailing scoring window in days
        #[arg(long = "window-days", default_value_t = 180)]
        window_days: i64,
        /// Maximum sources to return
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Replay `sync_prediction_outcome` for every scored prediction with a
    /// `source_article_id`, rebuilding `news_source_accuracy` from scratch.
    /// Idempotent — re-running produces no double-counting.
    ///
    /// EXAMPLES:
    ///   pftui analytics news-sources rebuild-accuracy --since 180d --json
    ///   pftui analytics news-sources rebuild-accuracy --since 365d --dry-run --json
    #[command(name = "rebuild-accuracy")]
    RebuildAccuracy {
        /// Lookback window: Nh / Nd / Nw / Nm — only predictions scored within
        /// the window are replayed. Default: all scored predictions.
        #[arg(long)]
        since: Option<String>,
        /// Preview the scan without mutating the ledger.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsRiskFactorsCommand {
    /// Add or update a risk-factor exposure mapping for a held asset.
    Add {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        factor: String,
        /// long / short — direction of the asset's exposure to the factor
        #[arg(long, default_value = "long")]
        direction: String,
        /// Exposure multiplier (1.0 = baseline, >1.0 amplified, <1.0 muted)
        #[arg(long, default_value = "1.0")]
        exposure: f64,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List all mappings, optionally filtered to one symbol.
    List {
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Delete a mapping.
    Delete {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        factor: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsCalibrationMatrixCommand {
    /// Rebuild the calibration_matrix rows from user_predictions outcomes.
    ///
    /// Deletes existing rows and re-inserts one row per
    /// (timeframe, topic, conviction_band) tuple with (n, hit_rate,
    /// stated_confidence) computed over the trailing `--since` days.
    Rebuild {
        /// Trailing window in days for predictions to include
        #[arg(long, default_value = "365")]
        since: i64,
        #[arg(long)]
        json: bool,
    },
    /// Show the current calibration_matrix rows.
    List {
        #[arg(long)]
        layer: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsNewsSilenceCommand {
    /// Re-compute per-(topic, day-of-week) baselines from the trailing
    /// `--since` window of `news_cache` rows.
    ///
    /// EXAMPLES:
    ///   pftui analytics news-silence rebuild-baselines --since 90d --json
    #[command(name = "rebuild-baselines")]
    RebuildBaselines {
        /// Lookback window: Nh / Nd / Nw / Nm (default: 90d)
        #[arg(long, default_value = "90d")]
        since: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsAlignmentCommand {
    /// Today's operator-vs-analyst alignment score (computed on demand if not yet stored)
    ///
    /// Aggregates the per-asset gap between Skylar's stated views (journal entries
    /// authored 'skylar' last 14d + operator_replies if present) and the analyst
    /// convergence per held asset, weighted by allocation. Score is 0-100.
    ///
    /// EXAMPLES:
    ///   pftui analytics alignment current --json
    Current {
        #[arg(long)]
        json: bool,
    },
    /// Time-series of stored alignment scores
    ///
    /// EXAMPLES:
    ///   pftui analytics alignment history --json
    ///   pftui analytics alignment history --since 90d --json
    ///   pftui analytics alignment history --since 2026-01-01 --json
    History {
        /// Lookback window (Nd, Nw, Nm) or YYYY-MM-DD anchor. Default: 90d.
        #[arg(long, default_value = "90d")]
        since: String,
        #[arg(long)]
        json: bool,
    },
    /// Recompute the score for one date (optionally store + emit drift alert)
    ///
    /// EXAMPLES:
    ///   pftui analytics alignment compute --date 2026-06-01 --json
    ///   pftui analytics alignment compute --date 2026-06-01 --store --json
    Compute {
        /// Date to compute (YYYY-MM-DD). Default: today.
        #[arg(long)]
        date: Option<String>,
        /// Persist the computed row to `alignment_score_history` and run the
        /// drift-alert check against the recent history window.
        #[arg(long)]
        store: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsTechnicalsCommand {
    /// Full standard-indicator panel (momentum/trend/volume/volatility) with a
    /// bull/bear scorecard — Stochastic, Williams %R, CCI, ROC, ADX/DMI, MACD,
    /// OBV, MFI, ATR, Bollinger %b, computed on the fly.
    Indicators {
        /// Symbol to analyze (alias or ticker, e.g. BTC, GC=F, SPY)
        symbol: String,
        #[arg(long)]
        json: bool,
    },
    /// Pure price-action market-structure read: swing highs/lows, trend
    /// classification (uptrend/downtrend/range), break-of-structure events,
    /// MA posture + extension. Computed straight from price_history.
    Structure {
        /// Symbol to analyze (e.g. GC=F, BTC, SPY)
        symbol: String,
        /// Bar timeframe: daily, weekly, or monthly (weekly/monthly bars
        /// aggregated from daily history)
        #[arg(long, default_value = "daily")]
        timeframe: String,
        #[arg(long)]
        json: bool,
    },
    /// Composite Cyber Dots read — faithful port of the operator's
    /// PineScript indicator (docs/reference/cyber-dots.pine): Gaussian
    /// CyberBands QB state, Zone bands, CyberLine (VIDYA/Donchian/hybrid),
    /// strength dots, Bollinger reversals, Pi Cycle top/bottom, MTF RSI
    /// zones, and hybrid breakout arrows, with a one-line composite verdict
    /// and a dated recent-signal list.
    Cyber {
        /// Symbol to analyze (e.g. BTC, GC=F, SPY). `BTC` falls back to the
        /// deep `BTC-USD` series automatically.
        symbol: String,
        /// Bar timeframe: daily, weekly, or monthly (weekly/monthly bars
        /// aggregated from daily history; Pi Cycle always runs on daily closes)
        #[arg(long, default_value = "daily")]
        timeframe: String,
        /// Number of most-recent dated signal events to list
        #[arg(long = "lookback-signals", default_value_t = 10)]
        lookback_signals: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsCyclesCommand {
    /// Cycle-position read (BTC halving/4yr cycle, gold ~6.9yr cycle (8yr is folklore)).
    /// Position only — never a price prediction.
    #[command(
        after_help = "Examples:\n  pftui analytics cycles clock\n  pftui analytics cycles clock --asset BTC --json\n  pftui analytics cycles clock --asset GC=F\n\nDefault (no --asset) prints both BTC and gold clocks. --json emits a\n{btc, gold, note} envelope with snake_case fields."
    )]
    Clock {
        /// Restrict to one asset: BTC or GC=F (default: both)
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Deterministic multi-degree cycle-theory report: cycle lows per
    /// degree, low-to-low timing bands (P15-P85), translation ledger,
    /// FLD/VTL state, failed-cycle + half-cycle + inversion flags, nesting
    /// clarity. Timing only — a window, never a date, never a price
    /// prediction. Doctrine: docs/CYCLE-THEORY.md
    Analyze {
        /// Symbol/asset to analyze, positional (BTC falls back to deep BTC-USD).
        /// May also be given as --asset for consistency with hurst/avwap/regime-break.
        symbol: Option<String>,
        /// Asset (alias for the positional symbol; e.g. BTC, gold, GC=F)
        #[arg(long)]
        asset: Option<String>,
        /// Restrict output to one degree (e.g. daily, investor, 4-year,
        /// intermediate, major)
        #[arg(long)]
        degree: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Mechanical cycle-bottom signal suite: a deterministic confluence of
    /// independent cycle-low confirmations, each at its natural timeframe.
    /// Position/measurement only — never a price prediction.
    #[command(
        after_help = "Scores 7 composite cycle-bottom criteria, each at its natural timeframe and\nchecked on the latest bar (N/7 confluence):\n  1. Momentum line turning up                 the RSI's moving average ticked up\n  2. Momentum line above price momentum       the RSI average reclaimed the RSI\n  3. Double-smoothed stochastic bottoming     stochastic ticked up AND crossed its trigger (oversold = context)\n  4. Roofing filter confirming up             de-trended cycle filter in bottom zone (<0) AND ticked up\n  5. Volatility bands bullish (daily)         daily momentum bands in the bullish state\n  6. Significant reversal dots (wk/mo)        weekly/monthly strength dots net-bullish\n  7. Trend line reclaimed (weekly)            price reclaimed the weekly trackline\n  bonus: pi-cycle bottom (daily)              fired recently — reported, NOT counted in the 7\n\nThe momentum/stochastic/roofing criteria run on the --timeframe (default monthly);\nthe band/dot/line/pi criteria always run on their own natural aggregation. The\nJSON includes `core_watch[]` for the four monthly cycle-watch items, plus every\ncomposite's atomic `components[]`. Numeric components include previous/current\ncomparison fields and signed `distance_to_trigger` when available.\n\nExamples:\n  pftui analytics cycles bottom-signals --asset BTC\n  pftui analytics cycles bottom-signals --asset BTC --timeframe monthly --json\n  pftui analytics cycles bottom-signals --asset gold --timeframe weekly"
    )]
    BottomSignals {
        /// Symbol/asset, positional (BTC falls back to deep BTC-USD).
        symbol: Option<String>,
        /// Asset (alias for the positional symbol; e.g. BTC, gold, GC=F)
        #[arg(long)]
        asset: Option<String>,
        /// Timeframe for the RSI/stochastic/roofing criteria: monthly (default), weekly, or daily
        #[arg(long, default_value = "monthly")]
        timeframe: String,
        #[arg(long)]
        json: bool,
        /// Reliability backtest: measure each criterion's lead/lag + hit-rate
        /// vs the verified cycle-low anchors (no-lookahead, point-in-time).
        #[command(subcommand)]
        sub: Option<BottomSignalsCommand>,
    },
    /// Mechanical cycle-TOP / cycle-high exhaustion signal suite: the symmetric
    /// mirror of bottom-signals — a deterministic confluence of independent
    /// cycle-TOP confirmations, each at its natural timeframe.
    /// Position/measurement only.
    #[command(
        after_help = "Scores 7 composite cycle-top / cycle-high criteria, each at its natural timeframe and\nchecked on the latest bar (N/7 confluence) — the inverted mirror of bottom-signals:\n  1. Momentum line turning down               the RSI's moving average ticked down\n  2. Momentum line below price momentum        the RSI average crossed below the RSI\n  3. Double-smoothed stochastic topping        stochastic ticked down AND crossed below its trigger (overbought = context)\n  4. Roofing filter confirming down            de-trended cycle filter in top zone (>0) AND ticked down\n  5. Volatility bands bearish (daily)          daily momentum bands in the bearish state\n  6. Significant exhaustion/reversal dots (wk/mo) weekly/monthly strength dots net-bearish\n  7. Trend line lost (weekly)                  price lost the weekly trackline\n  bonus: pi-cycle top (daily)                  fired recently — reported, NOT counted in the 7\n\nThe momentum/stochastic/roofing criteria run on the --timeframe (default monthly);\nthe band/dot/line/pi criteria always run on their own natural aggregation.\n\nExamples:\n  pftui analytics cycles top-signals --asset BTC\n  pftui analytics cycles top-signals --asset BTC --timeframe monthly --json\n  pftui analytics cycles top-signals backtest --asset BTC --json"
    )]
    TopSignals {
        /// Symbol/asset, positional (BTC falls back to deep BTC-USD).
        symbol: Option<String>,
        /// Asset (alias for the positional symbol; e.g. BTC, gold, GC=F)
        #[arg(long)]
        asset: Option<String>,
        /// Timeframe for the RSI/stochastic/roofing criteria: monthly (default), weekly, or daily
        #[arg(long, default_value = "monthly")]
        timeframe: String,
        #[arg(long)]
        json: bool,
        /// Reliability backtest vs completed native cycle highs AND forward-
        /// return expectancy vs price-structure swing HIGHS (no-lookahead,
        /// point-in-time), plus the flexible trigger-backtest event study.
        #[command(subcommand)]
        sub: Option<TopSignalsCommand>,
    },
    /// Dashboard of every armed cycle-signal alert (cycle-BOTTOM and
    /// cycle-TOP), with per-signal detail: decoded label/timeframe/polarity/
    /// target, armed-at + recurring/cooldown, fired-yet + last-fired +
    /// time-since + fire count, and a fast CURRENT LIVE READ (met N/7 for a
    /// confluence rule, met/unmet + distance for a criterion/component).
    /// Status view only — never runs the backtest.
    #[command(
        after_help = "Lists every alert whose condition is a cycle signal — confluence threshold\n(cycle_bottom_<tf>_<N> / cycle_top_<tf>_<N>), single criterion, or single\ncomponent — in either polarity, with its live state and firing history.\n\nThe live read is computed once per (asset, timeframe, polarity) and reused\nacross rules that share it; assets with no price history degrade gracefully\n(\"no price history\") rather than erroring. Signal metadata + counts only — no\ndollar values.\n\nExamples:\n  pftui analytics cycles tracked\n  pftui analytics cycles tracked --asset BTC --json\n  pftui analytics cycles tracked --polarity top"
    )]
    Tracked {
        /// Restrict to one asset (e.g. BTC, gold, GC=F). BTC matches BTC-USD.
        #[arg(long)]
        asset: Option<String>,
        /// Restrict to one side: top | bottom (default: both)
        #[arg(long)]
        polarity: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Translation ledger for one degree: per completed cycle the length,
    /// top position, LT/MID/RT class, and failed flag
    #[command(
        after_help = "Examples:\n  pftui analytics cycles ledger BTC --degree 4-year\n  pftui analytics cycles ledger --asset GC=F --degree major --json\n\nDegrees come from `cycles analyze` (daily, investor, 4-year, intermediate,\nmajor). Each row: cycle length, top position, LT/MID/RT translation class,\nfailed flag."
    )]
    Ledger {
        /// Symbol/asset, positional. May also be given as --asset.
        symbol: Option<String>,
        /// Asset (alias for the positional symbol; e.g. BTC, gold, GC=F)
        #[arg(long)]
        asset: Option<String>,
        /// Degree name (e.g. daily, investor, 4-year, intermediate, major)
        #[arg(long)]
        degree: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum BottomSignalsCommand {
    /// Reliability backtest of the 7 cycle-bottom criteria (+ N/7 confluence)
    /// against the verified cycle-low anchors: per-criterion hit-rate /
    /// precision, signed lead/lag distribution, coverage / recall, and
    /// false-positive count. Point-in-time (no lookahead). Honest about the
    /// tiny anchor count — emits a small_n caveat.
    #[command(
        after_help = "Measures, for each of the 7 composite criteria and the N/7 confluence at\nthresholds >=3 / >=4 / >=5, how reliably the signal LEADS a verified cycle low.\n\nMethod (no lookahead): at each historical bar i the engine reads ONLY\nhistory[..=i]; a criterion 'fires' on the rising edge (newly true). Each firing\nis matched to the nearest verified low within +/- the match window:\n  precision (hit-rate)  fraction of firings near a real low\n  lead/lag              signed days fired->low (negative = led the low); median + range\n  coverage (recall)     fraction of known lows the criterion flagged in-window\n  false positives       firings with no nearby low\n\nEvaluation cadence is serialized as eval_stride_days: daily timeframe evaluates\nevery bar; weekly/monthly use a weekly daily-bar cadence.\n\nHONESTY: there are only ~3 documented lows per asset; a 3-sample hit-rate is\nNOT robust. The result carries small_n / insufficient_anchors flags.\n\nExamples:\n  pftui analytics cycles bottom-signals backtest --asset BTC --json\n  pftui analytics cycles bottom-signals backtest --asset gold --timeframe weekly --window 120"
    )]
    Backtest {
        /// Symbol/asset, positional (BTC falls back to deep BTC-USD).
        symbol: Option<String>,
        /// Asset (alias for the positional symbol; e.g. BTC, gold, GC=F)
        #[arg(long)]
        asset: Option<String>,
        /// Timeframe for the RSI/stochastic/roofing criteria: monthly (default), weekly, or daily
        #[arg(long, default_value = "monthly")]
        timeframe: String,
        /// Match window in DAYS (+/-) around a verified low for a firing to count as a hit
        #[arg(long)]
        window: Option<i64>,
        /// Also compute the asset-agnostic forward-return EXPECTANCY block:
        /// mean/median/positive-rate forward returns at 30/90/180/365d per
        /// confluence threshold and per criterion, expectancy LIFT vs the
        /// unconditioned baseline, and closeness (days + price-%) to the
        /// nearest price-structure swing low. Works for any symbol with history.
        #[arg(long)]
        expectancy: bool,
        /// DRIFT-DETRENDED expectancy: report each forward return as EXCESS over
        /// the asset's trailing-365d local drift (isolates the signal's edge
        /// from secular/time-local trend). Implies --expectancy; drops bars
        /// without a full trailing year (smaller sample). Default off → raw.
        #[arg(long)]
        detrend: bool,
        #[arg(long)]
        json: bool,
    },
    /// Flexible event-study backtest for arbitrary criterion/component trigger
    /// combinations, including forward returns at custom horizons.
    #[command(
        name = "trigger-backtest",
        after_help = "Backtests arbitrary cycle-low trigger combinations. Keys can be either\ncomposite criteria (e.g. momentum_above_price, dss_bottoming) or atomic\ncomponents (e.g. rsi_ma_cross_above_rsi, dss_cross_above_trigger,\ndss_turned_up). The trigger fires on the false->true edge of the combined\ncondition, then reports timing and price distance to the nearest verified cycle\nlow plus forward returns at each requested horizon.\n\nExamples:\n  pftui analytics cycles bottom-signals trigger-backtest --asset BTC \\\n    --trigger rsi_ma_cross_above_rsi --horizons 7d,30d,365d --json"
    )]
    TriggerBacktest {
        /// Symbol/asset, positional (BTC falls back to deep BTC-USD).
        symbol: Option<String>,
        /// Asset (alias for the positional symbol; e.g. BTC, gold, GC=F)
        #[arg(long)]
        asset: Option<String>,
        /// Criterion/component key(s). Repeat or comma-separate.
        #[arg(long = "trigger", required = true)]
        triggers: Vec<String>,
        /// Combination mode for multiple triggers: all or any
        #[arg(long, default_value = "all")]
        mode: String,
        /// Forward-return horizons, comma-separated; supports d/w/m/y suffixes
        #[arg(long, default_value = "7d,30d,365d")]
        horizons: String,
        /// Timeframe for the RSI/stochastic/roofing criteria: monthly (default), weekly, or daily
        #[arg(long, default_value = "monthly")]
        timeframe: String,
        /// Match window in DAYS (+/-) around a verified low for a firing to count as a hit
        #[arg(long)]
        window: Option<i64>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum TopSignalsCommand {
    /// Forward-return expectancy backtest of the 7 cycle-top criteria (+ N/7
    /// confluence) vs asset-agnostic price-structure swing HIGHS. Point-in-time
    /// (no lookahead). A good top signal precedes a DECLINE, so the headline
    /// hit-rate is the NEGATIVE forward-return rate. Tops have NO doctrine
    /// anchors — this is price-structure-only; honest about it.
    #[command(
        after_help = "Measures, for each of the 7 composite top criteria and the N/7 confluence at\nthresholds >=3 / >=4 / >=5, the forward-return expectancy after a top signal fires.\n\nMethod (no lookahead): at each historical bar i the engine reads ONLY\nhistory[..=i]; a criterion 'fires' on the rising edge (newly true). Forward\nreturns at 30/90/180/365d are measured AFTER each firing:\n  mean/median forward return   a good top precedes a decline (negative)\n  negative_rate_pct            fraction of firings followed by a DECLINE = top hit-rate\n  lift vs baseline             mean minus the unconditioned same-horizon baseline (negative = good)\n  closeness                    days + price-% to the nearest price-structure swing high\n\nUNLIKE bottom-signals there are NO documented doctrine TOP anchors (doctrine\nanchors are cycle LOWS), so the reliability section is empty and the expectancy\nblock (price-structure swing highs) carries the read. Honest small_n caveat.\n\nExamples:\n  pftui analytics cycles top-signals backtest --asset BTC --expectancy --json\n  pftui analytics cycles top-signals backtest --asset gold --timeframe weekly --window 120 --expectancy"
    )]
    Backtest {
        /// Symbol/asset, positional (BTC falls back to deep BTC-USD).
        symbol: Option<String>,
        /// Asset (alias for the positional symbol; e.g. BTC, gold, GC=F)
        #[arg(long)]
        asset: Option<String>,
        /// Timeframe for the RSI/stochastic/roofing criteria: monthly (default), weekly, or daily
        #[arg(long, default_value = "monthly")]
        timeframe: String,
        /// Match window in DAYS (+/-) around a swing high for closeness matching
        #[arg(long)]
        window: Option<i64>,
        /// Compute the forward-return EXPECTANCY block (mean/median/negative-rate
        /// forward returns at 30/90/180/365d per confluence threshold and per
        /// criterion, lift vs the unconditioned baseline, and closeness to the
        /// nearest price-structure swing high). Works for any symbol with history.
        #[arg(long)]
        expectancy: bool,
        /// DRIFT-DETRENDED expectancy: report each forward return as EXCESS over
        /// the asset's trailing-365d local drift (isolates the signal's edge
        /// from secular/time-local trend). Implies --expectancy; drops bars
        /// without a full trailing year (smaller sample). Default off → raw.
        #[arg(long)]
        detrend: bool,
        #[arg(long)]
        json: bool,
    },
    /// Flexible event-study backtest for arbitrary criterion/component trigger
    /// combinations, including forward returns at custom horizons.
    #[command(
        name = "trigger-backtest",
        after_help = "Backtests arbitrary cycle-low trigger combinations. Keys can be either\ncomposite criteria (e.g. momentum_above_price, dss_bottoming) or atomic\ncomponents (e.g. rsi_ma_cross_above_rsi, dss_cross_above_trigger,\ndss_turned_up). The trigger fires on the false->true edge of the combined\ncondition, then reports timing and price distance to the nearest verified cycle\nlow plus forward returns at each requested horizon.\n\nExamples:\n  pftui analytics cycles bottom-signals trigger-backtest --asset BTC \\\n    --trigger rsi_ma_cross_above_rsi --horizons 7d,30d,365d --json\n  pftui analytics cycles bottom-signals trigger-backtest --asset BTC \\\n    --trigger rsi_ma_cross_above_rsi,dss_cross_above_trigger,dss_turned_up --mode all --json\n  pftui analytics cycles bottom-signals trigger-backtest --asset gold \\\n    --trigger dss_bottoming --timeframe monthly --horizons 30d,180d,365d"
    )]
    TriggerBacktest {
        /// Symbol/asset, positional (BTC falls back to deep BTC-USD).
        symbol: Option<String>,
        /// Asset (alias for the positional symbol; e.g. BTC, gold, GC=F)
        #[arg(long)]
        asset: Option<String>,
        /// Criterion/component key(s). Repeat or comma-separate.
        #[arg(long = "trigger", required = true)]
        triggers: Vec<String>,
        /// Combination mode for multiple triggers: all or any
        #[arg(long, default_value = "all")]
        mode: String,
        /// Forward-return horizons, comma-separated; supports d/w/m/y suffixes
        #[arg(long, default_value = "7d,30d,365d")]
        horizons: String,
        /// Timeframe for the RSI/stochastic/roofing criteria: monthly (default), weekly, or daily
        #[arg(long, default_value = "monthly")]
        timeframe: String,
        /// Match window in DAYS (+/-) around a verified low for a firing to count as a hit
        #[arg(long)]
        window: Option<i64>,
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
    /// GEX (Gamma Exposure) snapshot + gamma-neutral zone for a symbol
    Gex {
        /// Symbol to read (e.g. SPY, QQQ, GLD, SLV)
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        json: bool,
    },
    /// Technical indicators for one or all assets (RSI, MACD, SMA, Bollinger, ATR)
    #[command(
        after_help = "The --symbol/--timeframe/--limit/--include options below apply to the BARE form\n(`technicals` with no subcommand — the legacy RSI/MACD/SMA/BB/ATR panel).\nThe `indicators`, `structure`, and `cyber` SUBCOMMANDS instead take a POSITIONAL\nsymbol — e.g. `analytics technicals indicators BTC` (NOT `--symbol BTC`).\n\nExamples:\n  pftui analytics technicals --symbol BTC,GC=F          # bare panel, flag form\n  pftui analytics technicals indicators BTC             # subcommand, positional\n  pftui analytics technicals structure GC=F --timeframe weekly"
    )]
    Technicals {
        #[command(subcommand)]
        command: Option<AnalyticsTechnicalsCommand>,
        /// Filter to a single symbol or a comma-separated symbol list (e.g. BTC,GC=F) — BARE form only (subcommands take a positional SYMBOL)
        #[arg(long, visible_alias = "symbols")]
        symbol: Option<String>,
        #[arg(long, default_value = "1d")]
        timeframe: String,
        #[arg(long)]
        limit: Option<usize>,
        /// Comma-separated extended indicators/signals to include. Channels
        /// subset: `gaussian-channel`, `zone-channel`, `volatility-trend`,
        /// `donchian-trend`. Signals subset: `mtf-rsi`, `pi-cycle`,
        /// `mtf-breakout`, `bollinger-reversal`, `rsi-extreme`. Pass `all` to
        /// include every extended output. Default: legacy RSI/MACD/SMA/BB/ATR
        /// set only.
        #[arg(long)]
        include: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Market cycle position clocks (BTC halving/4yr cycle, gold ~6.9yr cycle (8yr is folklore))
    Cycles {
        #[command(subcommand)]
        command: AnalyticsCyclesCommand,
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
        /// Filter by direction: "bullish" or "bearish"
        #[arg(long)]
        direction: Option<String>,
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
    #[command(
        after_help = "Compares pftui scenario probabilities against prediction market\nconsensus (Polymarket contracts). Flags divergences above the threshold\n(default: 15pp).\n\nRequires scenario↔contract mappings created via:\n  pftui data predictions map --scenario \"<name>\" --search \"<query>\"\n\nExample:\n  pftui analytics calibration --json\n  pftui analytics calibration --by-layer --json\n  pftui analytics calibration --threshold 10 --json\n\nSee also: data predictions map, analytics scenario list"
    )]
    Calibration {
        /// Divergence threshold in percentage points (default: 15)
        #[arg(long, default_value = "15")]
        threshold: f64,
        /// Trailing window for realised prediction accuracy calibration
        #[arg(long, default_value = "90")]
        window_days: i64,
        /// Include strict per-layer calibration with sample size and 1σ uncertainty
        #[arg(long = "by-layer")]
        by_layer: bool,
        #[arg(long)]
        json: bool,
    },
    /// Manage per-held-asset risk-factor mappings (drives Risk Concentration section)
    ///
    /// Each row: (symbol, factor, direction, exposure_multiplier) — e.g.
    /// "SI=F has 1.4x long exposure to electrification". The macro / high
    /// analyst routines write these so the report's Risk Concentration
    /// section has rows.
    ///
    /// EXAMPLES:
    ///   pftui analytics risk-factors add --symbol SI=F --factor electrification --direction long --exposure 1.4
    ///   pftui analytics risk-factors list --json
    ///   pftui analytics risk-factors delete --symbol SI=F --factor electrification
    #[command(name = "risk-factors")]
    RiskFactors {
        #[command(subcommand)]
        command: AnalyticsRiskFactorsCommand,
    },
    /// Rebuild the `calibration_matrix` table from `user_predictions` outcomes
    ///
    /// Aggregates every scored prediction (outcome IN 'correct','partial','wrong')
    /// grouped by (timeframe, topic, conviction_band) into rows of
    /// (n, hit_rate, stated_confidence). The Self-Retrospective Calibration
    /// section of the report reads from this table. Run after a batch of
    /// outcomes lands or before report generation.
    ///
    /// EXAMPLES:
    ///   pftui analytics calibration-matrix rebuild
    ///   pftui analytics calibration-matrix rebuild --since 180 --json
    #[command(name = "calibration-matrix")]
    CalibrationMatrix {
        #[command(subcommand)]
        command: AnalyticsCalibrationMatrixCommand,
    },
    /// Compare scenario news pressure against mapped prediction-market movement
    #[command(
        name = "narrative-divergence",
        after_help = "Scores each active scenario by comparing 24h topic-tagged news pressure\nagainst mapped prediction-market movement. Positive scores mean narrative\nis running ahead of money; negative scores mean pricing moved with little\nheadline confirmation. Computed live from news_cache + contract mappings;\nnothing is persisted.\n\nExamples:\n  pftui analytics narrative-divergence --json\n  pftui analytics narrative-divergence --hours 48 --threshold 1.5\n\nSee also: data news topics, data predictions map, analytics calibration"
    )]
    NarrativeDivergence {
        /// News lookback window in hours
        #[arg(long, default_value = "24")]
        hours: i64,
        /// Alert threshold in z-score units
        #[arg(long, default_value = "2")]
        threshold: f64,
        #[arg(long)]
        json: bool,
    },
    /// Compare topic news volume against rolling weekday baselines
    #[command(
        name = "news-silence",
        after_help = "Reports whether tier-1/2 article volume by topic is silent, normal,\nor saturated versus a rolling weekday-matched baseline.\n\nExamples:\n  pftui analytics news-silence --json\n  pftui analytics news-silence --window-days 60\n  pftui analytics news-silence rebuild-baselines --since 90d --json\n\nSee also: data news, data news topics, analytics narrative-divergence"
    )]
    NewsSilence {
        #[command(subcommand)]
        command: Option<AnalyticsNewsSilenceCommand>,
        /// Rolling baseline window in days
        #[arg(long, default_value = "90")]
        window_days: i64,
        #[arg(long)]
        json: bool,
    },
    /// Aggregate lessons referenced by recent prediction writes
    Lessons {
        #[command(subcommand)]
        command: AnalyticsLessonsCommand,
    },
    /// Thesis review scheduling: set review-by dates and list overdue sections
    ///
    /// The thesis table holds durable per-section beliefs. Without review
    /// dates, sections silently rot. `set-review` schedules a re-review
    /// date for a section; `review-due` lists sections whose date has
    /// passed plus sections with no date at all ("unscheduled").
    #[command(
        after_help = "Examples:\n  pftui analytics thesis set-review cycle-frameworks --date 2026-09-01\n  pftui analytics thesis review-due --json\n\nSee also: analytics thesis-chains, analytics views stale"
    )]
    Thesis {
        #[command(subcommand)]
        command: AnalyticsThesisCommand,
    },
    /// Debate accuracy scoring: track which side (bull/bear) was right historically
    #[command(
        name = "debate-score",
        after_help = "Score resolved debates to track which side (bull/bear) was historically\ncorrect. Feeds into system accuracy tracking.\n\nWorkflow:\n  1. Debates are created and resolved via `agent debate`\n  2. Score resolved debates with `analytics debate-score add`\n  3. View accuracy stats with `analytics debate-score accuracy`\n  4. Find unscored debates with `analytics debate-score unscored`\n\nExamples:\n  pftui analytics debate-score add --debate-id 1 --winner bull --outcome \"BTC hit 185k\"\n  pftui analytics debate-score list --json\n  pftui analytics debate-score accuracy --topic BTC --json\n  pftui analytics debate-score unscored --json\n\nSee also: agent debate start, agent debate history, agent debate summary"
    )]
    DebateScore {
        #[command(subcommand)]
        command: AnalyticsDebateScoreCommand,
    },
    /// News-source accuracy ledger and rankings
    #[command(
        name = "news-sources",
        after_help = "Tracks how often predictions derived from specific news articles later\nscore correct, partial, or wrong. Use source article IDs from `data news --json`\nwhen writing predictions.\n\nExamples:\n  pftui analytics news-sources accuracy --json\n  pftui analytics news-sources accuracy --domain bloomberg.com --topic fed --json\n  pftui analytics news-sources rank --topic iran --json"
    )]
    NewsSources {
        #[command(subcommand)]
        command: AnalyticsNewsSourcesCommand,
    },
    /// Per-analyst, per-asset directional views with conviction scores (F57: Timeframe Analyst Self-Awareness)
    #[command(
        after_help = "Each timeframe analyst (LOW/MEDIUM/HIGH/MACRO) writes a structured\nview per asset on every run. Views include direction, conviction (-5 to +5),\nreasoning, key evidence, and blind spots.\n\nSubcommands:\n  set              — write/update an analyst's view on an asset\n  list             — list views with optional analyst/asset filters\n  matrix           — full cross-analyst view matrix\n  portfolio-matrix — portfolio-aware matrix with coverage stats\n  history          — view evolution over time for an asset\n  divergence       — surface assets where analysts strongly disagree\n  accuracy         — per-analyst accuracy against price outcomes\n  delete           — remove a view\n\nExamples:\n  pftui analytics views set --analyst low --asset BTC --direction bull \\\n    --conviction 3 --reasoning \"Momentum strong\" --json\n  pftui analytics views list --asset BTC --json\n  pftui analytics views history --asset BTC --json\n  pftui analytics views divergence --json\n  pftui analytics views accuracy --json\n  pftui analytics views matrix --json\n\nSee also: analytics alignment, analytics divergence"
    )]
    Views {
        #[command(subcommand)]
        command: AnalyticsViewsCommand,
    },
    /// Identified opportunities: undervalued positions, scenario plays, entry points
    Opportunities {
        #[arg(long)]
        json: bool,
    },
    /// Real-rates analytics: US-vs-G10 differentials and TIPS/breakeven spreads
    #[command(name = "real-rates")]
    RealRates {
        #[command(subcommand)]
        command: AnalyticsRealRatesCommand,
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
    #[command(
        after_help = "This view is most useful when medium analyst views are populated.\nExamples:\n  pftui analytics views set --analyst medium --asset BTC --direction bull --conviction 2 --reasoning \"Rotation improving\"\n  pftui analytics views portfolio-matrix --json"
    )]
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
    /// Cross-timeframe alignment OR operator-vs-analyst alignment score (subcommands)
    ///
    /// Bare form (no subcommand): cross-timeframe alignment for analyst layers.
    /// `current` / `history` / `compute`: operator-vs-analyst daily alignment score
    /// aggregating Skylar's stated views vs analyst convergence per held asset.
    ///
    /// EXAMPLES:
    ///   pftui analytics alignment --json
    ///   pftui analytics alignment current --json
    ///   pftui analytics alignment history --since 90d --json
    ///   pftui analytics alignment compute --date 2026-06-01 --store --json
    Alignment {
        #[command(subcommand)]
        command: Option<AnalyticsAlignmentCommand>,
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
    #[command(
        name = "cross-timeframe",
        after_help = "\
EXAMPLES:
  pftui analytics cross-timeframe --json             # Full alignment + divergence + breaks
  pftui analytics cross-timeframe --resolve --json    # Add resolution analysis for divergent assets
  pftui analytics cross-timeframe --resolve --symbol BTC --json

See also: analytics alignment, analytics divergence, analytics correlations, analytics regime-transitions"
    )]
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
    #[command(after_help = "\
EXAMPLES:
  pftui analytics digest --agent-filter low-agent --json
  pftui analytics digest --from 2026-04-06 --json
  pftui analytics digest --from yesterday --agent-filter medium-agent --json")]
    Digest {
        /// Include only digest items on or after this date (YYYY-MM-DD, today, yesterday)
        #[arg(long)]
        from: Option<String>,
        /// Build the role-aware digest for a specific agent
        #[arg(long = "agent-filter")]
        agent_filter: Option<String>,
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
    /// Unified market snapshot: prices + sentiment + regime in one call
    #[command(
        name = "market-snapshot",
        after_help = "\
Combines portfolio/market prices, news sentiment scoring, and regime\ncontext into a single JSON payload. Replaces three separate agent calls\n(data prices --market, analytics news-sentiment, analytics regime-flows)\nwith one command.\n\nExamples:\n  pftui analytics market-snapshot --json    # Full snapshot for agent consumption\n  pftui analytics market-snapshot           # Terminal summary\n\nSee also: data prices, analytics news-sentiment, analytics regime-flows"
    )]
    MarketSnapshot {
        #[arg(long)]
        json: bool,
        /// Automatically refresh prices if cache is stale (>2h old)
        #[arg(long)]
        auto_refresh: bool,
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
    #[command(
        after_help = "Common workflows:\n  pftui analytics alerts check             Check all alerts against current data\n  pftui analytics alerts check --today     Check only today's triggers\n  pftui analytics alerts check --newly-triggered --json  Only new triggers (agent-friendly)\n  pftui analytics alerts check --condition correlation_break --json  Filter by condition\n  pftui analytics alerts check --kind macro --json  Filter by alert kind\n  pftui analytics alerts triage            Prioritized alert dashboard with urgency tiers\n  pftui analytics alerts list              List alert rules\n  pftui analytics alerts list --triggered  Show triggered alert log\n  pftui analytics alerts add \"BTC > 100000\" Add a custom alert rule\n  pftui analytics alerts seed-defaults     Seed smart-alert defaults for holdings\n\nAlso accessible via: pftui data alerts check, pftui data alerts list"
    )]
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
    /// Run-health instrumentation: echo risk, blind divergence, scenario churn, rivalry scoreboard
    #[command(
        after_help = "Per-run epistemic health of the multi-agent intelligence system.\n\nMetrics:\n  agreement_rate         share of voices agreeing with the operator stance (0-1);\n                         > 0.85 flags echo risk\n  blind_divergence       mean |house conviction − blind conviction| across held\n                         assets; > 2.0 flags a house view far from the raw-data read\n  panel_dispersion       stddev of panel persona confidences; < 4.0 flags persona washing\n  novelty_rate           share of the run's notes that are novel\n  scenario_delta_total   sum |Δprobability| across scenarios today\n  audit_pass_rate        accuracy-audit claims_passed/claims_total\n  conviction_price_corr  max |Pearson r| between layer conviction trajectories and\n                         held-asset closes; > 0.6 flags momentum dressed as structure\n                         (standing rule 15)\n\nWorkflows:\n  pftui analytics epistemics record --date 2026-06-10 --agreement 0.7 --panel-dispersion 6.2\n  pftui analytics epistemics show --date 2026-06-10 --json\n  pftui analytics epistemics history --limit 14\n  pftui analytics epistemics rivalry --json            # house vs antithesis scoreboard\n  pftui analytics epistemics conviction-price --json   # per layer × held asset"
    )]
    Epistemics {
        #[command(subcommand)]
        command: AnalyticsEpistemicsCommand,
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

        /// Curated geopolitics relevance filter: keyword-matched contracts only, excluding contracts resolving >12 months out, already past resolution, or with zero 24h volume
        #[arg(long, conflicts_with = "category")]
        geo: bool,

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
    /// Ranked power-structure checklist combining regime flows, FIC/MIC balance, and conflict stress
    #[command(
        name = "power-signals",
        after_help = "Aggregates the existing power-structure stack into one ranked checklist:\n  - `analytics regime-flows`\n  - `analytics power-flow assess`\n  - `analytics power-flow conflicts`\n\nUse this when an agent needs one JSON call for geopolitical stress, safe-haven rotation,\nand FIC/MIC/TIC balance instead of stitching three commands together."
    )]
    PowerSignals {
        /// Number of days to use for power-flow/conflict lookback (default: 30)
        #[arg(long, default_value_t = 30)]
        days: usize,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
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
    #[command(
        name = "morning-brief",
        after_help = "Combines situation room, 24h deltas, cross-timeframe synthesis,\nactive scenario probabilities, correlation breaks, catalysts, portfolio impact,\ntriggered alerts, and news sentiment into a single payload.\n\nDesigned for morning-brief agents that previously needed 5-6 separate\nanalytics commands to assemble intelligence.\n\nSee also: analytics situation, analytics deltas, analytics synthesis"
    )]
    MorningBrief {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
        /// Compute only specific sections (comma-separated). Omitted sections are null/empty.
        /// Available: situation, deltas, synthesis, scenarios, correlation_breaks, catalysts, impact, alerts, news_sentiment
        #[arg(long)]
        section: Option<String>,
    },
    /// Consolidated evening analysis: morning-brief + narrative + opportunities + conviction changes + prediction stats + cross-timeframe resolution in one call
    #[command(
        name = "evening-brief",
        after_help = "Deep evening analysis payload for agents. Extends morning-brief with:\n  - Narrative: structured recap, key themes, analytical memory\n  - Opportunities: identified entry points, scenario plays\n  - Conviction changes: shifts over the past 7 days\n  - Prediction stats: overall accuracy scorecard\n  - Cross-timeframe resolution: divergent assets with stance guidance\n\nDesigned for the evening analyst who previously needed 20+ separate\nanalytics commands to assemble a full picture.\n\nSee also: analytics morning-brief, analytics narrative, analytics cross-timeframe"
    )]
    EveningBrief {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
        /// Compute only specific sections (comma-separated). Omitted sections are null/empty.
        /// Available: situation, deltas, synthesis, scenarios, correlation_breaks, catalysts, impact, alerts, news_sentiment, narrative, opportunities, conviction_changes, prediction_stats, cross_timeframe_resolution
        #[arg(long)]
        section: Option<String>,
    },
    /// Routine workflow guidance: prioritized action items, pending predictions, triggered alerts, stale convictions, scenario shifts
    #[command(
        after_help = "Single-call routine priority advisor for agents. Answers\n\"what should I focus on right now?\" by aggregating:\n\n  - Triggered alerts needing acknowledgment\n  - Pending predictions past target date needing scoring\n  - Stale convictions (7+ days without update)\n  - Recently-updated scenarios (last 24h)\n\nAction items are ranked by urgency (critical > high > medium > low)\nwith suggested CLI commands for each.\n\nDesigned for agent routines that need a single entry point\nto determine workflow priorities.\n\nSee also: analytics alerts triage, analytics morning-brief"
    )]
    Guidance {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
    },
    /// Regime-asset flow correlation: cross-references regime state with asset class flows to detect power structure patterns
    #[command(
        name = "regime-flows",
        after_help = "Cross-references the current market regime with asset class flows to detect\npower structure patterns automatically. Monitors key ratios (gold/oil,\ncopper/gold, BTC/gold), safe-haven vs risk flows, energy complex signals,\nand defense sector tracking.\n\nDetects patterns: safe-haven rotation, geopolitical stress, inflationary pulse,\nrisk-on breakout, deflationary signal, dollar wrecking ball, energy crisis,\nand regime divergence.\n\nSee also: analytics macro regime, analytics correlations, analytics movers themes"
    )]
    RegimeFlows {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
    },
    /// Regime transition probability scoring: analyzes signal momentum, current state, and historical patterns to score likelihood of regime changes
    #[command(
        name = "regime-transitions",
        after_help = "Scores the probability of transitioning from the current regime to each\npossible state (risk-on, risk-off, crisis, stagflation, etc.).\n\nAnalyzes:\n  - 6 signal momentum indicators (VIX, DXY, yields, equities, gold, oil)\n  - Current regime confidence and duration\n  - Special regime triggers (crisis: VIX>30+oil>90, stagflation: gold up+equities down)\n  - Historical transition frequency and patterns\n\nEach candidate shows probability, key drivers, confirmation triggers, and\ninvalidation conditions.\n\nSee also: analytics macro regime, analytics regime-flows, analytics synthesis"
    )]
    RegimeTransitions {
        /// Output as JSON for agent/script consumption (recommended)
        #[arg(long)]
        json: bool,
    },
    /// Prediction backtesting: replay scored predictions against historical prices to compute theoretical P&L
    #[command(
        after_help = "Replays all scored predictions against historical price data.\nFor each: entry price at prediction date, exit price at target/scored date,\ntheoretical P&L based on conviction-weighted position sizing.\n\nConviction weights on $10,000 notional:\n  high = 10% ($1,000 position)\n  medium = 5% ($500 position)\n  low = 2% ($200 position)\n\nExamples:\n  pftui analytics backtest predictions --json\n  pftui analytics backtest predictions --symbol BTC-USD --json\n  pftui analytics backtest predictions --agent low-timeframe --json\n  pftui analytics backtest predictions --conviction high --json\n\nSee also: journal prediction scorecard, analytics views accuracy"
    )]
    Backtest {
        #[command(subcommand)]
        command: AnalyticsBacktestCommand,
    },
    /// Macro environment feature vector — where today sits vs its history (z-scored, no look-ahead)
    Environment {
        #[command(subcommand)]
        command: AnalyticsEnvironmentCommand,
    },
    /// Closest historic environment analogs + the target asset's forward-return distribution after them
    #[command(
        after_help = "Finds the historical days whose macro backdrop (equities/gold/oil/dollar/rates/vol)\nmost resembles today via a covariance-whitened (Mahalanobis) distance, then reports\nthe distribution of the chosen asset's forward returns following those analogs — with a\nbootstrap CI and an honest analog-quality note.\n\nExamples:\n  pftui analytics analog --asset BTC --horizon 90 --json\n  pftui analytics analog --asset GC=F --horizon 180 --k 30"
    )]
    Analog {
        /// Asset whose forward returns are measured after each analog (alias or ticker)
        #[arg(long)]
        asset: String,
        /// Forward-return horizon in calendar days
        #[arg(long, default_value_t = 90)]
        horizon: i64,
        /// Number of nearest analogs to use
        #[arg(long, default_value_t = 25)]
        k: usize,
        /// Exclude analogs within this many days of today (avoid trivially-recent matches)
        #[arg(long = "exclude-days", default_value_t = 90)]
        exclude_days: i64,
        #[arg(long)]
        json: bool,
    },
    /// Synthesized positioning for an asset: analog forward returns + regime quad + cycle clock, with honesty stats
    #[command(
        after_help = "Composes the measured analog forward-return distribution, the growth×inflation\nregime quad, and the cycle clock into a single auditable stance (each driver shows its\nscore, weight, and reason). Applies a humility default — thin analog evidence or a CI\nstraddling zero caps confidence and says so.\n\nExample:\n  pftui analytics positioning --asset BTC --horizon 90 --json"
    )]
    Positioning {
        /// Asset to position (alias or ticker)
        #[arg(long)]
        asset: String,
        #[arg(long, default_value_t = 90)]
        horizon: i64,
        #[arg(long, default_value_t = 25)]
        k: usize,
        #[arg(long)]
        json: bool,
    },
    /// Extreme-Value-Theory tail risk (POT/GPD): fat-tail-aware VaR + Expected Shortfall + tail-fatness ξ
    #[command(
        name = "tail-risk",
        after_help = "Fits a Generalized Pareto Distribution to the LEFT TAIL of an asset's daily\nreturns (Peaks-Over-Threshold). Gaussian/historical VaR understates crash depth\nfor fat-tailed assets; the GPD shape ξ measures HOW fat the tail is (ξ>0 = power-law,\nfatter than normal) and gives a principled VaR / Expected-Shortfall, with the\nhistorical estimate shown alongside. Closed-form probability-weighted-moments fit\n(auditable, far less shape-biased than plain method-of-moments; valid for ξ<1).\nVaR below the threshold quantile uses the empirical quantile (POT is valid only above it).\n\nExamples:\n  pftui analytics tail-risk --asset BTC --json\n  pftui analytics tail-risk --asset gold --lookback 1000 --threshold 95"
    )]
    TailRisk {
        /// Asset to analyze (alias or ticker, e.g. BTC, gold, SPY)
        #[arg(long)]
        asset: String,
        /// Use only the most recent N bars (default: all history)
        #[arg(long)]
        lookback: Option<u32>,
        /// Loss-distribution percentile for the POT threshold (80–99, default 95)
        #[arg(long, default_value_t = 95.0)]
        threshold: f64,
        #[arg(long)]
        json: bool,
    },
    /// Tail dependence between two assets: do they co-crash? (Kendall τ + empirical/Clayton lower-tail λ_L)
    #[command(
        name = "tail-dependence",
        after_help = "Correlation hides the failure mode that matters most: two assets can have modest\ncorrelation yet plunge TOGETHER in a crash. The lower-tail-dependence λ_L = P(Y crashing |\nX crashing) measures exactly that. Reports Pearson + Kendall τ, an empirical λ_L/λ_U at the\nchosen tail quantile, and the Clayton-copula λ_L (via τ inversion). Answers whether a\ndiversification pair (e.g. BTC vs gold) actually holds up when it's needed.\n\nExamples:\n  pftui analytics tail-dependence --asset BTC --vs gold --json\n  pftui analytics tail-dependence --asset BTC --vs SPY --q 5"
    )]
    TailDependence {
        /// First asset (alias or ticker)
        #[arg(long)]
        asset: String,
        /// Second asset to test co-movement against (alias or ticker)
        #[arg(long)]
        vs: String,
        /// Tail quantile in percent for the empirical estimate (1–20, default 5)
        #[arg(long, default_value_t = 5.0)]
        q: f64,
        #[arg(long)]
        json: bool,
    },
    /// Anchored VWAP from a cycle low (or halving/ATH): average cost-basis since the anchor + price position
    #[command(
        name = "avwap",
        after_help = "Anchored VWAP = the volume-weighted average price from a chosen anchor bar to now.\nAnchored to the last cycle low it's the average cost-basis of everyone who bought\nsince the bottom: price ABOVE = the average post-low buyer is in profit (basis\ndefended, accumulation intact); a break BELOW = that buyer is underwater. If any\nbar in the window lacks volume it degrades to a flat-weight anchored average price\nand says so (never a silent fake VWAP).\n\nExamples:\n  pftui analytics avwap --asset BTC --json\n  pftui analytics avwap --asset BTC --anchor halving\n  pftui analytics avwap --asset gold --anchor-date 2022-09-26"
    )]
    Avwap {
        /// Asset (alias or ticker, e.g. BTC, gold, SPY)
        #[arg(long)]
        asset: String,
        /// Anchor: cycle-low (default), halving (BTC only), or ath
        #[arg(long, default_value = "cycle-low")]
        anchor: String,
        /// Explicit anchor date YYYY-MM-DD (overrides --anchor)
        #[arg(long = "anchor-date")]
        anchor_date: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Hurst exponent (R/S): is the asset trending, random-walk, or mean-reverting?
    #[command(
        after_help = "Rescaled-Range Hurst exponent over the asset's LOG returns — a regime gauge.\nH>0.5 persistent/trending (trend-following has an edge); H≈0.5 random walk (no\nedge); H<0.5 mean-reverting (fade extremes). Anis-Lloyd/Peters bias-corrected.\n\nExamples:\n  pftui analytics hurst --asset BTC --json\n  pftui analytics hurst --asset gold --lookback 512"
    )]
    Hurst {
        /// Asset (alias or ticker)
        #[arg(long)]
        asset: String,
        /// Use only the most recent N bars (default: all history)
        #[arg(long)]
        lookback: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    /// Regime-break detection (CUSUM change-point): when did the drift last structurally break?
    #[command(
        name = "regime-break",
        after_help = "Page's two-sided CUSUM on daily returns — detects when the return DRIFT\nstructurally shifted (a healthy dip vs 'the trend just broke', the key call for a\ndip-accumulator). Reports past change-points, the last one, and how close a fresh\nbreak is to firing now. k = slack (σ multiples, default 0.5), h = alarm threshold\n(default 5).\n\nExamples:\n  pftui analytics regime-break --asset BTC --json\n  pftui analytics regime-break --asset gold --k 0.5 --h 4"
    )]
    RegimeBreak {
        /// Asset (alias or ticker)
        #[arg(long)]
        asset: String,
        /// Use only the most recent N bars (default: all history)
        #[arg(long)]
        lookback: Option<u32>,
        /// CUSUM slack in σ multiples (default 0.5 — detects ~1σ drift shifts)
        #[arg(long, default_value_t = 0.5)]
        k: f64,
        /// CUSUM alarm threshold in σ multiples (default 5)
        #[arg(long, default_value_t = 5.0)]
        h: f64,
        #[arg(long)]
        json: bool,
    },
    /// Risk-side capstone: EVT tail-risk + co-crash dependence + regime + vol/drawdown in one view
    #[command(
        name = "risk-dashboard",
        after_help = "The risk-side analogue of `positioning` — composes the measured risk\nprimitives (EVT fat-tail VaR/Expected-Shortfall + ξ, anchored co-crash tail\ndependence vs a partner, the Hurst/DFA regime, CUSUM drift-break, annualized vol,\nand drawdown) into one auditable view + a plain-language composite read. Each\nline is the same computation as its dedicated command.\n\nExamples:\n  pftui analytics risk-dashboard --asset BTC --json\n  pftui analytics risk-dashboard --asset gold --vs SPY"
    )]
    RiskDashboard {
        /// Asset to assess (alias or ticker)
        #[arg(long)]
        asset: String,
        /// Co-crash partner for tail dependence (default: gold, or BTC for gold)
        #[arg(long)]
        vs: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Risk-aware basket weights (equal / inverse-vol / risk-parity / downside-risk-parity) with per-asset risk contributions + diversification ratio
    #[command(
        after_help = "Compute portfolio weights for a basket of assets from their common price\nhistory, under four risk-aware schemes:\n  equal                 1/N (baseline)\n  inverse-vol           w_i ∝ 1/σ_i — equalizes standalone risk, ignores correlation\n  risk-parity           equal risk CONTRIBUTION (ERC) — each asset adds the same\n                        share of portfolio variance, using the full covariance\n  downside-risk-parity  ERC on the SEMIcovariance (co-downside only) — sizes for\n                        JOINT-CRASH risk rather than symmetric vol\n\nReports each asset's weight, annualized vol, and risk contribution, plus the\nportfolio vol and the diversification ratio (Σwᵢσᵢ / σ_portfolio, ≥1; higher =\nmore diversification benefit captured). Portfolio vol + diversification are\nalways full-variance (comparable across methods). Tickers with ^/=/- use their\nalias (gold, silver, us10y, dxy, vix).\n\nExamples:\n  pftui analytics basket weights --assets BTC,gold,SPY --method risk-parity\n  pftui analytics basket weights --assets BTC,gold,SPY --method downside-risk-parity\n  pftui analytics basket weights --assets BTC,gold --method inverse-vol --lookback 365 --json"
    )]
    Basket {
        #[command(subcommand)]
        command: AnalyticsBasketCommand,
    },
    /// Drawdown survival & recovery: Triple-Penance max-DD/time-under-water + risk-of-ruin (the TIME/solvency complement to EVT/CDaR depth)
    #[command(
        after_help = "Model how LONG an asset stays underwater and the chance of being forced out\nbefore the cycle turns — the time/solvency axis the depth-only EVT and CDaR\nviews are missing (Bailey & López de Prado, Triple Penance).\n\n  Recovery cliff   gain needed to erase a drawdown D: D/(1−D) (50%→+100%, 80%→+400%)\n  Triple Penance   expected max drawdown, time-to-trough, and ~3×-longer recovery\n                   at confidence α, with an AR(1) serial-correlation correction\n                   (trending cycles understate underwater time on the i.i.d. view)\n  Risk of ruin     P(ever breaching the drawdown budget) = exp(−2μ·b/σ²)\n\nμ≤0 (an asset sitting at a cycle low) makes recovery unbounded and ruin certain\n— flagged loudly (reliable=false) rather than returning a misleading number.\nDepth is still measured by EVT/CDaR; this is the duration + probability layer.\n\nExamples:\n  pftui analytics survival --asset BTC\n  pftui analytics survival --asset gold --budget 30 --confidence 0.99 --json"
    )]
    Survival {
        /// Asset to assess (alias or ticker)
        #[arg(long)]
        asset: String,
        /// Drawdown budget the risk-of-ruin is measured against, percent
        #[arg(long, default_value = "25")]
        budget: f64,
        /// Confidence α for the Triple-Penance max-drawdown figures
        #[arg(long, default_value = "0.95")]
        confidence: f64,
        /// Lookback window in trading days (0 = all history)
        #[arg(long, default_value = "0")]
        lookback: usize,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Strategy backtesting: define trade conditions as an expression and test them against full price history
    #[command(
        after_help = "Define a trade rule as an expression over price, indicators, and timeframes,\nthen backtest it against the full historical price database.\n\nExpression language:\n  close, open, high, low, volume        primary asset's daily field\n  close(BTC), close(GOLD)               another symbol (alphanumeric ticker OR alias)\n  sma(close, 200), ema(close, 21)       moving averages\n  rsi(14), rsi(close(BTC), 14)          RSI\n  atr(14) cci(20) williams_r(14) roc(10) standard OHLC indicators\n  fisher(10)                            Ehlers Fisher Transform (sharp turning-point oscillator)\n  stoch_k(14,3) stoch_d(14,3)           Stochastic %K / %D\n  adx(14) plus_di(14) minus_di(14)      ADX trend strength + directional\n  supertrend(10,3) supertrend_dir(10,3) ATR-band trailing stop + regime (+1/−1)\n  macd(12,26,9)  macd_line/macd_signal(12,26,9)   MACD (macd()=histogram)\n  bb_upper/bb_lower/bb_mid/bb_pct(20,2) Bollinger bands + %b\n  obv() mfi(14)                         volume indicators (need volume data)\n  atr(BTC,14) adx(gold,14)              any OHLC indicator on another symbol\n  highest(close,20) lowest(low,20)      rolling max/min over N bars\n  ago(close,1) pct_change(close,5)      lag / N-bar percent change\n  abs(close - sma(close,50))            absolute value\n  ... @weekly | @monthly                evaluate at a higher timeframe\n  >  <  >=  <=  ==                       comparisons\n  crosses_above / crosses_below         strict edge crossings\n  and  or  not                          boolean logic\n\nBreakout idiom: highest/lowest INCLUDE the current bar, so a prior-N-bar high is\n  ago(highest(high, N), 1) — e.g. entry \"close > ago(highest(close, 50), 1)\".\n\nSYMBOLS IN EXPRESSIONS must be alphanumeric (SPY, BTC) — tickers with '^', '=',\nor '-' (^TNX, GC=F, BTC-USD) CANNOT be typed directly; use their ALIAS instead:\n  gold=GC=F  silver=SI=F  us10y=^TNX  fedfunds=^IRX  us5y=^FVX  us30y=^TYX  dxy=DX-Y.NYB.\nSo 'rate hiking vs cutting' is a moving-average crossing on us10y (the ^TNX alias).\n\nExamples:\n  pftui analytics strategy backtest --asset BTC --entry \"close crosses_above sma(close, 200) @weekly\" --exit \"hold 365d\" --json\n  pftui analytics strategy backtest --asset BTC --entry \"rsi(14) @monthly < 90\" --exit \"hold 90d\"\n  pftui analytics strategy backtest --asset BTC --entry \"macd(12,26,9) > 0 and adx(14) > 25\" --exit \"hold 10d\"\n  pftui analytics strategy backtest --asset BTC --entry \"bb_pct(20,2) < 0.05\" --exit \"hold 10d\" --trailing-stop 15\n  pftui analytics strategy backtest --asset BTC --entry \"rsi(14) < 35\" --exit \"hold 10d\" --commission 0.1 --slippage 0.05 --next-bar-fill\n  pftui analytics strategy backtest --asset BTC --entry \"close crosses_above sma(close,200)\" --exit \"hold 180d\" --vol-target 20\n  pftui analytics strategy segment --asset GC=F --when \"us10y > sma(us10y, 200)\"\n  pftui analytics strategy compare --asset GC=F --when \"us10y > sma(us10y, 200)\" --when-label hiking --vs \"us10y < sma(us10y, 200)\" --vs-label cutting\n  pftui analytics strategy explain --asset BTC --entry \"close crosses_above sma(close, 200) @weekly\"\n\nReturns are statistics over price ratios (percent / growth), not monetary balances.\n\nJSON note: per-trade entry_price/exit_price in --json output are JSON NUMBERS\n(f64), not the string-decimals used by cycles/TA — the backtest engine computes\nthem in floating point and they are reference/display values, never stored money.\nreturn_pct and all ratios are likewise numbers."
    )]
    Strategy {
        #[command(subcommand)]
        command: AnalyticsStrategyCommand,
    },
    /// `sources_registry` — canonical person/framework/institution/outlet lookup
    Sources {
        #[command(subcommand)]
        command: AnalyticsSourcesCommand,
    },
    /// `event_annotations` — operator-curated macro/market event catalogue
    Events {
        #[command(subcommand)]
        command: AnalyticsEventsCommand,
    },
    /// `reasoning_fragments` — typed heuristics + lesson_fragment_edges
    Fragments {
        #[command(subcommand)]
        command: AnalyticsFragmentsCommand,
    },
    /// `calibration_adjustments` — per-(layer, topic, conviction) discount/boost rules.
    /// Accessed via `pftui analytics calibration-adjustments`.
    #[command(name = "calibration-adjustments")]
    CalibrationAdjustments {
        #[arg(long)]
        layer: Option<String>,
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        conviction: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// `failure_correlations` — pairwise co-failure rates between lesson clusters
    Failures {
        #[command(subcommand)]
        command: AnalyticsFailuresCommand,
    },
    /// `clusters` — lesson cluster_key taxonomy (list + per-cluster prediction usage)
    Clusters {
        #[command(subcommand)]
        command: AnalyticsClustersCommand,
    },
    /// `thesis_dependencies` — formalized cross-asset if-then chains.
    /// List, show, validate, or manually add chains.
    #[command(
        name = "thesis-chains",
        after_help = "Cross-asset thesis dependency graph: structured\nantecedent → consequent triples extracted from the thesis table,\nprediction_lessons, and agent_messages.\n\nExamples:\n  pftui analytics thesis-chains list --json\n  pftui analytics thesis-chains list --state confirmed --json\n  pftui analytics thesis-chains show 1 --json\n  pftui analytics thesis-chains validate 1 --json\n  pftui analytics thesis-chains extract --dry-run --json\n  pftui analytics thesis-chains extract --from-thesis --from-lessons --apply --json\n  pftui analytics thesis-chains add --antecedent \"XAU > 4500\" \\\n    --consequent \"BTC > 100000\" --relation implies --conviction high"
    )]
    ThesisChains {
        #[command(subcommand)]
        command: AnalyticsThesisChainsCommand,
    },
    /// `prediction_falsification_rules` — auto/manual falsification triggers per prediction
    Falsifications {
        #[arg(long = "rule-type")]
        rule_type: Option<String>,
        #[arg(long = "auto-eligible")]
        auto_eligible: bool,
        #[arg(long = "for-prediction")]
        for_prediction: Option<i64>,
        #[arg(long)]
        json: bool,
    },

    /// Recommendation ledger: record, score, scoreboard, accuracy, linking
    #[command(
        name = "recommendations",
        after_help = "The recommendation ledger — every system recommendation is recorded\nwith the close that priced it and scored against forward returns, so\nthe system can notice when its own advice loses money (gold post-mortem:\nadd-into-a-drawdown went unmeasured for 5 months).\n\nSubcommands:\n  record              Record one action (add/wait/hold/trim/avoid) per symbol\n  list                List recommendations with optional filters\n  score               Fill forward returns for elapsed horizons (default mode)\n  scoreboard          Per symbol × action: n, % positive, mean fwd return,\n                      plus the per-symbol WINDOW-QUALITY (ADD−WAIT) delta\n  accuracy            Legacy hit-rate breakdown by recommendation type\n  link                Manually link a reply or transaction to a recommendation\n  relink-historical   Retroactively link existing operator_replies and transactions\n\nExamples:\n  pftui analytics recommendations record --symbol GC=F --action wait \\\n    --rationale \"extension >12% over 200dma\" --json\n  pftui analytics recommendations scoreboard --symbol GC=F --json\n  pftui analytics recommendations score --json\n  pftui analytics recommendations list --symbol BTC --limit 20 --json"
    )]
    Recommendations {
        #[command(subcommand)]
        command: AnalyticsRecommendationsCommand,
    },
    /// Adversary pseudo-analyst layer — argue against the dominant convergence.
    ///
    /// The write-time adversary (per-prediction "case against" composed
    /// deterministically from the substrate) lives under
    /// `pftui journal prediction adversary`. THIS namespace covers the
    /// synthesis-time adversary: a per-asset, per-run pseudo-analyst that
    /// runs AFTER the four timeframe analysts have written their views
    /// for a run, but BEFORE the synthesis (evening/morning) agent reads
    /// them. See `agents/routines/adversary-analyst.md`.
    #[command(
        name = "adversary",
        after_help = "Synthesis-time adversary layer (per-asset, per-run).\n\nSubcommands:\n  synthesis add               Record a counter-case for an asset\n  synthesis show              List recorded synthesis-time adversary views\n  synthesis fragility-rank    Rank assets by max fragility score\n\nSee also:\n  pftui journal prediction adversary   — write-time per-prediction adversary\n  agents/routines/adversary-analyst.md — pseudo-analyst routine"
    )]
    Adversary {
        #[command(subcommand)]
        command: AnalyticsAdversaryCommand,
    },
    /// Capital flow aggregates: rolling-window net flow and top inflow/outflow per asset (F59 scaffold)
    #[command(
        after_help = "Aggregates rows from the `capital_flows` table over a rolling\nwindow. Outflows and redemptions are signed negative.\n\nExamples:\n  pftui analytics flows summary --json\n  pftui analytics flows summary --since 30d --json\n\nSee also: pftui data flows refresh, pftui data flows show"
    )]
    Flows {
        #[command(subcommand)]
        command: AnalyticsFlowsCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsFlowsCommand {
    /// Per-asset rolling-window aggregate (net flow, top inflow/outflow)
    Summary {
        /// Lookback window: NNd, NNw, NNm, or YYYY-MM-DD. Default 7d.
        #[arg(long, default_value = "7d")]
        since: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsAdversaryCommand {
    /// Synthesis-time adversary views (per-asset, per-run)
    Synthesis {
        #[command(subcommand)]
        command: AnalyticsAdversarySynthesisCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsAdversarySynthesisCommand {
    /// Add a synthesis-time adversary view for an asset
    Add {
        /// Asset symbol (e.g. BTC, GLD, SPY)
        #[arg(long)]
        asset: String,
        /// One-line summary of the dominant convergence across the
        /// four timeframe analysts (what the adversary is arguing against)
        #[arg(long = "convergence")]
        convergence: String,
        /// The strongest opposing case using only data the four
        /// analysts already saw (quoted verbatim into the daily report
        /// when fragility_score >= 3)
        #[arg(long = "counter")]
        counter: String,
        /// JSON-encoded array of supporting evidence points, e.g.
        /// `'["realized cap stalling","ETF flow tail risk"]'`. Pass `[]`
        /// for none.
        #[arg(long = "evidence")]
        evidence: String,
        /// JSON-encoded array of falsification triggers, e.g.
        /// `'["BTC closes < 65k for 5 sessions"]'`. Pass `[]` for none.
        #[arg(long = "falsification")]
        falsification: String,
        /// Fragility of the dominant convergence on a 1..=5 scale.
        /// A score of 3 or higher triggers the synthesis-gating
        /// contract documented in AGENTS.md.
        #[arg(long = "fragility")]
        fragility: i64,
        /// Optional ISO-8601 override for `recorded_at` (defaults to now)
        #[arg(long = "recorded-at")]
        recorded_at: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show recorded synthesis-time adversary views
    Show {
        /// Filter to a single asset
        #[arg(long)]
        asset: Option<String>,
        /// Restrict to rows recorded within the window (e.g. `7d`,
        /// `24h`, or ISO-8601 `YYYY-MM-DD`)
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Rank assets by their max synthesis-time fragility score
    #[command(name = "fragility-rank")]
    FragilityRank {
        /// Restrict to rows recorded within the window (e.g. `7d`,
        /// `24h`, or ISO-8601 `YYYY-MM-DD`)
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsRecommendationsCommand {
    /// Record a ledger entry: one scored, timestamped action per symbol per run.
    #[command(
        after_help = "The recommendation ledger (gold post-mortem T2): every decision-card\naction is recorded with the close that priced it, so the system's own\nadvice becomes scoreable. entry_price is auto-filled from the latest\nprice_history close on or before --date (falling back SYM → SYM-USD;\nthe series used is stored in price_series).\n\nActions:\n  add    open the accumulation window / scale in\n  wait   named-gate wait — a first-class, scored recommendation,\n         NOT a failure to decide\n  hold   no change to the position\n  trim   reduce (exchange-held assets only — never physical metal)\n  avoid  do not initiate\n\nExamples:\n  pftui analytics recommendations record --symbol GC=F --action wait \\\n    --rationale \"extension gate: >12% above 200dma\" --source decision-architect\n  pftui analytics recommendations record --symbol BTC --action add --json"
    )]
    Record {
        /// Asset symbol (e.g. GC=F, BTC)
        #[arg(long)]
        symbol: String,
        /// Action: add | wait | hold | trim | avoid
        #[arg(long)]
        action: String,
        /// One-line rationale for the call
        #[arg(long)]
        rationale: Option<String>,
        /// Run date (YYYY-MM-DD; default today)
        #[arg(long)]
        date: Option<String>,
        /// Which writer recorded it (default decision-architect)
        #[arg(long, default_value = "decision-architect")]
        source: String,
        #[arg(long)]
        json: bool,
    },
    /// List recommendations with optional filters.
    List {
        #[arg(long, help = "Filter by report date (YYYY-MM-DD).")]
        date: Option<String>,
        #[arg(long, help = "Filter by asset (e.g. BTC, GLD).")]
        asset: Option<String>,
        #[arg(long, help = "Alias for --asset (ledger vocabulary).")]
        symbol: Option<String>,
        #[arg(
            long = "type",
            help = "Filter by recommendation type (add/wait/hold/trim/avoid/...)."
        )]
        recommendation_type: Option<String>,
        #[arg(
            long,
            help = "Filter to recommendations on or after this date (YYYY-MM-DD or e.g. 30d)."
        )]
        since: Option<String>,
        #[arg(long, help = "Maximum rows (newest first).")]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Score recommendations: forward returns (default) or legacy outcome scores.
    #[command(
        after_help = "Default (no --all/--id): fill fwd_30d_pct / fwd_90d_pct / fwd_180d_pct\nfor any priced ledger row whose horizon has elapsed — percent change from\nentry_price to the close at run_date+N. Idempotent: a scored horizon is\nnever overwritten. Runs automatically in the tail of `pftui data refresh`\n(this machine has no daemon).\n\nWith --all or --id: the legacy outcome-score pass (recommendation →\noperator action → bounded [-100,100] quality score at --horizon days).\n\nExamples:\n  pftui analytics recommendations score --json          # forward returns\n  pftui analytics recommendations score --all --horizon 30 --json"
    )]
    Score {
        #[arg(
            long,
            help = "Legacy outcome scoring: score every recommendation without an outcome."
        )]
        all: bool,
        #[arg(
            long,
            help = "Legacy outcome scoring: score a single recommendation by id."
        )]
        id: Option<i64>,
        #[arg(
            long,
            default_value = "30",
            help = "Legacy outcome scoring: days after report_date to evaluate."
        )]
        horizon: i64,
        #[arg(
            long,
            help = "Legacy outcome scoring: restrict to recommendations on or after this date."
        )]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// THE ledger deliverable: per symbol × action forward-return scoreboard.
    #[command(
        after_help = "Per (symbol × action): n, % positive, and mean forward return at\n30/90/180 days. Plus the WINDOW-QUALITY line per symbol: mean 90d\nforward return after ADD minus after WAIT — positive means the system's\ntiming added value over just waiting; negative means its ADD calls were\nworse than its own WAIT calls (the gold failure, made measurable).\n\nDecision-architect contract: consult this scoreboard for the symbol\nBEFORE composing a decision card, and cite its verdict in the card.\n\nExamples:\n  pftui analytics recommendations scoreboard --json\n  pftui analytics recommendations scoreboard --symbol GC=F"
    )]
    Scoreboard {
        /// Restrict to one symbol
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Hit-rate accuracy broken down by recommendation type and (optionally) asset.
    Accuracy {
        #[arg(long = "type", help = "Filter by recommendation type.")]
        recommendation_type: Option<String>,
        #[arg(long, help = "Filter by asset.")]
        asset: Option<String>,
        #[arg(
            long,
            default_value = "90d",
            help = "Lookback window (e.g. 30d, 90d, or YYYY-MM-DD)."
        )]
        since: String,
        #[arg(
            long = "threshold",
            default_value = "0",
            help = "Score threshold for counting a hit."
        )]
        threshold: f64,
        #[arg(long = "by-asset")]
        by_asset: bool,
        #[arg(long)]
        json: bool,
    },
    /// Manually link an operator_reply or transaction to a recommendation.
    Link {
        #[arg(long, help = "Recommendation id.")]
        id: i64,
        #[arg(long = "reply", help = "operator_replies.id to link.")]
        reply_id: Option<i64>,
        #[arg(long = "transaction", help = "transactions.id to link.")]
        transaction_id: Option<i64>,
        #[arg(
            long,
            help = "Action status: accepted/rejected/partial/deferred/ignored"
        )]
        action_status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Retroactively link existing operator_replies and transactions to open recommendations.
    RelinkHistorical {
        #[arg(
            long,
            default_value = "7",
            help = "Transaction-link window in days (default 7)."
        )]
        window: i64,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsSourcesCommand {
    /// List sources, optionally filtered by type
    List {
        #[arg(long = "type")]
        source_type: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Upsert a source by canonical_id
    Set {
        canonical_id: String,
        #[arg(long = "display-name")]
        display_name: String,
        #[arg(long = "type")]
        source_type: String,
        /// Comma-separated aliases
        #[arg(long)]
        aliases: Option<String>,
        /// Comma-separated topic tags
        #[arg(long)]
        topics: Option<String>,
        #[arg(long = "accuracy-rating")]
        accuracy_rating: Option<String>,
        #[arg(long = "framework-summary")]
        framework_summary: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a source by canonical_id
    Remove {
        canonical_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsEventsCommand {
    /// List event annotations
    List {
        #[arg(long)]
        category: Option<String>,
        /// Only events on or after this YYYY-MM-DD date
        #[arg(long)]
        since: Option<String>,
        /// Filter to a single asset symbol in the event's asset_impact list
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Add a new event annotation
    Add {
        #[arg(long = "event-date")]
        event_date: String,
        #[arg(long = "event-time")]
        event_time: Option<String>,
        #[arg(long)]
        category: String,
        #[arg(long)]
        headline: String,
        #[arg(long)]
        detail: Option<String>,
        #[arg(long)]
        source: Option<String>,
        /// Magnitude 1..=5 (default 3)
        #[arg(long, default_value = "3")]
        magnitude: i64,
        #[arg(long)]
        persistence: Option<String>,
        /// Comma-separated asset symbols affected
        #[arg(long = "asset-impact")]
        asset_impact: Option<String>,
        /// Comma-separated related scenario keys
        #[arg(long = "related-scenario")]
        related_scenario: Option<String>,
        /// Comma-separated related prediction ids
        #[arg(long = "related-prediction")]
        related_prediction: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsFragmentsCommand {
    /// List reasoning fragments. With `--for-claim`, classify and return
    /// applicable fragments via lesson_fragment_edges.
    List {
        #[arg(long = "type")]
        fragment_type: Option<String>,
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        cluster: Option<String>,
        #[arg(long = "for-claim")]
        for_claim: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show a single fragment with its lesson edges
    Show {
        canonical_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsThesisChainsCommand {
    /// List chains, optionally filtered by state or graph node
    List {
        /// Filter by current_state: confirmed, open, disconfirmed, stale
        #[arg(long)]
        state: Option<String>,
        /// Filter to chains touching a given node id or symbol substring
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show a single chain by id
    Show {
        id: i64,
        #[arg(long)]
        json: bool,
    },
    /// Evaluate antecedent + consequent against recent prices and update state
    Validate {
        id: i64,
        /// Reference date for the lookup (YYYY-MM-DD); defaults to today
        #[arg(long = "as-of")]
        as_of: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Heuristic backfill: scan thesis.content, prediction_lessons.why_wrong,
    /// and recent agent_messages for implication phrases and propose new
    /// chains. Dry-run by default — pass `--apply` to write.
    #[command(
        after_help = "Examples:\n  pftui analytics thesis-chains extract --dry-run --json\n  pftui analytics thesis-chains extract --from-thesis --from-lessons --since 90d --apply --json\n\nPatterns detected: 'if X then Y', 'when X, Y', 'X implies Y', 'X -> Y',\n'X drives/accelerates Y', 'X dampens/weakens Y', 'X contradicts Y',\n'X is contingent on Y'. De-dupes against existing chains."
    )]
    Extract {
        /// Read `thesis.content` rows
        #[arg(long = "from-thesis")]
        from_thesis: bool,
        /// Read `prediction_lessons.why_wrong` rows
        #[arg(long = "from-lessons")]
        from_lessons: bool,
        /// Read recent `agent_messages.content` rows
        #[arg(long = "from-messages")]
        from_messages: bool,
        /// Lookback window for `agent_messages` (e.g. 30d, 12w, 3m or YYYY-MM-DD)
        #[arg(long, default_value = "90d")]
        since: String,
        /// Show proposed chains without persisting (default)
        #[arg(long = "dry-run", default_value = "false")]
        dry_run: bool,
        /// Persist proposed chains via `thesis_dependencies::insert`
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
    /// Manually add a chain
    Add {
        /// Antecedent free-form text (e.g. "XAU > 4500")
        #[arg(long)]
        antecedent: String,
        /// Consequent free-form text (e.g. "BTC > 100000")
        #[arg(long)]
        consequent: String,
        /// Relation: implies, contradicts, contingent-on, accelerates, dampens
        #[arg(long)]
        relation: String,
        /// Optional canonical id for the antecedent node
        #[arg(long = "antecedent-id")]
        antecedent_id: Option<String>,
        /// Optional canonical id for the consequent node
        #[arg(long = "consequent-id")]
        consequent_id: Option<String>,
        #[arg(long)]
        conviction: Option<String>,
        /// Initial evidence_count (default 1)
        #[arg(long = "evidence-count", default_value = "1")]
        evidence_count: i64,
        /// Comma-separated source prediction_lessons ids
        #[arg(long = "source-lesson-ids")]
        source_lesson_ids: Option<String>,
        /// Comma-separated source thesis section slugs
        #[arg(long = "source-thesis-sections")]
        source_thesis_sections: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsFailuresCommand {
    /// Pairwise cluster co-failure correlations
    Correlations {
        #[arg(long)]
        cluster: Option<String>,
        /// Minimum co-wrong share, e.g. 0.5
        #[arg(long = "min-share")]
        min_share: Option<f64>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsClustersCommand {
    /// List distinct cluster_keys with lesson counts
    List {
        #[arg(long)]
        json: bool,
    },
    /// Cluster stats: lesson count + predictions referencing each cluster
    Stats {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsLessonsCommand {
    /// Lessons referenced by predictions created in a recent window
    Applied {
        /// Window to inspect: 24h, 7d, today, yesterday, or YYYY-MM-DD
        #[arg(long, default_value = "24h")]
        since: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Retire stale, uncited active lessons whose cluster is idle.
    ///
    /// A lesson is retired if it is `status='active'` AND has not been
    /// cited (or, if never cited, was created) more than
    /// `--retire-after-days` ago, AND has no recent wrong-scored
    /// predictions in the same topic cluster.
    ///
    /// Use `--dry-run` to preview without mutating. The status change is
    /// also journaled to `agent_messages` so analyst routines see that
    /// the substrate has been pruned.
    #[command(
        after_help = "Examples:\n  pftui analytics lessons curate --dry-run --json\n  pftui analytics lessons curate --retire-after-days 90\n\nSee also: analytics lessons revive, analytics lessons health"
    )]
    Curate {
        /// Do not mutate; report what would be retired
        #[arg(long)]
        dry_run: bool,
        /// Retire lessons stale for at least this many days
        #[arg(long = "retire-after-days", default_value = "60")]
        retire_after_days: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manually un-retire a previously retired lesson by id.
    #[command(after_help = "Example:\n  pftui analytics lessons revive 144 --json")]
    Revive {
        /// Lesson id (the `id` column of `prediction_lessons`)
        id: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Library health summary: total/active/retired counts and avg
    /// citations per active lesson.
    #[command(after_help = "Example:\n  pftui analytics lessons health --json")]
    Health {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Standing operational rules consolidated from the lesson library.
    ///
    /// The lesson book injects only the most recent ~25 lessons into
    /// prompts, so a failure pattern repeated across many lessons (e.g.
    /// magnitude overshoot) crowds out older distinct lessons. A standing
    /// rule consolidates one pattern into one imperative rule with its
    /// rationale and source lesson ids. Active rules are injected into
    /// analyst prompts in full.
    #[command(
        after_help = "Examples:\n  pftui analytics lessons rules add --rule \"Cap magnitude forecasts at 1.5x trailing realized vol.\" \\\n    --rationale \"Magnitude overshoot is the dominant repeated miss.\" --sources \"12,40,77\"\n  pftui analytics lessons rules list --json\n  pftui analytics lessons rules cite 3\n  pftui analytics lessons rules retire 3\n\nSee also: analytics lessons curate, analytics lessons health"
    )]
    Rules {
        #[command(subcommand)]
        command: AnalyticsLessonsRulesCommand,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsThesisCommand {
    /// Schedule a review-by date for a thesis section
    #[command(name = "set-review")]
    SetReview {
        /// Thesis section slug (must exist in the thesis table)
        section: String,
        /// Review-by date (YYYY-MM-DD)
        #[arg(long)]
        date: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List thesis sections due for review (review_by <= today) and unscheduled sections
    #[command(name = "review-due")]
    ReviewDue {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsLessonsRulesCommand {
    /// Add a standing rule (imperative, 1-3 sentences)
    Add {
        /// The operational rule text
        #[arg(long)]
        rule: String,
        /// Why — including the failure pattern this rule prevents
        #[arg(long)]
        rationale: Option<String>,
        /// Comma-separated prediction_lessons ids this rule consolidates (e.g. "12,40,77")
        #[arg(long)]
        sources: Option<String>,
        /// Enforcement level: advisory (prompt-injected) or validator (machine-checked)
        #[arg(long, default_value = "advisory")]
        enforcement: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List standing rules (active only by default; compact prompt-injectable render)
    List {
        /// Include retired rules
        #[arg(long)]
        all: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Retire a standing rule by id
    Retire {
        /// Rule id
        id: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Record a violation of a standing rule (increments violation_count)
    Cite {
        /// Rule id
        id: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsEnvironmentCommand {
    /// Show today's macro environment as z-scored features (vs their history)
    Current {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsStrategyCommand {
    /// Backtest an entry rule with an exit rule → trades, win rate, CAGR, drawdown vs buy-and-hold
    Backtest {
        /// Primary asset traded (alias or ticker, e.g. BTC, GC=F, SPY)
        #[arg(long)]
        asset: String,
        /// Entry condition expression (the rising edge opens a position)
        #[arg(long)]
        entry: String,
        /// Exit rule: "hold <N>d" (default "hold 90d") or a condition expression
        #[arg(long)]
        exit: Option<String>,
        /// Stop-loss percent (e.g. 15 = exit if price falls 15% from entry, checked intra-bar)
        #[arg(long = "stop-loss")]
        stop_loss: Option<f64>,
        /// Take-profit percent (e.g. 30 = exit if price rises 30% from entry, checked intra-bar)
        #[arg(long = "take-profit")]
        take_profit: Option<f64>,
        /// Trailing-stop percent (exit if price falls this % below the highest high since entry)
        #[arg(long = "trailing-stop")]
        trailing_stop: Option<f64>,
        /// Commission per side as a percent of notional (e.g. 0.1 = 0.1%/side; charged on entry AND exit)
        #[arg(long)]
        commission: Option<f64>,
        /// Slippage per side as a percent (entries fill higher, exits lower; e.g. 0.05 = 5bp/side)
        #[arg(long)]
        slippage: Option<f64>,
        /// Fill at the NEXT bar's close instead of the signal bar's close (removes same-bar look-ahead)
        #[arg(long = "next-bar-fill")]
        next_bar_fill: bool,
        /// Vol-target sizing: weight each trade to this annualized vol % (e.g. 20). Adds a risk-normalized equity curve.
        #[arg(long = "vol-target")]
        vol_target: Option<f64>,
        /// Trailing window (bars) for the realized-vol estimate used by --vol-target
        #[arg(long = "vol-window", default_value_t = 30)]
        vol_window: usize,
        /// Cap on per-trade leverage for --vol-target (e.g. 3 = never size above 3×)
        #[arg(long = "max-leverage", default_value_t = 3.0)]
        max_leverage: f64,
        /// Restrict the backtest to bars on/after this date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// Restrict the backtest to bars on/before this date (YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
        /// Limit the printed trade list (human output only)
        #[arg(long)]
        limit: Option<usize>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Segment an asset's forward returns by a regime mask (in-state vs out-of-state)
    Segment {
        /// Asset whose returns are segmented
        #[arg(long)]
        asset: String,
        /// Boolean condition defining the "in-state" regime
        #[arg(long)]
        when: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Compare an asset's forward returns under two independent regime masks (e.g. hiking vs cutting)
    Compare {
        #[arg(long)]
        asset: String,
        /// First regime condition
        #[arg(long)]
        when: String,
        /// Label for the first regime
        #[arg(long = "when-label", default_value = "regime-a")]
        when_label: String,
        /// Second regime condition
        #[arg(long)]
        vs: String,
        /// Label for the second regime
        #[arg(long = "vs-label", default_value = "regime-b")]
        vs_label: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Parse and resolve an expression, reporting series coverage without simulating
    Explain {
        #[arg(long)]
        asset: String,
        /// Expression to parse and resolve (entry or condition)
        #[arg(long)]
        entry: String,
        #[arg(long)]
        json: bool,
    },
    /// Parameter sweep with multiple-testing correction: backtest each value of $P and judge the BEST via Deflated Sharpe
    #[command(
        after_help = "Sweep one parameter across a grid and apply the Deflated Sharpe Ratio so the\nbest config is judged AFTER accounting for selection over N trials — the\noverfitting guard a single backtest can't give. Put `$P` in the entry rule where\nthe swept value goes.\n\nExamples:\n  pftui analytics strategy sweep --asset BTC --entry \"rsi(14) < $P\" --values \"20,25,30,35,40\" --exit \"hold 10d\"\n  pftui analytics strategy sweep --asset BTC --entry \"rsi($P) < 35\" --values \"7,14,21,28\" --exit \"hold 14d\" --json"
    )]
    Sweep {
        /// Primary asset traded (alias or ticker)
        #[arg(long)]
        asset: String,
        /// Entry rule containing the `$P` placeholder for the swept value
        #[arg(long)]
        entry: String,
        /// Comma-separated values to substitute for `$P` (e.g. "20,25,30,35,40")
        #[arg(long)]
        values: String,
        /// Exit rule: "hold <N>d" (default "hold 90d") or a condition expression
        #[arg(long)]
        exit: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Walk-forward optimization: optimize $P on each train fold, measure on the next held-out test fold (OOS)
    #[command(
        name = "walkforward",
        after_help = "Splits the timeline into folds, optimizes the `$P` parameter on each train\nsegment, then measures the chosen value on the NEXT (held-out, out-of-sample)\nsegment. The Walk-Forward Efficiency (avg OOS Sharpe / avg in-sample-best Sharpe)\nis the honest \"does the optimization generalize or is it curve-fit?\" read that\neven a deflated single sweep can't fully give. Warmup-correct (full-history\nindicators, trades partitioned by date).\n\nExamples:\n  pftui analytics strategy walkforward --asset BTC --entry \"rsi(14) < $P\" --values \"20,25,30,35,40\" --exit \"hold 10d\" --folds 4\n  pftui analytics strategy walkforward --asset BTC --entry \"rsi($P) < 35\" --values \"7,14,21,28\" --folds 5 --json"
    )]
    Walkforward {
        /// Primary asset traded (alias or ticker)
        #[arg(long)]
        asset: String,
        /// Entry rule containing the `$P` placeholder for the optimized value
        #[arg(long)]
        entry: String,
        /// Comma-separated values to optimize `$P` over (e.g. "20,25,30,35,40")
        #[arg(long)]
        values: String,
        /// Exit rule: "hold <N>d" (default "hold 90d") or a condition expression
        #[arg(long)]
        exit: Option<String>,
        /// Number of train/test folds (default 4; needs ≥2)
        #[arg(long, default_value_t = 4)]
        folds: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AnalyticsBacktestCommand {
    /// Replay scored predictions against historical prices for theoretical P&L
    Predictions {
        /// Filter by symbol (e.g. BTC-USD, GC=F)
        #[arg(long)]
        symbol: Option<String>,
        /// Filter by source agent (e.g. low-timeframe, high-timeframe)
        #[arg(long)]
        agent: Option<String>,
        /// Filter by timeframe (low, medium, high, macro)
        #[arg(long)]
        timeframe: Option<String>,
        /// Filter by conviction level (high, medium, low)
        #[arg(long)]
        conviction: Option<String>,
        /// Maximum number of predictions to include
        #[arg(long)]
        limit: Option<usize>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Aggregate backtest report: win rate by conviction, timeframe, asset class, and source agent
    #[command(
        after_help = "Aggregates prediction backtest results into a structured report.\nBreaks down win rate, P&L, and accuracy by:\n  - Conviction level (high/medium/low)\n  - Timeframe (low/medium/high/macro)\n  - Asset class (equity/crypto/commodity/fund/forex)\n  - Source agent (which timeframe analyst)\n\nIncludes a Sharpe-ratio equivalent for the prediction-based strategy\nand identifies the most/least reliable conviction levels and agents.\n\nExamples:\n  pftui analytics backtest report --json\n  pftui analytics backtest report\n\nSee also: analytics backtest predictions, analytics views accuracy"
    )]
    Report {
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Per-agent accuracy breakdown: detailed backtest profile for a specific agent
    #[command(
        after_help = "Produces a detailed accuracy profile for a single agent.\nIncludes win rate, P&L, Sharpe equivalent, streaks, best/worst trades,\nand breakdowns by conviction, timeframe, asset class, and symbol.\n\nAlso ranks the agent among all agents with ≥3 decided trades.\n\nExamples:\n  pftui analytics backtest agent --agent low-timeframe --json\n  pftui analytics backtest agent --agent macro-timeframe\n  pftui analytics backtest agent --agent high-timeframe --json\n\nSee also: analytics backtest report, analytics views accuracy"
    )]
    Agent {
        /// Agent name (e.g. low-timeframe, high-timeframe, macro-timeframe)
        #[arg(long)]
        agent: String,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Automated diagnostics: pattern detection, bias analysis, and actionable recommendations
    #[command(
        after_help = "Analyses backtest data to identify systematic prediction problems.\nDetects: poor win rates, asset class weaknesses, conviction miscalibration,\nmean-reversion bias, loss magnitude asymmetry, losing streaks, and overtrading.\n\nEach finding includes severity (critical/warning/info), a detailed explanation\nof what the data shows, and a specific actionable recommendation.\n\nOptional --agent filter narrows analysis to a single agent.\n\nExamples:\n  pftui analytics backtest diagnostics --json\n  pftui analytics backtest diagnostics --agent evening-analyst --json\n  pftui analytics backtest diagnostics\n\nSee also: analytics backtest report, analytics backtest agent"
    )]
    Diagnostics {
        /// Filter to a specific agent (optional — analyses all agents if omitted)
        #[arg(long)]
        agent: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Scenario-conditional backtest: hit rate of predictions made under a named regime
    #[command(
        after_help = "Computes hit rates conditioned on the regime that was active when each\nprediction was made. Joins `scenario_prediction_links` to `user_predictions`,\nfilters by per-scenario probability bands, and reports correct/partial/wrong.\n\nRegime presets:\n  --regime stagflation-iran-cool  (Inflation Spike ≥85 AND Iran ≤20)\n  --regime crisis                 (Hard Recession ≥40 AND Iran ≥30)\n  --regime risk-on                (Risk-On ≥40)\n\nExamples:\n  pftui analytics backtest scenario --regime stagflation-iran-cool --json\n  pftui analytics backtest scenario --inflation-min 80 --iran-max 25 --json\n  pftui analytics backtest scenario --regime crisis --layer LOW --topic commodities --json"
    )]
    Scenario {
        /// Regime preset name (stagflation-iran-cool, crisis, risk-on)
        #[arg(long)]
        regime: Option<String>,
        /// Inflation Spike probability minimum (0-100)
        #[arg(long)]
        inflation_min: Option<f64>,
        /// Inflation Spike probability maximum (0-100)
        #[arg(long)]
        inflation_max: Option<f64>,
        /// Hard Recession probability minimum (0-100)
        #[arg(long)]
        recession_min: Option<f64>,
        /// Hard Recession probability maximum (0-100)
        #[arg(long)]
        recession_max: Option<f64>,
        /// Iran-US escalation probability minimum (0-100)
        #[arg(long)]
        iran_min: Option<f64>,
        /// Iran-US escalation probability maximum (0-100)
        #[arg(long)]
        iran_max: Option<f64>,
        /// Risk-On probability minimum (0-100)
        #[arg(long)]
        risk_on_min: Option<f64>,
        /// Risk-On probability maximum (0-100)
        #[arg(long)]
        risk_on_max: Option<f64>,
        /// Filter by layer / timeframe (low, medium, high, macro)
        #[arg(long)]
        layer: Option<String>,
        /// Filter by topic (commodities, equities, crypto, fed, inflation, ...)
        #[arg(long)]
        topic: Option<String>,
        /// Filter by conviction (high, medium, low)
        #[arg(long)]
        conviction: Option<String>,
        /// Output as JSON for agent/script consumption
        #[arg(long)]
        json: bool,
    },
    /// Layer-bias matrix conditioned on a regime (LOW/MEDIUM/HIGH/MACRO × topic hit rates)
    #[command(
        after_help = "Same shape as the calibration matrix but conditioned on the regime.\nSurfaces rows like 'LOW layer commodities hit rate was 65% during\nstagflation-iran-cool but 30% during crisis'.\n\nExamples:\n  pftui analytics backtest layer-bias --regime stagflation-iran-cool --json\n  pftui analytics backtest layer-bias --regime crisis --json"
    )]
    LayerBias {
        /// Regime preset name
        #[arg(long)]
        regime: Option<String>,
        /// Inflation Spike probability minimum (0-100)
        #[arg(long)]
        inflation_min: Option<f64>,
        /// Inflation Spike probability maximum (0-100)
        #[arg(long)]
        inflation_max: Option<f64>,
        /// Hard Recession probability minimum (0-100)
        #[arg(long)]
        recession_min: Option<f64>,
        /// Hard Recession probability maximum (0-100)
        #[arg(long)]
        recession_max: Option<f64>,
        /// Iran-US escalation probability minimum (0-100)
        #[arg(long)]
        iran_min: Option<f64>,
        /// Iran-US escalation probability maximum (0-100)
        #[arg(long)]
        iran_max: Option<f64>,
        /// Risk-On probability minimum (0-100)
        #[arg(long)]
        risk_on_min: Option<f64>,
        /// Risk-On probability maximum (0-100)
        #[arg(long)]
        risk_on_max: Option<f64>,
        /// Output as JSON for agent/script consumption
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
    #[command(
        after_help = "Analyzes logged power flow events to produce a comprehensive assessment:\n\n\
        • Per-complex net scores, event counts, and trend direction\n\
        • First-half vs second-half trend comparison for momentum detection\n\
        • Directed power shifts between complexes\n\
        • Key events (magnitude ≥ 4)\n\
        • Regime classification (FIC/MIC/TIC-dominant or contested)\n\
        • Regime shift detection when a complex reverses direction\n\n\
        Designed for weekly assessments by medium-timeframe analysts.\n\n\
        See also: analytics power-flow balance, analytics power-flow list, analytics regime-flows"
    )]
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
    /// FIC/MIC conflict monitor: cross-references defense (ITA, XAR, PPA) with energy (XLE, CL=F) and VIX during crisis regimes
    #[command(
        after_help = "Cross-references defense sector ETFs (ITA, XAR, PPA, LMT, RTX) with\nenergy (XLE, CL=F, BZ=F) and VIX to produce a geopolitical conflict\nassessment.\n\nDetects conflict signals:\n  • Defense sector bid strength\n  • Oil supply-risk premium\n  • VIX fear regime\n  • Safe-haven gold bid\n  • Equity risk-off rotation\n\nIncludes a Defense/Energy ratio (ITA/XLE), composite conflict score (0-100),\nand cross-references logged FIC/MIC power flow events for structural context.\n\nExamples:\n  pftui analytics power-flow conflicts --json\n  pftui analytics power-flow conflicts --days 14\n\nSee also: analytics power-flow assess, analytics regime-flows, analytics crisis"
    )]
    Conflicts {
        /// Number of days for power flow lookback (default: 30)
        #[arg(long, default_value_t = 30)]
        days: usize,
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

    /// Print command execution time (elapsed_ms on stderr; agents can use for latency monitoring)
    #[arg(long, global = true)]
    pub timing: bool,

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
    #[command(
        after_help = "Looking for alerts? Use:\n  pftui data alerts check      Check alerts against current data\n  pftui data alerts list       List alert rules\n  pftui analytics alerts triage  Prioritized alert dashboard\n  pftui analytics alerts       Full alert management (add, ack, seed-defaults)"
    )]
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

    /// Prediction tracking shortcut for autoscore workflows
    Prediction {
        #[command(subcommand)]
        command: JournalPredictionCommand,
    },

    /// Report generation and chart-rendering primitives
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },

    /// Multi-timeframe analytics engine views (includes scenario, situation, signals, synthesis)
    #[command(
        name = "analytics",
        after_help = "Key subcommands:\n  alerts     Alert rules: add, list, check, ack, seed-defaults (also: data alerts)\n  scenario   Macro scenario tracking: probabilities, triggers, history (alias: scenarios)\n  situation  Situation Room: active situations, regime, branches, indicators\n  signals    Technical and cross-timeframe signals\n  synthesis  Cross-timeframe alignment and divergence analysis"
    )]
    Analytics {
        #[command(subcommand)]
        command: AnalyticsCommand,
    },

    /// Research harness: the signal registry and event-study engine — measured
    /// expectancy (baseline lift, MAE/MFE, significance) instead of narrative
    #[command(
        after_help = "The research harness converts deterministic engine signals\n(market structure, Cyber, cycle engine, SMA/RSI/Mayer thresholds) into\nMEASURED expectancy: per signal x asset x horizon forward-return stats\nwith baseline lift, MAE/MFE, overlap-honest significance and walk-forward\nas-of semantics.\n\nWorkflows:\n  pftui research signals list --json          # the registry (ids, versions)\n  pftui research backtest                     # all signals x held assets + SPY\n  pftui research backtest --asset GC=F --json # one asset, persist + print\n  pftui research expectancy --signal cyber_qb_flip_bear --json\n  pftui research events --signal structure_weekly_flip_down --asset BTC-USD"
    )]
    Research {
        #[command(subcommand)]
        command: ResearchCommand,
    },
}

// ---------------------------------------------------------------------------
// Research harness (R1a): signal registry + event-study engine
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
pub enum ResearchCommand {
    /// Retroactive forecast scoring: the analyst judgment stream as a scored corpus
    #[command(
        after_help = "Horizon conventions (canonical, fixed — src/research/forecast_scoring.rs):\n  low    7 trading days      medium 45 calendar days\n  high   135 calendar days   macro  365 calendar days\n  blind / antithesis score at ALL FOUR horizons (measurement layers)\n\nWorkflows:\n  pftui research forecasts score                 Backfill + fill elapsed pendings (idempotent)\n  pftui research forecasts report --asset GC=F   Per layer × asset hit rates and streaks\n  pftui research forecasts streaks --threshold 5 Current wrong-sign streak feed\n  pftui research forecasts verify                Recompute scored rows vs today's (repaired)\n                                                 price series; report drift > 0.5pp (read-only)\n  pftui research forecasts verify --reissue      Supersede drifted rows + insert corrected rows\n                                                 (append-only remediation; journaled)"
    )]
    Forecasts {
        #[command(subcommand)]
        command: ResearchForecastsCommand,
    },
    /// Active forecast misalignments: (layer, asset) pairs on a wrong-sign streak ≥ 5
    #[command(
        after_help = "A misalignment trips when a canonical layer's CURRENT consecutive\nwrong-sign streak on one asset reaches 5 (detected in the `data refresh`\ntail from the scored forecast corpus). While ACTIVE:\n  - the layer's views on that asset are on PROBATION — listed but excluded\n    from convergence voting (analytics views list/convergence mark them)\n  - `journal prediction add` caps that layer's confidence on the symbol\n    at 0.25 (--override-confidence-cap to bypass with rationale)\n  - `analytics epistemics record` counts it into run_health\nRecovery is mechanical: a scored direction HIT on the asset ends it.\n\nExamples:\n  pftui research misalignments            # active only\n  pftui research misalignments --all      # full episode ledger\n  pftui research misalignments --json"
    )]
    Misalignments {
        /// Show the full episode ledger (recovered episodes included)
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Competence dossier per analytical domain: measured expectancy, scored record, worked precedents
    #[command(
        after_help = "Compiles, from EXISTING measured data only (no narrative), the evidence\nthat a domain's signals and forecasts actually carry edge:\n  (a) the domain's signal-expectancy rows (ta → structure_/cyber_,\n      cycles → cycle_; macro → scenario-ledger discipline stats instead)\n  (b) the scored-forecast record for the domain's layers\n      (ta → low+medium, cycles → medium+high, macro → macro)\n  (c) worked precedents: the 3 highest-|lift| SIGNIFICANT signals with\n      their dated event lists and forward returns\nEmpty sections render \"no measured evidence yet\" — never prose.\n\nExamples:\n  pftui research dossier ta --asset GC=F\n  pftui research dossier cycles --json\n  pftui research dossier macro"
    )]
    Dossier {
        /// Analytical domain: ta | cycles | macro
        domain: String,
        /// Restrict to one asset/symbol
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Signal registry: canonical deterministic event emitters (id, version, description)
    Signals {
        #[command(subcommand)]
        command: ResearchSignalsCommand,
    },
    /// Run event studies (signals x assets), persist expectancy rows, print the table
    Backtest {
        /// Restrict to one signal id (default: every registry signal)
        #[arg(long)]
        signal: Option<String>,
        /// Restrict to one asset/symbol (default: held assets + SPY; deep
        /// series like BTC-USD are substituted automatically)
        #[arg(long)]
        asset: Option<String>,
        /// Walk-forward cutoff date YYYY-MM-DD (default: today). Only events
        /// and forward windows fully resolved by this date enter the stats
        #[arg(long = "as-of")]
        as_of: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Read the persisted expectancy table (latest as_of per signal x asset)
    Expectancy {
        /// Filter by signal id
        #[arg(long)]
        signal: Option<String>,
        /// Filter by asset/symbol
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Raw dated event list for one signal x asset with per-event forward returns
    Events {
        /// Signal id (see `research signals list`)
        #[arg(long)]
        signal: String,
        /// Asset/symbol (deep series substituted automatically)
        #[arg(long)]
        asset: String,
        /// Show only the most recent N events
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Shadow book: counterfactual portfolio that mechanically executes every recommendations-ledger row
    #[command(
        after_help = "The shadow book answers \"does following the desk beat ignoring it?\"\nwith a number. Three books, all seeded with the operator's ACTUAL\nholdings at inception (the first recommendations-ledger row's run_date),\nso shadow-vs-actual is a pure decisions-since-inception comparison:\n\n  SHADOW  executes every ledger row under the mechanical policy\n  ACTUAL  the operator's real transactions, valued daily\n  HOLD    inception holdings frozen — the do-nothing benchmark\n\nMechanical policy v1 (versioned — published numbers bind to it):\n  add  → +1.0pp of total NAV from cash into the symbol at the row's\n         entry_price (skipped with a warning when cash < 1pp)\n  trim → −1.0pp symbol→cash (capped at held value)\n  wait/hold/avoid → no trade; same-day rows apply in id order\n\nComputed on demand from recommendations + price_history + transactions —\nno shadow position tables. Under 90 days of ledger history the output\ncarries a BENCHMARK ACCRUING banner.\n\nExamples:\n  pftui research shadowbook\n  pftui research shadowbook --json"
    )]
    Shadowbook {
        #[arg(long)]
        json: bool,
    },
    /// Re-verify the thesis evidence contract: re-run embedded [pftui] SQL, recompute [derived], check [ext] references
    #[command(
        name = "verify-thesis",
        after_help = "Curated thesis sections carry numeric claims in a re-checkable evidence\nformat: [pftui] tags with verification SQL (fenced ```sql blocks or inline\nbackticked SELECTs), [derived] computed values, [ext: URL] citations. This\ncommand re-extracts every tagged claim, re-runs the SQL READ-ONLY against\nthe live DB, and classifies:\n\n  verified      re-run matches the claim (±2% numeric, exact dates)\n  drift         output near the claim but outside tolerance — claimed vs\n                current shown. SNAPSHOT claims (current/live/as-of framing)\n                drift by aging (severity info, staleness reported);\n                STRUCTURAL claims (cycle peaks, anchors) drifting is\n                suspect — an error or a data change\n  broken        the SQL errored (schema drift, repaired series) or an\n                [ext] reference is missing\n  unverifiable  tagged claim with no runnable SQL / no mechanical derivation\n  untagged      numeric claim with NO tag in a contract section — the\n                contract-violation class\n\nRepair stays curated: fix wrong STRUCTURAL values on the L4 thesis row\n(reviewed UPDATE / analytics thesis set) and journal the old→new change\n(author system, section system). Never rewrite SNAPSHOT values — refresh\nthe section's as-of line instead.\n\nExamples:\n  pftui research verify-thesis\n  pftui research verify-thesis --section btc-cycle-framework --json"
    )]
    VerifyThesis {
        /// Restrict to one thesis section (default: every section)
        #[arg(long)]
        section: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ResearchSignalsCommand {
    /// List every registered signal with version and description
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ResearchForecastsCommand {
    /// Score every analyst_view_history row not yet scored + fill elapsed pendings (idempotent)
    Score {
        #[arg(long)]
        json: bool,
    },
    /// Per (layer × asset × horizon) scored-forecast report with per-layer TOTALS
    Report {
        /// Filter to one layer (low|medium|high|macro|blind|antithesis)
        #[arg(long)]
        layer: Option<String>,
        /// Filter to one asset symbol
        #[arg(long)]
        asset: Option<String>,
        /// Only views recorded in the last N days
        #[arg(long)]
        window_days: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Current consecutive wrong-sign streaks ≥ threshold per (layer, asset)
    Streaks {
        /// Minimum current streak length to report
        #[arg(long, default_value_t = 5)]
        threshold: usize,
        #[arg(long)]
        json: bool,
    },
    /// Recompute every SCORED row against today's price series and report drift (read-only)
    Verify {
        /// Drift tolerance in percentage points (|recomputed − stored|)
        #[arg(long, default_value_t = 0.5)]
        threshold_pp: f64,
        /// Remediate drift: mark drifted rows status='superseded' and insert corrected rows (journaled)
        #[arg(long)]
        reissue: bool,
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
            "report",
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
    fn cycles_analyze_accepts_positional_and_asset_flag() {
        // Agent-ergonomics: the symbol may be given positionally (back-compat)
        // OR via --asset (parity with hurst/avwap/regime-break).
        let pos = Cli::parse_from(["pftui", "analytics", "cycles", "analyze", "BTC"]);
        let Some(Command::Analytics {
            command: AnalyticsCommand::Cycles { command },
        }) = pos.command
        else {
            panic!("expected cycles");
        };
        let AnalyticsCyclesCommand::Analyze { symbol, asset, .. } = command else {
            panic!("expected analyze");
        };
        assert_eq!(symbol.as_deref(), Some("BTC"));
        assert_eq!(asset, None);

        let flag = Cli::parse_from(["pftui", "analytics", "cycles", "analyze", "--asset", "gold"]);
        let Some(Command::Analytics {
            command: AnalyticsCommand::Cycles { command },
        }) = flag.command
        else {
            panic!("expected cycles");
        };
        let AnalyticsCyclesCommand::Analyze { symbol, asset, .. } = command else {
            panic!("expected analyze");
        };
        assert_eq!(symbol, None);
        assert_eq!(asset.as_deref(), Some("gold"));
    }

    #[test]
    fn parses_research_forecasts_commands() {
        let cli = Cli::try_parse_from(["pftui", "research", "forecasts", "score", "--json"])
            .expect("score parses");
        assert!(matches!(
            cli.command,
            Some(Command::Research {
                command: ResearchCommand::Forecasts {
                    command: ResearchForecastsCommand::Score { json: true }
                }
            })
        ));

        let cli = Cli::try_parse_from([
            "pftui",
            "research",
            "forecasts",
            "report",
            "--layer",
            "low",
            "--asset",
            "GC=F",
            "--window-days",
            "90",
        ])
        .expect("report parses");
        match cli.command {
            Some(Command::Research {
                command:
                    ResearchCommand::Forecasts {
                        command:
                            ResearchForecastsCommand::Report {
                                layer,
                                asset,
                                window_days,
                                json,
                            },
                    },
            }) => {
                assert_eq!(layer.as_deref(), Some("low"));
                assert_eq!(asset.as_deref(), Some("GC=F"));
                assert_eq!(window_days, Some(90));
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }

        let cli = Cli::try_parse_from(["pftui", "research", "forecasts", "streaks"])
            .expect("streaks parses");
        match cli.command {
            Some(Command::Research {
                command:
                    ResearchCommand::Forecasts {
                        command: ResearchForecastsCommand::Streaks { threshold, json },
                    },
            }) => {
                assert_eq!(threshold, 5, "default threshold");
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }

        let cli = Cli::try_parse_from(["pftui", "research", "forecasts", "verify"])
            .expect("verify parses");
        match cli.command {
            Some(Command::Research {
                command:
                    ResearchCommand::Forecasts {
                        command:
                            ResearchForecastsCommand::Verify {
                                threshold_pp,
                                reissue,
                                json,
                            },
                    },
            }) => {
                assert!((threshold_pp - 0.5).abs() < 1e-12, "default 0.5pp");
                assert!(!reissue, "verify is read-only by default");
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }

        let cli = Cli::try_parse_from([
            "pftui",
            "research",
            "forecasts",
            "verify",
            "--threshold-pp",
            "1.0",
            "--reissue",
            "--json",
        ])
        .expect("verify --reissue parses");
        assert!(matches!(
            cli.command,
            Some(Command::Research {
                command: ResearchCommand::Forecasts {
                    command: ResearchForecastsCommand::Verify {
                        reissue: true,
                        json: true,
                        ..
                    }
                }
            })
        ));
    }

    #[test]
    fn parses_research_misalignments_and_dossier() {
        let cli = Cli::try_parse_from(["pftui", "research", "misalignments"])
            .expect("misalignments parses");
        assert!(matches!(
            cli.command,
            Some(Command::Research {
                command: ResearchCommand::Misalignments {
                    all: false,
                    json: false
                }
            })
        ));

        let cli = Cli::try_parse_from(["pftui", "research", "misalignments", "--all", "--json"])
            .expect("misalignments flags parse");
        assert!(matches!(
            cli.command,
            Some(Command::Research {
                command: ResearchCommand::Misalignments {
                    all: true,
                    json: true
                }
            })
        ));

        let cli = Cli::try_parse_from(["pftui", "research", "dossier", "ta", "--asset", "GC=F"])
            .expect("dossier parses");
        match cli.command {
            Some(Command::Research {
                command:
                    ResearchCommand::Dossier {
                        domain,
                        asset,
                        json,
                    },
            }) => {
                assert_eq!(domain, "ta");
                assert_eq!(asset.as_deref(), Some("GC=F"));
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_research_verify_thesis() {
        let cli = Cli::try_parse_from([
            "pftui",
            "research",
            "verify-thesis",
            "--section",
            "btc-cycle-framework",
            "--json",
        ])
        .expect("verify-thesis parses");
        match cli.command {
            Some(Command::Research {
                command: ResearchCommand::VerifyThesis { section, json },
            }) => {
                assert_eq!(section.as_deref(), Some("btc-cycle-framework"));
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }

        let cli = Cli::try_parse_from(["pftui", "research", "verify-thesis"])
            .expect("bare verify-thesis parses");
        match cli.command {
            Some(Command::Research {
                command: ResearchCommand::VerifyThesis { section, json },
            }) => {
                assert!(section.is_none());
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_epistemics_record_forecast_flags() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "epistemics",
            "record",
            "--date",
            "2026-06-11",
            "--forecast-hit-rate",
            "0.41",
            "--active-misalignments",
            "2",
        ])
        .expect("epistemics record parses");
        match cli.command {
            Some(Command::Analytics {
                command:
                    AnalyticsCommand::Epistemics {
                        command:
                            AnalyticsEpistemicsCommand::Record {
                                forecast_hit_rate,
                                active_misalignments,
                                ..
                            },
                    },
            }) => {
                assert_eq!(forecast_hit_rate, Some(0.41));
                assert_eq!(active_misalignments, Some(2));
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_chart_from_db() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "stacked-bar",
            "--from-db",
            "portfolio",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "stacked-bar");
                assert_eq!(from_db.as_deref(), Some("portfolio"));
                assert!(from_json.is_none());
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_chart_from_json_out_png() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "prob-bar",
            "--from-json",
            "scenario.json",
            "--format",
            "png",
            "--out",
            "scenario.png",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        out,
                        format,
                        json,
                    },
            }) => {
                assert_eq!(chart_name, "prob-bar");
                assert!(from_db.is_none());
                assert_eq!(
                    from_json.as_deref(),
                    Some(std::path::Path::new("scenario.json"))
                );
                assert_eq!(out.as_deref(), Some(std::path::Path::new("scenario.png")));
                assert_eq!(format, ReportChartFormat::Png);
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_drift_bar_from_db() {
        let cli =
            Cli::try_parse_from(["pftui", "report", "chart", "drift-bar", "--from-db", "BTC"])
                .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "drift-bar");
                assert_eq!(from_db.as_deref(), Some("BTC"));
                assert!(from_json.is_none());
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(!json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_what_changed_strip_from_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "what-changed-strip",
            "--from-json",
            "deltas.json",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "what-changed-strip");
                assert!(from_db.is_none());
                assert_eq!(
                    from_json.as_deref(),
                    Some(std::path::Path::new("deltas.json"))
                );
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_open_predictions_table_from_db_html() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "open-predictions-table",
            "--from-db",
            "pending",
            "--format",
            "html",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "open-predictions-table");
                assert_eq!(from_db.as_deref(), Some("pending"));
                assert!(from_json.is_none());
                assert_eq!(format, ReportChartFormat::Html);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_outlook_arrows_from_db() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "outlook-arrows",
            "--from-db",
            "BTC",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "outlook-arrows");
                assert_eq!(from_db.as_deref(), Some("BTC"));
                assert!(from_json.is_none());
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_factor_exposure_from_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "factor-exposure",
            "--from-json",
            "factors.json",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "factor-exposure");
                assert!(from_db.is_none());
                assert_eq!(
                    from_json.as_deref(),
                    Some(std::path::Path::new("factors.json"))
                );
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_conviction_grid_from_db() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "conviction-grid",
            "--from-db",
            "all",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "conviction-grid");
                assert_eq!(from_db.as_deref(), Some("all"));
                assert!(from_json.is_none());
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_mismatch_card_from_json_html() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "mismatch-card",
            "--from-json",
            "mismatch.json",
            "--format",
            "html",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "mismatch-card");
                assert!(from_db.is_none());
                assert_eq!(from_json.unwrap(), PathBuf::from("mismatch.json"));
                assert_eq!(format, ReportChartFormat::Html);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_decision_card_from_json_html() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "decision-card",
            "--from-json",
            "decision.json",
            "--format",
            "html",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "decision-card");
                assert!(from_db.is_none());
                assert_eq!(from_json.unwrap(), PathBuf::from("decision.json"));
                assert_eq!(format, ReportChartFormat::Html);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_regime_quadrant_from_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "regime-quadrant",
            "--from-json",
            "regime.json",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "regime-quadrant");
                assert!(from_db.is_none());
                assert_eq!(from_json.unwrap(), PathBuf::from("regime.json"));
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_conviction_trajectory_from_db() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "conviction-trajectory",
            "--from-db",
            "BTC 30d",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "conviction-trajectory");
                assert_eq!(from_db.as_deref(), Some("BTC 30d"));
                assert!(from_json.is_none());
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_analyst_convergence_card_from_db_html() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "analyst-convergence-card",
            "--from-db",
            "Gold 30d",
            "--format",
            "html",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "analyst-convergence-card");
                assert_eq!(from_db.as_deref(), Some("Gold 30d"));
                assert!(from_json.is_none());
                assert_eq!(format, ReportChartFormat::Html);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_report_calibration_reliability_from_db() {
        let cli = Cli::try_parse_from([
            "pftui",
            "report",
            "chart",
            "calibration-reliability",
            "--from-db",
            "90d",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Report {
                command:
                    ReportCommand::Chart {
                        chart_name,
                        from_db,
                        from_json,
                        format,
                        json,
                        ..
                    },
            }) => {
                assert_eq!(chart_name, "calibration-reliability");
                assert_eq!(from_db.as_deref(), Some("90d"));
                assert!(from_json.is_none());
                assert_eq!(format, ReportChartFormat::Svg);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
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
    fn parses_portfolio_status() {
        let cli = Cli::try_parse_from(["pftui", "portfolio", "status", "--json"]).unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command: Some(PortfolioCommand::Status { json }),
            }) => assert!(json),
            _ => panic!("expected portfolio status command"),
        }
    }

    #[test]
    fn parses_portfolio_status_no_json() {
        let cli = Cli::try_parse_from(["pftui", "portfolio", "status"]).unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command: Some(PortfolioCommand::Status { json }),
            }) => assert!(!json),
            _ => panic!("expected portfolio status command"),
        }
    }

    #[test]
    fn parses_portfolio_drawdown_json() {
        let cli = Cli::try_parse_from(["pftui", "portfolio", "drawdown", "--json"]).unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command: Some(PortfolioCommand::Drawdown { json }),
            }) => assert!(json),
            _ => panic!("expected portfolio drawdown command"),
        }
    }

    #[test]
    fn parses_portfolio_set_cash_safety_flags() {
        let cli = Cli::try_parse_from([
            "pftui",
            "portfolio",
            "set-cash",
            "USD",
            "45000",
            "--confirm",
            "--dry-run",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command:
                    Some(PortfolioCommand::SetCash {
                        symbol,
                        amount,
                        confirm,
                        dry_run,
                        json,
                    }),
            }) => {
                assert_eq!(symbol, "USD");
                assert_eq!(amount, "45000");
                assert!(confirm);
                assert!(dry_run);
                assert!(json);
            }
            _ => panic!("expected portfolio set-cash command"),
        }
    }

    #[test]
    fn parses_portfolio_transaction_add_preview_flags() {
        let cli = Cli::try_parse_from([
            "pftui",
            "portfolio",
            "transaction",
            "add",
            "--symbol",
            "GC=F",
            "--category",
            "commodity",
            "--tx-type",
            "buy",
            "--quantity",
            "2",
            "--price",
            "4500",
            "--dry-run",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command:
                    Some(PortfolioCommand::Transaction {
                        command:
                            PortfolioTransactionCommand::Add {
                                symbol,
                                dry_run,
                                json,
                                ..
                            },
                    }),
            }) => {
                assert_eq!(symbol.as_deref(), Some("GC=F"));
                assert!(dry_run);
                assert!(json);
            }
            _ => panic!("expected portfolio transaction add command"),
        }
    }

    #[test]
    fn parses_portfolio_transaction_remove_preview_flags() {
        let cli = Cli::try_parse_from([
            "pftui",
            "portfolio",
            "transaction",
            "remove",
            "21",
            "--unpaired",
            "--dry-run",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command:
                    Some(PortfolioCommand::Transaction {
                        command:
                            PortfolioTransactionCommand::Remove {
                                id,
                                unpaired,
                                dry_run,
                                json,
                            },
                    }),
            }) => {
                assert_eq!(id, 21);
                assert!(unpaired);
                assert!(dry_run);
                assert!(json);
            }
            _ => panic!("expected portfolio transaction remove command"),
        }
    }

    #[test]
    fn parses_portfolio_transaction_repair_pairs_dry_run() {
        let cli = Cli::try_parse_from([
            "pftui",
            "portfolio",
            "transaction",
            "repair-pairs",
            "--dry-run",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command:
                    Some(PortfolioCommand::Transaction {
                        command:
                            PortfolioTransactionCommand::RepairPairs {
                                dry_run,
                                confirm,
                                skip,
                                max_days,
                                max_notional_pct,
                                json,
                            },
                    }),
            }) => {
                assert!(dry_run);
                assert!(!confirm);
                assert!(skip.is_empty());
                assert_eq!(max_days, 2);
                assert!((max_notional_pct - 10.0).abs() < f64::EPSILON);
                assert!(json);
            }
            _ => panic!("expected portfolio transaction repair-pairs command"),
        }
    }

    #[test]
    fn parses_portfolio_transaction_repair_pairs_confirm_with_skip() {
        let cli = Cli::try_parse_from([
            "pftui",
            "portfolio",
            "transaction",
            "repair-pairs",
            "--confirm",
            "--skip",
            "17",
            "--skip",
            "42",
            "--max-days",
            "3",
            "--max-notional-pct",
            "15.0",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command:
                    Some(PortfolioCommand::Transaction {
                        command:
                            PortfolioTransactionCommand::RepairPairs {
                                dry_run,
                                confirm,
                                skip,
                                max_days,
                                max_notional_pct,
                                json,
                            },
                    }),
            }) => {
                assert!(!dry_run);
                assert!(confirm);
                assert_eq!(skip, vec![17, 42]);
                assert_eq!(max_days, 3);
                assert!((max_notional_pct - 15.0).abs() < f64::EPSILON);
                assert!(!json);
            }
            _ => panic!("expected portfolio transaction repair-pairs command"),
        }
    }

    #[test]
    fn parses_portfolio_transaction_import_delta() {
        let cli = Cli::try_parse_from([
            "pftui",
            "portfolio",
            "transaction",
            "import-delta",
            "export.csv",
            "--apply",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command:
                    Some(PortfolioCommand::Transaction {
                        command:
                            PortfolioTransactionCommand::ImportDelta {
                                csv,
                                dry_run,
                                apply,
                                json,
                            },
                    }),
            }) => {
                assert_eq!(csv, "export.csv");
                assert!(!dry_run);
                assert!(apply);
                assert!(json);
            }
            _ => panic!("expected portfolio transaction import-delta command"),
        }
    }

    #[test]
    fn parses_analytics_news_sources_accuracy_include_pre_deployment() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "news-sources",
            "accuracy",
            "--window-days",
            "365",
            "--include-pre-deployment",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::NewsSources {
                    command:
                        AnalyticsNewsSourcesCommand::Accuracy {
                            window_days,
                            include_pre_deployment,
                            json,
                            ..
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics news-sources accuracy command");
        };
        assert_eq!(window_days, Some(365));
        assert!(include_pre_deployment);
        assert!(json);
    }

    #[test]
    fn parse_portfolio_snapshot_alias_resolves_to_status() {
        let cli = Cli::try_parse_from(["pftui", "portfolio", "snapshot", "--json"]).unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command: Some(PortfolioCommand::Status { json }),
            }) => assert!(json),
            _ => panic!("expected portfolio status via snapshot alias"),
        }
    }

    #[test]
    fn parse_portfolio_snapshot_alias_no_flags() {
        let cli = Cli::try_parse_from(["pftui", "portfolio", "snapshot"]).unwrap();
        match cli.command {
            Some(Command::Portfolio {
                command: Some(PortfolioCommand::Status { json }),
            }) => assert!(!json),
            _ => panic!("expected portfolio status via snapshot alias"),
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
                command:
                    DataCommand::Cot {
                        symbol,
                        force_refresh,
                        json,
                    },
            }) => {
                assert_eq!(symbol.as_deref(), Some("GC=F"));
                assert!(!force_refresh);
                assert!(json);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn parses_data_cot_force_refresh_command() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "cot", "--force-refresh", "--json"]).unwrap();
        match cli.command {
            Some(Command::Data {
                command:
                    DataCommand::Cot {
                        symbol,
                        force_refresh,
                        json,
                    },
            }) => {
                assert!(symbol.is_none());
                assert!(force_refresh);
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
                        command: AgentMessageCommand::Ack { id, all, to, json },
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
                        command: AgentMessageCommand::Ack { id, all, to, json },
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
        let result =
            Cli::try_parse_from(["pftui", "agent", "message", "ack", "--id", "1", "--all"]);
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
    fn parse_prediction_rescore_audit_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "rescore-audit",
            "--apply-high-confidence",
            "--json",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::RescoreAudit {
                            apply_high_confidence,
                            json,
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction rescore-audit command");
        };

        assert!(apply_high_confidence);
        assert!(json);
    }

    #[test]
    fn parse_prediction_rescore_audit_dry_default() {
        let cli = Cli::try_parse_from(["pftui", "journal", "prediction", "rescore-audit"])
            .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::RescoreAudit {
                            apply_high_confidence,
                            json,
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction rescore-audit command");
        };

        assert!(!apply_high_confidence);
        assert!(!json);
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
                            id,
                            note_pos,
                            probability,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal scenario update command");
        };

        assert_eq!(value.as_deref(), Some("Hard Landing"));
        assert_eq!(id, None);
        assert_eq!(note_pos.as_deref(), Some("labor rolling over"));
        assert_eq!(probability, Some(65.0));
    }

    #[test]
    fn parse_scenario_update_with_id() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "scenario",
            "update",
            "--id",
            "42",
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
                            id,
                            probability,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal scenario update command");
        };

        assert_eq!(value, None);
        assert_eq!(id, Some(42));
        assert_eq!(probability, Some(65.0));
    }

    #[test]
    fn parse_analytics_calibration_default() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "calibration", "--json"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Calibration {
                    threshold,
                    window_days,
                    by_layer,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected analytics calibration command");
        };
        assert!((threshold - 15.0).abs() < f64::EPSILON);
        assert_eq!(window_days, 90);
        assert!(!by_layer);
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
            "--window-days",
            "30",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Calibration {
                    threshold,
                    window_days,
                    by_layer,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected analytics calibration command");
        };
        assert!((threshold - 10.0).abs() < f64::EPSILON);
        assert_eq!(window_days, 30);
        assert!(!by_layer);
        assert!(json);
    }

    #[test]
    fn parse_analytics_calibration_by_layer() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "calibration", "--by-layer", "--json"])
                .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Calibration {
                    threshold,
                    window_days,
                    by_layer,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected analytics calibration command");
        };
        assert!((threshold - 15.0).abs() < f64::EPSILON);
        assert_eq!(window_days, 90);
        assert!(by_layer);
        assert!(json);
    }

    #[test]
    fn parse_analytics_narrative_divergence_custom() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "narrative-divergence",
            "--hours",
            "48",
            "--threshold",
            "1.5",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::NarrativeDivergence {
                    hours,
                    threshold,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected analytics narrative divergence command");
        };
        assert_eq!(hours, 48);
        assert!((threshold - 1.5).abs() < f64::EPSILON);
        assert!(json);
    }

    #[test]
    fn parse_analytics_news_silence_custom() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "news-silence",
            "--window-days",
            "60",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::NewsSilence {
                    command: subcmd,
                    window_days,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected analytics news silence command");
        };
        assert!(subcmd.is_none());
        assert_eq!(window_days, 60);
        assert!(json);
    }

    #[test]
    fn parse_analytics_news_silence_rebuild_baselines() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "news-silence",
            "rebuild-baselines",
            "--since",
            "90d",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::NewsSilence {
                    command: Some(AnalyticsNewsSilenceCommand::RebuildBaselines { since, json }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics news-silence rebuild-baselines command");
        };
        assert_eq!(since, "90d");
        assert!(json);
    }

    #[test]
    fn parse_analytics_news_sources_rebuild_accuracy() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "news-sources",
            "rebuild-accuracy",
            "--since",
            "180d",
            "--dry-run",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::NewsSources {
                    command:
                        AnalyticsNewsSourcesCommand::RebuildAccuracy {
                            since,
                            dry_run,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics news-sources rebuild-accuracy command");
        };
        assert_eq!(since.as_deref(), Some("180d"));
        assert!(dry_run);
        assert!(json);
    }

    #[test]
    fn parse_system_data_coverage() {
        let cli = Cli::try_parse_from(["pftui", "system", "data-coverage", "--json"]).unwrap();

        let Some(Command::System {
            command: SystemCommand::DataCoverage { json },
        }) = cli.command
        else {
            panic!("expected system data-coverage command");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_lessons_applied() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "lessons",
            "applied",
            "--since",
            "24h",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Lessons { command },
        }) = cli.command
        else {
            panic!("expected analytics lessons command");
        };
        let AnalyticsLessonsCommand::Applied { since, json } = command else {
            panic!("expected analytics lessons applied");
        };
        assert_eq!(since, "24h");
        assert!(json);
    }

    #[test]
    fn parse_analytics_lessons_curate() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "lessons",
            "curate",
            "--dry-run",
            "--retire-after-days",
            "45",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics {
            command: AnalyticsCommand::Lessons { command },
        }) = cli.command
        else {
            panic!("expected analytics lessons command");
        };
        let AnalyticsLessonsCommand::Curate {
            dry_run,
            retire_after_days,
            json,
        } = command
        else {
            panic!("expected analytics lessons curate");
        };
        assert!(dry_run);
        assert_eq!(retire_after_days, 45);
        assert!(json);
    }

    #[test]
    fn parse_analytics_lessons_revive_and_health() {
        let revive_cli =
            Cli::try_parse_from(["pftui", "analytics", "lessons", "revive", "144", "--json"])
                .unwrap();
        let Some(Command::Analytics {
            command: AnalyticsCommand::Lessons { command },
        }) = revive_cli.command
        else {
            panic!("expected analytics lessons command");
        };
        let AnalyticsLessonsCommand::Revive { id, json } = command else {
            panic!("expected analytics lessons revive");
        };
        assert_eq!(id, 144);
        assert!(json);

        let health_cli =
            Cli::try_parse_from(["pftui", "analytics", "lessons", "health", "--json"]).unwrap();
        let Some(Command::Analytics {
            command: AnalyticsCommand::Lessons { command },
        }) = health_cli.command
        else {
            panic!("expected analytics lessons command");
        };
        let AnalyticsLessonsCommand::Health { json } = command else {
            panic!("expected analytics lessons health");
        };
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
                            allocation_bias: _,
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
        let cli = Cli::try_parse_from(["pftui", "analytics", "views", "matrix", "--json"]).unwrap();

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
    fn parse_analytics_views_portfolio_matrix() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "views", "portfolio-matrix", "--json"])
                .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command: AnalyticsViewsCommand::PortfolioMatrix { json },
                },
        }) = cli.command
        else {
            panic!("expected analytics views portfolio-matrix command");
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
    fn parse_analytics_views_history() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "views",
            "history",
            "--asset",
            "BTC",
            "--analyst",
            "low",
            "--limit",
            "20",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::History {
                            asset,
                            analyst,
                            limit,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views history command");
        };
        assert_eq!(asset, "BTC");
        assert_eq!(analyst.as_deref(), Some("low"));
        assert_eq!(limit, Some(20));
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_history_minimal() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "views", "history", "--asset", "GLD"])
            .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::History {
                            asset,
                            analyst,
                            limit,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views history command");
        };
        assert_eq!(asset, "GLD");
        assert!(analyst.is_none());
        assert!(limit.is_none());
        assert!(!json);
    }

    #[test]
    fn parse_analytics_views_divergence() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "views",
            "divergence",
            "--min-spread",
            "3",
            "--asset",
            "BTC",
            "--layer",
            "high",
            "--limit",
            "5",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::Divergence {
                            min_spread,
                            asset,
                            layer,
                            limit,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views divergence command");
        };
        assert_eq!(min_spread, 3);
        assert_eq!(asset.as_deref(), Some("BTC"));
        assert_eq!(layer.as_deref(), Some("high"));
        assert_eq!(limit, Some(5));
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_divergence_defaults() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "views", "divergence", "--json"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::Divergence {
                            min_spread,
                            asset,
                            layer,
                            limit,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views divergence command");
        };
        assert_eq!(min_spread, 2); // default
        assert!(asset.is_none());
        assert!(layer.is_none());
        assert!(limit.is_none());
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_accuracy() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "views",
            "accuracy",
            "--analyst",
            "low",
            "--asset",
            "BTC",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::Accuracy {
                            analyst,
                            asset,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views accuracy command");
        };
        assert_eq!(analyst.as_deref(), Some("low"));
        assert_eq!(asset.as_deref(), Some("BTC"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_accuracy_defaults() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "views", "accuracy", "--json"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::Accuracy {
                            analyst,
                            asset,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics views accuracy command");
        };
        assert!(analyst.is_none());
        assert!(asset.is_none());
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
                    command: AnalyticsDebateScoreCommand::List { winner, json, .. },
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
                    command: AnalyticsDebateScoreCommand::Accuracy { topic, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics debate-score accuracy command");
        };
        assert_eq!(topic.as_deref(), Some("BTC"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_news_sources_accuracy() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "news-sources",
            "accuracy",
            "--domain",
            "bloomberg.com",
            "--topic",
            "fed",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::NewsSources {
                    command:
                        AnalyticsNewsSourcesCommand::Accuracy {
                            domain,
                            topic,
                            json,
                            ..
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics news-sources accuracy command");
        };
        assert_eq!(domain.as_deref(), Some("bloomberg.com"));
        assert_eq!(topic.as_deref(), Some("fed"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_news_sources_rank() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "news-sources",
            "rank",
            "--topic",
            "iran",
            "--limit",
            "5",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::NewsSources {
                    command:
                        AnalyticsNewsSourcesCommand::Rank {
                            topic,
                            limit,
                            window_days,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics news-sources rank command");
        };
        assert_eq!(topic.as_deref(), Some("iran"));
        assert_eq!(limit, 5);
        assert_eq!(window_days, 180);
        assert!(!json);
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
                    command: AnalyticsDebateScoreCommand::Unscored { limit, json },
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
    fn parse_analytics_scenario_timeline() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "timeline",
            "--days",
            "14",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::Timeline { days, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario timeline command");
        };

        assert_eq!(days, Some(14));
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_timeline_no_args() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "scenario", "timeline"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::Timeline { days, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario timeline command");
        };

        assert_eq!(days, None);
        assert!(!json);
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
        let cli = Cli::try_parse_from(["pftui", "analytics", "scenario", "suggest"]).unwrap();

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
    fn parse_analytics_scenario_detect_with_options() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "scenario",
            "detect",
            "--hours",
            "48",
            "--limit",
            "3",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Scenario {
                    command: AnalyticsScenarioCommand::Detect { hours, limit, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics scenario detect command");
        };

        assert_eq!(hours, 48);
        assert_eq!(limit, 3);
        assert!(json);
    }

    #[test]
    fn parse_analytics_scenario_impact_matrix_json() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "scenario", "impact-matrix", "--json"])
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
        let cli = Cli::try_parse_from(["pftui", "analytics", "scenario", "impact-matrix"]).unwrap();

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
    fn parse_analytics_macro_regime_history_with_date_filters() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "regime",
            "history",
            "--from",
            "2026-03-20",
            "--to",
            "2026-03-30",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Regime {
                            command:
                                AnalyticsMacroRegimeCommand::History {
                                    limit,
                                    from,
                                    to,
                                    json,
                                },
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro regime history command");
        };

        assert!(limit.is_none());
        assert_eq!(from.as_deref(), Some("2026-03-20"));
        assert_eq!(to.as_deref(), Some("2026-03-30"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_regime_transitions_with_date_filters() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "regime",
            "transitions",
            "--from",
            "2026-03-15",
            "--limit",
            "10",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Regime {
                            command:
                                AnalyticsMacroRegimeCommand::Transitions {
                                    limit,
                                    from,
                                    to,
                                    json,
                                },
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro regime transitions command");
        };

        assert_eq!(limit, Some(10));
        assert_eq!(from.as_deref(), Some("2026-03-15"));
        assert!(to.is_none());
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_regime_summary() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "regime",
            "summary",
            "--from",
            "2026-03-01",
            "--to",
            "2026-03-31",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Regime {
                            command: AnalyticsMacroRegimeCommand::Summary { from, to, json },
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro regime summary command");
        };

        assert_eq!(from.as_deref(), Some("2026-03-01"));
        assert_eq!(to.as_deref(), Some("2026-03-31"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_regime_confidence_trend() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "regime",
            "confidence-trend",
            "--window",
            "10",
            "--from",
            "2026-03-01",
            "--limit",
            "50",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Regime {
                            command:
                                AnalyticsMacroRegimeCommand::ConfidenceTrend {
                                    limit,
                                    window,
                                    from,
                                    to,
                                    json,
                                },
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro regime confidence-trend command");
        };

        assert_eq!(limit, Some(50));
        assert_eq!(window, 10);
        assert_eq!(from.as_deref(), Some("2026-03-01"));
        assert!(to.is_none());
        assert!(json);
    }

    #[test]
    fn parse_analytics_macro_regime_confidence_trend_defaults() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "macro", "regime", "confidence-trend"])
                .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Regime {
                            command:
                                AnalyticsMacroRegimeCommand::ConfidenceTrend {
                                    limit,
                                    window,
                                    from,
                                    to,
                                    json,
                                },
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro regime confidence-trend command");
        };

        assert!(limit.is_none());
        assert_eq!(window, 5); // default
        assert!(from.is_none());
        assert!(to.is_none());
        assert!(!json);
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
                            command: Some(AnalyticsMacroCyclesCommand::Current { country, json }),
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
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "macro", "cycles", "current", "--json"])
                .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Cycles {
                            command: Some(AnalyticsMacroCyclesCommand::Current { country, json }),
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
    fn parse_analytics_technicals_symbols_alias() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "technicals",
            "--symbols",
            "BTC,GC=F",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Technicals { symbol, json, .. },
        }) = cli.command
        else {
            panic!("expected analytics technicals command");
        };

        assert_eq!(symbol.as_deref(), Some("BTC,GC=F"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_technicals_include_channels() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "technicals",
            "--symbols",
            "TEST",
            "--include",
            "gaussian-channel,zone-channel,volatility-trend,donchian-trend",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command: AnalyticsCommand::Technicals { include, json, .. },
        }) = cli.command
        else {
            panic!("expected analytics technicals command");
        };

        assert_eq!(
            include.as_deref(),
            Some("gaussian-channel,zone-channel,volatility-trend,donchian-trend")
        );
        assert!(json);
    }

    #[test]
    fn parse_analytics_technicals_structure_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "technicals",
            "structure",
            "GC=F",
            "--timeframe",
            "weekly",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Technicals {
                    command:
                        Some(AnalyticsTechnicalsCommand::Structure {
                            symbol,
                            timeframe,
                            json,
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics technicals structure command");
        };

        assert_eq!(symbol, "GC=F");
        assert_eq!(timeframe, "weekly");
        assert!(json);
    }

    #[test]
    fn parse_analytics_technicals_structure_defaults_to_daily() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "technicals", "structure", "BTC"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Technicals {
                    command:
                        Some(AnalyticsTechnicalsCommand::Structure {
                            symbol,
                            timeframe,
                            json,
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics technicals structure command");
        };

        assert_eq!(symbol, "BTC");
        assert_eq!(timeframe, "daily");
        assert!(!json);
    }

    #[test]
    fn parse_analytics_technicals_cyber_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "technicals",
            "cyber",
            "BTC",
            "--timeframe",
            "weekly",
            "--lookback-signals",
            "5",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Technicals {
                    command:
                        Some(AnalyticsTechnicalsCommand::Cyber {
                            symbol,
                            timeframe,
                            lookback_signals,
                            json,
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics technicals cyber command");
        };

        assert_eq!(symbol, "BTC");
        assert_eq!(timeframe, "weekly");
        assert_eq!(lookback_signals, 5);
        assert!(json);
    }

    #[test]
    fn parse_analytics_technicals_cyber_defaults() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "technicals", "cyber", "GC=F"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Technicals {
                    command:
                        Some(AnalyticsTechnicalsCommand::Cyber {
                            symbol,
                            timeframe,
                            lookback_signals,
                            json,
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics technicals cyber command");
        };

        assert_eq!(symbol, "GC=F");
        assert_eq!(timeframe, "daily");
        assert_eq!(lookback_signals, 10);
        assert!(!json);
    }

    #[test]
    fn parse_analytics_cycles_clock_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "cycles",
            "clock",
            "--asset",
            "BTC",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Cycles {
                    command: AnalyticsCyclesCommand::Clock { asset, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics cycles clock command");
        };

        assert_eq!(asset.as_deref(), Some("BTC"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_cycles_clock_no_asset() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "cycles", "clock"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Cycles {
                    command: AnalyticsCyclesCommand::Clock { asset, json },
                },
        }) = cli.command
        else {
            panic!("expected analytics cycles clock command");
        };

        assert!(asset.is_none());
        assert!(!json);
    }

    #[test]
    fn parse_analytics_cycles_analyze_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "cycles",
            "analyze",
            "GC=F",
            "--degree",
            "major",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Cycles {
                    command:
                        AnalyticsCyclesCommand::Analyze {
                            symbol,
                            asset: _,
                            degree,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics cycles analyze command");
        };

        assert_eq!(symbol.as_deref(), Some("GC=F"));
        assert_eq!(degree.as_deref(), Some("major"));
        assert!(json);
    }

    #[test]
    fn parse_analytics_cycles_analyze_no_degree() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "cycles", "analyze", "BTC"]).unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Cycles {
                    command:
                        AnalyticsCyclesCommand::Analyze {
                            symbol,
                            asset: _,
                            degree,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics cycles analyze command");
        };

        assert_eq!(symbol.as_deref(), Some("BTC"));
        assert!(degree.is_none());
        assert!(!json);
    }

    #[test]
    fn parse_analytics_cycles_ledger_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "cycles",
            "ledger",
            "BTC",
            "--degree",
            "investor",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Cycles {
                    command:
                        AnalyticsCyclesCommand::Ledger {
                            symbol,
                            asset: _,
                            degree,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected analytics cycles ledger command");
        };

        assert_eq!(symbol.as_deref(), Some("BTC"));
        assert_eq!(degree, "investor");
        assert!(json);
    }

    #[test]
    fn parse_analytics_cycles_ledger_requires_degree() {
        assert!(Cli::try_parse_from(["pftui", "analytics", "cycles", "ledger", "BTC"]).is_err());
    }

    #[test]
    fn parse_analytics_macro_log_add_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "macro",
            "log",
            "add",
            "--development",
            "Fed credibility slipping",
            "--cycle-impact",
            "Late-cycle fragility",
            "--outcome-shift",
            "Higher stagflation odds",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Macro {
                    command:
                        Some(AnalyticsMacroCommand::Log {
                            command:
                                Some(AnalyticsMacroLogCommand::Add {
                                    value,
                                    development,
                                    date,
                                    cycle_impact,
                                    outcome_shift,
                                    json,
                                }),
                            ..
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics macro log add command");
        };

        assert!(value.is_none());
        assert_eq!(development.as_deref(), Some("Fed credibility slipping"));
        assert!(date.is_none());
        assert_eq!(cycle_impact.as_deref(), Some("Late-cycle fragility"));
        assert_eq!(outcome_shift.as_deref(), Some("Higher stagflation odds"));
        assert!(json);
    }

    #[test]
    fn macro_outcomes_help_points_to_scenario_update() -> Result<()> {
        let help = subcommand_help(&["analytics", "macro", "outcomes"])?;
        assert!(help.contains("journal scenario update"));
        assert!(help.contains("--id 42 --probability 65"));
        Ok(())
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
    fn parse_analytics_signals_direction_filter() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "signals",
            "--direction",
            "bullish",
            "--severity",
            "critical",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Signals {
                    direction,
                    severity,
                    json,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics signals command");
        };

        assert_eq!(direction.as_deref(), Some("bullish"));
        assert_eq!(severity.as_deref(), Some("critical"));
        assert!(json);
    }

    #[test]
    fn parse_situation_update_log_accepts_elevated_severity() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "situation",
            "update",
            "log",
            "--situation",
            "Iran Escalation",
            "--headline",
            "Brent above 95",
            "--severity",
            "elevated",
            "--json",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Situation {
                    command:
                        Some(SituationCommand::Update {
                            command: SituationUpdateCommand::Log { severity, json, .. },
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics situation update log command");
        };

        assert_eq!(severity, "elevated");
        assert!(json);
    }

    #[test]
    fn parse_situation_update_log_rejects_unknown_severity() {
        let result = Cli::try_parse_from([
            "pftui",
            "analytics",
            "situation",
            "update",
            "log",
            "--situation",
            "Iran Escalation",
            "--headline",
            "Brent above 95",
            "--severity",
            "high",
        ]);
        let err = match result {
            Ok(_) => panic!("expected clap to reject invalid severity"),
            Err(err) => err.to_string(),
        };

        assert!(err.contains("possible values"));
        assert!(err.contains("low"));
        assert!(err.contains("normal"));
        assert!(err.contains("elevated"));
        assert!(err.contains("critical"));
    }

    #[test]
    fn parse_analytics_signals_direction_with_symbol() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "signals",
            "--symbol",
            "BTC-USD",
            "--direction",
            "bearish",
            "--source",
            "technical",
        ])
        .unwrap();

        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Signals {
                    symbol,
                    direction,
                    source,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics signals command");
        };

        assert_eq!(symbol.as_deref(), Some("BTC-USD"));
        assert_eq!(direction.as_deref(), Some("bearish"));
        assert_eq!(source, "technical");
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
    fn parse_prediction_add_agent_alias() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "BTC above 70k",
            "--agent",
            "medium-agent",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command: JournalPredictionCommand::Add { source_agent, .. },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(source_agent.as_deref(), Some("medium-agent"));
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
                    command: JournalPredictionCommand::Add { value, claim, .. },
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
                    command: JournalPredictionCommand::Add { value, claim, .. },
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
    fn parse_prediction_add_lessons_flag() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "--claim",
            "BTC decouples from SPY into close",
            "--lessons",
            "218,240",
            "--json",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            claim,
                            lessons,
                            json,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(claim.as_deref(), Some("BTC decouples from SPY into close"));
        assert_eq!(lessons.as_deref(), Some("218,240"));
        assert!(json);
    }

    #[test]
    fn parse_prediction_add_source_attribution_flags() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "--claim",
            "Fed cut odds rise after Bloomberg report",
            "--topic",
            "fed",
            "--source-article-id",
            "12",
            "--json",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            topic,
                            source_article_id,
                            json,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert_eq!(topic.as_deref(), Some("fed"));
        assert_eq!(source_article_id, Some(12));
        assert!(json);
    }

    #[test]
    fn parse_prediction_add_override_cap() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "add",
            "--claim",
            "LOW mechanism still matters after cap",
            "--timeframe",
            "low",
            "--source-agent",
            "low-agent",
            "--override-cap",
        ])
        .expect("cli should parse");

        let Some(Command::Journal {
            command:
                Some(JournalCommand::Prediction {
                    command:
                        JournalPredictionCommand::Add {
                            override_cap,
                            source_agent,
                            timeframe,
                            ..
                        },
                }),
        }) = cli.command
        else {
            panic!("expected journal prediction add command");
        };

        assert!(override_cap);
        assert_eq!(source_agent.as_deref(), Some("low-agent"));
        assert_eq!(timeframe.as_deref(), Some("low"));
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
        let cli = Cli::try_parse_from(["pftui", "analytics", "correlations", "--json"]).unwrap();
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
        let Some(AnalyticsCorrelationsCommand::List {
            period,
            limit,
            json,
            ..
        }) = command
        else {
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
        let Some(AnalyticsCorrelationsCommand::List {
            period,
            limit,
            json,
            ..
        }) = command
        else {
            panic!("expected List subcommand");
        };
        assert!(!json);
        assert_eq!(limit, 25);
        assert_eq!(period.as_deref(), Some("7d"));
    }

    #[test]
    fn test_analytics_predictions_bare() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "predictions", "--json"]).unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Predictions {
            command: subcmd,
            category,
            search,
            geo: _,
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
            geo: _,
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
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
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
        let cli = Cli::try_parse_from(["pftui", "data", "predictions", "stats", "--json"]).unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        assert!(matches!(
            subcmd,
            Some(DataPredictionsCommand::Stats { json: true, .. })
        ));

        // analytics predictions stats --json
        let cli2 =
            Cli::try_parse_from(["pftui", "analytics", "predictions", "stats", "--json"]).unwrap();
        let Some(Command::Analytics { command: cmd2 }) = cli2.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Predictions {
            command: subcmd2, ..
        } = cmd2
        else {
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
        let DataCommand::Predictions {
            command: subcmd3, ..
        } = cmd3
        else {
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
        let cli4 = Cli::try_parse_from(["pftui", "data", "predictions", "stats"]).unwrap();
        let Some(Command::Data { command: cmd4 }) = cli4.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions {
            command: subcmd4, ..
        } = cmd4
        else {
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
            "--lesson-coverage",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Scorecard {
                date,
                limit,
                lesson_coverage,
                json,
            }) => {
                assert_eq!(date.as_deref(), Some("2026-03-25"));
                assert!(limit.is_none());
                assert!(lesson_coverage);
                assert!(json);
            }
            _ => panic!("expected scorecard subcommand"),
        }
    }

    #[test]
    fn parse_journal_prediction_scorecard_with_lesson_coverage() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "scorecard",
            "--date",
            "today",
            "--lesson-coverage",
            "--json",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        match command {
            Some(JournalCommand::Prediction { command }) => match command {
                JournalPredictionCommand::Scorecard {
                    date,
                    limit,
                    lesson_coverage,
                    json,
                } => {
                    assert_eq!(date.as_deref(), Some("today"));
                    assert!(limit.is_none());
                    assert!(lesson_coverage);
                    assert!(json);
                }
                _ => panic!("expected Scorecard"),
            },
            _ => panic!("expected Prediction"),
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
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
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
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Markets {
                category,
                search,
                geo: _,
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
        let cli = Cli::try_parse_from(["pftui", "data", "predictions", "map", "--list", "--json"])
            .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
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
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Map {
                scenario,
                search,
                contract,
                list,
                auto_suggest,
                json,
            }) => {
                assert_eq!(scenario.as_deref(), Some("US Recession 2026"));
                assert_eq!(search.as_deref(), Some("recession"));
                assert!(contract.is_none());
                assert!(!list);
                assert!(!auto_suggest);
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
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
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
    fn parse_data_predictions_map_auto_suggest() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "map",
            "--auto-suggest",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Map {
                auto_suggest,
                json,
                scenario,
                ..
            }) => {
                assert!(auto_suggest);
                assert!(json);
                assert!(scenario.is_none());
            }
            _ => panic!("expected map subcommand"),
        }
    }

    #[test]
    fn parse_data_predictions_map_contract_id_alias() {
        // --contract-id is a visible alias for --contract per the TODO contract
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "map",
            "--scenario",
            "Fed Cut April",
            "--contract-id",
            "0xabc123",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Map {
                scenario,
                contract,
                auto_suggest,
                ..
            }) => {
                assert_eq!(scenario.as_deref(), Some("Fed Cut April"));
                assert_eq!(contract.as_deref(), Some("0xabc123"));
                assert!(!auto_suggest);
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
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
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
    fn parse_data_predictions_suggest_mappings() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "suggest-mappings",
            "--scenario",
            "US Recession 2026",
            "--limit",
            "3",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::SuggestMappings {
                scenario,
                limit,
                json,
            }) => {
                assert_eq!(scenario.as_deref(), Some("US Recession 2026"));
                assert_eq!(limit, 3);
                assert!(json);
            }
            _ => panic!("expected suggest-mappings subcommand"),
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
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Unmap {
                scenario, contract, ..
            }) => {
                assert_eq!(scenario, "Iran Strike");
                assert!(contract.is_none());
            }
            _ => panic!("expected unmap subcommand"),
        }
    }

    #[test]
    fn parse_analytics_predictions_add() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "predictions",
            "add",
            "--claim",
            "BTC above 100k by June",
            "--symbol",
            "BTC-USD",
            "--timeframe",
            "medium",
            "--conviction",
            "high",
            "--confidence",
            "0.75",
            "--source-agent",
            "low-timeframe",
            "--target-date",
            "2026-06-30",
            "--lessons",
            "218,240",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Add {
                claim,
                symbol,
                conviction,
                timeframe,
                confidence,
                source_agent,
                target_date,
                lessons,
                json,
                ..
            }) => {
                assert_eq!(claim, "BTC above 100k by June");
                assert_eq!(symbol.as_deref(), Some("BTC-USD"));
                assert_eq!(conviction.as_deref(), Some("high"));
                assert_eq!(timeframe.as_deref(), Some("medium"));
                assert_eq!(confidence, Some(0.75));
                assert_eq!(source_agent.as_deref(), Some("low-timeframe"));
                assert_eq!(target_date.as_deref(), Some("2026-06-30"));
                assert_eq!(lessons.as_deref(), Some("218,240"));
                assert!(json);
            }
            _ => panic!("expected add subcommand"),
        }
    }

    #[test]
    fn parse_data_predictions_add() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "add",
            "--claim",
            "Gold breaks 3000",
            "--timeframe",
            "high",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Add {
                claim,
                timeframe,
                json,
                symbol,
                ..
            }) => {
                assert_eq!(claim, "Gold breaks 3000");
                assert_eq!(timeframe.as_deref(), Some("high"));
                assert!(json);
                assert!(symbol.is_none());
            }
            _ => panic!("expected add subcommand"),
        }
    }

    #[test]
    fn parse_predictions_add_minimal() {
        // Minimum required: just --claim
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "predictions",
            "add",
            "--claim",
            "VIX spikes above 30",
        ])
        .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics command");
        };
        let AnalyticsCommand::Predictions {
            command: subcmd, ..
        } = command
        else {
            panic!("expected predictions command");
        };
        match subcmd {
            Some(DataPredictionsCommand::Add {
                claim,
                timeframe,
                confidence,
                conviction,
                json,
                ..
            }) => {
                assert_eq!(claim, "VIX spikes above 30");
                assert!(timeframe.is_none());
                assert!(confidence.is_none());
                assert!(conviction.is_none());
                assert!(!json);
            }
            _ => panic!("expected add subcommand"),
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
        let cli = Cli::try_parse_from(["pftui", "analytics", "alignment", "--summary", "--json"])
            .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alignment {
            command: subcommand,
            symbol,
            summary,
            json,
        } = command
        else {
            panic!("expected Alignment");
        };
        assert!(subcommand.is_none());
        assert!(summary);
        assert!(json);
        assert!(symbol.is_none());
    }

    #[test]
    fn parse_alignment_bare_no_summary() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "alignment", "--json"]).unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alignment {
            command: subcommand,
            summary,
            json,
            ..
        } = command
        else {
            panic!("expected Alignment");
        };
        assert!(subcommand.is_none());
        assert!(!summary);
        assert!(json);
    }

    #[test]
    fn parse_alignment_current_subcommand() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "alignment", "current", "--json"]).unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alignment { command, .. } = command else {
            panic!("expected Alignment");
        };
        assert!(matches!(
            command,
            Some(AnalyticsAlignmentCommand::Current { json: true })
        ));
    }

    #[test]
    fn parse_alignment_history_subcommand_default_since() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "alignment", "history", "--json"]).unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alignment { command, .. } = command else {
            panic!("expected Alignment");
        };
        match command {
            Some(AnalyticsAlignmentCommand::History { since, json }) => {
                assert_eq!(since, "90d");
                assert!(json);
            }
            _ => panic!("expected History"),
        }
    }

    #[test]
    fn parse_alignment_compute_with_store() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "alignment",
            "compute",
            "--date",
            "2026-06-01",
            "--store",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alignment { command, .. } = command else {
            panic!("expected Alignment");
        };
        match command {
            Some(AnalyticsAlignmentCommand::Compute { date, store, json }) => {
                assert_eq!(date.as_deref(), Some("2026-06-01"));
                assert!(store);
                assert!(json);
            }
            _ => panic!("expected Compute"),
        }
    }

    #[test]
    fn parse_news_sentiment_defaults() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "news-sentiment", "--json"]).unwrap();
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
            "--breaking",
            "--filter-independence",
            "independent,wire",
            "--with-sentiment",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            breaking,
            filter_independence,
            with_sentiment,
            json,
            ..
        } = command
        else {
            panic!("expected News");
        };
        assert!(breaking);
        assert_eq!(filter_independence.as_deref(), Some("independent,wire"));
        assert!(with_sentiment);
        assert!(json);
    }

    #[test]
    fn parse_data_news_feeds_list_json() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "news", "feeds", "list", "--json"]).unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Feeds { command }),
            ..
        } = command
        else {
            panic!("expected news feeds command");
        };
        let DataNewsFeedsCommand::List { json } = command else {
            panic!("expected news feeds list");
        };
        assert!(json);
    }

    #[test]
    fn parse_data_news_feeds_reset_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "news",
            "feeds",
            "reset",
            "Bloomberg Commodities",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Feeds { command }),
            ..
        } = command
        else {
            panic!("expected news feeds command");
        };
        let DataNewsFeedsCommand::Reset { feed_id, json } = command else {
            panic!("expected news feeds reset");
        };
        assert_eq!(feed_id, "Bloomberg Commodities");
        assert!(json);
    }

    #[test]
    fn parse_data_news_sources_list_json() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "news", "sources", "list", "--json"]).unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Sources { command }),
            ..
        } = command
        else {
            panic!("expected news sources command");
        };
        let DataNewsSourcesCommand::List { json } = command else {
            panic!("expected news sources list");
        };
        assert!(json);
    }

    #[test]
    fn parse_data_news_sources_unclassified_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "news",
            "sources",
            "unclassified",
            "--since",
            "14d",
            "--min-articles",
            "3",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Sources { command }),
            ..
        } = command
        else {
            panic!("expected news sources command");
        };
        let DataNewsSourcesCommand::Unclassified {
            since,
            min_articles,
            json,
        } = command
        else {
            panic!("expected news sources unclassified");
        };
        assert_eq!(since, "14d");
        assert_eq!(min_articles, 3);
        assert!(json);
    }

    #[test]
    fn parse_data_news_sources_stats_json() {
        let cli = Cli::try_parse_from([
            "pftui", "data", "news", "sources", "stats", "--since", "30d", "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Sources { command }),
            ..
        } = command
        else {
            panic!("expected news sources command");
        };
        let DataNewsSourcesCommand::Stats { since, json } = command else {
            panic!("expected news sources stats");
        };
        assert_eq!(since, "30d");
        assert!(json);
    }

    #[test]
    fn parse_data_news_sources_set_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "news",
            "sources",
            "set",
            "example.com",
            "--tier",
            "4",
            "--notes",
            "blog",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Sources { command }),
            ..
        } = command
        else {
            panic!("expected news sources command");
        };
        let DataNewsSourcesCommand::Set {
            domain,
            tier,
            notes,
            json,
        } = command
        else {
            panic!("expected news sources set");
        };
        assert_eq!(domain, "example.com");
        assert_eq!(tier, 4);
        assert_eq!(notes.as_deref(), Some("blog"));
        assert!(json);
    }

    #[test]
    fn parse_data_news_sources_remove_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "news",
            "sources",
            "remove",
            "example.com",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Sources { command }),
            ..
        } = command
        else {
            panic!("expected news sources command");
        };
        let DataNewsSourcesCommand::Remove { domain, json } = command else {
            panic!("expected news sources remove");
        };
        assert_eq!(domain, "example.com");
        assert!(json);
    }

    #[test]
    fn parse_data_news_topics_list_json() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "news", "topics", "list", "--json"]).unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Topics { command }),
            ..
        } = command
        else {
            panic!("expected news topics command");
        };
        let DataNewsTopicsCommand::List { json } = command else {
            panic!("expected news topics list");
        };
        assert!(json);
    }

    #[test]
    fn parse_data_news_topics_set_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "news",
            "topics",
            "set",
            "iran-hormuz",
            "--primary-market-id",
            "polymarket-iran-ceasefire-2026",
            "--secondary-market-id",
            "polymarket-oil-above-100-EOM",
            "--notes",
            "Hormuz shock checks",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Topics { command }),
            ..
        } = command
        else {
            panic!("expected news topics command");
        };
        let DataNewsTopicsCommand::Set {
            topic,
            primary_market_id,
            secondary_market_id,
            notes,
            json,
        } = command
        else {
            panic!("expected news topics set");
        };
        assert_eq!(topic, "iran-hormuz");
        assert_eq!(primary_market_id, "polymarket-iran-ceasefire-2026");
        assert_eq!(
            secondary_market_id.as_deref(),
            Some("polymarket-oil-above-100-EOM")
        );
        assert_eq!(notes.as_deref(), Some("Hormuz shock checks"));
        assert!(json);
    }

    #[test]
    fn parse_data_news_topics_remove_json() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "news",
            "topics",
            "remove",
            "fed-policy",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::News {
            command: Some(DataNewsCommand::Topics { command }),
            ..
        } = command
        else {
            panic!("expected news topics command");
        };
        let DataNewsTopicsCommand::Remove { topic, json } = command else {
            panic!("expected news topics remove");
        };
        assert_eq!(topic, "fed-policy");
        assert!(json);
    }

    #[test]
    fn parse_analytics_morning_brief_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "morning-brief", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::MorningBrief { json, section } = command else {
            panic!("expected MorningBrief");
        };
        assert!(json);
        assert!(section.is_none());
    }

    #[test]
    fn parse_analytics_morning_brief_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "morning-brief"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::MorningBrief { json, section } = command else {
            panic!("expected MorningBrief");
        };
        assert!(!json);
        assert!(section.is_none());
    }

    #[test]
    fn parse_analytics_morning_brief_section_filter() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "morning-brief",
            "--json",
            "--section",
            "alerts,scenarios",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::MorningBrief { json, section } = command else {
            panic!("expected MorningBrief");
        };
        assert!(json);
        assert_eq!(section.as_deref(), Some("alerts,scenarios"));
    }

    #[test]
    fn parse_analytics_evening_brief_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "evening-brief", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::EveningBrief { json, section } = command else {
            panic!("expected EveningBrief");
        };
        assert!(json);
        assert!(section.is_none());
    }

    #[test]
    fn parse_analytics_evening_brief_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "evening-brief"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::EveningBrief { json, section } = command else {
            panic!("expected EveningBrief");
        };
        assert!(!json);
        assert!(section.is_none());
    }

    #[test]
    fn parse_analytics_evening_brief_section_filter() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "evening-brief",
            "--json",
            "--section",
            "alerts,narrative,scenarios",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::EveningBrief { json, section } = command else {
            panic!("expected EveningBrief");
        };
        assert!(json);
        assert_eq!(section.as_deref(), Some("alerts,narrative,scenarios"));
    }

    #[test]
    fn parse_analytics_guidance_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "guidance", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Guidance { json } = command else {
            panic!("expected Guidance");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_guidance_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "guidance"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Guidance { json } = command else {
            panic!("expected Guidance");
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
    fn parse_analytics_power_signals_json() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "power-signals",
            "--days",
            "14",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::PowerSignals { days, json } = command else {
            panic!("expected PowerSignals");
        };
        assert_eq!(days, 14);
        assert!(json);
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
    fn parse_analytics_backtest_predictions_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "backtest", "predictions", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Predictions {
            symbol,
            agent,
            timeframe,
            conviction,
            limit,
            json,
        } = command
        else {
            panic!("expected Predictions");
        };
        assert!(json);
        assert!(symbol.is_none());
        assert!(agent.is_none());
        assert!(timeframe.is_none());
        assert!(conviction.is_none());
        assert!(limit.is_none());
    }

    #[test]
    fn parse_analytics_backtest_predictions_with_filters() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "backtest",
            "predictions",
            "--symbol",
            "BTC-USD",
            "--agent",
            "low-timeframe",
            "--conviction",
            "high",
            "--limit",
            "10",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Predictions {
            symbol,
            agent,
            timeframe,
            conviction,
            limit,
            json,
        } = command
        else {
            panic!("expected Predictions");
        };
        assert!(json);
        assert_eq!(symbol.as_deref(), Some("BTC-USD"));
        assert_eq!(agent.as_deref(), Some("low-timeframe"));
        assert_eq!(conviction.as_deref(), Some("high"));
        assert_eq!(limit, Some(10));
        assert!(timeframe.is_none());
    }

    #[test]
    fn parse_analytics_backtest_predictions_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "backtest", "predictions"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Predictions { json, .. } = command else {
            panic!("expected Predictions");
        };
        assert!(!json);
    }

    #[test]
    fn parse_analytics_backtest_report_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "backtest", "report", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Report { json } = command else {
            panic!("expected Report");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_backtest_report_no_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "backtest", "report"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Report { json } = command else {
            panic!("expected Report");
        };
        assert!(!json);
    }

    #[test]
    fn parse_analytics_backtest_agent_json() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "backtest",
            "agent",
            "--agent",
            "low-timeframe",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Agent { agent, json } = command else {
            panic!("expected Agent");
        };
        assert_eq!(agent, "low-timeframe");
        assert!(json);
    }

    #[test]
    fn parse_analytics_backtest_agent_no_json() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "backtest",
            "agent",
            "--agent",
            "macro-timeframe",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Agent { agent, json } = command else {
            panic!("expected Agent");
        };
        assert_eq!(agent, "macro-timeframe");
        assert!(!json);
    }

    #[test]
    fn parse_analytics_backtest_diagnostics_json() {
        let cli = Cli::parse_from(["pftui", "analytics", "backtest", "diagnostics", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Diagnostics { agent, json } = command else {
            panic!("expected Diagnostics");
        };
        assert!(agent.is_none());
        assert!(json);
    }

    #[test]
    fn parse_analytics_backtest_diagnostics_with_agent() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "backtest",
            "diagnostics",
            "--agent",
            "evening-analyst",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Backtest { command } = command else {
            panic!("expected Backtest");
        };
        let AnalyticsBacktestCommand::Diagnostics { agent, json } = command else {
            panic!("expected Diagnostics");
        };
        assert_eq!(agent.as_deref(), Some("evening-analyst"));
        assert!(json);
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
    fn parse_power_flow_conflicts_defaults() {
        let cli = Cli::parse_from(["pftui", "analytics", "power-flow", "conflicts"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::PowerFlow { command } = command else {
            panic!("expected PowerFlow");
        };
        let AnalyticsPowerFlowCommand::Conflicts { days, json } = command else {
            panic!("expected Conflicts");
        };
        assert_eq!(days, 30);
        assert!(!json);
    }

    #[test]
    fn parse_power_flow_conflicts_all_flags() {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "power-flow",
            "conflicts",
            "--days",
            "14",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::PowerFlow { command } = command else {
            panic!("expected PowerFlow");
        };
        let AnalyticsPowerFlowCommand::Conflicts { days, json } = command else {
            panic!("expected Conflicts");
        };
        assert_eq!(days, 14);
        assert!(json);
    }

    #[test]
    fn journal_entry_add_positional_value() {
        let cli = Cli::try_parse_from(["pftui", "journal", "entry", "add", "Gold looking strong"])
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
        let cli =
            Cli::try_parse_from(["pftui", "journal", "entry", "add", "--tag", "macro"]).unwrap();
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
    fn journal_entry_add_accepts_tags_alias() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "entry",
            "add",
            "note",
            "--tags",
            "macro,oil,geopolitical",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        let JournalCommand::Entry { command: entry_cmd } = command.unwrap() else {
            panic!("expected Entry");
        };
        let JournalEntryCommand::Add { tags, .. } = entry_cmd else {
            panic!("expected Add");
        };
        assert_eq!(tags.as_deref(), Some("macro,oil,geopolitical"));
    }

    #[test]
    fn journal_entry_add_accepts_repeated_tag_flags() {
        let cli = Cli::try_parse_from([
            "pftui", "journal", "entry", "add", "note", "--tag", "macro", "--tag", "oil",
        ])
        .unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        let JournalCommand::Entry { command: entry_cmd } = command.unwrap() else {
            panic!("expected Entry");
        };
        let JournalEntryCommand::Add { tag, .. } = entry_cmd else {
            panic!("expected Add");
        };
        assert_eq!(tag, vec!["macro".to_string(), "oil".to_string()]);
    }

    #[test]
    fn journal_entry_add_help_shows_content_flag() -> Result<()> {
        let help = subcommand_help(&["journal", "entry", "add"])?;
        assert!(
            help.contains("--content"),
            "help should show --content flag"
        );
        assert!(help.contains("--date"), "help should show --date flag");
        assert!(
            help.contains("YYYY-MM-DD"),
            "help should describe date format"
        );
        assert!(help.contains("--tags"), "help should show --tags flag");
        Ok(())
    }

    #[test]
    fn parse_prediction_lessons_unresolved_list() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "lessons",
            "--unresolved",
            "--json",
        ])
        .unwrap();
        let Some(Command::Journal { command }) = cli.command else {
            panic!("expected journal command");
        };
        let Some(JournalCommand::Prediction { command }) = command else {
            panic!("expected prediction command");
        };
        let JournalPredictionCommand::Lessons {
            command,
            unresolved,
            json,
            ..
        } = command
        else {
            panic!("expected lessons command");
        };
        assert!(command.is_none());
        assert!(unresolved);
        assert!(json);
    }

    #[test]
    fn parse_prediction_lessons_bulk_command() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "lessons",
            "bulk",
            "--input",
            "/tmp/lessons.json",
            "--unresolved",
            "--dry-run",
            "--json",
        ])
        .unwrap();
        let Some(Command::Journal { command }) = cli.command else {
            panic!("expected journal command");
        };
        let Some(JournalCommand::Prediction { command }) = command else {
            panic!("expected prediction command");
        };
        let JournalPredictionCommand::Lessons { command, .. } = command else {
            panic!("expected lessons command");
        };
        let Some(JournalPredictionLessonsCommand::Bulk {
            input,
            auto_stub,
            unresolved,
            dry_run,
            json,
        }) = command
        else {
            panic!("expected bulk subcommand");
        };
        assert_eq!(input.as_deref(), Some("/tmp/lessons.json"));
        assert!(!auto_stub);
        assert!(unresolved);
        assert!(dry_run);
        assert!(json);
    }

    #[test]
    fn parse_prediction_lessons_bulk_auto_stub_without_input() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "prediction",
            "lessons",
            "bulk",
            "--auto-stub",
            "--json",
        ])
        .unwrap();
        let Some(Command::Journal { command }) = cli.command else {
            panic!("expected journal command");
        };
        let Some(JournalCommand::Prediction { command }) = command else {
            panic!("expected prediction command");
        };
        let JournalPredictionCommand::Lessons { command, .. } = command else {
            panic!("expected lessons command");
        };
        let Some(JournalPredictionLessonsCommand::Bulk {
            input,
            auto_stub,
            unresolved,
            dry_run,
            json,
        }) = command
        else {
            panic!("expected bulk subcommand");
        };
        assert!(input.is_none());
        assert!(auto_stub);
        assert!(!unresolved);
        assert!(!dry_run);
        assert!(json);
    }

    #[test]
    fn parse_data_fear_greed_history_json() {
        let cli = Cli::try_parse_from(["pftui", "data", "fear-greed", "--history", "14", "--json"])
            .unwrap();
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data command");
        };
        let DataCommand::FearGreed { history, json } = command else {
            panic!("expected fear-greed command");
        };
        assert_eq!(history, Some(14));
        assert!(json);
    }

    #[test]
    fn parse_data_prices_market_flag() {
        let cli = Cli::try_parse_from(["pftui", "data", "prices", "--market", "--json"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices {
            market,
            json,
            auto_refresh: _,
            command: _,
        } = command
        else {
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
        let DataCommand::Prices {
            market,
            json,
            auto_refresh: _,
            command: _,
        } = command
        else {
            panic!("expected Prices");
        };
        assert!(!market);
        assert!(json);
    }

    #[test]
    fn parse_data_quotes_alias_resolves_to_prices() {
        let cli = Cli::try_parse_from(["pftui", "data", "quotes", "--json"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices {
            market,
            json,
            auto_refresh: _,
            command: _,
        } = command
        else {
            panic!("expected Prices via quotes alias");
        };
        assert!(!market);
        assert!(json);
    }

    #[test]
    fn parse_data_quotes_alias_with_market_flag() {
        let cli = Cli::try_parse_from(["pftui", "data", "quotes", "--market", "--json"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices {
            market,
            json,
            auto_refresh: _,
            command: _,
        } = command
        else {
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
        let DataCommand::Prices {
            market,
            json,
            auto_refresh: _,
            command: _,
        } = command
        else {
            panic!("expected Prices via quotes alias");
        };
        assert!(!market);
        assert!(!json);
    }

    #[test]
    fn parse_data_prices_audit_subcommand() {
        let cli = Cli::try_parse_from([
            "pftui", "data", "prices", "audit", "--symbol", "BTC-USD", "--json",
        ])
        .unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices {
            command: Some(DataPricesCommand::Audit { symbol, json }),
            ..
        } = command
        else {
            panic!("expected Prices audit subcommand");
        };
        assert_eq!(symbol.as_deref(), Some("BTC-USD"));
        assert!(json);
    }

    #[test]
    fn parse_data_prices_audit_defaults() {
        let cli = Cli::try_parse_from(["pftui", "data", "prices", "audit"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices {
            command: Some(DataPricesCommand::Audit { symbol, json }),
            ..
        } = command
        else {
            panic!("expected Prices audit subcommand");
        };
        assert!(symbol.is_none());
        assert!(!json);
    }

    #[test]
    fn parse_data_audit_subcommand() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "audit",
            "--table",
            "price_history",
            "--json",
        ])
        .unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Audit { table, json } = command else {
            panic!("expected Audit");
        };
        assert_eq!(table.as_deref(), Some("price_history"));
        assert!(json);
    }

    #[test]
    fn parse_data_decontaminate_defaults_to_dry_run() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "decontaminate",
            "--symbol",
            "BTC",
            "--before",
            "2026-06-12",
        ])
        .unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Decontaminate {
            symbol,
            before,
            dry_run,
            confirm,
            json,
        } = command
        else {
            panic!("expected Decontaminate");
        };
        assert_eq!(symbol, "BTC");
        assert_eq!(before.as_deref(), Some("2026-06-12"));
        assert!(!dry_run);
        assert!(!confirm, "no --confirm means dry run");
        assert!(!json);
    }

    #[test]
    fn parse_data_decontaminate_dry_run_conflicts_with_confirm() {
        assert!(Cli::try_parse_from([
            "pftui",
            "data",
            "decontaminate",
            "--symbol",
            "BTC",
            "--dry-run",
            "--confirm",
        ])
        .is_err());
    }

    #[test]
    fn parse_data_refresh_accept_outlier() {
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "refresh",
            "--accept-outlier",
            "BTC-USD,GC=F",
            "--accept-outlier",
            "OBSCURE",
        ])
        .unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Refresh { accept_outlier, .. } = command else {
            panic!("expected Refresh");
        };
        assert_eq!(accept_outlier, vec!["BTC-USD", "GC=F", "OBSCURE"]);
    }

    #[test]
    fn parse_data_prices_auto_refresh_flag() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "prices", "--auto-refresh", "--json"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices {
            market,
            json,
            auto_refresh,
            command: _,
        } = command
        else {
            panic!("expected Prices");
        };
        assert!(!market);
        assert!(json);
        assert!(auto_refresh);
    }

    #[test]
    fn parse_data_quotes_auto_refresh_flag() {
        let cli =
            Cli::try_parse_from(["pftui", "data", "quotes", "--auto-refresh", "--market"]).unwrap();
        let Command::Data { command, .. } = cli.command.unwrap() else {
            panic!("expected Data");
        };
        let DataCommand::Prices {
            market,
            json,
            auto_refresh,
            command: _,
        } = command
        else {
            panic!("expected Prices via quotes alias");
        };
        assert!(market);
        assert!(!json);
        assert!(auto_refresh);
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
        let cli = Cli::try_parse_from(["pftui", "analytics", "cross-timeframe", "--json"]).unwrap();
        let Command::Analytics { command } = cli.command.unwrap() else {
            panic!("expected Analytics");
        };
        let AnalyticsCommand::CrossTimeframe { resolve, json, .. } = command else {
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
        let cli =
            Cli::try_parse_from(["pftui", "journal", "prediction", "lessons", "--json"]).unwrap();
        let Command::Journal { command } = cli.command.unwrap() else {
            panic!("expected Journal");
        };
        match command {
            Some(JournalCommand::Prediction { command }) => match command {
                JournalPredictionCommand::Lessons { command, json, .. } => {
                    assert!(command.is_none());
                    assert!(json);
                }
                _ => panic!("expected Lessons"),
            },
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
            Some(JournalCommand::Prediction { command }) => match command {
                JournalPredictionCommand::Lessons {
                    miss_type,
                    limit,
                    json,
                    ..
                } => {
                    assert_eq!(miss_type.as_deref(), Some("timing"));
                    assert_eq!(limit, Some(5));
                    assert!(json);
                }
                _ => panic!("expected Lessons"),
            },
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
            Some(JournalCommand::Prediction { command }) => match command {
                JournalPredictionCommand::Lessons { command, .. } => match command {
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
                        assert_eq!(
                            signal_misread.as_deref(),
                            Some("Volume divergence was bearish")
                        );
                        assert!(json);
                    }
                    _ => panic!("expected Add"),
                },
                _ => panic!("expected Lessons"),
            },
            _ => panic!("expected Prediction"),
        }
    }

    #[test]
    fn parse_situation_populate() {
        let cli =
            Cli::try_parse_from(["pftui", "analytics", "situation", "populate", "--json"]).unwrap();
        let Some(Command::Analytics {
            command: AnalyticsCommand::Situation { command, .. },
        }) = cli.command
        else {
            panic!("expected analytics situation command");
        };
        match command {
            Some(SituationCommand::Populate { json }) => assert!(json),
            _ => panic!("expected Populate"),
        }
    }

    #[test]
    fn parse_situation_populate_no_json() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "situation", "populate"]).unwrap();
        let Some(Command::Analytics {
            command: AnalyticsCommand::Situation { command, .. },
        }) = cli.command
        else {
            panic!("expected analytics situation command");
        };
        match command {
            Some(SituationCommand::Populate { json }) => assert!(!json),
            _ => panic!("expected Populate"),
        }
    }

    #[test]
    fn parse_stress_test_list_scenarios() {
        let cli = Cli::try_parse_from([
            "pftui",
            "portfolio",
            "stress-test",
            "--list-scenarios",
            "--json",
        ])
        .unwrap();
        let Some(Command::Portfolio {
            command:
                Some(PortfolioCommand::StressTest {
                    scenario,
                    list_scenarios,
                    json,
                }),
        }) = cli.command
        else {
            panic!("expected portfolio stress-test command");
        };
        assert!(list_scenarios);
        assert!(json);
        assert!(scenario.is_none());
    }

    #[test]
    fn parse_stress_test_with_scenario() {
        let cli = Cli::try_parse_from(["pftui", "portfolio", "stress-test", "2008 GFC", "--json"])
            .unwrap();
        let Some(Command::Portfolio {
            command:
                Some(PortfolioCommand::StressTest {
                    scenario,
                    list_scenarios,
                    json,
                }),
        }) = cli.command
        else {
            panic!("expected portfolio stress-test command");
        };
        assert!(!list_scenarios);
        assert!(json);
        assert_eq!(scenario, Some("2008 GFC".to_string()));
    }

    #[test]
    fn parse_analytics_trends_list_verbose() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "trends",
            "list",
            "--verbose",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Trends { command } = command else {
            panic!("expected trends");
        };
        let AnalyticsTrendsCommand::List { verbose, json, .. } = command else {
            panic!("expected list");
        };
        assert!(verbose);
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_analytics_trends_list_no_verbose() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "trends",
            "list",
            "--timeframe",
            "high",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Trends { command } = command else {
            panic!("expected trends");
        };
        let AnalyticsTrendsCommand::List {
            verbose, timeframe, ..
        } = command
        else {
            panic!("expected list");
        };
        assert!(!verbose);
        assert_eq!(timeframe.as_deref(), Some("high"));
        Ok(())
    }

    #[test]
    fn parse_analytics_trends_list_all_filters() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "trends",
            "list",
            "--timeframe",
            "high",
            "--direction",
            "accelerating",
            "--conviction",
            "high",
            "--category",
            "energy",
            "--status",
            "active",
            "--limit",
            "5",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Trends { command } = command else {
            panic!("expected trends");
        };
        let AnalyticsTrendsCommand::List {
            timeframe,
            direction,
            conviction,
            category,
            status,
            limit,
            json,
            ..
        } = command
        else {
            panic!("expected list");
        };
        assert_eq!(timeframe.as_deref(), Some("high"));
        assert_eq!(direction.as_deref(), Some("accelerating"));
        assert_eq!(conviction.as_deref(), Some("high"));
        assert_eq!(category.as_deref(), Some("energy"));
        assert_eq!(status.as_deref(), Some("active"));
        assert_eq!(limit, Some(5));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_calendar_list() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui", "data", "calendar", "list", "--days", "14", "--impact", "high", "--json",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Calendar { command, .. } = command else {
            panic!("expected calendar");
        };
        let Some(CalendarCommand::List {
            days,
            impact,
            event_type,
            json,
        }) = command
        else {
            panic!("expected list");
        };
        assert_eq!(days, 14);
        assert_eq!(impact.as_deref(), Some("high"));
        assert!(event_type.is_none());
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_calendar_default_list() -> Result<()> {
        // `pftui data calendar --json` should parse with no subcommand (defaults to list)
        let cli = Cli::parse_from(["pftui", "data", "calendar", "--json"]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Calendar {
            command,
            days,
            impact,
            event_type,
            json,
        } = command
        else {
            panic!("expected calendar");
        };
        assert!(command.is_none());
        assert_eq!(days, 7); // default
        assert!(impact.is_none());
        assert!(event_type.is_none());
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_calendar_default_list_with_filters() -> Result<()> {
        // `pftui data calendar --days 14 --impact high --type geopolitical --json`
        let cli = Cli::parse_from([
            "pftui",
            "data",
            "calendar",
            "--days",
            "14",
            "--impact",
            "high",
            "--type",
            "geopolitical",
            "--json",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Calendar {
            command,
            days,
            impact,
            event_type,
            json,
        } = command
        else {
            panic!("expected calendar");
        };
        assert!(command.is_none());
        assert_eq!(days, 14);
        assert_eq!(impact.as_deref(), Some("high"));
        assert_eq!(event_type.as_deref(), Some("geopolitical"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_calendar_add_geopolitical() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "data",
            "calendar",
            "add",
            "--date",
            "2026-04-06",
            "--name",
            "Iran Hormuz Deadline",
            "--impact",
            "high",
            "--type",
            "geopolitical",
            "--json",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Calendar { command, .. } = command else {
            panic!("expected calendar");
        };
        let Some(CalendarCommand::Add {
            date,
            name,
            impact,
            event_type,
            symbol,
            json,
        }) = command
        else {
            panic!("expected add");
        };
        assert_eq!(date, "2026-04-06");
        assert_eq!(name, "Iran Hormuz Deadline");
        assert_eq!(impact, "high");
        assert_eq!(event_type, "geopolitical");
        assert!(symbol.is_none());
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_calendar_remove() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "data",
            "calendar",
            "remove",
            "--date",
            "2026-04-06",
            "--name",
            "Iran Hormuz Deadline",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Calendar { command, .. } = command else {
            panic!("expected calendar");
        };
        let Some(CalendarCommand::Remove { date, name, json }) = command else {
            panic!("expected remove");
        };
        assert_eq!(date, "2026-04-06");
        assert_eq!(name, "Iran Hormuz Deadline");
        assert!(!json);
        Ok(())
    }

    #[test]
    fn parse_calendar_add_with_symbol() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "data",
            "calendar",
            "add",
            "--date",
            "2026-04-15",
            "--name",
            "AAPL Earnings",
            "--impact",
            "high",
            "--type",
            "earnings",
            "--symbol",
            "AAPL",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Calendar { command, .. } = command else {
            panic!("expected calendar");
        };
        let Some(CalendarCommand::Add {
            date,
            name,
            event_type,
            symbol,
            ..
        }) = command
        else {
            panic!("expected add");
        };
        assert_eq!(date, "2026-04-15");
        assert_eq!(name, "AAPL Earnings");
        assert_eq!(event_type, "earnings");
        assert_eq!(symbol.as_deref(), Some("AAPL"));
        Ok(())
    }

    #[test]
    fn parse_calendar_list_with_type_filter() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "data",
            "calendar",
            "list",
            "--type",
            "geopolitical",
            "--json",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Calendar { command, .. } = command else {
            panic!("expected calendar");
        };
        let Some(CalendarCommand::List {
            event_type, json, ..
        }) = command
        else {
            panic!("expected list");
        };
        assert_eq!(event_type.as_deref(), Some("geopolitical"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_refresh_only_single_source() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "data", "refresh", "--only", "prices"]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Refresh {
            only,
            skip,
            timeout,
            ..
        } = command
        else {
            panic!("expected refresh");
        };
        assert_eq!(only, vec!["prices"]);
        assert!(skip.is_empty());
        assert!(timeout.is_none());
        Ok(())
    }

    #[test]
    fn parse_refresh_only_multiple_sources() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "data",
            "refresh",
            "--only",
            "prices,news_rss,sentiment",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Refresh {
            only,
            skip,
            timeout,
            ..
        } = command
        else {
            panic!("expected refresh");
        };
        assert_eq!(only, vec!["prices", "news_rss", "sentiment"]);
        assert!(skip.is_empty());
        assert!(timeout.is_none());
        Ok(())
    }

    #[test]
    fn parse_refresh_skip_sources() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "data", "refresh", "--skip", "worldbank,bls"]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Refresh {
            only,
            skip,
            timeout,
            ..
        } = command
        else {
            panic!("expected refresh");
        };
        assert!(only.is_empty());
        assert_eq!(skip, vec!["worldbank", "bls"]);
        assert!(timeout.is_none());
        Ok(())
    }

    #[test]
    fn parse_refresh_stale_flag() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "data", "refresh", "--stale"]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Refresh {
            stale,
            only,
            skip,
            timeout,
            ..
        } = command
        else {
            panic!("expected refresh");
        };
        assert!(stale);
        assert!(only.is_empty());
        assert!(skip.is_empty());
        assert!(timeout.is_none());
        Ok(())
    }

    #[test]
    fn parse_refresh_only_and_skip_conflict() {
        let result = Cli::try_parse_from([
            "pftui", "data", "refresh", "--only", "prices", "--skip", "bls",
        ]);
        assert!(result.is_err(), "--only and --skip should conflict");
    }

    #[test]
    fn parse_refresh_stale_and_only_conflict() {
        let result =
            Cli::try_parse_from(["pftui", "data", "refresh", "--stale", "--only", "prices"]);
        assert!(result.is_err(), "--stale and --only should conflict");
    }

    #[test]
    fn parse_refresh_only_with_json() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "data", "refresh", "--only", "prices", "--json"]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Refresh {
            only,
            json,
            timeout,
            ..
        } = command
        else {
            panic!("expected refresh");
        };
        assert_eq!(only, vec!["prices"]);
        assert!(json);
        assert!(timeout.is_none());
        Ok(())
    }

    #[test]
    fn parse_refresh_timeout_flag() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "data", "refresh", "--timeout", "90", "--json"]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Refresh { timeout, json, .. } = command else {
            panic!("expected refresh");
        };
        assert_eq!(timeout, Some(90));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_correlations_breaks_severity_filter() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "correlations",
            "breaks",
            "--severity",
            "severe",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Correlations {
            command: Some(AnalyticsCorrelationsCommand::Breaks { severity, json, .. }),
            ..
        } = command
        else {
            panic!("expected correlations breaks");
        };
        assert_eq!(severity.as_deref(), Some("severe"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_correlations_breaks_no_severity() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "correlations",
            "breaks",
            "--threshold",
            "0.50",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Correlations {
            command:
                Some(AnalyticsCorrelationsCommand::Breaks {
                    severity,
                    threshold,
                    ..
                }),
            ..
        } = command
        else {
            panic!("expected correlations breaks");
        };
        assert!(severity.is_none());
        assert!((threshold - 0.50).abs() < f64::EPSILON);
        Ok(())
    }

    #[test]
    fn parse_correlations_breaks_verbose_flag() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "correlations",
            "breaks",
            "--verbose",
            "--history-depth",
            "10",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Correlations {
            command:
                Some(AnalyticsCorrelationsCommand::Breaks {
                    verbose,
                    history_depth,
                    json,
                    ..
                }),
            ..
        } = command
        else {
            panic!("expected correlations breaks");
        };
        assert!(verbose);
        assert_eq!(history_depth, 10);
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_correlations_breaks_verbose_defaults() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "analytics", "correlations", "breaks"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Correlations {
            command:
                Some(AnalyticsCorrelationsCommand::Breaks {
                    verbose,
                    history_depth,
                    ..
                }),
            ..
        } = command
        else {
            panic!("expected correlations breaks");
        };
        assert!(!verbose);
        assert_eq!(history_depth, 7);
        Ok(())
    }

    #[test]
    fn parse_market_snapshot() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "analytics", "market-snapshot", "--json"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::MarketSnapshot {
            json,
            auto_refresh: _,
        } = command
        else {
            panic!("expected market-snapshot");
        };
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_market_snapshot_no_json() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "analytics", "market-snapshot"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::MarketSnapshot {
            json,
            auto_refresh: _,
        } = command
        else {
            panic!("expected market-snapshot");
        };
        assert!(!json);
        Ok(())
    }

    #[test]
    fn parse_market_snapshot_auto_refresh() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "market-snapshot",
            "--auto-refresh",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::MarketSnapshot { json, auto_refresh } = command else {
            panic!("expected market-snapshot");
        };
        assert!(json);
        assert!(auto_refresh);
        Ok(())
    }

    #[test]
    fn parse_timing_flag_global() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "--timing", "data", "status", "--json"]);
        assert!(cli.timing);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Status { json, .. } = command else {
            panic!("expected status");
        };
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_timing_flag_after_subcommand() -> Result<()> {
        // Global flags can appear after the subcommand too
        let cli = Cli::parse_from(["pftui", "data", "status", "--timing"]);
        assert!(cli.timing);
        Ok(())
    }

    #[test]
    fn parse_timing_flag_default_off() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "data", "status"]);
        assert!(!cli.timing);
        Ok(())
    }

    #[test]
    fn parse_alerts_check_newly_triggered_flag() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "alerts",
            "check",
            "--newly-triggered",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Check {
            newly_triggered,
            json,
            ..
        } = command
        else {
            panic!("expected check");
        };
        assert!(newly_triggered);
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_alerts_check_kind_and_condition_filters() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "alerts",
            "check",
            "--kind",
            "technical",
            "--condition",
            "correlation_break",
            "--symbol",
            "BTC-USD",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Check {
            kind,
            condition,
            symbol,
            json,
            ..
        } = command
        else {
            panic!("expected check");
        };
        assert_eq!(kind.as_deref(), Some("technical"));
        assert_eq!(condition.as_deref(), Some("correlation_break"));
        assert_eq!(symbol.as_deref(), Some("BTC-USD"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_alerts_check_defaults() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "analytics", "alerts", "check"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Check {
            today,
            newly_triggered,
            kind,
            condition,
            symbol,
            status,
            urgency,
            json,
        } = command
        else {
            panic!("expected check");
        };
        assert!(!today);
        assert!(!newly_triggered);
        assert!(kind.is_none());
        assert!(condition.is_none());
        assert!(symbol.is_none());
        assert!(status.is_none());
        assert!(urgency.is_none());
        assert!(!json);
        Ok(())
    }

    #[test]
    fn parse_data_alerts_check_newly_triggered_flag() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "data",
            "alerts",
            "check",
            "--newly-triggered",
            "--condition",
            "correlation_break",
            "--json",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Alerts {
            command:
                Some(DataAlertsRedirect::Check {
                    newly_triggered,
                    condition,
                    json,
                    ..
                }),
        } = command
        else {
            panic!("expected data alerts check");
        };
        assert!(newly_triggered);
        assert_eq!(condition.as_deref(), Some("correlation_break"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_alerts_check_status_filter() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "alerts",
            "check",
            "--status",
            "triggered",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Check { status, json, .. } = command else {
            panic!("expected check");
        };
        assert_eq!(status.as_deref(), Some("triggered"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_data_alerts_check_status_filter() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui", "data", "alerts", "check", "--status", "armed", "--json",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Alerts {
            command: Some(DataAlertsRedirect::Check { status, json, .. }),
        } = command
        else {
            panic!("expected data alerts check");
        };
        assert_eq!(status.as_deref(), Some("armed"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_alerts_check_status_combined_with_kind() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "alerts",
            "check",
            "--status",
            "triggered",
            "--kind",
            "price",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Check {
            status, kind, json, ..
        } = command
        else {
            panic!("expected check");
        };
        assert_eq!(status.as_deref(), Some("triggered"));
        assert_eq!(kind.as_deref(), Some("price"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_alerts_check_urgency_filter() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "alerts",
            "check",
            "--urgency",
            "critical",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Check { urgency, json, .. } = command else {
            panic!("expected check");
        };
        assert_eq!(urgency.as_deref(), Some("critical"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_data_alerts_check_urgency_filter() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "data",
            "alerts",
            "check",
            "--urgency",
            "watch",
            "--json",
        ]);
        let Some(Command::Data { command }) = cli.command else {
            panic!("expected data");
        };
        let DataCommand::Alerts {
            command: Some(DataAlertsRedirect::Check { urgency, json, .. }),
        } = command
        else {
            panic!("expected data alerts check");
        };
        assert_eq!(urgency.as_deref(), Some("watch"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_alerts_ack_by_ids() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "analytics", "alerts", "ack", "1", "2", "3"]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Ack {
            ids,
            all_triggered,
            json,
            ..
        } = command
        else {
            panic!("expected ack");
        };
        assert_eq!(ids, vec![1, 2, 3]);
        assert!(!all_triggered);
        assert!(!json);
        Ok(())
    }

    #[test]
    fn parse_alerts_ack_all_triggered() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "alerts",
            "ack",
            "--all-triggered",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Ack {
            ids,
            all_triggered,
            json,
            ..
        } = command
        else {
            panic!("expected ack");
        };
        assert!(ids.is_empty());
        assert!(all_triggered);
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_alerts_ack_all_triggered_with_filters() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "alerts",
            "ack",
            "--all-triggered",
            "--condition",
            "correlation_break",
            "--kind",
            "macro",
            "--symbol",
            "GC=F",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Alerts { command } = command else {
            panic!("expected alerts");
        };
        let AnalyticsAlertsCommand::Ack {
            ids,
            all_triggered,
            condition,
            kind,
            symbol,
            json,
        } = command
        else {
            panic!("expected ack");
        };
        assert!(ids.is_empty());
        assert!(all_triggered);
        assert_eq!(condition.as_deref(), Some("correlation_break"));
        assert_eq!(kind.as_deref(), Some("macro"));
        assert_eq!(symbol.as_deref(), Some("GC=F"));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_alerts_ack_ids_conflicts_with_all_triggered() {
        // IDs and --all-triggered are mutually exclusive.
        let result = Cli::try_parse_from([
            "pftui",
            "analytics",
            "alerts",
            "ack",
            "1",
            "--all-triggered",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_alerts_ack_filter_requires_all_triggered() {
        // --condition without --all-triggered should fail.
        let result = Cli::try_parse_from([
            "pftui",
            "analytics",
            "alerts",
            "ack",
            "--condition",
            "price_cross",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_analytics_digest_with_from_and_agent_filter() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "analytics",
            "digest",
            "--from",
            "2026-04-06",
            "--agent-filter",
            "low-agent",
            "--limit",
            "5",
            "--json",
        ]);
        let Some(Command::Analytics { command }) = cli.command else {
            panic!("expected analytics");
        };
        let AnalyticsCommand::Digest {
            from,
            agent_filter,
            limit,
            json,
        } = command
        else {
            panic!("expected digest");
        };
        assert_eq!(from.as_deref(), Some("2026-04-06"));
        assert_eq!(agent_filter.as_deref(), Some("low-agent"));
        assert_eq!(limit, Some(5));
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_system_schema_verify_json() -> Result<()> {
        let cli = Cli::parse_from(["pftui", "system", "schema", "verify", "--json"]);
        let Some(Command::System { command }) = cli.command else {
            panic!("expected system");
        };
        let SystemCommand::Schema { command } = command else {
            panic!("expected schema");
        };
        let SchemaCommand::Verify { json } = command else {
            panic!("expected verify");
        };
        assert!(json);
        Ok(())
    }

    #[test]
    fn parse_system_schema_repair_flags() -> Result<()> {
        let cli = Cli::parse_from([
            "pftui",
            "system",
            "schema",
            "repair",
            "--dry-run",
            "--confirm",
            "--json",
        ]);
        let Some(Command::System { command }) = cli.command else {
            panic!("expected system");
        };
        let SystemCommand::Schema { command } = command else {
            panic!("expected schema");
        };
        let SchemaCommand::Repair {
            dry_run,
            confirm,
            json,
        } = command
        else {
            panic!("expected repair");
        };
        assert!(dry_run);
        assert!(confirm);
        assert!(json);
        Ok(())
    }

    // ── R5 memory-consolidation layer ───────────────────────────────────

    #[test]
    fn parse_journal_notes_repetition() {
        let cli = Cli::try_parse_from([
            "pftui",
            "journal",
            "notes",
            "repetition",
            "--author",
            "analyst-medium",
            "--days",
            "60",
            "--json",
        ])
        .unwrap();
        let Some(Command::Journal {
            command: Some(JournalCommand::Notes { command }),
        }) = cli.command
        else {
            panic!("expected journal notes command");
        };
        let JournalNotesCommand::Repetition { author, days, json } = command else {
            panic!("expected notes repetition");
        };
        assert_eq!(author.as_deref(), Some("analyst-medium"));
        assert_eq!(days, 60);
        assert!(json);
    }

    #[test]
    fn parse_journal_notes_repetition_defaults() {
        let cli = Cli::try_parse_from(["pftui", "journal", "notes", "repetition"]).unwrap();
        let Some(Command::Journal {
            command: Some(JournalCommand::Notes { command }),
        }) = cli.command
        else {
            panic!("expected journal notes command");
        };
        let JournalNotesCommand::Repetition { author, days, json } = command else {
            panic!("expected notes repetition");
        };
        assert!(author.is_none());
        assert_eq!(days, 30);
        assert!(!json);
    }

    #[test]
    fn parse_analytics_lessons_rules_add() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "lessons",
            "rules",
            "add",
            "--rule",
            "Cap magnitude forecasts at 1.5x trailing vol.",
            "--rationale",
            "Magnitude overshoot dominates the lesson book.",
            "--sources",
            "12,40,77",
            "--enforcement",
            "validator",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics {
            command: AnalyticsCommand::Lessons { command },
        }) = cli.command
        else {
            panic!("expected analytics lessons command");
        };
        let AnalyticsLessonsCommand::Rules { command } = command else {
            panic!("expected lessons rules");
        };
        let AnalyticsLessonsRulesCommand::Add {
            rule,
            rationale,
            sources,
            enforcement,
            json,
        } = command
        else {
            panic!("expected rules add");
        };
        assert!(rule.starts_with("Cap magnitude"));
        assert!(rationale.is_some());
        assert_eq!(sources.as_deref(), Some("12,40,77"));
        assert_eq!(enforcement, "validator");
        assert!(json);
    }

    #[test]
    fn parse_analytics_lessons_rules_list_retire_cite() {
        let cli = Cli::try_parse_from(["pftui", "analytics", "lessons", "rules", "list", "--all"])
            .unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Lessons {
                    command:
                        AnalyticsLessonsCommand::Rules {
                            command: AnalyticsLessonsRulesCommand::List { all, json },
                        },
                },
        }) = cli.command
        else {
            panic!("expected rules list");
        };
        assert!(all);
        assert!(!json);

        let cli =
            Cli::try_parse_from(["pftui", "analytics", "lessons", "rules", "retire", "7"]).unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Lessons {
                    command:
                        AnalyticsLessonsCommand::Rules {
                            command: AnalyticsLessonsRulesCommand::Retire { id, .. },
                        },
                },
        }) = cli.command
        else {
            panic!("expected rules retire");
        };
        assert_eq!(id, 7);

        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "lessons",
            "rules",
            "cite",
            "3",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Lessons {
                    command:
                        AnalyticsLessonsCommand::Rules {
                            command: AnalyticsLessonsRulesCommand::Cite { id, json },
                        },
                },
        }) = cli.command
        else {
            panic!("expected rules cite");
        };
        assert_eq!(id, 3);
        assert!(json);
    }

    #[test]
    fn parse_analytics_thesis_set_review_and_review_due() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "thesis",
            "set-review",
            "cycle-frameworks",
            "--date",
            "2026-09-01",
        ])
        .unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Thesis {
                    command:
                        AnalyticsThesisCommand::SetReview {
                            section,
                            date,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected thesis set-review");
        };
        assert_eq!(section, "cycle-frameworks");
        assert_eq!(date, "2026-09-01");
        assert!(!json);

        let cli =
            Cli::try_parse_from(["pftui", "analytics", "thesis", "review-due", "--json"]).unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Thesis {
                    command: AnalyticsThesisCommand::ReviewDue { json },
                },
        }) = cli.command
        else {
            panic!("expected thesis review-due");
        };
        assert!(json);
    }

    #[test]
    fn parse_analytics_views_stale() {
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "views",
            "stale",
            "--days",
            "14",
            "--move-pct",
            "5",
            "--json",
        ])
        .unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::Stale {
                            days,
                            move_pct,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected views stale");
        };
        assert_eq!(days, 14);
        assert!((move_pct - 5.0).abs() < f64::EPSILON);
        assert!(json);

        // Defaults: 21 days, 10% move.
        let cli = Cli::try_parse_from(["pftui", "analytics", "views", "stale"]).unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Views {
                    command:
                        AnalyticsViewsCommand::Stale {
                            days,
                            move_pct,
                            json,
                        },
                },
        }) = cli.command
        else {
            panic!("expected views stale");
        };
        assert_eq!(days, 21);
        assert!((move_pct - 10.0).abs() < f64::EPSILON);
        assert!(!json);
    }

    #[test]
    fn parse_data_predictions_add_alias_discipline_flags() {
        // Alias with --falsify + cap-override flags parses.
        let cli = Cli::try_parse_from([
            "pftui",
            "data",
            "predictions",
            "add",
            "--claim",
            "BTC reclaims six figures",
            "--timeframe",
            "medium",
            "--confidence",
            "0.7",
            "--falsify",
            "BTC-USD close above 100000 by 2026-12-31",
            "--override-confidence-cap",
            "--cap-rationale",
            "regime change invalidates the trailing record",
            "--skip-preflight",
            "--json",
        ])
        .unwrap();
        let Some(Command::Data {
            command:
                DataCommand::Predictions {
                    command:
                        Some(DataPredictionsCommand::Add {
                            claim,
                            falsify,
                            override_confidence_cap,
                            cap_rationale,
                            skip_preflight,
                            accept_preflight,
                            with_adversary,
                            json,
                            ..
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected data predictions add");
        };
        assert_eq!(claim, "BTC reclaims six figures");
        assert_eq!(
            falsify.as_deref(),
            Some("BTC-USD close above 100000 by 2026-12-31")
        );
        assert!(override_confidence_cap);
        assert!(cap_rationale.is_some());
        assert!(skip_preflight);
        assert!(!accept_preflight);
        assert!(!with_adversary);
        assert!(json);
    }

    #[test]
    fn parse_analytics_predictions_add_alias_without_falsify() {
        // Alias without --falsify parses; falsify is None so the 0.3
        // unfalsifiable cap applies downstream in run_add_with_preflight.
        let cli = Cli::try_parse_from([
            "pftui",
            "analytics",
            "predictions",
            "add",
            "--claim",
            "BTC structurally repriced higher",
            "--timeframe",
            "medium",
            "--confidence",
            "0.9",
        ])
        .unwrap();
        let Some(Command::Analytics {
            command:
                AnalyticsCommand::Predictions {
                    command:
                        Some(DataPredictionsCommand::Add {
                            falsify,
                            override_confidence_cap,
                            cap_rationale,
                            skip_preflight,
                            ..
                        }),
                    ..
                },
        }) = cli.command
        else {
            panic!("expected analytics predictions add");
        };
        assert!(falsify.is_none());
        assert!(!override_confidence_cap);
        assert!(cap_rationale.is_none());
        assert!(
            !skip_preflight,
            "preflight must be ON by default, like journal add"
        );
    }
    // ── Research harness (R1a) ──────────────────────────────────────────

    #[test]
    fn parse_research_signals_list() {
        let cli = Cli::try_parse_from(["pftui", "research", "signals", "list", "--json"]).unwrap();
        let Some(Command::Research {
            command:
                ResearchCommand::Signals {
                    command: ResearchSignalsCommand::List { json },
                },
        }) = cli.command
        else {
            panic!("expected research signals list");
        };
        assert!(json);
    }

    #[test]
    fn parse_research_backtest_with_filters() {
        let cli = Cli::try_parse_from([
            "pftui",
            "research",
            "backtest",
            "--signal",
            "cyber_qb_flip_bear",
            "--asset",
            "GC=F",
            "--as-of",
            "2026-06-01",
            "--json",
        ])
        .unwrap();
        let Some(Command::Research {
            command:
                ResearchCommand::Backtest {
                    signal,
                    asset,
                    as_of,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected research backtest");
        };
        assert_eq!(signal.as_deref(), Some("cyber_qb_flip_bear"));
        assert_eq!(asset.as_deref(), Some("GC=F"));
        assert_eq!(as_of.as_deref(), Some("2026-06-01"));
        assert!(json);
    }

    #[test]
    fn parse_research_backtest_defaults() {
        let cli = Cli::try_parse_from(["pftui", "research", "backtest"]).unwrap();
        let Some(Command::Research {
            command:
                ResearchCommand::Backtest {
                    signal,
                    asset,
                    as_of,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected research backtest");
        };
        assert!(signal.is_none() && asset.is_none() && as_of.is_none() && !json);
    }

    #[test]
    fn parse_research_expectancy_and_events() {
        let cli = Cli::try_parse_from([
            "pftui",
            "research",
            "expectancy",
            "--asset",
            "BTC-USD",
            "--json",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Research {
                command: ResearchCommand::Expectancy { .. }
            })
        ));

        let cli = Cli::try_parse_from([
            "pftui",
            "research",
            "events",
            "--signal",
            "structure_weekly_flip_down",
            "--asset",
            "BTC-USD",
            "--limit",
            "12",
        ])
        .unwrap();
        let Some(Command::Research {
            command:
                ResearchCommand::Events {
                    signal,
                    asset,
                    limit,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected research events");
        };
        assert_eq!(signal, "structure_weekly_flip_down");
        assert_eq!(asset, "BTC-USD");
        assert_eq!(limit, 12);
        assert!(!json);
    }

    #[test]
    fn parse_research_events_requires_signal_and_asset() {
        assert!(Cli::try_parse_from(["pftui", "research", "events"]).is_err());
        assert!(Cli::try_parse_from(["pftui", "research", "events", "--signal", "x"]).is_err());
    }
}
