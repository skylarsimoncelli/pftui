use std::collections::HashMap;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::config::{self, Config, PortfolioMode};
use crate::db::{allocations, price_cache, price_history};
use crate::db::transactions::{self, get_unique_symbols, insert_transaction, list_transactions};
use crate::models::allocation::Allocation;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::models::price::{HistoryRecord, PriceQuote};
use crate::models::transaction::{NewTransaction, Transaction, TxType};
use crate::price::{PriceCommand, PriceService, PriceUpdate};
use crate::tui::theme::{self, Theme};
use crate::tui::views::markets;
use crate::db::watchlist as db_watchlist;
use crate::tui::views::economy;
use crate::tui::views::watchlist as watchlist_view;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceFlashDirection {
    Up,
    Down,
    Same,
}

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

/// Which field is active in the add-transaction form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxFormField {
    TxType,
    Quantity,
    PricePer,
    Date,
}

impl TxFormField {
    pub fn next(self) -> Self {
        match self {
            TxFormField::TxType => TxFormField::Quantity,
            TxFormField::Quantity => TxFormField::PricePer,
            TxFormField::PricePer => TxFormField::Date,
            TxFormField::Date => TxFormField::Date, // last field — Enter submits
        }
    }

    pub fn prev(self) -> Self {
        match self {
            TxFormField::TxType => TxFormField::TxType,
            TxFormField::Quantity => TxFormField::TxType,
            TxFormField::PricePer => TxFormField::Quantity,
            TxFormField::Date => TxFormField::PricePer,
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            TxFormField::TxType => "Type",
            TxFormField::Quantity => "Qty",
            TxFormField::PricePer => "Price",
            TxFormField::Date => "Date",
        }
    }
}

/// State for the inline add-transaction form.
#[derive(Debug, Clone)]
pub struct TxFormState {
    pub symbol: String,
    pub category: AssetCategory,
    pub active_field: TxFormField,
    pub tx_type: TxType,
    pub quantity_input: String,
    pub price_input: String,
    pub date_input: String,
    pub error: Option<String>,
}

impl TxFormState {
    pub fn new(symbol: String, category: AssetCategory) -> Self {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        TxFormState {
            symbol,
            category,
            active_field: TxFormField::TxType,
            tx_type: TxType::Buy,
            quantity_input: String::new(),
            price_input: String::new(),
            date_input: today,
            error: None,
        }
    }
}

/// State for delete-transaction confirmation.
#[derive(Debug, Clone)]
pub struct DeleteConfirmState {
    pub symbol: String,
    pub tx_count: usize,
    pub tx_ids: Vec<i64>,
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

    // Track symbols where history fetch was attempted (to distinguish "loading" from "no data")
    pub history_attempted: std::collections::HashSet<String>,

    // Animation
    pub tick_count: u64,
    pub price_flash_ticks: HashMap<String, (u64, PriceFlashDirection)>,
    pub last_value_update_tick: u64,

    // Price error display
    pub last_price_error: Option<String>,
    pub last_price_error_tick: u64,

    // Daily portfolio change (sum of (current_price - prev_close) * quantity)
    pub daily_portfolio_change: Option<Decimal>,

    // Previous day category allocations (for ▲/▼ change indicators)
    pub prev_day_cat_allocations: HashMap<AssetCategory, Decimal>,

    // Keystroke echo
    pub last_key_display: String,
    pub last_key_tick: u64,

    // Row highlight flash on selection change
    pub last_selection_change_tick: u64,

    // Sort indicator flash on sort change
    pub last_sort_change_tick: u64,

    // Theme toast on cycle
    pub theme_toast_tick: u64,

    // Transaction form (inline add/delete)
    pub tx_form: Option<TxFormState>,
    pub delete_confirm: Option<DeleteConfirmState>,

    // Portfolio sparkline timeframe
    pub sparkline_timeframe: ChartTimeframe,

    // Crosshair cursor on charts
    pub crosshair_mode: bool,
    pub crosshair_x: usize, // column index within chart width

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
            history_attempted: std::collections::HashSet::new(),
            tick_count: 0,
            price_flash_ticks: HashMap::new(),
            last_value_update_tick: 0,
            last_price_error: None,
            last_price_error_tick: 0,
            daily_portfolio_change: None,
            prev_day_cat_allocations: HashMap::new(),
            last_key_display: String::new(),
            last_key_tick: 0,
            last_selection_change_tick: 0,
            last_sort_change_tick: 0,
            theme_toast_tick: 0,
            tx_form: None,
            delete_confirm: None,
            sparkline_timeframe: ChartTimeframe::ThreeMonths,
            crosshair_mode: false,
            crosshair_x: 0,
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
            for (sym, _, _) in &batch {
                self.history_attempted.insert(sym.clone());
            }
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
        self.history_attempted.insert(symbol.to_string());
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
            self.crosshair_mode = false;
            self.crosshair_x = 0;
            self.last_selection_change_tick = self.tick_count;
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
        self.compute_prev_day_cat_allocations();
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

    /// Compute previous-day category allocation percentages from price history.
    /// Uses the second-to-last close price for each symbol (the most recent
    /// "previous" close) to estimate what allocations looked like yesterday.
    fn compute_prev_day_cat_allocations(&mut self) {
        let mut prev_prices: HashMap<String, Decimal> = HashMap::new();
        for (symbol, records) in &self.price_history {
            // records are chronological; second-to-last is "previous day"
            if records.len() >= 2 {
                prev_prices.insert(symbol.clone(), records[records.len() - 2].close);
            }
        }

        if prev_prices.is_empty() {
            self.prev_day_cat_allocations.clear();
            return;
        }

        // Compute previous-day values per position, aggregate by category
        let mut cat_values: HashMap<AssetCategory, Decimal> = HashMap::new();
        let mut total = dec!(0);
        for pos in &self.positions {
            let prev_price = if pos.category == AssetCategory::Cash {
                Some(dec!(1))
            } else {
                prev_prices.get(&pos.symbol).copied()
            };
            if let Some(pp) = prev_price {
                let val = pp * pos.quantity;
                *cat_values.entry(pos.category).or_insert(dec!(0)) += val;
                total += val;
            }
        }

        self.prev_day_cat_allocations.clear();
        if total > dec!(0) {
            for (cat, val) in &cat_values {
                self.prev_day_cat_allocations
                    .insert(*cat, (*val / total) * dec!(100));
            }
        }
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

        // LOCF (Last Observation Carried Forward): for each symbol, if no price
        // exists on a given date, use the most recent prior close. This prevents
        // positions from contributing $0 on dates with no data, which would cause
        // the portfolio value to swing wildly (sine wave bug).
        let mut last_known: HashMap<&str, Decimal> = HashMap::new();
        let mut history = Vec::new();
        for date in &all_dates {
            let mut total = dec!(0);
            let mut has_data = false;

            // Update last_known prices for all symbols that have data on this date
            for (symbol, sym_prices) in &price_by_date {
                if let Some(&close) = sym_prices.get(date.as_str()) {
                    last_known.insert(symbol, close);
                }
            }

            for pos in &self.positions {
                if pos.category == AssetCategory::Cash {
                    total += pos.quantity;
                    has_data = true;
                    continue;
                }
                if let Some(&price) = last_known.get(pos.symbol.as_str()) {
                    total += pos.quantity * price;
                    has_data = true;
                }
            }
            if has_data {
                history.push((date.clone(), total));
            }
        }
        self.portfolio_value_history = history;
        self.compute_daily_change();
    }

    /// Compute daily portfolio change by comparing each position's current price
    /// to its most recent historical close (previous trading day).
    fn compute_daily_change(&mut self) {
        if self.portfolio_mode == PortfolioMode::Percentage {
            self.daily_portfolio_change = None;
            return;
        }

        let mut total_change = dec!(0);
        let mut has_data = false;

        for pos in &self.positions {
            // Cash doesn't change in value
            if pos.category == AssetCategory::Cash {
                continue;
            }

            let current_price = match pos.current_price {
                Some(p) => p,
                None => continue,
            };

            // Find the most recent historical close for this symbol
            if let Some(records) = self.price_history.get(&pos.symbol) {
                if records.is_empty() {
                    continue;
                }
                // Records are sorted by date. The last record is the most recent.
                // If the last record IS today's price, use the second-to-last.
                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                let prev_close = if records.len() >= 2 && records.last().map(|r| r.date.as_str()) == Some(today.as_str()) {
                    // Last record is today — use the one before it
                    records[records.len() - 2].close
                } else if let Some(last) = records.last() {
                    // Last record is a previous day — use it
                    last.close
                } else {
                    continue;
                };

                let price_change = current_price - prev_close;
                total_change += price_change * pos.quantity;
                has_data = true;
            }
        }

        self.daily_portfolio_change = if has_data { Some(total_change) } else { None };
    }

