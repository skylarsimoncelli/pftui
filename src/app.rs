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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Positions,
    Transactions,
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
    pub detail_open: bool,

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
    pub g_pending: bool,
    pub terminal_height: u16,

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

    // Animation
    pub tick_count: u64,
    pub price_flash_ticks: HashMap<String, u64>,
    pub last_value_update_tick: u64,

    // DB
    db_path: std::path::PathBuf,
}

/// Returns true when the UI should hide value-sensitive data.
pub fn is_privacy_view(app: &App) -> bool {
    app.portfolio_mode == PortfolioMode::Percentage || app.show_percentages_only
}

impl App {
    pub fn new(config: &Config, db_path: std::path::PathBuf) -> Self {
        App {
            should_quit: false,
            view_mode: ViewMode::Positions,
            show_help: false,
            detail_open: false,
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
            g_pending: false,
            terminal_height: 24, // sensible default, updated on resize
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
            if let Ok(all) = price_history::get_all_symbols_history(&conn, 90) {
                for (symbol, records) in all {
                    self.price_history.insert(symbol, records);
                }
            }
        }
        if self.portfolio_mode == PortfolioMode::Full {
            self.compute_portfolio_value_history();
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

    fn request_all_history(&self, service: &PriceService) {
        let mut seen = std::collections::HashSet::new();
        let mut batch = Vec::new();

        // Collect portfolio symbols
        let symbols = self.get_symbols();
        for (symbol, category) in &symbols {
            if seen.insert(symbol.clone()) {
                batch.push((symbol.clone(), *category, 90));
            }
        }

        // Collect chart comparison symbols (indices, benchmarks)
        // so charts are ready when the user opens the detail panel
        for pos in &self.positions {
            for (sym, cat) in Self::chart_fetch_symbols(pos) {
                if seen.insert(sym.clone()) {
                    batch.push((sym, cat, 90));
                }
            }
        }

        // Send as a single batch for concurrent fetching
        if !batch.is_empty() {
            service.send_command(PriceCommand::FetchHistoryBatch(batch));
        }
    }

    fn request_history_for_symbol(&self, symbol: &str, category: AssetCategory) {
        if let Some(ref service) = self.price_service {
            service.send_command(PriceCommand::FetchHistory(
                symbol.to_string(),
                category,
                90,
            ));
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
        // Filter positions
        let mut positions: Vec<Position> = match self.category_filter {
            Some(cat) => self
                .positions
                .iter()
                .filter(|p| p.category == cat)
                .cloned()
                .collect(),
            None => self.positions.clone(),
        };

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
            // Regular equity/fund/forex/commodity: just its own chart
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
            return vec![ChartVariant {
                label,
                kind: ChartKind::Single {
                    symbol: yahoo_sym,
                    category: cat,
                },
            }];
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
                        self.price_history.insert(symbol, records);
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
                return;
            }
            KeyCode::Esc if self.show_help => {
                self.show_help = false;
                return;
            }
            KeyCode::Esc if self.detail_open => {
                self.detail_open = false;
                return;
            }
            _ => {}
        }

        if self.show_help {
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
                }
            }

            // Privacy toggle
            KeyCode::Char('p') => {
                if self.portfolio_mode == PortfolioMode::Full {
                    self.show_percentages_only = !self.show_percentages_only;
                }
            }

            // Detail view toggle
            KeyCode::Enter if matches!(self.view_mode, ViewMode::Positions) => {
                if let Some(pos) = self.selected_position().cloned() {
                    self.detail_open = !self.detail_open;
                    self.chart_index = 0;
                    if self.detail_open {
                        let fetch_syms = Self::chart_fetch_symbols(&pos);
                        for (sym, cat) in &fetch_syms {
                            self.request_history_for_symbol(sym, *cat);
                        }
                    }
                }
            }

            // Chart variant cycling with J/K (when detail open)
            KeyCode::Char('K') if self.detail_open => {
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
            KeyCode::Char('J') if self.detail_open => {
                if let Some(pos) = self.selected_position() {
                    let count = Self::chart_variants_for_position(pos).len();
                    if count > 1 {
                        self.chart_index = (self.chart_index + 1) % count;
                    }
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
        }
    }

    fn move_up(&mut self) {
        match self.view_mode {
            ViewMode::Positions => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            ViewMode::Transactions => {
                self.tx_selected_index = self.tx_selected_index.saturating_sub(1);
            }
        }
    }

    fn jump_to_top(&mut self) {
        match self.view_mode {
            ViewMode::Positions => {
                self.selected_index = 0;
            }
            ViewMode::Transactions => {
                self.tx_selected_index = 0;
            }
        }
    }

    fn jump_to_bottom(&mut self) {
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
        }
    }

    pub fn set_terminal_height(&mut self, h: u16) {
        self.terminal_height = h;
    }

    /// Half-page size: (terminal_height - 4 for header/status) / 2, minimum 1
    fn half_page(&self) -> usize {
        let content_rows = self.terminal_height.saturating_sub(4) as usize;
        (content_rows / 2).max(1)
    }

    fn scroll_down_half_page(&mut self) {
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
        }
    }

    fn scroll_up_half_page(&mut self) {
        let step = self.half_page();
        match self.view_mode {
            ViewMode::Positions => {
                self.selected_index = self.selected_index.saturating_sub(step);
            }
            ViewMode::Transactions => {
                self.tx_selected_index = self.tx_selected_index.saturating_sub(step);
            }
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
    fn test_regular_equity_single_chart() {
        let pos = make_position("AAPL", AssetCategory::Equity);
        let variants = App::chart_variants_for_position(&pos);

        // Regular equities get a single chart, no "All" prepended
        assert_eq!(variants.len(), 1);
        match &variants[0].kind {
            ChartKind::Single { symbol, .. } => assert_eq!(symbol, "AAPL"),
            _ => panic!("Expected Single chart for equity"),
        }
    }

    #[test]
    fn test_crypto_non_btc_single_chart() {
        let pos = make_position("ETH", AssetCategory::Crypto);
        let variants = App::chart_variants_for_position(&pos);

        // Non-BTC crypto gets single chart with -USD suffix
        assert_eq!(variants.len(), 1);
        match &variants[0].kind {
            ChartKind::Single { symbol, category } => {
                assert_eq!(symbol, "ETH-USD");
                assert_eq!(*category, AssetCategory::Equity); // routed to Yahoo
            }
            _ => panic!("Expected Single chart for crypto"),
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
