use std::collections::HashMap;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::config::{self, Config, PortfolioMode};
use crate::db::{allocations, price_cache, price_history};
use crate::db::transactions::{get_unique_symbols, list_transactions};
use crate::models::allocation::Allocation;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::models::price::{HistoryRecord, PriceQuote};
use crate::models::transaction::Transaction;
use crate::price::{PriceCommand, PriceService, PriceUpdate};
use crate::tui::theme::{self, Theme};
use crate::tui::views::markets;
use crate::db::watchlist as db_watchlist;
use crate::tui::views::economy;
use crate::tui::views::watchlist as watchlist_view;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Positions,
    Transactions,
    Markets,
    Economy,
    Watchlist,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartTimeframe {
    OneWeek,
    OneMonth,
    ThreeMonths,
    SixMonths,
    OneYear,
    FiveYears,
}

impl ChartTimeframe {
    pub fn days(self) -> u32 {
        match self {
            ChartTimeframe::OneWeek => 7,
            ChartTimeframe::OneMonth => 30,
            ChartTimeframe::ThreeMonths => 90,
            ChartTimeframe::SixMonths => 180,
            ChartTimeframe::OneYear => 365,
            ChartTimeframe::FiveYears => 1825,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChartTimeframe::OneWeek => "1W",
            ChartTimeframe::OneMonth => "1M",
            ChartTimeframe::ThreeMonths => "3M",
            ChartTimeframe::SixMonths => "6M",
            ChartTimeframe::OneYear => "1Y",
            ChartTimeframe::FiveYears => "5Y",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ChartTimeframe::OneWeek => ChartTimeframe::OneMonth,
            ChartTimeframe::OneMonth => ChartTimeframe::ThreeMonths,
            ChartTimeframe::ThreeMonths => ChartTimeframe::SixMonths,
            ChartTimeframe::SixMonths => ChartTimeframe::OneYear,
            ChartTimeframe::OneYear => ChartTimeframe::FiveYears,
            ChartTimeframe::FiveYears => ChartTimeframe::OneWeek,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            ChartTimeframe::OneWeek => ChartTimeframe::FiveYears,
            ChartTimeframe::OneMonth => ChartTimeframe::OneWeek,
            ChartTimeframe::ThreeMonths => ChartTimeframe::OneMonth,
            ChartTimeframe::SixMonths => ChartTimeframe::ThreeMonths,
            ChartTimeframe::OneYear => ChartTimeframe::SixMonths,
            ChartTimeframe::FiveYears => ChartTimeframe::OneYear,
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Name,
    Category,
    GainPct,
    TotalGain,
    Allocation,
    Date,
}

#[derive(Debug, Clone)]
pub enum ChartKind {
    /// Single price chart for one symbol
    Single {
        symbol: String,
        category: AssetCategory,
    },
    /// Ratio of two symbols (numerator / denominator)
    Ratio {
        num_symbol: String,
        num_category: AssetCategory,
        den_symbol: String,
        den_category: AssetCategory,
    },
    /// All individual charts stacked in a multi-panel view
    All,
}

#[derive(Debug, Clone)]
pub struct ChartVariant {
    pub label: String,
    pub kind: ChartKind,
}

impl ChartVariant {
    pub fn single(symbol: &str, label: &str, category: AssetCategory) -> Self {
        ChartVariant {
            label: label.to_string(),
            kind: ChartKind::Single {
                symbol: symbol.to_string(),
                category,
            },
        }
    }

    pub fn ratio(
        label: &str,
        num_sym: &str,
        num_cat: AssetCategory,
        den_sym: &str,
        den_cat: AssetCategory,
    ) -> Self {
        ChartVariant {
            label: label.to_string(),
            kind: ChartKind::Ratio {
                num_symbol: num_sym.to_string(),
                num_category: num_cat,
                den_symbol: den_sym.to_string(),
                den_category: den_cat,
            },
        }
    }
}

pub struct App {
    pub should_quit: bool,
    pub view_mode: ViewMode,
    pub show_help: bool,
    pub help_scroll: usize,
    pub detail_open: bool,
    pub detail_popup_open: bool,

    // Mode
    pub portfolio_mode: PortfolioMode,
    pub show_percentages_only: bool,

    // Data
    pub transactions: Vec<Transaction>,
    pub allocations: Vec<Allocation>,
    pub positions: Vec<Position>,
    pub prices: HashMap<String, Decimal>,
    pub base_currency: String,

    // Price history
    pub price_history: HashMap<String, Vec<HistoryRecord>>,
    pub portfolio_value_history: Vec<(String, Decimal)>,

    // Display (filtered/sorted)
    pub display_positions: Vec<Position>,
    pub display_transactions: Vec<Transaction>,

    // Navigation
    pub selected_index: usize,
    pub tx_selected_index: usize,
    pub markets_selected_index: usize,
    pub economy_selected_index: usize,
    pub watchlist_selected_index: usize,
    pub watchlist_entries: Vec<db_watchlist::WatchlistEntry>,
    pub g_pending: bool,
    pub terminal_height: u16,
    pub terminal_width: u16,

    // Search
    pub search_mode: bool,
    pub search_query: String,

    // Sorting
    pub sort_field: SortField,
    pub sort_ascending: bool,

    // Filter
    pub category_filter: Option<AssetCategory>,
    filter_cycle_index: usize,

    // Price service
    price_service: Option<PriceService>,
    pub prices_live: bool,
    last_refresh: Option<Instant>,
    refresh_interval_secs: u64,

    // Totals
    pub total_value: Decimal,
    pub total_cost: Decimal,

    // Theme
    pub theme: Theme,
    pub theme_name: String,

    // Chart
    pub chart_index: usize, // which chart variant to show for current position
    pub chart_timeframe: ChartTimeframe,

    // History fetch tracking: max days fetched per symbol (to avoid re-fetching)
    fetched_history_days: HashMap<String, u32>,

    // Animation
    pub tick_count: u64,
    pub price_flash_ticks: HashMap<String, u64>,
    pub last_value_update_tick: u64,

    // DB
    db_path: std::path::PathBuf,
}

/// Returns true when the UI should hide value-sensitive data.
/// Merge incoming history records with existing in-memory data.
/// Keeps the union of dates, preferring newer (incoming) prices for
/// overlapping dates. This prevents shorter-range re-fetches from
/// discarding longer-range data already loaded from the DB cache.
fn merge_history_into(
    history: &mut HashMap<String, Vec<HistoryRecord>>,
    symbol: String,
    new_records: Vec<HistoryRecord>,
) {
    use std::collections::BTreeMap;
    if let Some(existing) = history.get(&symbol) {
        if existing.is_empty() {
            history.insert(symbol, new_records);
            return;
        }
        // Build a map from the existing records (date -> record)
        let mut by_date: BTreeMap<String, HistoryRecord> = existing
            .iter()
            .map(|r| (r.date.clone(), r.clone()))
            .collect();
        // Overwrite/insert with new records (newer data wins)
        for r in new_records {
            by_date.insert(r.date.clone(), r);
        }
        // BTreeMap is sorted by key (date string in YYYY-MM-DD format)
        let merged: Vec<HistoryRecord> = by_date.into_values().collect();
        history.insert(symbol, merged);
    } else {
        history.insert(symbol, new_records);
    }
}

pub fn is_privacy_view(app: &App) -> bool {
    app.portfolio_mode == PortfolioMode::Percentage || app.show_percentages_only
}

impl App {
    pub fn new(config: &Config, db_path: std::path::PathBuf) -> Self {
        App {
            should_quit: false,
            view_mode: ViewMode::Positions,
            show_help: false,
            help_scroll: 0,
            detail_open: false,
            detail_popup_open: false,
            portfolio_mode: config.portfolio_mode,
            show_percentages_only: config.portfolio_mode == PortfolioMode::Percentage,
            transactions: Vec::new(),
            allocations: Vec::new(),
            positions: Vec::new(),
            prices: HashMap::new(),
            base_currency: config.base_currency.clone(),
            price_history: HashMap::new(),
            portfolio_value_history: Vec::new(),
            display_positions: Vec::new(),
            display_transactions: Vec::new(),
            selected_index: 0,
            tx_selected_index: 0,
            markets_selected_index: 0,
            economy_selected_index: 0,
            watchlist_selected_index: 0,
            watchlist_entries: Vec::new(),
            g_pending: false,
            terminal_height: 24, // sensible default, updated on resize
            terminal_width: 120, // sensible default, updated on resize
            search_mode: false,
            search_query: String::new(),
            sort_field: SortField::Allocation,
            sort_ascending: false,
            category_filter: None,
            filter_cycle_index: 0,
            price_service: None,
            prices_live: false,
            last_refresh: None,
            refresh_interval_secs: config.refresh_interval,
            total_value: dec!(0),
            total_cost: dec!(0),
            theme: theme::theme_by_name(&config.theme),
            theme_name: config.theme.clone(),
            chart_index: 0,
            chart_timeframe: ChartTimeframe::ThreeMonths,
            fetched_history_days: HashMap::new(),
            tick_count: 0,
            price_flash_ticks: HashMap::new(),
            last_value_update_tick: 0,
            db_path,
        }
    }

    pub fn init(&mut self) {
        self.load_data();
        self.load_cached_prices();
        self.load_cached_history();
        self.load_watchlist();
        self.recompute();

        // Start price service
        let config = Config {
            base_currency: self.base_currency.clone(),
            refresh_interval: self.refresh_interval_secs,
            portfolio_mode: self.portfolio_mode,
            theme: self.theme_name.clone(),
        };
        let service = PriceService::start(config);
        self.request_price_fetch(&service);
        self.request_all_history(&service);
        self.price_service = Some(service);
        self.request_market_data();
        self.request_economy_data();
    }

    fn load_data(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            match self.portfolio_mode {
                PortfolioMode::Full => {
                    self.transactions = list_transactions(&conn).unwrap_or_default();
                }
                PortfolioMode::Percentage => {
                    self.allocations = allocations::list_allocations(&conn).unwrap_or_default();
                }
            }
        }
    }

    fn load_cached_prices(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            if let Ok(cached) = price_cache::get_all_cached_prices(&conn) {
                for quote in cached {
                    self.prices.insert(quote.symbol, quote.price);
                }
            }
        }
    }