    pub fn selected_position(&self) -> Option<&Position> {
        self.display_positions.get(self.selected_index)
    }

    /// Returns a context-aware breadcrumb string for the status bar.
    /// Shows the navigation path based on current view, selection, and chart state.
    /// Examples: "Positions › AAPL › 3M Chart › AAPL/SPX", "Positions › AAPL › Detail"
    pub fn breadcrumb(&self) -> String {
        let view_label = match self.view_mode {
            ViewMode::Positions => "Positions",
            ViewMode::Transactions => "Transactions",
            ViewMode::Markets => "Markets",
            ViewMode::Economy => "Economy",
            ViewMode::Watchlist => "Watchlist",
        };

        // Only positions view has deeper navigation context
        if self.view_mode != ViewMode::Positions {
            return view_label.to_string();
        }

        let pos = match self.selected_position() {
            Some(p) => p,
            None => return view_label.to_string(),
        };

        let sym = &pos.symbol;

        // Detail popup takes precedence
        if self.detail_popup_open {
            return format!("{view_label} › {sym} › Detail");
        }

        // Show chart variant info when a specific chart is selected (not "All" at index 0)
        let variants = Self::chart_variants_for_position(pos);
        if self.chart_index > 0 {
            if let Some(variant) = variants.get(self.chart_index) {
                return format!(
                    "{view_label} › {sym} › {} › {}",
                    self.chart_timeframe.label(),
                    variant.label
                );
            }
        }

        // Default: just view + selected symbol
        format!("{view_label} › {sym}")
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
                ChartVariant::ratio("USD/BTC", "DX-Y.NYB", AssetCategory::Forex, "BTC-USD", AssetCategory::Equity),
            ]
        } else if is_cash {
            let pair = format!("{}USD=X", sym);
            let pair_label = format!("{}/USD", sym);
            let ratio_label = format!("{}/DXY", sym);
            vec![
                ChartVariant::single(&pair, &pair_label, AssetCategory::Forex),
                ChartVariant::ratio(&ratio_label, &pair, AssetCategory::Forex, "DX-Y.NYB", AssetCategory::Forex),
                ChartVariant::ratio(&format!("{}/Gold", sym), &pair, AssetCategory::Forex, "GC=F", AssetCategory::Commodity),
                ChartVariant::ratio(&format!("{}/BTC", sym), &pair, AssetCategory::Forex, "BTC-USD", AssetCategory::Equity),
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
                    // Equity, Fund, non-Gold Commodity: {SYM}/SPX, {SYM}/QQQ, and for commodities {SYM}/BTC
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
                    if is_commodity {
                        variants.push(ChartVariant::ratio(
                            &format!("{}/BTC", pos.symbol),
                            &yahoo_sym,
                            cat,
                            "BTC-USD",
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
                        let direction = match self.prices.get(&quote.symbol) {
                            Some(&old_price) if quote.price > old_price => PriceFlashDirection::Up,
                            Some(&old_price) if quote.price < old_price => PriceFlashDirection::Down,
                            _ => PriceFlashDirection::Same,
                        };
                        self.price_flash_ticks
                            .insert(quote.symbol.clone(), (self.tick_count, direction));
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
                    Some(PriceUpdate::Error(msg)) => {
                        self.last_price_error = Some(msg);
                        self.last_price_error_tick = self.tick_count;
                    }
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

        // Delete confirmation mode
        if self.delete_confirm.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirm_delete();
                }
                _ => {
                    // Any other key cancels
                    self.delete_confirm = None;
                }
            }
            return;
        }

        // Add-transaction form mode
        if self.tx_form.is_some() {
            self.handle_tx_form_key(key);
            return;
        }

        // Record keystroke for status bar echo
        self.record_keystroke(&key);

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
            // Crosshair toggle with x (Positions view only)
            KeyCode::Char('x') if matches!(self.view_mode, ViewMode::Positions) => {
                self.crosshair_mode = !self.crosshair_mode;
                if self.crosshair_mode {
                    // Start crosshair at the rightmost position (most recent data).
                    // Chart width is ~43% of terminal minus 2 for borders.
                    let estimated_chart_width =
                        ((self.terminal_width as usize * 43) / 100).saturating_sub(2);
                    self.crosshair_x = estimated_chart_width.saturating_sub(1);
                }
            }

            // Timeframe cycling with h/l — or crosshair movement when active
            KeyCode::Char('h') | KeyCode::Left if matches!(self.view_mode, ViewMode::Positions) => {
                if self.crosshair_mode {
                    self.crosshair_x = self.crosshair_x.saturating_sub(1);
                } else {
                    self.chart_timeframe = self.chart_timeframe.prev();
                    self.refetch_chart_history();
                }
            }
            KeyCode::Char('l') | KeyCode::Right if matches!(self.view_mode, ViewMode::Positions) => {
                if self.crosshair_mode {
                    self.crosshair_x = self.crosshair_x.saturating_add(1);
                    // clamped during render
                } else {
                    self.chart_timeframe = self.chart_timeframe.next();
                    self.refetch_chart_history();
                }
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
                self.last_sort_change_tick = self.tick_count;
                self.recompute();
            }
            KeyCode::Char('%') => {
                if !is_privacy_view(self) {
                    self.sort_field = SortField::GainPct;
                    self.sort_ascending = false;
                    self.last_sort_change_tick = self.tick_count;
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
                    self.last_sort_change_tick = self.tick_count;
                    self.recompute();
                }
            }
            KeyCode::Char('n') => {
                self.sort_field = SortField::Name;
                self.sort_ascending = true;
                self.last_sort_change_tick = self.tick_count;
                self.recompute();
            }
            KeyCode::Char('c') => {
                self.sort_field = SortField::Category;
                self.sort_ascending = true;
                self.last_sort_change_tick = self.tick_count;
                self.recompute();
            }
            KeyCode::Char('d') => {
                if self.portfolio_mode != PortfolioMode::Percentage {
                    self.sort_field = SortField::Date;
                    self.sort_ascending = false;
                    self.last_sort_change_tick = self.tick_count;
                    self.recompute();
                }
            }
            KeyCode::Tab => {
                self.sort_ascending = !self.sort_ascending;
                self.last_sort_change_tick = self.tick_count;
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

            // Portfolio sparkline timeframe cycling
            KeyCode::Char(']') => {
                self.sparkline_timeframe = self.sparkline_timeframe.next();
            }
            KeyCode::Char('[') => {
                self.sparkline_timeframe = self.sparkline_timeframe.prev();
            }

            // Add transaction (Shift+A) — opens inline form for selected position
            KeyCode::Char('A') if matches!(self.view_mode, ViewMode::Positions) => {
                if self.portfolio_mode == PortfolioMode::Full {
                    self.open_tx_form();
                }
            }

            // Delete position transactions (Shift+X) — confirmation prompt
            KeyCode::Char('X') if matches!(self.view_mode, ViewMode::Positions) => {
                if self.portfolio_mode == PortfolioMode::Full {
                    self.open_delete_confirm();
                }
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
        self.theme_toast_tick = self.tick_count;
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


    /// Record a keystroke for the status bar echo display.
    /// Handles g-prefix sequences (gg, G) and modifier keys (Ctrl+d, etc.).
    fn record_keystroke(&mut self, key: &KeyEvent) {
        let display = match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    format!("Ctrl+{}", c)
                } else if key.modifiers.contains(KeyModifiers::ALT) {
                    format!("Alt+{}", c)
                } else if c == 'G' {
                    "G".to_string()
                } else if c == 'g' {
                    // If g was already pending, this completes "gg"
                    if self.g_pending {
                        "gg".to_string()
                    } else {
                        "g".to_string()
                    }
                } else {
                    c.to_string()
                }
            }
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::Up => "↑".to_string(),
            KeyCode::Down => "↓".to_string(),
            KeyCode::Left => "←".to_string(),
            KeyCode::Right => "→".to_string(),
            _ => return, // Don't display unknown keys
        };
        self.last_key_display = display;
        self.last_key_tick = self.tick_count;
    }
    // ── Transaction form methods ──

    /// Open the add-transaction form for the currently selected position.
    fn open_tx_form(&mut self) {
        if let Some(pos) = self.selected_position().cloned() {
            self.tx_form = Some(TxFormState::new(pos.symbol, pos.category));
        }
    }

    /// Open delete confirmation for the currently selected position.
    fn open_delete_confirm(&mut self) {
        if let Some(pos) = self.selected_position().cloned() {
            let tx_ids: Vec<i64> = self
                .transactions
                .iter()
                .filter(|tx| tx.symbol == pos.symbol)
                .map(|tx| tx.id)
                .collect();
            if tx_ids.is_empty() {
                return;
            }
            self.delete_confirm = Some(DeleteConfirmState {
                symbol: pos.symbol,
                tx_count: tx_ids.len(),
                tx_ids,
            });
        }
    }

    /// Handle a key event while the add-transaction form is open.
    fn handle_tx_form_key(&mut self, key: KeyEvent) {
        // Safety: caller checks tx_form.is_some()
        let form = match self.tx_form.as_mut() {
            Some(f) => f,
            None => return,
        };

        // Clear previous error on any keystroke
        form.error = None;

        match key.code {
            KeyCode::Esc => {
                self.tx_form = None;
            }
            KeyCode::Tab => {
                form.active_field = form.active_field.next();
            }
            KeyCode::BackTab => {
                form.active_field = form.active_field.prev();
            }
            KeyCode::Enter => {
                if form.active_field == TxFormField::Date {
                    // Submit the form
                    self.submit_tx_form();
                } else {
                    form.active_field = form.active_field.next();
                }
            }
            KeyCode::Backspace => {
                match form.active_field {
                    TxFormField::TxType => {} // toggle, no backspace
                    TxFormField::Quantity => { form.quantity_input.pop(); }
                    TxFormField::PricePer => { form.price_input.pop(); }
                    TxFormField::Date => { form.date_input.pop(); }
                }
            }
            KeyCode::Char(c) => {
                match form.active_field {
                    TxFormField::TxType => {
                        // Any key toggles between Buy/Sell
                        form.tx_type = match form.tx_type {
                            TxType::Buy => TxType::Sell,
                            TxType::Sell => TxType::Buy,
                        };
                    }
                    TxFormField::Quantity => {
                        // Accept digits and decimal point
                        if c.is_ascii_digit() || c == '.' {
                            form.quantity_input.push(c);
                        }
                    }
                    TxFormField::PricePer => {
                        if c.is_ascii_digit() || c == '.' {
                            form.price_input.push(c);
                        }
                    }
                    TxFormField::Date => {
                        // Accept digits and hyphens for YYYY-MM-DD
                        if c.is_ascii_digit() || c == '-' {
                            form.date_input.push(c);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Validate and submit the transaction form.
    fn submit_tx_form(&mut self) {
        let form = match self.tx_form.as_ref() {
            Some(f) => f,
            None => return,
        };

        // Validate quantity
        let quantity: Decimal = match form.quantity_input.parse() {
            Ok(q) if q > dec!(0) => q,
            _ => {
                if let Some(f) = self.tx_form.as_mut() {
                    f.error = Some("Invalid quantity".to_string());
                    f.active_field = TxFormField::Quantity;
                }
                return;
            }
        };

        // Validate price
        let price_per: Decimal = match form.price_input.parse() {
            Ok(p) if p > dec!(0) => p,
            _ => {
                if let Some(f) = self.tx_form.as_mut() {
                    f.error = Some("Invalid price".to_string());
                    f.active_field = TxFormField::PricePer;
                }
                return;
            }
        };

        // Validate date format (basic: YYYY-MM-DD length check)
        if form.date_input.len() != 10 || form.date_input.chars().filter(|c| *c == '-').count() != 2 {
            if let Some(f) = self.tx_form.as_mut() {
                f.error = Some("Date must be YYYY-MM-DD".to_string());
                f.active_field = TxFormField::Date;
            }
            return;
        }

        let new_tx = NewTransaction {
            symbol: form.symbol.clone(),
            category: form.category,
            tx_type: form.tx_type,
            quantity,
            price_per,
            currency: self.base_currency.clone(),
            date: form.date_input.clone(),
            notes: None,
        };

        // Insert into DB
        if let Ok(conn) = Connection::open(&self.db_path) {
            match insert_transaction(&conn, &new_tx) {
                Ok(_) => {
                    self.tx_form = None;
                    self.load_data();
                    self.recompute();
                }
                Err(e) => {
                    if let Some(f) = self.tx_form.as_mut() {
                        f.error = Some(format!("DB error: {e}"));
                    }
                }
            }
        } else if let Some(f) = self.tx_form.as_mut() {
            f.error = Some("Cannot open database".to_string());
        }
    }

    /// Execute the confirmed delete of all transactions for a position.
    fn confirm_delete(&mut self) {
        let state = match self.delete_confirm.take() {
            Some(s) => s,
            None => return,
        };

        if let Ok(conn) = Connection::open(&self.db_path) {
            for id in &state.tx_ids {
                let _ = transactions::delete_transaction(&conn, *id);
            }
            self.load_data();
            self.recompute();
            // Adjust selection if we deleted the last position
            if self.selected_index >= self.display_positions.len() && self.selected_index > 0 {
                self.selected_index = self.display_positions.len() - 1;
            }
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

        assert_eq!(variants.len(), 4); // All + DXY single + USD/Gold ratio + USD/BTC ratio
        assert_eq!(variants[0].label, "All");
        assert_eq!(variants[1].label, "Dollar Index (DXY)");
        assert_eq!(variants[2].label, "USD/Gold");
        assert_eq!(variants[3].label, "USD/BTC");

        // DXY should be single chart for USD
        match &variants[1].kind {
            ChartKind::Single { symbol, .. } => assert_eq!(symbol, "DX-Y.NYB"),
            _ => panic!("Expected Single chart for DXY"),
        }

        // USD/BTC should be a ratio (DXY / BTC-USD), not a single BTC chart
        match &variants[3].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "DX-Y.NYB");
                assert_eq!(den_symbol, "BTC-USD");
            }
            _ => panic!("Expected Ratio chart for USD/BTC"),
        }
    }

    #[test]
    fn test_non_usd_cash_variants_ratio_dxy() {
        let pos = make_position("EUR", AssetCategory::Cash);
        let variants = App::chart_variants_for_position(&pos);

        assert_eq!(variants.len(), 5); // All + EUR/USD + EUR/DXY + EUR/Gold + EUR/BTC
        assert_eq!(variants[0].label, "All");
        assert_eq!(variants[1].label, "EUR/USD");
        assert_eq!(variants[2].label, "EUR/DXY");
        assert_eq!(variants[3].label, "EUR/Gold");
        assert_eq!(variants[4].label, "EUR/BTC");

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

        // EUR/Gold should be a ratio (EURUSD=X / GC=F)
        match &variants[3].kind {
            ChartKind::Ratio {
                num_symbol,
                den_symbol,
                ..
            } => {
                assert_eq!(num_symbol, "EURUSD=X");
                assert_eq!(den_symbol, "GC=F");
            }
            _ => panic!("Expected Ratio chart for EUR/Gold"),
        }

        // EUR/BTC should be a ratio (EURUSD=X / BTC-USD)
        match &variants[4].kind {
            ChartKind::Ratio {
                num_symbol,
                den_symbol,
                ..
            } => {
                assert_eq!(num_symbol, "EURUSD=X");
                assert_eq!(den_symbol, "BTC-USD");
            }
            _ => panic!("Expected Ratio chart for EUR/BTC"),
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
        assert_eq!(labels[0], "All");
        assert_eq!(labels[1], "GBP/USD");
        assert_eq!(labels[2], "GBP/DXY");
        assert_eq!(labels[3], "GBP/Gold");
        assert_eq!(labels[4], "GBP/BTC");

        // Verify GBP/USD pair symbol is correct
        match &variants[1].kind {
            ChartKind::Single { symbol, .. } => assert_eq!(symbol, "GBPUSD=X"),
            _ => panic!("Expected Single chart for GBP/USD"),
        }

        // GBP/BTC should be a ratio (GBPUSD=X / BTC-USD)
        match &variants[4].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "GBPUSD=X");
                assert_eq!(den_symbol, "BTC-USD");
            }
            _ => panic!("Expected Ratio chart for GBP/BTC"),
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
    fn test_commodity_non_gold_has_btc_ratio() {
        // Silver and other non-Gold commodities should have a /BTC ratio variant
        let pos = make_position("SLV", AssetCategory::Commodity);
        let variants = App::chart_variants_for_position(&pos);

        let labels = variant_labels(&variants);
        assert!(labels.contains(&"SLV/SPX".to_string()), "Silver should have SLV/SPX ratio");
        assert!(labels.contains(&"SLV/QQQ".to_string()), "Silver should have SLV/QQQ ratio");
        assert!(labels.contains(&"SLV/BTC".to_string()), "Silver should have SLV/BTC ratio");

        // Verify SLV/BTC is actually a ratio with correct symbols
        let btc_variant = variants.iter().find(|v| v.label == "SLV/BTC").unwrap();
        match &btc_variant.kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "SLV");
                assert_eq!(den_symbol, "BTC-USD");
            }
            _ => panic!("Expected Ratio chart for SLV/BTC"),
        }
    }

    #[test]
    fn test_equity_has_no_btc_ratio() {
        // Equities should NOT have a /BTC ratio (only commodities get it)
        let pos = make_position("AAPL", AssetCategory::Equity);
        let variants = App::chart_variants_for_position(&pos);

        let labels = variant_labels(&variants);
        assert!(!labels.contains(&"AAPL/BTC".to_string()), "Equities should not have /BTC ratio");
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
mod crosshair_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn make_crosshair_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        let mut app = App::new(&config, PathBuf::from(":memory:"));
        app.view_mode = ViewMode::Positions;
        app.terminal_width = 120;
        app
    }

    #[test]
    fn test_crosshair_starts_disabled() {
        let app = make_crosshair_app();
        assert!(!app.crosshair_mode);
        assert_eq!(app.crosshair_x, 0);
    }

    #[test]
    fn test_x_toggles_crosshair_on() {
        let mut app = make_crosshair_app();
        app.handle_key(key('x'));
        assert!(app.crosshair_mode);
        // Should start at rightmost based on estimated chart width
        // Chart width ≈ 120 * 43 / 100 - 2 = 49
        assert!(app.crosshair_x > 0);
    }

    #[test]
    fn test_x_toggles_crosshair_off() {
        let mut app = make_crosshair_app();
        app.handle_key(key('x'));
        assert!(app.crosshair_mode);
        app.handle_key(key('x'));
        assert!(!app.crosshair_mode);
    }

    #[test]
    fn test_crosshair_h_moves_left() {
        let mut app = make_crosshair_app();
        app.handle_key(key('x'));
        let initial_x = app.crosshair_x;
        app.handle_key(key('h'));
        assert_eq!(app.crosshair_x, initial_x - 1);
    }

    #[test]
    fn test_crosshair_l_moves_right() {
        let mut app = make_crosshair_app();
        app.handle_key(key('x'));
        app.handle_key(key('h')); // move left first
        app.handle_key(key('h'));
        let after_left = app.crosshair_x;
        app.handle_key(key('l'));
        assert_eq!(app.crosshair_x, after_left + 1);
    }

    #[test]
    fn test_crosshair_h_clamps_at_zero() {
        let mut app = make_crosshair_app();
        app.crosshair_mode = true;
        app.crosshair_x = 0;
        app.handle_key(key('h'));
        assert_eq!(app.crosshair_x, 0);
    }

    #[test]
    fn test_crosshair_h_l_changes_timeframe_when_crosshair_off() {
        let mut app = make_crosshair_app();
        assert!(!app.crosshair_mode);
        let original_tf = app.chart_timeframe;
        app.handle_key(key('l'));
        assert_ne!(app.chart_timeframe, original_tf);
    }

    #[test]
    fn test_crosshair_h_l_does_not_change_timeframe_when_crosshair_on() {
        let mut app = make_crosshair_app();
        app.handle_key(key('x')); // enable crosshair
        let tf = app.chart_timeframe;
        app.handle_key(key('l'));
        assert_eq!(app.chart_timeframe, tf);
        app.handle_key(key('h'));
        assert_eq!(app.chart_timeframe, tf);
    }

    #[test]
    fn test_crosshair_x_no_effect_in_other_views() {
        let mut app = make_crosshair_app();
        app.view_mode = ViewMode::Markets;
        app.handle_key(key('x'));
        assert!(!app.crosshair_mode);
    }

    #[test]
    fn test_crosshair_resets_on_position_change() {
        let mut app = make_crosshair_app();
        // Set up positions so selection change is possible
        app.display_positions = vec![
            crate::models::position::Position {
                symbol: "BTC".to_string(),
                name: "Bitcoin".to_string(),
                quantity: rust_decimal_macros::dec!(1),
                avg_cost: rust_decimal_macros::dec!(50000),
                total_cost: rust_decimal_macros::dec!(50000),
                currency: "USD".to_string(),
                current_price: Some(rust_decimal_macros::dec!(60000)),
                current_value: Some(rust_decimal_macros::dec!(60000)),
                gain: Some(rust_decimal_macros::dec!(10000)),
                gain_pct: Some(rust_decimal_macros::dec!(20)),
                allocation_pct: Some(rust_decimal_macros::dec!(50)),
                category: crate::models::asset::AssetCategory::Crypto,
            },
            crate::models::position::Position {
                symbol: "ETH".to_string(),
                name: "Ethereum".to_string(),
                quantity: rust_decimal_macros::dec!(10),
                avg_cost: rust_decimal_macros::dec!(3000),
                total_cost: rust_decimal_macros::dec!(30000),
                currency: "USD".to_string(),
                current_price: Some(rust_decimal_macros::dec!(4000)),
                current_value: Some(rust_decimal_macros::dec!(40000)),
                gain: Some(rust_decimal_macros::dec!(10000)),
                gain_pct: Some(rust_decimal_macros::dec!(33)),
                allocation_pct: Some(rust_decimal_macros::dec!(50)),
                category: crate::models::asset::AssetCategory::Crypto,
            },
        ];
        app.selected_index = 0;
        app.crosshair_mode = true;
        app.crosshair_x = 25;

        // Move selection down — should reset crosshair
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert!(!app.crosshair_mode);
        assert_eq!(app.crosshair_x, 0);
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

#[cfg(test)]
mod daily_change_tests {
    use super::*;
    use std::path::PathBuf;

    fn make_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        App::new(&config, PathBuf::from("/tmp/pftui_test_daily.db"))
    }

    fn make_position(symbol: &str, qty: Decimal, price: Option<Decimal>, category: AssetCategory) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category,
            quantity: qty,
            avg_cost: dec!(100),
            total_cost: qty * dec!(100),
            currency: "USD".to_string(),
            current_price: price,
            current_value: price.map(|p| p * qty),
            gain: price.map(|p| p * qty - qty * dec!(100)),
            gain_pct: None,
            allocation_pct: None,
        }
    }

    #[test]
    fn test_daily_change_no_history() {
        let mut app = make_app();
        app.positions = vec![make_position("AAPL", dec!(10), Some(dec!(150)), AssetCategory::Equity)];
        app.compute_daily_change();
        assert_eq!(app.daily_portfolio_change, None);
    }

    #[test]
    fn test_daily_change_with_prev_close() {
        let mut app = make_app();
        app.positions = vec![make_position("AAPL", dec!(10), Some(dec!(155)), AssetCategory::Equity)];
        // Add history with a previous day close of 150
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(148), volume: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
        ]);
        app.compute_daily_change();
        // (155 - 150) * 10 = 50
        assert_eq!(app.daily_portfolio_change, Some(dec!(50)));
    }

    #[test]
    fn test_daily_change_negative() {
        let mut app = make_app();
        app.positions = vec![make_position("AAPL", dec!(10), Some(dec!(145)), AssetCategory::Equity)];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
        ]);
        app.compute_daily_change();
        // (145 - 150) * 10 = -50
        assert_eq!(app.daily_portfolio_change, Some(dec!(-50)));
    }

    #[test]
    fn test_daily_change_multiple_positions() {
        let mut app = make_app();
        app.positions = vec![
            make_position("AAPL", dec!(10), Some(dec!(155)), AssetCategory::Equity),
            make_position("GOOG", dec!(5), Some(dec!(2800)), AssetCategory::Equity),
        ];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
        ]);
        app.price_history.insert("GOOG".to_string(), vec![
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(2750), volume: None },
        ]);
        app.compute_daily_change();
        // AAPL: (155-150)*10 = 50, GOOG: (2800-2750)*5 = 250. Total = 300
        assert_eq!(app.daily_portfolio_change, Some(dec!(300)));
    }

    #[test]
    fn test_daily_change_skips_cash() {
        let mut app = make_app();
        app.positions = vec![
            make_position("USD", dec!(10000), Some(dec!(1)), AssetCategory::Cash),
            make_position("AAPL", dec!(10), Some(dec!(155)), AssetCategory::Equity),
        ];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
        ]);
        app.compute_daily_change();
        // Only AAPL: (155-150)*10 = 50 (cash excluded)
        assert_eq!(app.daily_portfolio_change, Some(dec!(50)));
    }

    #[test]
    fn test_daily_change_percentage_mode_returns_none() {
        let mut app = make_app();
        app.portfolio_mode = PortfolioMode::Percentage;
        app.positions = vec![make_position("AAPL", dec!(10), Some(dec!(155)), AssetCategory::Equity)];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
        ]);
        app.compute_daily_change();
        assert_eq!(app.daily_portfolio_change, None);
    }

    #[test]
    fn test_daily_change_today_record_uses_prev() {
        let mut app = make_app();
        app.positions = vec![make_position("AAPL", dec!(10), Some(dec!(160)), AssetCategory::Equity)];
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
            HistoryRecord { date: today, close: dec!(158), volume: None },
        ]);
        app.compute_daily_change();
        // Should use 2026-02-28 close (150), not today's record
        // (160 - 150) * 10 = 100
        assert_eq!(app.daily_portfolio_change, Some(dec!(100)));
    }
}

#[cfg(test)]
mod portfolio_value_history_tests {
    use super::*;
    use std::path::PathBuf;

    fn make_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        App::new(&config, PathBuf::from("/tmp/pftui_test_pvh.db"))
    }

    fn make_position(symbol: &str, qty: Decimal, category: AssetCategory) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category,
            quantity: qty,
            avg_cost: dec!(100),
            total_cost: qty * dec!(100),
            currency: "USD".to_string(),
            current_price: None,
            current_value: None,
            gain: None,
            gain_pct: None,
            allocation_pct: None,
        }
    }

    #[test]
    fn test_locf_fills_missing_dates() {
        // The core bug: if AAPL has price on day 1 and 3 but not day 2,
        // day 2 should use AAPL's day-1 price (not contribute $0).
        let mut app = make_app();
        app.positions = vec![make_position("AAPL", dec!(10), AssetCategory::Equity)];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-01-01".to_string(), close: dec!(150), volume: None },
            // No record for 2026-01-02
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(155), volume: None },
        ]);
        // Add a second symbol that has data on day 2 to create the date
        app.positions.push(make_position("GOOG", dec!(5), AssetCategory::Equity));
        app.price_history.insert("GOOG".to_string(), vec![
            HistoryRecord { date: "2026-01-02".to_string(), close: dec!(2800), volume: None },
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(2850), volume: None },
        ]);
        app.compute_portfolio_value_history();

        // Day 1: AAPL=10*150=1500, GOOG has no data yet = not included
        // Day 2: AAPL LOCF=10*150=1500, GOOG=5*2800=14000 → 15500
        // Day 3: AAPL=10*155=1550, GOOG=5*2850=14250 → 15800
        assert_eq!(app.portfolio_value_history.len(), 3);
        assert_eq!(app.portfolio_value_history[0], ("2026-01-01".to_string(), dec!(1500)));
        assert_eq!(app.portfolio_value_history[1], ("2026-01-02".to_string(), dec!(15500)));
        assert_eq!(app.portfolio_value_history[2], ("2026-01-03".to_string(), dec!(15800)));
    }

    #[test]
    fn test_locf_no_sine_wave_with_staggered_data() {
        // Simulate the sine wave scenario: two assets with alternating price data.
        // Without LOCF, the total swings wildly. With LOCF, it's smooth.
        let mut app = make_app();
        app.positions = vec![
            make_position("AAPL", dec!(10), AssetCategory::Equity),
            make_position("GOOG", dec!(5), AssetCategory::Equity),
        ];
        // AAPL has prices on odd days, GOOG on even days
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-01-01".to_string(), close: dec!(100), volume: None },
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(100), volume: None },
            HistoryRecord { date: "2026-01-05".to_string(), close: dec!(100), volume: None },
        ]);
        app.price_history.insert("GOOG".to_string(), vec![
            HistoryRecord { date: "2026-01-02".to_string(), close: dec!(200), volume: None },
            HistoryRecord { date: "2026-01-04".to_string(), close: dec!(200), volume: None },
        ]);
        app.compute_portfolio_value_history();

        // With LOCF, once both assets have appeared, the total stays consistent:
        // Day 1: AAPL=10*100=1000 (GOOG not yet seen)
        // Day 2: AAPL LOCF=1000, GOOG=5*200=1000 → 2000
        // Day 3: AAPL=1000, GOOG LOCF=1000 → 2000
        // Day 4: AAPL LOCF=1000, GOOG=1000 → 2000
        // Day 5: AAPL=1000, GOOG LOCF=1000 → 2000
        // Without LOCF, days would swing between ~1000 and ~2000 (the sine wave bug)
        assert_eq!(app.portfolio_value_history.len(), 5);
        // Day 1 only has AAPL
        assert_eq!(app.portfolio_value_history[0].1, dec!(1000));
        // Days 2-5 should all be 2000 (no sine wave)
        for i in 1..5 {
            assert_eq!(app.portfolio_value_history[i].1, dec!(2000),
                "Day {} should be 2000 but was {}", i + 1, app.portfolio_value_history[i].1);
        }
    }

    #[test]
    fn test_locf_cash_always_included() {
        let mut app = make_app();
        app.positions = vec![
            make_position("USD", dec!(5000), AssetCategory::Cash),
            make_position("AAPL", dec!(10), AssetCategory::Equity),
        ];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-01-01".to_string(), close: dec!(150), volume: None },
            HistoryRecord { date: "2026-01-02".to_string(), close: dec!(155), volume: None },
        ]);
        app.compute_portfolio_value_history();

        // Cash always priced at quantity (1.0 * qty)
        assert_eq!(app.portfolio_value_history[0].1, dec!(5000) + dec!(10) * dec!(150));
        assert_eq!(app.portfolio_value_history[1].1, dec!(5000) + dec!(10) * dec!(155));
    }

    #[test]
    fn test_locf_position_not_included_before_first_price() {
        // A position should NOT contribute until its first price point appears
        let mut app = make_app();
        app.positions = vec![
            make_position("AAPL", dec!(10), AssetCategory::Equity),
            make_position("NEW", dec!(20), AssetCategory::Equity),
        ];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-01-01".to_string(), close: dec!(100), volume: None },
            HistoryRecord { date: "2026-01-02".to_string(), close: dec!(105), volume: None },
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(110), volume: None },
        ]);
        // NEW only gets a price on day 3
        app.price_history.insert("NEW".to_string(), vec![
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(50), volume: None },
        ]);
        app.compute_portfolio_value_history();

        // Day 1: AAPL=10*100=1000, NEW not yet priced
        assert_eq!(app.portfolio_value_history[0].1, dec!(1000));
        // Day 2: AAPL=10*105=1050, NEW still not yet priced
        assert_eq!(app.portfolio_value_history[1].1, dec!(1050));
        // Day 3: AAPL=10*110=1100, NEW=20*50=1000 → 2100
        assert_eq!(app.portfolio_value_history[2].1, dec!(2100));
    }

    #[test]
    fn test_empty_history_produces_empty_result() {
        let mut app = make_app();
        app.positions = vec![make_position("AAPL", dec!(10), AssetCategory::Equity)];
        // No price history at all
        app.compute_portfolio_value_history();
        assert!(app.portfolio_value_history.is_empty());
    }

    #[test]
    fn test_percentage_mode_clears_history() {
        let mut app = make_app();
        app.portfolio_mode = PortfolioMode::Percentage;
        app.portfolio_value_history = vec![("2026-01-01".to_string(), dec!(1000))];
        app.compute_portfolio_value_history();
        assert!(app.portfolio_value_history.is_empty());
    }
}