    fn load_cached_history(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            if let Ok(all) = price_history::get_all_symbols_history(&conn, ChartTimeframe::FiveYears.days()) {
                for (symbol, records) in all {
                    self.price_history.insert(symbol, records);
                }
            }
        }
        if self.portfolio_mode == PortfolioMode::Full {
            self.compute_portfolio_value_history();
        }
    }

    fn load_watchlist(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            self.watchlist_entries = db_watchlist::list_watchlist(&conn).unwrap_or_default();
        }
    }

    fn get_symbols(&self) -> Vec<(String, AssetCategory)> {
        if let Ok(conn) = Connection::open(&self.db_path) {
            match self.portfolio_mode {
                PortfolioMode::Full => get_unique_symbols(&conn).unwrap_or_default(),
                PortfolioMode::Percentage => {
                    allocations::get_unique_allocation_symbols(&conn).unwrap_or_default()
                }
            }
        } else {
            Vec::new()
        }
    }

    fn request_price_fetch(&self, service: &PriceService) {
        let symbols = self.get_symbols();
        if !symbols.is_empty() {
            service.send_command(PriceCommand::FetchAll(symbols));
        }
    }

    fn request_all_history(&mut self, service: &PriceService) {
        let mut seen = std::collections::HashSet::new();
        let mut batch = Vec::new();

        // On-demand strategy: only fetch 3M (90 days) at startup.
        // Longer timeframes are fetched on-demand when the user switches.
        // The DB cache may already have more data from previous sessions.
        let initial_days = ChartTimeframe::ThreeMonths.days();

        // Collect portfolio symbols
        let symbols = self.get_symbols();
        for (symbol, category) in &symbols {
            if seen.insert(symbol.clone()) {
                batch.push((symbol.clone(), *category, initial_days));
                self.fetched_history_days
                    .entry(symbol.clone())
                    .and_modify(|d| *d = (*d).max(initial_days))
                    .or_insert(initial_days);
            }
        }

        // Collect chart comparison symbols (indices, benchmarks)
        // so charts are ready when the user opens the detail panel
        for pos in &self.positions {
            for (sym, cat) in Self::chart_fetch_symbols(pos) {
                if seen.insert(sym.clone()) {
                    batch.push((sym.clone(), cat, initial_days));
                    self.fetched_history_days
                        .entry(sym.clone())
                        .and_modify(|d| *d = (*d).max(initial_days))
                        .or_insert(initial_days);
                }
            }
        }

        // Send as a single batch for concurrent fetching
        if !batch.is_empty() {
            service.send_command(PriceCommand::FetchHistoryBatch(batch));
        }
    }

    /// Request history for a single symbol, but only if we haven't already
    /// fetched at least `needed_days` worth of data for it this session.
    #[allow(dead_code)]
    fn request_history_for_symbol(&mut self, symbol: &str, category: AssetCategory) {
        let needed_days = self.chart_timeframe.days();
        self.request_history_if_needed(symbol, category, needed_days);
    }

    /// Fetch history for a symbol only if the cached fetch is shorter than needed.
    fn request_history_if_needed(&mut self, symbol: &str, category: AssetCategory, needed_days: u32) {
        let already_fetched = self.fetched_history_days.get(symbol).copied().unwrap_or(0);
        if already_fetched >= needed_days {
            return; // already have enough data
        }
        // Track that we're fetching this range
        self.fetched_history_days.insert(symbol.to_string(), needed_days);
        // Extract service ref and send command (avoid borrow conflict)
        if let Some(ref service) = self.price_service {
            service.send_command(PriceCommand::FetchHistory(
                symbol.to_string(),
                category,
                needed_days,
            ));
        }
    }

    /// Fetch spot prices and short history for all market overview symbols.
    fn request_market_data(&self) {
        if let Some(ref service) = self.price_service {
            let items = markets::market_symbols();
            let symbols: Vec<(String, AssetCategory)> = items
                .iter()
                .map(|item| (item.yahoo_symbol.clone(), item.category))
                .collect();
            if !symbols.is_empty() {
                service.send_command(PriceCommand::FetchAll(symbols.clone()));
                let batch: Vec<(String, AssetCategory, u32)> = symbols
                    .into_iter()
                    .map(|(sym, cat)| (sym, cat, 30)) // 30 days for change % calc
                    .collect();
                service.send_command(PriceCommand::FetchHistoryBatch(batch));
            }
        }
    }

    /// Fetch spot prices and short history for all economy dashboard symbols.
    fn request_economy_data(&self) {
        if let Some(ref service) = self.price_service {
            let items = economy::economy_symbols();
            let symbols: Vec<(String, AssetCategory)> = items
                .iter()
                .map(|item| (item.yahoo_symbol.clone(), economy::category_for_group(item.group)))
                .collect();
            if !symbols.is_empty() {
                service.send_command(PriceCommand::FetchAll(symbols.clone()));
                let batch: Vec<(String, AssetCategory, u32)> = symbols
                    .into_iter()
                    .map(|(sym, cat)| (sym, cat, 30))
                    .collect();
                service.send_command(PriceCommand::FetchHistoryBatch(batch));
            }
        }
    }

    /// Fetch spot prices and short history for all watchlist symbols.
    fn request_watchlist_data(&self) {
        if let Some(ref service) = self.price_service {
            if self.watchlist_entries.is_empty() {
                return;
            }
            let symbols: Vec<(String, AssetCategory)> = self
                .watchlist_entries
                .iter()
                .map(|e| {
                    let cat: AssetCategory = e.category.parse().unwrap_or(AssetCategory::Equity);
                    let yahoo = watchlist_view::yahoo_symbol_for(&e.symbol, cat);
                    (yahoo, cat)
                })
                .collect();
            if !symbols.is_empty() {
                service.send_command(PriceCommand::FetchAll(symbols.clone()));
                let batch: Vec<(String, AssetCategory, u32)> = symbols
                    .into_iter()
                    .map(|(sym, cat)| (sym, cat, 30))
                    .collect();
                service.send_command(PriceCommand::FetchHistoryBatch(batch));
            }
        }
    }

    /// Re-fetch history for the currently selected position's chart symbols
    /// using the current timeframe. Called when timeframe changes via h/l.
    /// Re-fetch chart history for the selected position if the current
    /// timeframe needs more data than we've already fetched.
    fn refetch_chart_history(&mut self) {
        if let Some(pos) = self.selected_position().cloned() {
            let needed_days = self.chart_timeframe.days();
            let fetch_syms = Self::chart_fetch_symbols(&pos);
            for (sym, cat) in &fetch_syms {
                self.request_history_if_needed(sym, *cat, needed_days);
            }
        }
    }

    /// Called when the selected position changes to auto-fetch chart data.
    /// Resets chart index and ensures history data is available for the new asset.
    fn on_position_selection_changed(&mut self) {
        if matches!(self.view_mode, ViewMode::Positions) {
            self.chart_index = 0;
            self.refetch_chart_history();
        }
    }

    pub fn recompute(&mut self) {
        match self.portfolio_mode {
            PortfolioMode::Full => {
                self.positions = compute_positions(&self.transactions, &self.prices);
            }
            PortfolioMode::Percentage => {
                self.positions =
                    compute_positions_from_allocations(&self.allocations, &self.prices);
            }
        }
        self.apply_filter_and_sort();
        self.compute_totals();
        self.last_value_update_tick = self.tick_count;
    }

    fn apply_filter_and_sort(&mut self) {
        // Filter positions by category
        let mut positions: Vec<Position> = match self.category_filter {
            Some(cat) => self
                .positions
                .iter()
                .filter(|p| p.category == cat)
                .cloned()
                .collect(),
            None => self.positions.clone(),
        };

        // Filter positions by search query
        if !self.search_query.is_empty() {
            let query = self.search_query.to_lowercase();
            positions.retain(|p| {
                p.symbol.to_lowercase().contains(&query)
                    || p.name.to_lowercase().contains(&query)
            });
        }

        // Sort positions
        match self.sort_field {
            SortField::Name => positions.sort_by(|a, b| a.symbol.cmp(&b.symbol)),
            SortField::Category => positions.sort_by(|a, b| {
                a.category.to_string().cmp(&b.category.to_string())
            }),
            SortField::GainPct => positions.sort_by(|a, b| {
                let ga = a.gain_pct.unwrap_or(dec!(0));
                let gb = b.gain_pct.unwrap_or(dec!(0));
                ga.cmp(&gb)
            }),
            SortField::TotalGain => positions.sort_by(|a, b| {
                let ga = a.gain.unwrap_or(dec!(0));
                let gb = b.gain.unwrap_or(dec!(0));
                ga.cmp(&gb)
            }),
            SortField::Allocation => positions.sort_by(|a, b| {
                let aa = a.allocation_pct.unwrap_or(dec!(0));
                let ab = b.allocation_pct.unwrap_or(dec!(0));
                aa.cmp(&ab)
            }),
            SortField::Date => {} // not applicable for positions
        }

        if !self.sort_ascending {
            positions.reverse();
        }

        self.display_positions = positions;

        // Sort transactions (only relevant in full mode)
        if self.portfolio_mode == PortfolioMode::Full {
            let mut txs = self.transactions.clone();

            // Filter transactions by search query
            if !self.search_query.is_empty() {
                let query = self.search_query.to_lowercase();
                txs.retain(|tx| tx.symbol.to_lowercase().contains(&query));
            }

            if matches!(self.sort_field, SortField::Date) {
                txs.sort_by(|a, b| a.date.cmp(&b.date));
                if !self.sort_ascending {
                    txs.reverse();
                }
            }
            self.display_transactions = txs;
        }

        // Clamp selection indices
        if !self.display_positions.is_empty() {
            self.selected_index = self
                .selected_index
                .min(self.display_positions.len() - 1);
        } else {
            self.selected_index = 0;
        }
        if !self.display_transactions.is_empty() {
            self.tx_selected_index = self
                .tx_selected_index
                .min(self.display_transactions.len() - 1);
        } else {
            self.tx_selected_index = 0;
        }
    }

    fn compute_totals(&mut self) {
        if self.portfolio_mode == PortfolioMode::Percentage {
            self.total_value = dec!(0);
            self.total_cost = dec!(0);
            return;
        }
        self.total_value = self
            .positions
            .iter()
            .filter_map(|p| p.current_value)
            .sum();
        self.total_cost = self.positions.iter().map(|p| p.total_cost).sum();
    }

    pub fn compute_portfolio_value_history(&mut self) {
        if self.portfolio_mode == PortfolioMode::Percentage {
            self.portfolio_value_history.clear();
            return;
        }

        let mut all_dates: Vec<String> = self
            .price_history
            .values()
            .flat_map(|records| records.iter().map(|r| r.date.clone()))
            .collect();
        all_dates.sort();
        all_dates.dedup();

        let mut price_by_date: HashMap<&str, HashMap<&str, Decimal>> = HashMap::new();
        for (symbol, records) in &self.price_history {
            let map: HashMap<&str, Decimal> = records
                .iter()
                .map(|r| (r.date.as_str(), r.close))
                .collect();
            price_by_date.insert(symbol.as_str(), map);
        }

        let mut history = Vec::new();
        for date in &all_dates {
            let mut total = dec!(0);
            let mut has_data = false;
            for pos in &self.positions {
                if pos.category == AssetCategory::Cash {
                    total += pos.quantity;
                    has_data = true;
                    continue;
                }
                if let Some(sym_prices) = price_by_date.get(pos.symbol.as_str()) {
                    if let Some(&close) = sym_prices.get(date.as_str()) {
                        total += pos.quantity * close;
                        has_data = true;
                    }
                }
            }
            if has_data {
                history.push((date.clone(), total));
            }
        }
        self.portfolio_value_history = history;
    }

    pub fn selected_position(&self) -> Option<&Position> {
        self.display_positions.get(self.selected_index)
    }

    /// Returns chart variants for a position.
    /// Index 0 is always "All" (multi-panel). Index 1+ are individual charts.
    pub fn chart_variants_for_position(pos: &Position) -> Vec<ChartVariant> {
        let sym = pos.symbol.to_uppercase();
        let is_crypto = pos.category == AssetCategory::Crypto;
        let is_cash = pos.category == AssetCategory::Cash;
        let is_commodity = pos.category == AssetCategory::Commodity;

        // Check for gold-like positions
        let is_gold = matches!(sym.as_str(), "GC=F" | "GOLD" | "XAUUSD" | "GLD" | "IAU");
        // Check for BTC-like positions
        let is_btc = matches!(sym.as_str(), "BTC" | "BTC-USD" | "BITCOIN");

        let individuals: Vec<ChartVariant> = if is_btc || (is_crypto && sym.contains("BTC")) {
            vec![
                ChartVariant::single("BTC-USD", "BTC/USD", AssetCategory::Equity),
                ChartVariant::ratio("BTC/SPX", "BTC-USD", AssetCategory::Equity, "^GSPC", AssetCategory::Equity),
                ChartVariant::ratio("BTC/Gold", "BTC-USD", AssetCategory::Equity, "GC=F", AssetCategory::Commodity),
                ChartVariant::ratio("BTC/QQQ", "BTC-USD", AssetCategory::Equity, "QQQ", AssetCategory::Equity),
            ]
        } else if is_gold || (is_commodity && (sym.contains("GC") || sym.contains("GOLD") || sym.contains("XAU"))) {
            vec![
                ChartVariant::single("GC=F", "Gold/USD", AssetCategory::Commodity),
                ChartVariant::ratio("Gold/BTC", "GC=F", AssetCategory::Commodity, "BTC-USD", AssetCategory::Equity),
                ChartVariant::ratio("Gold/SPX", "GC=F", AssetCategory::Commodity, "^GSPC", AssetCategory::Equity),
                ChartVariant::ratio("Gold/QQQ", "GC=F", AssetCategory::Commodity, "QQQ", AssetCategory::Equity),
            ]
        } else if is_cash && sym == "USD" {
            vec![
                ChartVariant::single("DX-Y.NYB", "Dollar Index (DXY)", AssetCategory::Forex),
                ChartVariant::ratio("USD/Gold", "DX-Y.NYB", AssetCategory::Forex, "GC=F", AssetCategory::Commodity),
                ChartVariant::single("BTC-USD", "BTC/USD", AssetCategory::Equity),
            ]
        } else if is_cash {
            let pair = format!("{}USD=X", sym);
            let pair_label = format!("{}/USD", sym);
            let ratio_label = format!("{}/DXY", sym);
            vec![
                ChartVariant::single(&pair, &pair_label, AssetCategory::Forex),
                ChartVariant::ratio(&ratio_label, &pair, AssetCategory::Forex, "DX-Y.NYB", AssetCategory::Forex),
                ChartVariant::single("GC=F", "Gold/USD", AssetCategory::Commodity),
                ChartVariant::single("BTC-USD", "BTC/USD", AssetCategory::Equity),
            ]
        } else {
            // Equity, Fund, non-BTC Crypto, non-Gold Commodity, Forex
            let yahoo_sym = if is_crypto {
                format!("{}-USD", sym)
            } else {
                pos.symbol.clone()
            };
            let cat = if is_crypto {
                AssetCategory::Equity // route to Yahoo
            } else {
                pos.category
            };
            let label = if pos.name.is_empty() {
                pos.symbol.clone()
            } else {
                format!("{} ({})", pos.name, pos.symbol)
            };

            let is_equity = pos.category == AssetCategory::Equity;
            let is_fund = pos.category == AssetCategory::Fund;

            if is_equity || is_fund || is_crypto || is_commodity {
                let mut variants = vec![
                    ChartVariant {
                        label: label.clone(),
                        kind: ChartKind::Single {
                            symbol: yahoo_sym.clone(),
                            category: cat,
                        },
                    },
                ];

                // Don't add ratio against yourself (e.g., ^GSPC/SPX or QQQ/QQQ)
                let is_spx = matches!(sym.as_str(), "^GSPC" | "SPY" | "VOO" | "IVV" | "SPX");
                let is_qqq = matches!(sym.as_str(), "^IXIC" | "QQQ" | "TQQQ" | "QQQM" | "NDX");

                if is_crypto {
                    // Non-BTC crypto: {SYM}/BTC and {SYM}/SPX
                    variants.push(ChartVariant::ratio(
                        &format!("{}/BTC", pos.symbol),
                        &yahoo_sym,
                        AssetCategory::Equity,
                        "BTC-USD",
                        AssetCategory::Equity,
                    ));
                    variants.push(ChartVariant::ratio(
                        &format!("{}/SPX", pos.symbol),
                        &yahoo_sym,
                        AssetCategory::Equity,
                        "^GSPC",
                        AssetCategory::Equity,
                    ));
                } else {
                    // Equity, Fund, non-Gold Commodity: {SYM}/SPX and {SYM}/QQQ
                    if !is_spx {
                        variants.push(ChartVariant::ratio(
                            &format!("{}/SPX", pos.symbol),
                            &yahoo_sym,
                            cat,
                            "^GSPC",
                            AssetCategory::Equity,
                        ));
                    }
                    if !is_qqq {
                        variants.push(ChartVariant::ratio(
                            &format!("{}/QQQ", pos.symbol),
                            &yahoo_sym,
                            cat,
                            "QQQ",
                            AssetCategory::Equity,
                        ));
                    }
                }

                variants
            } else {
                // Forex or other: just a single chart, no ratios
                return vec![ChartVariant {
                    label,
                    kind: ChartKind::Single {
                        symbol: yahoo_sym,
                        category: cat,
                    },
                }];
            }
        };

        // Prepend "All" as index 0
        let mut result = vec![ChartVariant {
            label: "All".to_string(),
            kind: ChartKind::All,
        }];
        result.extend(individuals);
        result
    }

    /// Returns all symbols that need to be fetched for chart variants.
    pub fn chart_fetch_symbols(pos: &Position) -> Vec<(String, AssetCategory)> {
        let mut symbols = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for variant in Self::chart_variants_for_position(pos) {
            match variant.kind {
                ChartKind::Single { symbol, category } => {
                    if seen.insert(symbol.clone()) {
                        symbols.push((symbol, category));
                    }
                }
                ChartKind::Ratio {
                    num_symbol,
                    num_category,
                    den_symbol,
                    den_category,
                } => {
                    if seen.insert(num_symbol.clone()) {
                        symbols.push((num_symbol, num_category));
                    }
                    if seen.insert(den_symbol.clone()) {
                        symbols.push((den_symbol, den_category));
                    }
                }
                ChartKind::All => {}
            }
        }
        symbols
    }

    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);

        if let Some(ref service) = self.price_service {
            let mut updated = false;
            let mut history_updated = false;
            loop {
                match service.try_recv() {
                    Some(PriceUpdate::Quote(quote)) => {
                        self.price_flash_ticks
                            .insert(quote.symbol.clone(), self.tick_count);
                        self.prices.insert(quote.symbol.clone(), quote.price);
                        self.cache_price(&quote);
                        updated = true;
                    }
                    Some(PriceUpdate::History(symbol, records)) => {
                        self.cache_history(&symbol, &records);
                        merge_history_into(&mut self.price_history, symbol, records);
                        history_updated = true;
                    }
                    Some(PriceUpdate::FetchComplete) => {
                        self.prices_live = true;
                        self.last_refresh = Some(Instant::now());
                        updated = true;
                    }
                    Some(PriceUpdate::Error(_)) => {}
                    None => break,
                }
            }
            if updated {
                self.recompute();
            }
            if history_updated && self.portfolio_mode == PortfolioMode::Full {
                self.compute_portfolio_value_history();
            }
        }

        if let Some(last) = self.last_refresh {
            if last.elapsed().as_secs() >= self.refresh_interval_secs {
                self.force_refresh();
            }
        }
    }

    fn cache_price(&self, quote: &PriceQuote) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            let _ = price_cache::upsert_price(&conn, quote);
        }
    }

    fn cache_history(&self, symbol: &str, records: &[HistoryRecord]) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            let source = self
                .positions
                .iter()
                .find(|p| p.symbol == symbol)
                .map(|p| match p.category {
                    AssetCategory::Crypto => "coingecko",
                    _ => "yahoo",
                })
                .unwrap_or("yahoo");
            let _ = price_history::upsert_history(&conn, symbol, source, records);
        }
    }

    pub fn force_refresh(&mut self) {
        self.prices_live = false;
        if let Some(ref service) = self.price_service {
            self.request_price_fetch(service);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Search mode input handling (must be checked before global keys
        // so that typing e.g. 'q' doesn't quit while searching)
        if self.search_mode {
            match key.code {
                KeyCode::Esc => {
                    self.search_mode = false;
                    self.search_query.clear();
                    self.apply_filter_and_sort();
                }
                KeyCode::Enter => {
                    // Confirm search — exit search mode but keep filter active
                    self.search_mode = false;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.selected_index = 0;
                    self.tx_selected_index = 0;
                    self.apply_filter_and_sort();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.selected_index = 0;
                    self.tx_selected_index = 0;
                    self.apply_filter_and_sort();
                }
                _ => {}
            }
            return;
        }

        // Global keys
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.help_scroll = 0;
                }
                return;
            }
            KeyCode::Esc if self.detail_popup_open => {
                self.detail_popup_open = false;
                return;
            }
            KeyCode::Esc if self.show_help => {
                self.show_help = false;
                return;
            }
            _ => {}
        }

        if self.show_help {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.help_scroll = self.help_scroll.saturating_add(1);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.help_scroll = self.help_scroll.saturating_add(self.half_page());
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.help_scroll = self.help_scroll.saturating_sub(self.half_page());
                }
                KeyCode::Char('G') => {
                    // Scrolled to max — render function will clamp
                    self.help_scroll = usize::MAX;
                }
                _ => {
                    // gg detection within help
                    if key.code == KeyCode::Char('g') {
                        if self.g_pending {
                            self.help_scroll = 0;
                            self.g_pending = false;
                        } else {
                            self.g_pending = true;
                        }
                    } else {
                        self.g_pending = false;
                    }
                }
            }
            return;
        }

        // Handle gg vim motion (two-key sequence)
        if self.g_pending {
            self.g_pending = false;
            if key.code == KeyCode::Char('g') {
                self.jump_to_top();
                return;
            }
            // g was pressed but followed by something other than g — fall through
        }

        // Set g_pending when g is pressed (first of potential gg sequence)
        if key.code == KeyCode::Char('g') {
            self.g_pending = true;
            return;
        }

        match key.code {
            // View switching
            KeyCode::Char('1') => {
                self.view_mode = ViewMode::Positions;
            }
            KeyCode::Char('2') => {
                // Transactions view not available in percentage mode
                if self.portfolio_mode != PortfolioMode::Percentage {
                    self.view_mode = ViewMode::Transactions;
                    self.detail_open = false;
                    self.detail_popup_open = false;
                }
            }
            KeyCode::Char('3') => {
                self.view_mode = ViewMode::Markets;
                self.detail_open = false;
                self.detail_popup_open = false;
                self.request_market_data();
            }
            KeyCode::Char('4') => {
                self.view_mode = ViewMode::Economy;
                self.detail_open = false;
                self.detail_popup_open = false;
                self.request_economy_data();
            }
            KeyCode::Char('5') => {
                self.view_mode = ViewMode::Watchlist;
                self.detail_open = false;
                self.detail_popup_open = false;
                self.load_watchlist();
                self.request_watchlist_data();
            }

            // Privacy toggle
            KeyCode::Char('p') => {
                if self.portfolio_mode == PortfolioMode::Full {
                    self.show_percentages_only = !self.show_percentages_only;
                }
            }

            // Detail popup toggle (chart is always visible in right pane)
            KeyCode::Enter if matches!(self.view_mode, ViewMode::Positions) => {
                if self.selected_position().is_some() {
                    self.detail_popup_open = !self.detail_popup_open;
                }
            }

            // Chart variant cycling with J/K (when detail open)
            KeyCode::Char('K') if matches!(self.view_mode, ViewMode::Positions) => {
                if let Some(pos) = self.selected_position() {
                    let count = Self::chart_variants_for_position(pos).len();
                    if count > 1 {
                        self.chart_index = if self.chart_index == 0 {
                            count - 1
                        } else {
                            self.chart_index - 1
                        };
                    }
                }
            }
            KeyCode::Char('J') if matches!(self.view_mode, ViewMode::Positions) => {
                if let Some(pos) = self.selected_position() {
                    let count = Self::chart_variants_for_position(pos).len();
                    if count > 1 {
                        self.chart_index = (self.chart_index + 1) % count;
                    }
                }
            }
            // Timeframe cycling with h/l (when detail open)
            KeyCode::Char('h') | KeyCode::Left if matches!(self.view_mode, ViewMode::Positions) => {
                self.chart_timeframe = self.chart_timeframe.prev();
                self.refetch_chart_history();
            }
            KeyCode::Char('l') | KeyCode::Right if matches!(self.view_mode, ViewMode::Positions) => {
                self.chart_timeframe = self.chart_timeframe.next();
                self.refetch_chart_history();
            }


            // Navigation
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_down_half_page();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_up_half_page();
            }

            // Sorting
            KeyCode::Char('a') => {
                self.sort_field = SortField::Allocation;
                self.sort_ascending = false;
                self.recompute();
            }
            KeyCode::Char('%') => {
                if !is_privacy_view(self) {
                    self.sort_field = SortField::GainPct;
                    self.sort_ascending = false;
                    self.recompute();
                }
            }
            KeyCode::Char('G') => {
                self.jump_to_bottom();
            }
            KeyCode::Char('$') => {
                if !is_privacy_view(self) {
                    self.sort_field = SortField::TotalGain;
                    self.sort_ascending = false;
                    self.recompute();
                }
            }
            KeyCode::Char('n') => {
                self.sort_field = SortField::Name;
                self.sort_ascending = true;
                self.recompute();
            }
            KeyCode::Char('c') => {
                self.sort_field = SortField::Category;
                self.sort_ascending = true;
                self.recompute();
            }
            KeyCode::Char('d') => {
                if self.portfolio_mode != PortfolioMode::Percentage {
                    self.sort_field = SortField::Date;
                    self.sort_ascending = false;
                    self.recompute();
                }
            }
            KeyCode::Tab => {
                self.sort_ascending = !self.sort_ascending;
                self.recompute();
            }

            // Filter
            KeyCode::Char('f') => {
                self.cycle_filter();
            }

            // Search
            KeyCode::Char('/') => {
                self.search_mode = true;
                self.search_query.clear();
                self.apply_filter_and_sort();
            }

            // Refresh
            KeyCode::Char('r') => {
                self.force_refresh();
            }

            // Theme cycle
            KeyCode::Char('t') => {
                self.cycle_theme();
            }

            _ => {}
        }
    }

    fn move_down(&mut self) {
        let old_pos_idx = self.selected_index;
        match self.view_mode {
            ViewMode::Positions => {
                if !self.display_positions.is_empty() {
                    self.selected_index =
                        (self.selected_index + 1).min(self.display_positions.len() - 1);
                }
            }
            ViewMode::Transactions => {
                if !self.display_transactions.is_empty() {
                    self.tx_selected_index =
                        (self.tx_selected_index + 1).min(self.display_transactions.len() - 1);
                }
            }
            ViewMode::Markets => {
                let count = markets::market_symbols().len();
                if count > 0 {
                    self.markets_selected_index =
                        (self.markets_selected_index + 1).min(count - 1);
                }
            }
            ViewMode::Economy => {
                let count = economy::economy_symbols().len();
                if count > 0 {
                    self.economy_selected_index =
                        (self.economy_selected_index + 1).min(count - 1);
                }
            }
            ViewMode::Watchlist => {
                if !self.watchlist_entries.is_empty() {
                    self.watchlist_selected_index =
                        (self.watchlist_selected_index + 1).min(self.watchlist_entries.len() - 1);
                }
            }
        }
        if matches!(self.view_mode, ViewMode::Positions) && self.selected_index != old_pos_idx {
            self.on_position_selection_changed();
        }
    }

    fn move_up(&mut self) {
        let old_pos_idx = self.selected_index;
        match self.view_mode {
            ViewMode::Positions => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            ViewMode::Transactions => {
                self.tx_selected_index = self.tx_selected_index.saturating_sub(1);
            }
            ViewMode::Markets => {
                self.markets_selected_index = self.markets_selected_index.saturating_sub(1);
            }
            ViewMode::Economy => {
                self.economy_selected_index = self.economy_selected_index.saturating_sub(1);
            }
            ViewMode::Watchlist => {
                self.watchlist_selected_index = self.watchlist_selected_index.saturating_sub(1);
            }
        }
        if matches!(self.view_mode, ViewMode::Positions) && self.selected_index != old_pos_idx {
            self.on_position_selection_changed();
        }
    }

    fn jump_to_top(&mut self) {
        let old_pos_idx = self.selected_index;
        match self.view_mode {
            ViewMode::Positions => {
                self.selected_index = 0;
            }
            ViewMode::Transactions => {
                self.tx_selected_index = 0;
            }
            ViewMode::Markets => {
                self.markets_selected_index = 0;
            }
            ViewMode::Economy => {
                self.economy_selected_index = 0;
            }
            ViewMode::Watchlist => {
                self.watchlist_selected_index = 0;
            }
        }
        if matches!(self.view_mode, ViewMode::Positions) && self.selected_index != old_pos_idx {
            self.on_position_selection_changed();
        }
    }

    fn jump_to_bottom(&mut self) {
        let old_pos_idx = self.selected_index;
        match self.view_mode {
            ViewMode::Positions => {
                if !self.display_positions.is_empty() {
                    self.selected_index = self.display_positions.len() - 1;
                }
            }
            ViewMode::Transactions => {
                if !self.display_transactions.is_empty() {
                    self.tx_selected_index = self.display_transactions.len() - 1;
                }
            }
            ViewMode::Markets => {
                let count = markets::market_symbols().len();
                if count > 0 {
                    self.markets_selected_index = count - 1;
                }
            }
            ViewMode::Economy => {
                let count = economy::economy_symbols().len();
                if count > 0 {
                    self.economy_selected_index = count - 1;
                }
            }
            ViewMode::Watchlist => {
                if !self.watchlist_entries.is_empty() {
                    self.watchlist_selected_index = self.watchlist_entries.len() - 1;
                }
            }
        }
        if matches!(self.view_mode, ViewMode::Positions) && self.selected_index != old_pos_idx {
            self.on_position_selection_changed();
        }
    }

    pub fn set_terminal_size(&mut self, w: u16, h: u16) {
        self.terminal_width = w;
        self.terminal_height = h;
    }

    /// Half-page size: (terminal_height - 4 for header/status) / 2, minimum 1
    fn half_page(&self) -> usize {
        let content_rows = self.terminal_height.saturating_sub(4) as usize;
        (content_rows / 2).max(1)
    }

    fn scroll_down_half_page(&mut self) {
        let old_pos_idx = self.selected_index;
        let step = self.half_page();
        match self.view_mode {
            ViewMode::Positions => {
                if !self.display_positions.is_empty() {
                    self.selected_index =
                        (self.selected_index + step).min(self.display_positions.len() - 1);
                }
            }
            ViewMode::Transactions => {
                if !self.display_transactions.is_empty() {
                    self.tx_selected_index =
                        (self.tx_selected_index + step).min(self.display_transactions.len() - 1);
                }
            }
            ViewMode::Markets => {
                let count = markets::market_symbols().len();
                if count > 0 {
                    self.markets_selected_index =
                        (self.markets_selected_index + step).min(count - 1);
                }
            }
            ViewMode::Economy => {
                let count = economy::economy_symbols().len();
                if count > 0 {
                    self.economy_selected_index =
                        (self.economy_selected_index + step).min(count - 1);
                }
            }
            ViewMode::Watchlist => {
                if !self.watchlist_entries.is_empty() {
                    self.watchlist_selected_index =
                        (self.watchlist_selected_index + step).min(self.watchlist_entries.len() - 1);
                }
            }
        }
        if matches!(self.view_mode, ViewMode::Positions) && self.selected_index != old_pos_idx {
            self.on_position_selection_changed();
        }
    }

    fn scroll_up_half_page(&mut self) {
        let old_pos_idx = self.selected_index;
        let step = self.half_page();
        match self.view_mode {
            ViewMode::Positions => {
                self.selected_index = self.selected_index.saturating_sub(step);
            }
            ViewMode::Transactions => {
                self.tx_selected_index = self.tx_selected_index.saturating_sub(step);
            }
            ViewMode::Markets => {
                self.markets_selected_index = self.markets_selected_index.saturating_sub(step);
            }
            ViewMode::Economy => {
                self.economy_selected_index = self.economy_selected_index.saturating_sub(step);
            }
            ViewMode::Watchlist => {
                self.watchlist_selected_index = self.watchlist_selected_index.saturating_sub(step);
            }
        }
        if matches!(self.view_mode, ViewMode::Positions) && self.selected_index != old_pos_idx {
            self.on_position_selection_changed();
        }
    }

    fn cycle_filter(&mut self) {
        let categories = AssetCategory::all();
        if self.category_filter.is_none() {
            self.filter_cycle_index = 0;
            self.category_filter = Some(categories[0]);
        } else if self.filter_cycle_index + 1 < categories.len() {
            self.filter_cycle_index += 1;
            self.category_filter = Some(categories[self.filter_cycle_index]);
        } else {
            self.category_filter = None;
            self.filter_cycle_index = 0;
        }
        self.recompute();
    }

    fn cycle_theme(&mut self) {
        let next = theme::next_theme_name(&self.theme_name);
        self.theme_name = next.to_string();
        self.theme = theme::theme_by_name(next);
        // Persist to config
        if let Ok(mut cfg) = config::load_config() {
            cfg.theme = self.theme_name.clone();
            let _ = config::save_config(&cfg);
        }
    }

    pub fn sort_field_label(&self) -> &'static str {
        match self.sort_field {
            SortField::Name => "name",
            SortField::Category => "category",
            SortField::GainPct => "gain%",
            SortField::TotalGain => "gain",
            SortField::Allocation => "alloc%",
            SortField::Date => "date",
        }
    }

    pub fn shutdown(self) {
        if let Some(service) = self.price_service {
            service.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_position(symbol: &str, category: AssetCategory) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: String::new(),
            category,
            quantity: dec!(1),
            avg_cost: dec!(0),
            total_cost: dec!(0),
            currency: "USD".to_string(),
            current_price: None,
            current_value: None,
            gain: None,
            gain_pct: None,
            allocation_pct: None,
        }
    }

    fn variant_symbols(variants: &[ChartVariant]) -> Vec<String> {
        variants
            .iter()
            .filter_map(|v| match &v.kind {
                ChartKind::Single { symbol, .. } => Some(symbol.clone()),
                _ => None,
            })
            .collect()
    }

    fn variant_labels(variants: &[ChartVariant]) -> Vec<String> {
        variants.iter().map(|v| v.label.clone()).collect()
    }

    #[test]
    fn test_btc_variants() {
        let pos = make_position("BTC", AssetCategory::Crypto);
        let variants = App::chart_variants_for_position(&pos);

        assert_eq!(variants.len(), 5); // All + 4 individuals
        assert_eq!(variants[0].label, "All");
        assert!(matches!(variants[0].kind, ChartKind::All));
        assert_eq!(variants[1].label, "BTC/USD");
        assert_eq!(variants[2].label, "BTC/SPX");
        assert_eq!(variants[3].label, "BTC/Gold");
        assert_eq!(variants[4].label, "BTC/QQQ");
    }

    #[test]
    fn test_gold_variants() {
        let pos = make_position("GC=F", AssetCategory::Commodity);
        let variants = App::chart_variants_for_position(&pos);

        assert_eq!(variants.len(), 5);
        assert_eq!(variants[0].label, "All");
        assert_eq!(variants[1].label, "Gold/USD");
        assert_eq!(variants[2].label, "Gold/BTC");
        assert_eq!(variants[3].label, "Gold/SPX");
        assert_eq!(variants[4].label, "Gold/QQQ");
    }

    #[test]
    fn test_usd_cash_variants() {
        let pos = make_position("USD", AssetCategory::Cash);
        let variants = App::chart_variants_for_position(&pos);

        assert_eq!(variants.len(), 4); // All + 3 individuals
        assert_eq!(variants[0].label, "All");
        assert_eq!(variants[1].label, "Dollar Index (DXY)");

        // DXY should be single chart for USD
        match &variants[1].kind {
            ChartKind::Single { symbol, .. } => assert_eq!(symbol, "DX-Y.NYB"),
            _ => panic!("Expected Single chart for DXY"),
        }
    }

    #[test]
    fn test_non_usd_cash_variants_ratio_dxy() {
        let pos = make_position("EUR", AssetCategory::Cash);
        let variants = App::chart_variants_for_position(&pos);

        assert_eq!(variants.len(), 5); // All + 4 individuals
        assert_eq!(variants[0].label, "All");
        assert_eq!(variants[1].label, "EUR/USD");
        assert_eq!(variants[2].label, "EUR/DXY");

        // EUR/DXY should be a ratio (EURUSD=X / DX-Y.NYB), not a single DXY chart
        match &variants[2].kind {
            ChartKind::Ratio {
                num_symbol,
                den_symbol,
                ..
            } => {
                assert_eq!(num_symbol, "EURUSD=X");
                assert_eq!(den_symbol, "DX-Y.NYB");
            }
            _ => panic!("Expected Ratio chart for EUR/DXY"),
        }

        // Should NOT contain DXY as a standalone single chart
        let singles = variant_symbols(&variants);
        assert!(!singles.contains(&"DX-Y.NYB".to_string()),
            "Non-USD cash should not have DXY as a standalone chart");
    }

    #[test]
    fn test_gbp_cash_variants() {
        let pos = make_position("GBP", AssetCategory::Cash);
        let variants = App::chart_variants_for_position(&pos);

        let labels = variant_labels(&variants);
        assert_eq!(labels[1], "GBP/USD");
        assert_eq!(labels[2], "GBP/DXY");

        // Verify GBP/USD pair symbol is correct
        match &variants[1].kind {
            ChartKind::Single { symbol, .. } => assert_eq!(symbol, "GBPUSD=X"),
            _ => panic!("Expected Single chart for GBP/USD"),
        }
    }

    #[test]
    fn test_regular_equity_has_ratio_variants() {
        let pos = make_position("AAPL", AssetCategory::Equity);
        let variants = App::chart_variants_for_position(&pos);

        // Equities get All + single + /SPX + /QQQ = 4 variants
        assert_eq!(variants.len(), 4);
        assert_eq!(variants[0].label, "All");
        assert!(matches!(variants[0].kind, ChartKind::All));

        // Single chart
        match &variants[1].kind {
            ChartKind::Single { symbol, .. } => assert_eq!(symbol, "AAPL"),
            _ => panic!("Expected Single chart for equity"),
        }

        // {SYM}/SPX ratio
        assert_eq!(variants[2].label, "AAPL/SPX");
        match &variants[2].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "AAPL");
                assert_eq!(den_symbol, "^GSPC");
            }
            _ => panic!("Expected Ratio chart for AAPL/SPX"),
        }

        // {SYM}/QQQ ratio
        assert_eq!(variants[3].label, "AAPL/QQQ");
        match &variants[3].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "AAPL");
                assert_eq!(den_symbol, "QQQ");
            }
            _ => panic!("Expected Ratio chart for AAPL/QQQ"),
        }
    }

    #[test]
    fn test_spy_skips_spx_ratio() {
        // SPY is an S&P 500 ETF — skip SPY/SPX ratio (would be ~1.0)
        let pos = make_position("SPY", AssetCategory::Equity);
        let variants = App::chart_variants_for_position(&pos);

        let labels = variant_labels(&variants);
        assert!(!labels.contains(&"SPY/SPX".to_string()), "SPY should not have SPY/SPX ratio");
        assert!(labels.contains(&"SPY/QQQ".to_string()), "SPY should have SPY/QQQ ratio");
    }

    #[test]
    fn test_qqq_skips_qqq_ratio() {
        // QQQ should skip QQQ/QQQ ratio
        let pos = make_position("QQQ", AssetCategory::Equity);
        let variants = App::chart_variants_for_position(&pos);

        let labels = variant_labels(&variants);
        assert!(labels.contains(&"QQQ/SPX".to_string()), "QQQ should have QQQ/SPX ratio");
        assert!(!labels.contains(&"QQQ/QQQ".to_string()), "QQQ should not have QQQ/QQQ ratio");
    }

    #[test]
    fn test_fund_has_ratio_variants() {
        let pos = make_position("VTI", AssetCategory::Fund);
        let variants = App::chart_variants_for_position(&pos);

        assert_eq!(variants.len(), 4); // All + single + /SPX + /QQQ
        assert_eq!(variants[0].label, "All");
        assert_eq!(variants[2].label, "VTI/SPX");
        assert_eq!(variants[3].label, "VTI/QQQ");
    }

    #[test]
    fn test_crypto_non_btc_has_ratio_variants() {
        let pos = make_position("ETH", AssetCategory::Crypto);
        let variants = App::chart_variants_for_position(&pos);

        // Non-BTC crypto gets All + single + /BTC + /SPX = 4 variants
        assert_eq!(variants.len(), 4);
        assert_eq!(variants[0].label, "All");

        // Single chart with -USD suffix
        match &variants[1].kind {
            ChartKind::Single { symbol, category } => {
                assert_eq!(symbol, "ETH-USD");
                assert_eq!(*category, AssetCategory::Equity); // routed to Yahoo
            }
            _ => panic!("Expected Single chart for crypto"),
        }

        // {SYM}/BTC ratio
        assert_eq!(variants[2].label, "ETH/BTC");
        match &variants[2].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "ETH-USD");
                assert_eq!(den_symbol, "BTC-USD");
            }
            _ => panic!("Expected Ratio chart for ETH/BTC"),
        }

        // {SYM}/SPX ratio
        assert_eq!(variants[3].label, "ETH/SPX");
        match &variants[3].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "ETH-USD");
                assert_eq!(den_symbol, "^GSPC");
            }
            _ => panic!("Expected Ratio chart for ETH/SPX"),
        }
    }

    #[test]
    fn test_chart_fetch_symbols_deduplicates() {
        let pos = make_position("BTC", AssetCategory::Crypto);
        let syms = App::chart_fetch_symbols(&pos);

        // BTC-USD appears in multiple variants but should be deduplicated
        let btc_count = syms.iter().filter(|(s, _)| s == "BTC-USD").count();
        assert_eq!(btc_count, 1);
    }

    #[test]
    fn test_chart_fetch_symbols_non_usd_cash_includes_dxy() {
        let pos = make_position("EUR", AssetCategory::Cash);
        let syms = App::chart_fetch_symbols(&pos);

        // Should include DX-Y.NYB for the ratio chart denominator
        assert!(syms.iter().any(|(s, _)| s == "DX-Y.NYB"),
            "Non-USD cash chart fetch should include DX-Y.NYB for ratio");
        assert!(syms.iter().any(|(s, _)| s == "EURUSD=X"),
            "Non-USD cash chart fetch should include the forex pair");
    }

    #[test]
    fn test_merge_history_into_empty_map() {
        use crate::models::price::HistoryRecord;
        let mut history = HashMap::new();
        let records = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(110), volume: None },
        ];
        merge_history_into(&mut history, "AAPL".to_string(), records);
        assert_eq!(history.get("AAPL").unwrap().len(), 2);
    }

    #[test]
    fn test_merge_history_into_preserves_older_data() {
        use crate::models::price::HistoryRecord;
        let mut history = HashMap::new();
        // Existing: 3 months of data
        history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None },
            HistoryRecord { date: "2025-02-01".into(), close: dec!(110), volume: None },
            HistoryRecord { date: "2025-03-01".into(), close: dec!(120), volume: None },
        ]);
        // New fetch returns only last month (shorter range)
        let new_records = vec![
            HistoryRecord { date: "2025-03-01".into(), close: dec!(125), volume: None },
        ];
        merge_history_into(&mut history, "AAPL".to_string(), new_records);
        let merged = history.get("AAPL").unwrap();
        // Should have all 3 dates (Jan, Feb preserved; Mar updated)
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].date, "2025-01-01");
        assert_eq!(merged[0].close, dec!(100)); // preserved
        assert_eq!(merged[2].date, "2025-03-01");
        assert_eq!(merged[2].close, dec!(125)); // updated
    }

    #[test]
    fn test_merge_history_into_adds_new_dates() {
        use crate::models::price::HistoryRecord;
        let mut history = HashMap::new();
        history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None },
        ]);
        let new_records = vec![
            HistoryRecord { date: "2025-01-02".into(), close: dec!(105), volume: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(110), volume: None },
        ];
        merge_history_into(&mut history, "AAPL".to_string(), new_records);
        let merged = history.get("AAPL").unwrap();
        assert_eq!(merged.len(), 3);
        // Should be sorted by date (BTreeMap guarantees this)
        assert_eq!(merged[0].date, "2025-01-01");
        assert_eq!(merged[1].date, "2025-01-02");
        assert_eq!(merged[2].date, "2025-01-03");
    }

    #[test]
    fn test_merge_history_into_existing_empty() {
        use crate::models::price::HistoryRecord;
        let mut history = HashMap::new();
        history.insert("AAPL".to_string(), Vec::new());
        let new_records = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None },
        ];
        merge_history_into(&mut history, "AAPL".to_string(), new_records);
        assert_eq!(history.get("AAPL").unwrap().len(), 1);
    }
}