#[cfg(test)]
mod flash_direction_tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_flash_direction_up_when_price_increases() {
        let mut prices: HashMap<String, Decimal> = HashMap::new();
        prices.insert("AAPL".to_string(), dec!(150));

        let new_price = dec!(155);
        let direction = match prices.get("AAPL") {
            Some(&old_price) if new_price > old_price => PriceFlashDirection::Up,
            Some(&old_price) if new_price < old_price => PriceFlashDirection::Down,
            _ => PriceFlashDirection::Same,
        };
        assert_eq!(direction, PriceFlashDirection::Up);
    }

    #[test]
    fn test_flash_direction_down_when_price_decreases() {
        let mut prices: HashMap<String, Decimal> = HashMap::new();
        prices.insert("AAPL".to_string(), dec!(150));

        let new_price = dec!(145);
        let direction = match prices.get("AAPL") {
            Some(&old_price) if new_price > old_price => PriceFlashDirection::Up,
            Some(&old_price) if new_price < old_price => PriceFlashDirection::Down,
            _ => PriceFlashDirection::Same,
        };
        assert_eq!(direction, PriceFlashDirection::Down);
    }

    #[test]
    fn test_flash_direction_same_when_price_unchanged() {
        let mut prices: HashMap<String, Decimal> = HashMap::new();
        prices.insert("AAPL".to_string(), dec!(150));

        let new_price = dec!(150);
        let direction = match prices.get("AAPL") {
            Some(&old_price) if new_price > old_price => PriceFlashDirection::Up,
            Some(&old_price) if new_price < old_price => PriceFlashDirection::Down,
            _ => PriceFlashDirection::Same,
        };
        assert_eq!(direction, PriceFlashDirection::Same);
    }

    #[test]
    fn test_flash_direction_same_when_no_previous_price() {
        let prices: HashMap<String, Decimal> = HashMap::new();

        let new_price = dec!(150);
        let direction = match prices.get("AAPL") {
            Some(&old_price) if new_price > old_price => PriceFlashDirection::Up,
            Some(&old_price) if new_price < old_price => PriceFlashDirection::Down,
            _ => PriceFlashDirection::Same,
        };
        assert_eq!(direction, PriceFlashDirection::Same);
    }

    #[test]
    fn test_flash_stores_tick_and_direction() {
        let mut flash_map: HashMap<String, (u64, PriceFlashDirection)> = HashMap::new();
        flash_map.insert("BTC".to_string(), (100, PriceFlashDirection::Up));

        let (tick, dir) = flash_map.get("BTC").unwrap();
        assert_eq!(*tick, 100);
        assert_eq!(*dir, PriceFlashDirection::Up);
    }
}

#[cfg(test)]
mod keystroke_echo_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_app() -> App {
        let config = Config::default();
        App::new(&config, std::path::PathBuf::from(":memory:"))
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn test_record_regular_key() {
        let mut app = make_app();
        app.tick_count = 50;
        app.record_keystroke(&key('j'));
        assert_eq!(app.last_key_display, "j");
        assert_eq!(app.last_key_tick, 50);
    }

    #[test]
    fn test_record_ctrl_key() {
        let mut app = make_app();
        app.tick_count = 100;
        app.record_keystroke(&ctrl_key('d'));
        assert_eq!(app.last_key_display, "Ctrl+d");
        assert_eq!(app.last_key_tick, 100);
    }

    #[test]
    fn test_record_shift_g() {
        let mut app = make_app();
        app.tick_count = 75;
        app.record_keystroke(&KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.last_key_display, "G");
    }

    #[test]
    fn test_record_gg_sequence() {
        let mut app = make_app();
        app.tick_count = 10;
        // First g
        app.record_keystroke(&key('g'));
        assert_eq!(app.last_key_display, "g");
        // Simulate g_pending being set (handle_key would do this)
        app.g_pending = true;
        app.tick_count = 11;
        // Second g
        app.record_keystroke(&key('g'));
        assert_eq!(app.last_key_display, "gg");
    }