#[cfg(test)]
mod vim_motion_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rust_decimal_macros::dec;
    use std::path::PathBuf;

    fn make_test_app(num_positions: usize) -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        let mut app = App::new(&config, PathBuf::from("/tmp/pftui_test_vim.db"));

        // Populate display_positions with dummy data
        for i in 0..num_positions {
            app.display_positions.push(Position {
                symbol: format!("SYM{}", i),
                name: format!("Symbol {}", i),
                category: AssetCategory::Equity,
                quantity: dec!(1),
                avg_cost: dec!(100),
                total_cost: dec!(100),
                currency: "USD".to_string(),
                current_price: Some(dec!(110)),
                current_value: Some(dec!(110)),
                gain: Some(dec!(10)),
                gain_pct: Some(dec!(10)),
                allocation_pct: Some(Decimal::from(100u64 / num_positions as u64)),
            });
        }
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }


    #[test]
    fn test_gg_jumps_to_top() {
        let mut app = make_test_app(10);
        app.selected_index = 5;

        // Press g once (sets pending)
        app.handle_key(key('g'));
        assert!(app.g_pending);
        assert_eq!(app.selected_index, 5); // hasn't moved yet

        // Press g again (gg motion)
        app.handle_key(key('g'));
        assert!(!app.g_pending);
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_g_pending_cleared_by_other_key() {
        let mut app = make_test_app(10);
        app.selected_index = 5;

        // Press g (sets pending)
        app.handle_key(key('g'));
        assert!(app.g_pending);

        // Press j (clears pending, moves down)
        app.handle_key(key('j'));
        assert!(!app.g_pending);
        assert_eq!(app.selected_index, 6);
    }

    #[test]
    fn test_shift_g_jumps_to_bottom() {
        let mut app = make_test_app(10);
        app.selected_index = 0;

        app.handle_key(key('G'));
        assert_eq!(app.selected_index, 9);
    }

    #[test]
    fn test_gg_from_bottom() {
        let mut app = make_test_app(10);
        app.selected_index = 9;

        app.handle_key(key('g'));
        app.handle_key(key('g'));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_g_jumps_on_empty_list() {
        let mut app = make_test_app(0);

        // G on empty list should not panic
        app.handle_key(key('G'));
        assert_eq!(app.selected_index, 0);

        // gg on empty list should not panic
        app.handle_key(key('g'));
        app.handle_key(key('g'));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_gg_in_transactions_view() {
        let mut app = make_test_app(5);
        app.view_mode = ViewMode::Transactions;

        // Add some display transactions
        for i in 0..5 {
            app.display_transactions.push(Transaction {
                id: i as i64,
                symbol: format!("TX{}", i),
                category: AssetCategory::Equity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(100),
                currency: "USD".to_string(),
                date: "2025-01-01".to_string(),
                notes: None,
                created_at: "2025-01-01".to_string(),
            });
        }
        app.tx_selected_index = 3;

        // gg should jump tx index to 0
        app.handle_key(key('g'));
        app.handle_key(key('g'));
        assert_eq!(app.tx_selected_index, 0);

        // G should jump tx index to last
        app.handle_key(key('G'));
        assert_eq!(app.tx_selected_index, 4);
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn test_ctrl_d_scrolls_down_half_page() {
        let mut app = make_test_app(30);
        app.terminal_height = 24; // content area = 24 - 4 = 20, half = 10
        app.selected_index = 0;

        app.handle_key(ctrl_key('d'));
        assert_eq!(app.selected_index, 10);

        // Second Ctrl+d goes to 20
        app.handle_key(ctrl_key('d'));
        assert_eq!(app.selected_index, 20);

        // Third Ctrl+d clamps to end (29)
        app.handle_key(ctrl_key('d'));
        assert_eq!(app.selected_index, 29);
    }

    #[test]
    fn test_ctrl_u_scrolls_up_half_page() {
        let mut app = make_test_app(30);
        app.terminal_height = 24;
        app.selected_index = 25;

        app.handle_key(ctrl_key('u'));
        assert_eq!(app.selected_index, 15);

        app.handle_key(ctrl_key('u'));
        assert_eq!(app.selected_index, 5);

        // Clamps to 0
        app.handle_key(ctrl_key('u'));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_half_page_on_empty_list() {
        let mut app = make_test_app(0);
        app.terminal_height = 24;

        // Should not panic on empty list
        app.handle_key(ctrl_key('d'));
        assert_eq!(app.selected_index, 0);

        app.handle_key(ctrl_key('u'));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_half_page_small_terminal() {
        let mut app = make_test_app(10);
        app.terminal_height = 6; // content = 6 - 4 = 2, half = 1
        app.selected_index = 0;

        app.handle_key(ctrl_key('d'));
        assert_eq!(app.selected_index, 1);

        app.handle_key(ctrl_key('u'));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_ctrl_d_in_transactions_view() {
        let mut app = make_test_app(0);
        app.view_mode = ViewMode::Transactions;
        app.terminal_height = 24;

        for i in 0..20 {
            app.display_transactions.push(Transaction {
                id: i as i64,
                symbol: format!("TX{}", i),
                category: AssetCategory::Equity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(100),
                currency: "USD".to_string(),
                date: "2025-01-01".to_string(),
                notes: None,
                created_at: "2025-01-01".to_string(),
            });
        }
        app.tx_selected_index = 0;

        app.handle_key(ctrl_key('d'));
        assert_eq!(app.tx_selected_index, 10);

        app.handle_key(ctrl_key('u'));
        assert_eq!(app.tx_selected_index, 0);
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rust_decimal_macros::dec;
    use std::path::PathBuf;

    fn make_search_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        let mut app = App::new(&config, PathBuf::from("/tmp/pftui_test_search.db"));

        // Add positions with various names/symbols
        app.positions = vec![
            Position {
                symbol: "AAPL".to_string(),
                name: "Apple Inc".to_string(),
                category: AssetCategory::Equity,
                quantity: dec!(10),
                avg_cost: dec!(150),
                total_cost: dec!(1500),
                currency: "USD".to_string(),
                current_price: Some(dec!(175)),
                current_value: Some(dec!(1750)),
                gain: Some(dec!(250)),
                gain_pct: Some(dec!(16.67)),
                allocation_pct: Some(dec!(25)),
            },
            Position {
                symbol: "BTC".to_string(),
                name: "Bitcoin".to_string(),
                category: AssetCategory::Crypto,
                quantity: dec!(1),
                avg_cost: dec!(30000),
                total_cost: dec!(30000),
                currency: "USD".to_string(),
                current_price: Some(dec!(50000)),
                current_value: Some(dec!(50000)),
                gain: Some(dec!(20000)),
                gain_pct: Some(dec!(66.67)),
                allocation_pct: Some(dec!(50)),
            },
            Position {
                symbol: "GOOGL".to_string(),
                name: "Alphabet Inc".to_string(),
                category: AssetCategory::Equity,
                quantity: dec!(5),
                avg_cost: dec!(100),
                total_cost: dec!(500),
                currency: "USD".to_string(),
                current_price: Some(dec!(140)),
                current_value: Some(dec!(700)),
                gain: Some(dec!(200)),
                gain_pct: Some(dec!(40)),
                allocation_pct: Some(dec!(25)),
            },
        ];
        // Set display_positions directly (no DB, so recompute would clear them)
        app.display_positions = app.positions.clone();
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn esc_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn enter_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn backspace_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
    }

    #[test]
    fn test_slash_enters_search_mode() {
        let mut app = make_search_app();
        assert!(!app.search_mode);

        app.handle_key(key('/'));
        assert!(app.search_mode);
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn test_search_filters_by_symbol() {
        let mut app = make_search_app();
        assert_eq!(app.display_positions.len(), 3);

        // Enter search mode and type "BTC"
        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        app.handle_key(key('c'));

        assert_eq!(app.display_positions.len(), 1);
        assert_eq!(app.display_positions[0].symbol, "BTC");
    }

    #[test]
    fn test_search_filters_by_name_case_insensitive() {
        let mut app = make_search_app();

        // Search by name substring
        app.handle_key(key('/'));
        app.handle_key(key('a'));
        app.handle_key(key('p'));
        app.handle_key(key('p'));
        app.handle_key(key('l'));
        app.handle_key(key('e'));

        assert_eq!(app.display_positions.len(), 1);
        assert_eq!(app.display_positions[0].symbol, "AAPL");
    }

    #[test]
    fn test_search_esc_clears_and_exits() {
        let mut app = make_search_app();

        // Enter search, type something
        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        assert_eq!(app.display_positions.len(), 1);

        // Esc should clear search and show all positions
        app.handle_key(esc_key());
        assert!(!app.search_mode);
        assert!(app.search_query.is_empty());
        assert_eq!(app.display_positions.len(), 3);
    }

    #[test]
    fn test_search_enter_confirms_filter() {
        let mut app = make_search_app();

        // Enter search, type, confirm
        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        app.handle_key(key('c'));
        app.handle_key(enter_key());

        // Search mode exits but filter stays
        assert!(!app.search_mode);
        assert_eq!(app.search_query, "btc");
        assert_eq!(app.display_positions.len(), 1);
    }

    #[test]
    fn test_search_backspace_removes_char() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        app.handle_key(key('c'));
        assert_eq!(app.display_positions.len(), 1);

        // Backspace to widen the filter
        app.handle_key(backspace_key());
        assert_eq!(app.search_query, "bt");

        app.handle_key(backspace_key());
        app.handle_key(backspace_key());
        assert!(app.search_query.is_empty());
        assert_eq!(app.display_positions.len(), 3);
    }

    #[test]
    fn test_search_no_match_shows_empty() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('x'));
        app.handle_key(key('y'));
        app.handle_key(key('z'));

        assert_eq!(app.display_positions.len(), 0);
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_search_resets_selection_index() {
        let mut app = make_search_app();
        app.selected_index = 2;

        app.handle_key(key('/'));
        app.handle_key(key('b'));
        // Typing should reset index to 0
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_search_blocks_normal_keys() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        // Typing 'q' in search mode should NOT quit
        app.handle_key(key('q'));
        assert!(!app.should_quit);
        assert_eq!(app.search_query, "q");
    }
}

#[cfg(test)]
mod timeframe_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rust_decimal_macros::dec;
    use std::path::PathBuf;

    #[test]
    fn test_timeframe_days() {
        assert_eq!(ChartTimeframe::OneWeek.days(), 7);
        assert_eq!(ChartTimeframe::OneMonth.days(), 30);
        assert_eq!(ChartTimeframe::ThreeMonths.days(), 90);
        assert_eq!(ChartTimeframe::SixMonths.days(), 180);
        assert_eq!(ChartTimeframe::OneYear.days(), 365);
        assert_eq!(ChartTimeframe::FiveYears.days(), 1825);
    }

    #[test]
    fn test_timeframe_labels() {
        assert_eq!(ChartTimeframe::OneWeek.label(), "1W");
        assert_eq!(ChartTimeframe::OneMonth.label(), "1M");
        assert_eq!(ChartTimeframe::ThreeMonths.label(), "3M");
        assert_eq!(ChartTimeframe::SixMonths.label(), "6M");
        assert_eq!(ChartTimeframe::OneYear.label(), "1Y");
        assert_eq!(ChartTimeframe::FiveYears.label(), "5Y");
    }

    #[test]
    fn test_timeframe_next_cycles() {
        let tf = ChartTimeframe::OneWeek;
        let tf = tf.next(); // 1M
        assert_eq!(tf, ChartTimeframe::OneMonth);
        let tf = tf.next(); // 3M
        assert_eq!(tf, ChartTimeframe::ThreeMonths);
        let tf = tf.next(); // 6M
        assert_eq!(tf, ChartTimeframe::SixMonths);
        let tf = tf.next(); // 1Y
        assert_eq!(tf, ChartTimeframe::OneYear);
        let tf = tf.next(); // 5Y
        assert_eq!(tf, ChartTimeframe::FiveYears);
        let tf = tf.next(); // wraps to 1W
        assert_eq!(tf, ChartTimeframe::OneWeek);
    }

    #[test]
    fn test_timeframe_prev_cycles() {
        let tf = ChartTimeframe::OneWeek;
        let tf = tf.prev(); // wraps to 5Y
        assert_eq!(tf, ChartTimeframe::FiveYears);
        let tf = tf.prev(); // 1Y
        assert_eq!(tf, ChartTimeframe::OneYear);
        let tf = tf.prev(); // 6M
        assert_eq!(tf, ChartTimeframe::SixMonths);
    }

    #[test]
    fn test_default_timeframe_is_three_months() {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        let app = App::new(&config, PathBuf::from("/tmp/pftui_test_tf.db"));
        assert_eq!(app.chart_timeframe, ChartTimeframe::ThreeMonths);
    }

    fn make_tf_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        let mut app = App::new(&config, PathBuf::from("/tmp/pftui_test_tf2.db"));
        app.display_positions.push(Position {
            symbol: "AAPL".to_string(),
            name: "Apple Inc".to_string(),
            category: AssetCategory::Equity,
            quantity: dec!(10),
            avg_cost: dec!(150),
            total_cost: dec!(1500),
            currency: "USD".to_string(),
            current_price: Some(dec!(175)),
            current_value: Some(dec!(1750)),
            gain: Some(dec!(250)),
            gain_pct: Some(dec!(16.67)),
            allocation_pct: Some(dec!(100)),
        });
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn test_l_cycles_timeframe_forward_in_positions_view() {
        let mut app = make_tf_app();
        assert_eq!(app.chart_timeframe, ChartTimeframe::ThreeMonths);

        app.handle_key(key('l'));
        assert_eq!(app.chart_timeframe, ChartTimeframe::SixMonths);

        app.handle_key(key('l'));
        assert_eq!(app.chart_timeframe, ChartTimeframe::OneYear);
    }

    #[test]
    fn test_h_cycles_timeframe_backward_in_positions_view() {
        let mut app = make_tf_app();
        assert_eq!(app.chart_timeframe, ChartTimeframe::ThreeMonths);

        app.handle_key(key('h'));
        assert_eq!(app.chart_timeframe, ChartTimeframe::OneMonth);

        app.handle_key(key('h'));
        assert_eq!(app.chart_timeframe, ChartTimeframe::OneWeek);
    }

    #[test]
    fn test_h_l_no_effect_when_not_positions_view() {
        let mut app = make_tf_app();
        app.view_mode = ViewMode::Markets;
        let original = app.chart_timeframe;

        app.handle_key(key('h'));
        assert_eq!(app.chart_timeframe, original);

        app.handle_key(key('l'));
        assert_eq!(app.chart_timeframe, original);
    }
}

#[cfg(test)]
mod responsive_tests {
    use super::*;
    use std::path::PathBuf;

    fn make_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        App::new(&config, PathBuf::from("/tmp/pftui_test_responsive.db"))
    }

    #[test]
    fn test_terminal_width_default() {
        let app = make_app();
        assert_eq!(app.terminal_width, 120);
    }

    #[test]
    fn test_terminal_height_default() {
        let app = make_app();
        assert_eq!(app.terminal_height, 24);
    }

    #[test]
    fn test_set_terminal_size_updates_both() {
        let mut app = make_app();
        app.set_terminal_size(80, 40);
        assert_eq!(app.terminal_width, 80);
        assert_eq!(app.terminal_height, 40);
    }

    #[test]
    fn test_set_terminal_size_narrow() {
        let mut app = make_app();
        app.set_terminal_size(60, 20);
        assert_eq!(app.terminal_width, 60);
        assert!(app.terminal_width < crate::tui::ui::COMPACT_WIDTH);
    }

    #[test]
    fn test_set_terminal_size_wide() {
        let mut app = make_app();
        app.set_terminal_size(160, 50);
        assert_eq!(app.terminal_width, 160);
        assert!(app.terminal_width >= crate::tui::ui::COMPACT_WIDTH);
    }
}

#[cfg(test)]
mod on_demand_history_tests {
    use super::*;
    use std::path::PathBuf;

    fn make_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        App::new(&config, PathBuf::from("/tmp/pftui_test_ondemand.db"))
    }

    #[test]
    fn test_fetched_history_days_starts_empty() {
        let app = make_app();
        assert!(app.fetched_history_days.is_empty());
    }

    #[test]
    fn test_request_history_if_needed_tracks_days() {
        let mut app = make_app();
        // No price service, so command won't send, but tracking should still work
        app.request_history_if_needed("AAPL", AssetCategory::Equity, 90);
        assert_eq!(app.fetched_history_days.get("AAPL"), Some(&90));
    }

    #[test]
    fn test_request_history_if_needed_skips_when_already_fetched() {
        let mut app = make_app();
        // Pre-populate as if we already fetched 365 days
        app.fetched_history_days.insert("AAPL".to_string(), 365);
        // Requesting 90 days should be a no-op (already have more)
        app.request_history_if_needed("AAPL", AssetCategory::Equity, 90);
        // Should still be 365, not downgraded to 90
        assert_eq!(app.fetched_history_days.get("AAPL"), Some(&365));
    }

    #[test]
    fn test_request_history_if_needed_upgrades_when_more_needed() {
        let mut app = make_app();
        // Pre-populate as if we fetched 90 days
        app.fetched_history_days.insert("AAPL".to_string(), 90);
        // Requesting 365 days should upgrade the tracked amount
        app.request_history_if_needed("AAPL", AssetCategory::Equity, 365);
        assert_eq!(app.fetched_history_days.get("AAPL"), Some(&365));
    }

    #[test]
    fn test_request_history_if_needed_exact_match_skips() {
        let mut app = make_app();
        app.fetched_history_days.insert("BTC".to_string(), 180);
        // Requesting exactly 180 should be a no-op
        app.request_history_if_needed("BTC", AssetCategory::Crypto, 180);
        assert_eq!(app.fetched_history_days.get("BTC"), Some(&180));
    }
}