    #[test]
    fn test_record_enter_key() {
        let mut app = make_app();
        app.record_keystroke(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.last_key_display, "Enter");
    }

    #[test]
    fn test_record_esc_key() {
        let mut app = make_app();
        app.record_keystroke(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.last_key_display, "Esc");
    }

    #[test]
    fn test_record_arrow_keys() {
        let mut app = make_app();
        app.record_keystroke(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.last_key_display, "↑");
        app.record_keystroke(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.last_key_display, "↓");
    }

    #[test]
    fn test_key_echo_text_generation() {
        let mut app = make_app();
        // Slash for search
        app.record_keystroke(&key('/'));
        assert_eq!(app.last_key_display, "/");
        // Number keys
        app.record_keystroke(&key('1'));
        assert_eq!(app.last_key_display, "1");
    }
}

#[cfg(test)]
mod breadcrumb_tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_app() -> App {
        let config = Config::default();
        App::new(&config, std::path::PathBuf::from(":memory:"))
    }

    fn make_position(symbol: &str, category: AssetCategory) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category,
            quantity: dec!(1),
            avg_cost: dec!(100),
            total_cost: dec!(100),
            currency: "USD".to_string(),
            current_price: Some(dec!(110)),
            current_value: Some(dec!(110)),
            gain: Some(dec!(10)),
            gain_pct: Some(dec!(10)),
            allocation_pct: Some(dec!(100)),
        }
    }

    #[test]
    fn test_breadcrumb_positions_no_selection() {
        let app = make_app();
        assert_eq!(app.breadcrumb(), "Positions");
    }

    #[test]
    fn test_breadcrumb_positions_with_selection() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL", AssetCategory::Equity)];
        app.selected_index = 0;
        assert_eq!(app.breadcrumb(), "Positions › AAPL");
    }

    #[test]
    fn test_breadcrumb_detail_popup() {
        let mut app = make_app();
        app.display_positions = vec![make_position("BTC", AssetCategory::Crypto)];
        app.selected_index = 0;
        app.detail_popup_open = true;
        assert_eq!(app.breadcrumb(), "Positions › BTC › Detail");
    }

    #[test]
    fn test_breadcrumb_chart_variant() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL", AssetCategory::Equity)];
        app.selected_index = 0;
        app.chart_index = 1; // First individual chart (index 0 = All)
        app.chart_timeframe = ChartTimeframe::ThreeMonths;
        let crumb = app.breadcrumb();
        // Should contain view, symbol, timeframe, and variant label
        assert!(crumb.starts_with("Positions › AAPL › 3M › "), "got: {crumb}");
    }

    #[test]
    fn test_breadcrumb_transactions_view() {
        let mut app = make_app();
        app.view_mode = ViewMode::Transactions;
        assert_eq!(app.breadcrumb(), "Transactions");
    }

    #[test]
    fn test_breadcrumb_markets_view() {
        let mut app = make_app();
        app.view_mode = ViewMode::Markets;
        assert_eq!(app.breadcrumb(), "Markets");
    }

    #[test]
    fn test_breadcrumb_economy_view() {
        let mut app = make_app();
        app.view_mode = ViewMode::Economy;
        assert_eq!(app.breadcrumb(), "Economy");
    }

    #[test]
    fn test_breadcrumb_watchlist_view() {
        let mut app = make_app();
        app.view_mode = ViewMode::Watchlist;
        assert_eq!(app.breadcrumb(), "Watchlist");
    }

    #[test]
    fn test_breadcrumb_detail_overrides_chart() {
        let mut app = make_app();
        app.display_positions = vec![make_position("GLD", AssetCategory::Commodity)];
        app.selected_index = 0;
        app.chart_index = 2;
        app.detail_popup_open = true;
        // Detail popup takes precedence over chart context
        assert_eq!(app.breadcrumb(), "Positions › GLD › Detail");
    }

    #[test]
    fn test_breadcrumb_chart_timeframe_label() {
        let mut app = make_app();
        app.display_positions = vec![make_position("SPY", AssetCategory::Equity)];
        app.selected_index = 0;
        app.chart_index = 1;
        app.chart_timeframe = ChartTimeframe::OneYear;
        let crumb = app.breadcrumb();
        assert!(crumb.contains("1Y"), "got: {crumb}");
    }
}

#[cfg(test)]
mod tx_form_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rust_decimal_macros::dec;

    fn make_app() -> App {
        let config = Config::default();
        App::new(&config, std::path::PathBuf::from(":memory:"))
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn shift_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
    }

    #[test]
    fn test_tx_form_field_next_cycles() {
        assert_eq!(TxFormField::TxType.next(), TxFormField::Quantity);
        assert_eq!(TxFormField::Quantity.next(), TxFormField::PricePer);
        assert_eq!(TxFormField::PricePer.next(), TxFormField::Date);
        assert_eq!(TxFormField::Date.next(), TxFormField::Date); // last stays
    }

    #[test]
    fn test_tx_form_field_prev_cycles() {
        assert_eq!(TxFormField::Date.prev(), TxFormField::PricePer);
        assert_eq!(TxFormField::PricePer.prev(), TxFormField::Quantity);
        assert_eq!(TxFormField::Quantity.prev(), TxFormField::TxType);
        assert_eq!(TxFormField::TxType.prev(), TxFormField::TxType); // first stays
    }

    #[test]
    fn test_tx_form_state_defaults() {
        let form = TxFormState::new("BTC".to_string(), AssetCategory::Crypto);
        assert_eq!(form.symbol, "BTC");
        assert_eq!(form.category, AssetCategory::Crypto);
        assert_eq!(form.tx_type, TxType::Buy);
        assert!(form.quantity_input.is_empty());
        assert!(form.price_input.is_empty());
        assert_eq!(form.date_input.len(), 10); // YYYY-MM-DD
        assert!(form.error.is_none());
    }

    #[test]
    fn test_tx_form_opens_on_shift_a() {
        let mut app = make_app();
        app.positions = vec![Position {
            symbol: "AAPL".to_string(),
            name: "Apple".to_string(),
            category: AssetCategory::Equity,
            quantity: dec!(10),
            avg_cost: dec!(150),
            total_cost: dec!(1500),
            currency: "USD".to_string(),
            current_price: Some(dec!(155)),
            current_value: Some(dec!(1550)),
            gain: Some(dec!(50)),
            gain_pct: Some(dec!(3.33)),
            allocation_pct: Some(dec!(100)),
        }];
        app.display_positions = app.positions.clone();
        app.selected_index = 0;

        assert!(app.tx_form.is_none());
        app.handle_key(shift_key('A'));
        assert!(app.tx_form.is_some());
        let form = app.tx_form.as_ref().unwrap();
        assert_eq!(form.symbol, "AAPL");
        assert_eq!(form.category, AssetCategory::Equity);
    }

    #[test]
    fn test_tx_form_esc_cancels() {
        let mut app = make_app();
        app.tx_form = Some(TxFormState::new("BTC".to_string(), AssetCategory::Crypto));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.tx_form.is_none());
    }

    #[test]
    fn test_tx_form_tab_advances_field() {
        let mut app = make_app();
        app.tx_form = Some(TxFormState::new("BTC".to_string(), AssetCategory::Crypto));

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.tx_form.as_ref().unwrap().active_field, TxFormField::Quantity);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.tx_form.as_ref().unwrap().active_field, TxFormField::PricePer);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.tx_form.as_ref().unwrap().active_field, TxFormField::Date);
    }

    #[test]
    fn test_tx_form_type_toggles() {
        let mut app = make_app();
        app.tx_form = Some(TxFormState::new("BTC".to_string(), AssetCategory::Crypto));
        assert_eq!(app.tx_form.as_ref().unwrap().tx_type, TxType::Buy);

        // Any char toggles type
        app.handle_key(key('x'));
        assert_eq!(app.tx_form.as_ref().unwrap().tx_type, TxType::Sell);

        app.handle_key(key('x'));
        assert_eq!(app.tx_form.as_ref().unwrap().tx_type, TxType::Buy);
    }

    #[test]
    fn test_tx_form_quantity_accepts_digits() {
        let mut app = make_app();
        app.tx_form = Some(TxFormState::new("BTC".to_string(), AssetCategory::Crypto));
        // Move to quantity field
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.tx_form.as_ref().unwrap().active_field, TxFormField::Quantity);

        app.handle_key(key('1'));
        app.handle_key(key('0'));
        app.handle_key(key('.'));
        app.handle_key(key('5'));
        assert_eq!(app.tx_form.as_ref().unwrap().quantity_input, "10.5");

        // Non-digit/non-dot is ignored
        app.handle_key(key('a'));
        assert_eq!(app.tx_form.as_ref().unwrap().quantity_input, "10.5");
    }

    #[test]
    fn test_tx_form_backspace_removes_char() {
        let mut app = make_app();
        app.tx_form = Some(TxFormState::new("BTC".to_string(), AssetCategory::Crypto));
        // Move to quantity field
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(key('1'));
        app.handle_key(key('0'));
        assert_eq!(app.tx_form.as_ref().unwrap().quantity_input, "10");

        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.tx_form.as_ref().unwrap().quantity_input, "1");
    }

    #[test]
    fn test_tx_form_does_not_quit_on_q() {
        let mut app = make_app();
        app.tx_form = Some(TxFormState::new("BTC".to_string(), AssetCategory::Crypto));
        app.handle_key(key('q'));
        // Should NOT quit — form eats the key
        assert!(!app.should_quit);
        assert!(app.tx_form.is_some());
    }

    #[test]
    fn test_tx_form_enter_advances_until_date() {
        let mut app = make_app();
        app.tx_form = Some(TxFormState::new("BTC".to_string(), AssetCategory::Crypto));

        // Enter on TxType → Quantity
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.tx_form.as_ref().unwrap().active_field, TxFormField::Quantity);

        // Enter on Quantity → PricePer
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.tx_form.as_ref().unwrap().active_field, TxFormField::PricePer);

        // Enter on PricePer → Date
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.tx_form.as_ref().unwrap().active_field, TxFormField::Date);
    }

    #[test]
    fn test_tx_form_validation_rejects_empty_quantity() {
        let mut app = make_app();
        let mut form = TxFormState::new("BTC".to_string(), AssetCategory::Crypto);
        form.active_field = TxFormField::Date;
        form.price_input = "100".to_string();
        // quantity_input is empty
        app.tx_form = Some(form);

        // Enter on Date attempts submit
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        // Should still have form open with error
        assert!(app.tx_form.is_some());
        assert!(app.tx_form.as_ref().unwrap().error.is_some());
        assert_eq!(
            app.tx_form.as_ref().unwrap().error.as_deref(),
            Some("Invalid quantity")
        );
    }

    #[test]
    fn test_tx_form_validation_rejects_empty_price() {
        let mut app = make_app();
        let mut form = TxFormState::new("BTC".to_string(), AssetCategory::Crypto);
        form.active_field = TxFormField::Date;
        form.quantity_input = "10".to_string();
        // price_input is empty
        app.tx_form = Some(form);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.tx_form.is_some());
        assert_eq!(
            app.tx_form.as_ref().unwrap().error.as_deref(),
            Some("Invalid price")
        );
    }

    #[test]
    fn test_tx_form_validation_rejects_bad_date() {
        let mut app = make_app();
        let mut form = TxFormState::new("BTC".to_string(), AssetCategory::Crypto);
        form.active_field = TxFormField::Date;
        form.quantity_input = "10".to_string();
        form.price_input = "100".to_string();
        form.date_input = "2026-1-1".to_string(); // wrong format
        app.tx_form = Some(form);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.tx_form.is_some());
        assert_eq!(
            app.tx_form.as_ref().unwrap().error.as_deref(),
            Some("Date must be YYYY-MM-DD")
        );
    }

    #[test]
    fn test_delete_confirm_state() {
        let state = DeleteConfirmState {
            symbol: "BTC".to_string(),
            tx_count: 3,
            tx_ids: vec![1, 2, 3],
        };
        assert_eq!(state.symbol, "BTC");
        assert_eq!(state.tx_count, 3);
        assert_eq!(state.tx_ids.len(), 3);
    }

    #[test]
    fn test_delete_confirm_cancels_on_non_y() {
        let mut app = make_app();
        app.delete_confirm = Some(DeleteConfirmState {
            symbol: "BTC".to_string(),
            tx_count: 1,
            tx_ids: vec![1],
        });

        app.handle_key(key('n'));
        assert!(app.delete_confirm.is_none());
    }

    #[test]
    fn test_delete_confirm_cancels_on_esc() {
        let mut app = make_app();
        app.delete_confirm = Some(DeleteConfirmState {
            symbol: "BTC".to_string(),
            tx_count: 1,
            tx_ids: vec![1],
        });

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.delete_confirm.is_none());
    }

    #[test]
    fn test_shift_a_does_nothing_in_percentage_mode() {
        let mut app = make_app();
        app.portfolio_mode = PortfolioMode::Percentage;
        app.display_positions = vec![Position {
            symbol: "AAPL".to_string(),
            name: "Apple".to_string(),
            category: AssetCategory::Equity,
            quantity: dec!(100),
            avg_cost: dec!(0),
            total_cost: dec!(0),
            currency: "USD".to_string(),
            current_price: None,
            current_value: None,
            gain: None,
            gain_pct: None,
            allocation_pct: Some(dec!(50)),
        }];
        app.selected_index = 0;

        app.handle_key(shift_key('A'));
        assert!(app.tx_form.is_none());
    }

    #[test]
    fn test_shift_a_does_nothing_with_no_selection() {
        let mut app = make_app();
        // No positions
        app.handle_key(shift_key('A'));
        assert!(app.tx_form.is_none());
    }

    #[test]
    fn test_tx_form_field_labels() {
        assert_eq!(TxFormField::TxType.label(), "Type");
        assert_eq!(TxFormField::Quantity.label(), "Qty");
        assert_eq!(TxFormField::PricePer.label(), "Price");
        assert_eq!(TxFormField::Date.label(), "Date");
    }

    #[test]
    fn test_sparkline_timeframe_default() {
        let app = make_app();
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::ThreeMonths));
    }

    #[test]
    fn test_sparkline_timeframe_cycle_forward() {
        let mut app = make_app();
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::ThreeMonths));
        app.handle_key(key(']'));
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::SixMonths));
        app.handle_key(key(']'));
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::OneYear));
        app.handle_key(key(']'));
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::FiveYears));
        app.handle_key(key(']'));
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::OneWeek));
    }

    #[test]
    fn test_sparkline_timeframe_cycle_backward() {
        let mut app = make_app();
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::ThreeMonths));
        app.handle_key(key('['));
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::OneMonth));
        app.handle_key(key('['));
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::OneWeek));
        app.handle_key(key('['));
        assert!(matches!(app.sparkline_timeframe, ChartTimeframe::FiveYears));
    }
}

#[cfg(test)]
mod sort_flash_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rust_decimal_macros::dec;
    use std::path::PathBuf;

    fn make_test_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        let mut app = App::new(&config, PathBuf::from("/tmp/pftui_test_sort_flash.db"));
        for i in 0..3 {
            app.display_positions.push(crate::models::position::Position {
                symbol: format!("SYM{}", i),
                name: format!("Symbol {}", i),
                category: crate::models::asset::AssetCategory::Equity,
                quantity: dec!(1),
                avg_cost: dec!(100),
                total_cost: dec!(100),
                currency: "USD".to_string(),
                current_price: Some(dec!(110)),
                current_value: Some(dec!(110)),
                gain: Some(dec!(10)),
                gain_pct: Some(dec!(10)),
                allocation_pct: Some(dec!(33)),
            });
        }
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn tab_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
    }

    #[test]
    fn test_sort_flash_starts_at_zero() {
        let app = make_test_app();
        assert_eq!(app.last_sort_change_tick, 0);
    }

    #[test]
    fn test_sort_flash_updates_on_sort_change() {
        let mut app = make_test_app();
        app.tick_count = 50;
        app.handle_key(key('n')); // sort by name
        assert_eq!(app.last_sort_change_tick, 50);
        assert_eq!(app.sort_field, SortField::Name);
    }

    #[test]
    fn test_sort_flash_updates_on_tab_toggle() {
        let mut app = make_test_app();
        let was_ascending = app.sort_ascending;
        app.tick_count = 100;
        app.handle_key(tab_key()); // toggle sort direction
        assert_eq!(app.last_sort_change_tick, 100);
        assert_ne!(app.sort_ascending, was_ascending);
    }

    #[test]
    fn test_sort_flash_updates_on_category_sort() {
        let mut app = make_test_app();
        app.tick_count = 75;
        app.handle_key(key('c')); // sort by category
        assert_eq!(app.last_sort_change_tick, 75);
        assert_eq!(app.sort_field, SortField::Category);
    }

    #[test]
    fn test_sort_flash_updates_on_allocation_sort() {
        let mut app = make_test_app();
        app.tick_count = 200;
        app.handle_key(key('a')); // sort by allocation
        assert_eq!(app.last_sort_change_tick, 200);
        assert_eq!(app.sort_field, SortField::Allocation);
    }
}

#[cfg(test)]
mod prev_day_alloc_tests {
    use super::*;
    use std::path::PathBuf;

    fn make_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
        };
        App::new(&config, PathBuf::from("/tmp/pftui_test_prevalloc.db"))
    }

    fn make_position(symbol: &str, qty: Decimal, price: Option<Decimal>, category: AssetCategory) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category,
            quantity: qty,
            avg_cost: dec!(100),
            total_cost: qty * dec!(100),
            currency: "USD".to_string(),
            current_price: price,
            current_value: price.map(|p| p * qty),
            gain: None,
            gain_pct: None,
            allocation_pct: None,
        }
    }

    #[test]
    fn test_prev_day_empty_without_history() {
        let mut app = make_app();
        app.positions = vec![
            make_position("AAPL", dec!(10), Some(dec!(150)), AssetCategory::Equity),
        ];
        app.compute_prev_day_cat_allocations();
        assert!(app.prev_day_cat_allocations.is_empty());
    }

    #[test]
    fn test_prev_day_uses_second_to_last_close() {
        let mut app = make_app();
        app.positions = vec![
            make_position("AAPL", dec!(10), Some(dec!(150)), AssetCategory::Equity),
            make_position("BTC", dec!(1), Some(dec!(60000)), AssetCategory::Crypto),
        ];
        // AAPL: prev close 140, BTC: prev close 50000
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(140), volume: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
        ]);
        app.price_history.insert("BTC".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(50000), volume: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(60000), volume: None },
        ]);
        app.compute_prev_day_cat_allocations();

        // Prev day: AAPL = 10*140 = 1400, BTC = 1*50000 = 50000, total = 51400
        // Equity: 1400/51400 * 100 ≈ 2.72%
        // Crypto: 50000/51400 * 100 ≈ 97.28%
        let equity_alloc = app.prev_day_cat_allocations.get(&AssetCategory::Equity).unwrap();
        let crypto_alloc = app.prev_day_cat_allocations.get(&AssetCategory::Crypto).unwrap();
        assert!(*equity_alloc > dec!(2) && *equity_alloc < dec!(3));
        assert!(*crypto_alloc > dec!(97) && *crypto_alloc < dec!(98));
    }

    #[test]
    fn test_prev_day_single_record_insufficient() {
        let mut app = make_app();
        app.positions = vec![
            make_position("AAPL", dec!(10), Some(dec!(150)), AssetCategory::Equity),
        ];
        // Only 1 record — no "previous" day
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
        ]);
        app.compute_prev_day_cat_allocations();
        assert!(app.prev_day_cat_allocations.is_empty());
    }

    #[test]
    fn test_prev_day_cash_always_priced_at_one() {
        let mut app = make_app();
        app.positions = vec![
            make_position("AAPL", dec!(10), Some(dec!(100)), AssetCategory::Equity),
            make_position("USD", dec!(1000), Some(dec!(1)), AssetCategory::Cash),
        ];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(100), volume: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(100), volume: None },
        ]);
        // No price_history for USD/Cash — should still use price 1.0
        app.compute_prev_day_cat_allocations();

        // Prev day: AAPL = 10*100 = 1000, USD = 1000*1 = 1000, total = 2000
        // Each should be 50%
        let equity = app.prev_day_cat_allocations.get(&AssetCategory::Equity).unwrap();
        let cash = app.prev_day_cat_allocations.get(&AssetCategory::Cash).unwrap();
        assert_eq!(*equity, dec!(50));
        assert_eq!(*cash, dec!(50));
    }

    #[test]
    fn test_prev_day_aggregates_by_category() {
        let mut app = make_app();
        app.positions = vec![
            make_position("AAPL", dec!(10), Some(dec!(150)), AssetCategory::Equity),
            make_position("GOOG", dec!(5), Some(dec!(200)), AssetCategory::Equity),
            make_position("BTC", dec!(1), Some(dec!(50000)), AssetCategory::Crypto),
        ];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(140), volume: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None },
        ]);
        app.price_history.insert("GOOG".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(190), volume: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(200), volume: None },
        ]);
        app.price_history.insert("BTC".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(48000), volume: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(50000), volume: None },
        ]);
        app.compute_prev_day_cat_allocations();

        // Prev day: AAPL = 10*140 = 1400, GOOG = 5*190 = 950, BTC = 1*48000 = 48000
        // Equity total = 2350, total = 50350
        // Equity: 2350/50350 ≈ 4.67%, Crypto: 48000/50350 ≈ 95.33%
        let equity = app.prev_day_cat_allocations.get(&AssetCategory::Equity).unwrap();
        let crypto = app.prev_day_cat_allocations.get(&AssetCategory::Crypto).unwrap();
        assert!(*equity > dec!(4) && *equity < dec!(5));
        assert!(*crypto > dec!(95) && *crypto < dec!(96));
    }
}
