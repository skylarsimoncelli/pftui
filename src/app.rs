use std::collections::HashMap;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::prelude::Rect;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::config::{self, Config, PortfolioMode, WatchlistColumn, WorkspaceLayout};
use crate::data::brave;
use crate::db::{allocations, price_cache, price_history};
use crate::db::scan_queries;
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
    Analytics,
    News,
    Journal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartRenderMode {
    Line,
    Candlestick,
}

impl ChartRenderMode {
    pub fn label(self) -> &'static str {
        match self {
            ChartRenderMode::Line => "Line",
            ChartRenderMode::Candlestick => "Candles",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            ChartRenderMode::Line => ChartRenderMode::Candlestick,
            ChartRenderMode::Candlestick => ChartRenderMode::Line,
        }
    }
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

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "1W" | "1w" => Some(ChartTimeframe::OneWeek),
            "1M" | "1m" => Some(ChartTimeframe::OneMonth),
            "3M" | "3m" => Some(ChartTimeframe::ThreeMonths),
            "6M" | "6m" => Some(ChartTimeframe::SixMonths),
            "1Y" | "1y" => Some(ChartTimeframe::OneYear),
            "5Y" | "5y" => Some(ChartTimeframe::FiveYears),
            _ => None,
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
pub enum ChangeTimeframe {
    OneHour,
    TwentyFourHour,
    SevenDay,
    ThirtyDay,
    YearToDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanBuilderMode {
    Edit,
    SaveName,
    LoadName,
}

impl ChangeTimeframe {
    pub fn label(self) -> &'static str {
        match self {
            ChangeTimeframe::OneHour => "1h",
            ChangeTimeframe::TwentyFourHour => "24h",
            ChangeTimeframe::SevenDay => "7d",
            ChangeTimeframe::ThirtyDay => "30d",
            ChangeTimeframe::YearToDate => "YTD",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ChangeTimeframe::OneHour => ChangeTimeframe::TwentyFourHour,
            ChangeTimeframe::TwentyFourHour => ChangeTimeframe::SevenDay,
            ChangeTimeframe::SevenDay => ChangeTimeframe::ThirtyDay,
            ChangeTimeframe::ThirtyDay => ChangeTimeframe::YearToDate,
            ChangeTimeframe::YearToDate => ChangeTimeframe::OneHour,
        }
    }

    /// Returns the number of days to look back for this timeframe.
    /// For YTD, returns None (needs special handling).
    pub fn lookback_days(self) -> Option<u32> {
        match self {
            ChangeTimeframe::OneHour => Some(1), // 1 hour within last day
            ChangeTimeframe::TwentyFourHour => Some(2), // need 2 days to compute 24h change
            ChangeTimeframe::SevenDay => Some(8),
            ChangeTimeframe::ThirtyDay => Some(31),
            ChangeTimeframe::YearToDate => None, // computed from Jan 1 of current year
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketCorrelationWindow {
    SevenDay,
    ThirtyDay,
    NinetyDay,
}

impl MarketCorrelationWindow {
    pub fn days(self) -> usize {
        match self {
            MarketCorrelationWindow::SevenDay => 7,
            MarketCorrelationWindow::ThirtyDay => 30,
            MarketCorrelationWindow::NinetyDay => 90,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MarketCorrelationWindow::SevenDay => "7d",
            MarketCorrelationWindow::ThirtyDay => "30d",
            MarketCorrelationWindow::NinetyDay => "90d",
        }
    }

    pub fn next(self) -> Self {
        match self {
            MarketCorrelationWindow::SevenDay => MarketCorrelationWindow::ThirtyDay,
            MarketCorrelationWindow::ThirtyDay => MarketCorrelationWindow::NinetyDay,
            MarketCorrelationWindow::NinetyDay => MarketCorrelationWindow::SevenDay,
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

/// Actions available in the right-click context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    ViewDetail,
    AddTransaction,
    Delete,
    CopySymbol,
}

impl ContextMenuAction {
    pub fn label(self) -> &'static str {
        match self {
            ContextMenuAction::ViewDetail => "View Detail",
            ContextMenuAction::AddTransaction => "Add Transaction",
            ContextMenuAction::Delete => "Delete",
            ContextMenuAction::CopySymbol => "Copy Symbol",
        }
    }

    /// Returns the ordered list of context menu actions for positions view.
    /// In percentage mode, transaction-related actions are excluded.
    pub fn for_positions(is_percentage_mode: bool) -> Vec<ContextMenuAction> {
        if is_percentage_mode {
            vec![ContextMenuAction::ViewDetail, ContextMenuAction::CopySymbol]
        } else {
            vec![
                ContextMenuAction::ViewDetail,
                ContextMenuAction::AddTransaction,
                ContextMenuAction::Delete,
                ContextMenuAction::CopySymbol,
            ]
        }
    }
}

/// State for the right-click context menu overlay.
#[derive(Debug, Clone)]
pub struct ContextMenuState {
    /// Screen position (column, row) where the menu should render.
    pub col: u16,
    pub row: u16,
    /// Currently highlighted menu item index.
    pub selected: usize,
    /// Available actions in this menu instance.
    pub actions: Vec<ContextMenuAction>,
    /// Symbol of the position this menu was opened for.
    pub symbol: String,
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
    pub show_drift_columns: bool,
    pub show_sector_grouping: bool,
    pub split_pane_open: bool,
    pub workspace_layout: WorkspaceLayout,

    // Data
    pub transactions: Vec<Transaction>,
    pub allocations: Vec<Allocation>,
    pub positions: Vec<Position>,
    pub prices: HashMap<String, Decimal>,
    pub fx_rates: HashMap<String, Decimal>,
    pub base_currency: String,
    pub allocation_targets: HashMap<String, crate::db::allocation_targets::AllocationTarget>,

    // Price history
    pub price_history: HashMap<String, Vec<HistoryRecord>>,
    pub portfolio_value_history: Vec<(String, Decimal)>,

    // Display (filtered/sorted)
    pub display_positions: Vec<Position>,
    pub display_transactions: Vec<Transaction>,

    // Navigation
    pub selected_index: usize,
    pub selected_symbol: Option<String>,
    pub tx_selected_index: usize,
    pub markets_selected_index: usize,
    pub markets_correlation_window: MarketCorrelationWindow,
    pub economy_selected_index: usize,
    pub watchlist_selected_index: usize,
    pub watchlist_entries: Vec<db_watchlist::WatchlistEntry>,
    pub watchlist_columns: Vec<WatchlistColumn>,
    pub watchlist_active_group: i64,
    watchlist_group_pending: bool,
    pub prediction_markets: Vec<crate::data::predictions::PredictionMarket>,
    pub journal_selected_index: usize,
    pub journal_entries: Vec<crate::db::journal::JournalEntry>,
    pub journal_search_query: String,
    pub news_selected_index: usize,
    pub news_entries: Vec<crate::db::news_cache::NewsEntry>,
    pub news_filter_source: Option<String>,
    pub news_filter_category: Option<String>,
    pub news_search_query: String,
    pub news_preview_expanded: bool,
    pub analytics_selected_index: usize,
    pub analytics_shock_scale_pct: i32,
    pub g_pending: bool,
    pub terminal_height: u16,
    pub terminal_width: u16,

    // Global asset search overlay
    pub search_overlay_open: bool,
    pub search_overlay_query: String,
    pub search_overlay_selected: usize,
    pub search_overlay_requested_symbols: std::collections::HashSet<String>,
    pub command_palette_open: bool,
    pub command_palette_input: String,
    pub command_palette_selected: usize,
    pub scan_builder_open: bool,
    pub scan_builder_mode: ScanBuilderMode,
    pub scan_builder_clause_input: String,
    pub scan_builder_name_input: String,
    pub scan_builder_clauses: Vec<String>,
    pub scan_builder_selected: usize,
    pub scan_builder_message: Option<String>,

    // Sorting
    pub sort_field: SortField,
    pub sort_ascending: bool,

    // Filter
    pub category_filter: Option<AssetCategory>,
    filter_cycle_index: usize,

    // Price service
    price_service: Option<PriceService>,
    pub prices_live: bool,
    pub last_refresh: Option<Instant>,
    auto_refresh_enabled: bool,
    refresh_interval_secs: u64,
    pub chart_sma_periods: Vec<usize>,

    // Totals
    pub total_value: Decimal,
    pub total_cost: Decimal,

    // Theme
    pub theme: Theme,
    pub theme_name: String,

    // Chart
    pub chart_index: usize, // which chart variant to show for current position
    pub chart_timeframe: ChartTimeframe,
    pub chart_render_mode: ChartRenderMode, // Line or Candlestick rendering
    pub change_timeframe: ChangeTimeframe, // timeframe for % change column in positions table
    pub benchmark_overlay: bool, // toggle SPY benchmark overlay on charts
    pub volume_overlay: bool, // toggle volume sub-chart (3-row braille bars)

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

    // Right-click context menu
    pub context_menu: Option<ContextMenuState>,

    // Asset detail popup (opened from search overlay)
    pub asset_detail: Option<crate::tui::views::asset_detail_popup::AssetDetailState>,
    /// Full-screen chart popup opened from search overlay.
    pub search_chart_popup: Option<crate::tui::views::search_chart_popup::SearchChartPopupState>,

    // BLS economic data (CPI, unemployment, NFP, earnings)
    pub bls_data: HashMap<String, crate::data::bls::BlsDataPoint>,
    // Parsed economic indicators from Brave/BLS fallback cache.
    pub economic_data: HashMap<String, crate::db::economic_data::EconomicDataEntry>,

    // World Bank global macro data (GDP growth, debt/GDP, reserves)
    pub worldbank_data: HashMap<(String, String), crate::data::worldbank::WorldBankDataPoint>,

    // Alerts overlay
    pub alerts_open: bool,
    pub alerts_scroll: usize,
    pub triggered_alert_count: usize,

    // Clipboard (OSC 52 pending write)
    pub clipboard_osc52: Option<String>,

    // Portfolio sparkline timeframe
    pub sparkline_timeframe: ChartTimeframe,

    // Crosshair cursor on charts
    pub crosshair_mode: bool,
    pub crosshair_x: usize, // column index within chart width

    /// Regime intelligence — composite risk-on/risk-off score from cross-asset signals.
    pub regime_score: crate::regime::RegimeScore,

    // Sentiment (Fear & Greed indices)
    pub crypto_fng: Option<(u8, String)>, // (value, classification)
    pub traditional_fng: Option<(u8, String)>,

    // Economic calendar
    pub calendar_events: Vec<crate::data::calendar::Event>,

    // Header click targets (column ranges set during render)
    /// Column range for the theme name indicator in the header (for mouse click cycling).
    pub header_theme_col_range: Option<(u16, u16)>,
    /// Column range for the privacy/percentage-view indicator in the header.
    pub header_privacy_col_range: Option<(u16, u16)>,

    // Allocation bar click targets (set during render)
    /// Absolute Rect of the allocation bars widget (for hit-testing mouse clicks).
    pub alloc_bar_area: Option<Rect>,
    /// Ordered list of categories as rendered in the allocation bars (top to bottom).
    /// Index 0 = first bar line inside the block border.
    pub alloc_bar_categories: Vec<AssetCategory>,

    // Timeframe selector click targets (set during portfolio_sparkline render)
    /// List of (ChangeTimeframe, column_range) for each clickable timeframe button.
    pub timeframe_selector_buttons: Vec<(ChangeTimeframe, (u16, u16))>,
    /// Row where the timeframe selector is rendered (absolute screen coordinate).
    pub timeframe_selector_row: Option<u16>,

    // DB
    pub db_path: std::path::PathBuf,
    last_saved_home_tab: ViewMode,

    // Background refresh state
    pub is_background_refreshing: bool,
    background_refresh_complete_rx: Option<std::sync::mpsc::Receiver<()>>,
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

/// Base64-encode a symbol string for OSC 52 clipboard.
fn base64_encode_symbol(symbol: &str) -> String {
    use std::io::Write;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = symbol.as_bytes();
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize]);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize]);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize]);
        } else {
            let _ = out.write_all(b"=");
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize]);
        } else {
            let _ = out.write_all(b"=");
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

pub fn is_privacy_view(app: &App) -> bool {
    app.portfolio_mode == PortfolioMode::Percentage || app.show_percentages_only
}

impl App {
    pub fn new(config: &Config, db_path: std::path::PathBuf) -> Self {
        let initial_view = if config.home_tab == "watchlist" {
            ViewMode::Watchlist
        } else {
            ViewMode::Positions
        };
        App {
            should_quit: false,
            view_mode: initial_view,
            show_help: false,
            help_scroll: 0,
            detail_open: false,
            detail_popup_open: false,
            portfolio_mode: config.portfolio_mode,
            show_percentages_only: config.portfolio_mode == PortfolioMode::Percentage,
            show_drift_columns: false,
            show_sector_grouping: false,
            split_pane_open: false,
            workspace_layout: config.layout,
            transactions: Vec::new(),
            allocations: Vec::new(),
            positions: Vec::new(),
            prices: HashMap::new(),
            fx_rates: HashMap::new(),
            base_currency: config.base_currency.clone(),
            allocation_targets: HashMap::new(),
            price_history: HashMap::new(),
            portfolio_value_history: Vec::new(),
            display_positions: Vec::new(),
            display_transactions: Vec::new(),
            selected_index: 0,
            selected_symbol: None,
            tx_selected_index: 0,
            markets_selected_index: 0,
            markets_correlation_window: MarketCorrelationWindow::ThirtyDay,
            economy_selected_index: 0,
            watchlist_selected_index: 0,
            watchlist_entries: Vec::new(),
            watchlist_columns: config.watchlist.columns.clone(),
            watchlist_active_group: 1,
            watchlist_group_pending: false,
            prediction_markets: Vec::new(),
            journal_selected_index: 0,
            journal_entries: Vec::new(),
            journal_search_query: String::new(),
            news_selected_index: 0,
            news_entries: Vec::new(),
            news_filter_source: None,
            news_filter_category: None,
            news_search_query: String::new(),
            news_preview_expanded: false,
            analytics_selected_index: 0,
            analytics_shock_scale_pct: 100,
            g_pending: false,
            terminal_height: 24, // sensible default, updated on resize
            terminal_width: 120, // sensible default, updated on resize
            search_overlay_open: false,
            search_overlay_query: String::new(),
            search_overlay_selected: 0,
            search_overlay_requested_symbols: std::collections::HashSet::new(),
            command_palette_open: false,
            command_palette_input: String::new(),
            command_palette_selected: 0,
            scan_builder_open: false,
            scan_builder_mode: ScanBuilderMode::Edit,
            scan_builder_clause_input: String::new(),
            scan_builder_name_input: String::new(),
            scan_builder_clauses: Vec::new(),
            scan_builder_selected: 0,
            scan_builder_message: None,
            sort_field: SortField::Allocation,
            sort_ascending: false,
            category_filter: None,
            filter_cycle_index: 0,
            price_service: None,
            prices_live: false,
            last_refresh: None,
            auto_refresh_enabled: config.auto_refresh,
            refresh_interval_secs: config.refresh_interval_secs,
            chart_sma_periods: config.chart_sma.clone(),
            total_value: dec!(0),
            total_cost: dec!(0),
            theme: theme::theme_by_name(&config.theme),
            theme_name: config.theme.clone(),
            chart_index: 0,
            chart_timeframe: ChartTimeframe::ThreeMonths,
            chart_render_mode: ChartRenderMode::Line,
            change_timeframe: ChangeTimeframe::TwentyFourHour,
            benchmark_overlay: false,
            volume_overlay: false,
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
            context_menu: None,
            asset_detail: None,
            search_chart_popup: None,
            alerts_open: false,
            alerts_scroll: 0,
            triggered_alert_count: 0,
            clipboard_osc52: None,
            sparkline_timeframe: ChartTimeframe::ThreeMonths,
            crosshair_mode: false,
            crosshair_x: 0,
            header_theme_col_range: None,
            header_privacy_col_range: None,
            alloc_bar_area: None,
            alloc_bar_categories: Vec::new(),
            timeframe_selector_buttons: Vec::new(),
            timeframe_selector_row: None,
            regime_score: crate::regime::RegimeScore {
                signals: Vec::new(),
                total: 0,
                active_count: 0,
            },
            crypto_fng: None,
            traditional_fng: None,
            calendar_events: Vec::new(),
            bls_data: HashMap::new(),
            economic_data: HashMap::new(),
            worldbank_data: HashMap::new(),
            db_path,
            last_saved_home_tab: initial_view,
            is_background_refreshing: false,
            background_refresh_complete_rx: None,
        }
    }

    /// Initialize app state from cached data only, without starting the price
    /// service or fetching live data. Used by the `snapshot` command.
    pub fn init_offline(&mut self) {
        self.load_data();
        self.load_cached_prices();
        self.load_fx_rates();
        self.load_cached_history();
        self.load_watchlist();
        self.load_journal();
        self.load_predictions();
        self.load_allocation_targets();
        self.load_alerts();
        self.load_sentiment();
        self.load_calendar();
        self.load_bls_data();
        self.load_economic_data();
        self.load_worldbank_data();
        self.recompute();
        self.recompute_regime();
    }

    pub fn init(&mut self) {
        self.load_data();
        self.load_cached_prices();
        self.load_fx_rates();
        self.load_cached_history();
        self.load_watchlist();
        self.load_journal();
        self.load_predictions();
        self.load_allocation_targets();
        self.load_alerts();
        self.load_sentiment();
        self.load_calendar();
        self.load_bls_data();
        self.load_economic_data();
        self.load_worldbank_data();
        self.recompute();
        self.recompute_regime();

        // Start price service
        let config = Config {
            base_currency: self.base_currency.clone(),
            refresh_interval: self.refresh_interval_secs,
            auto_refresh: self.auto_refresh_enabled,
            refresh_interval_secs: self.refresh_interval_secs,
            portfolio_mode: self.portfolio_mode,
            theme: self.theme_name.clone(),
            home_tab: if self.view_mode == ViewMode::Watchlist {
                "watchlist".to_string()
            } else {
                "positions".to_string()
            },
            layout: self.workspace_layout,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: self.chart_sma_periods.clone(),
            watchlist: config::WatchlistConfig::default(),
        };
        let service = PriceService::start(config);
        self.request_price_fetch(&service);
        self.request_all_history(&service);
        self.price_service = Some(service);
        self.request_market_data();
        self.request_economy_data();
        self.request_sentiment_data();

        // Start background refresh
        self.start_background_refresh();
    }

    /// Spawns a background thread to run `pftui refresh` on TUI startup.
    /// Non-blocking — TUI renders immediately from cache, status bar shows "Refreshing..." while in progress.
    fn start_background_refresh(&mut self) {
        let db_path = self.db_path.clone();
        let config = Config {
            base_currency: self.base_currency.clone(),
            refresh_interval: self.refresh_interval_secs,
            auto_refresh: self.auto_refresh_enabled,
            refresh_interval_secs: self.refresh_interval_secs,
            portfolio_mode: self.portfolio_mode,
            theme: self.theme_name.clone(),
            home_tab: if self.view_mode == ViewMode::Watchlist {
                "watchlist".to_string()
            } else {
                "positions".to_string()
            },
            layout: self.workspace_layout,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: self.chart_sma_periods.clone(),
            watchlist: config::WatchlistConfig::default(),
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.background_refresh_complete_rx = Some(rx);
        self.is_background_refreshing = true;

        std::thread::spawn(move || {
            if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                // Run refresh silently (notify=false to avoid desktop notifications)
                let _ = crate::commands::refresh::run(&conn, &config, false);
            }
            // Signal completion (ignore if receiver was dropped)
            let _ = tx.send(());
        });
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

    fn load_fx_rates(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            if let Ok(rates) = crate::db::fx_cache::get_all_fx_rates(&conn) {
                self.fx_rates = rates;
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
            self.watchlist_entries = db_watchlist::list_watchlist_by_group(&conn, self.watchlist_active_group).unwrap_or_default();
        }
    }

    fn load_journal(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            self.journal_entries = crate::db::journal::list_entries(&conn, Some(100), None, None, None, None)
                .unwrap_or_default();
        }
    }

    fn load_news(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            self.news_entries = crate::db::news_cache::get_latest_news(
                &conn,
                100, // limit
                self.news_filter_source.as_deref(),
                self.news_filter_category.as_deref(),
                if self.news_search_query.is_empty() {
                    None
                } else {
                    Some(&self.news_search_query)
                },
                Some(48), // last 48 hours
            )
            .unwrap_or_default();
        }
    }

    fn fetch_asset_brave_news(&mut self, symbol: &str) {
        let cfg = match config::load_config() {
            Ok(c) => c,
            Err(_) => return,
        };
        let key = match cfg.brave_api_key {
            Some(k) if !k.trim().is_empty() => k,
            _ => return,
        };

        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(_) => return,
        };
        let query = format!("{} stock news", symbol);
        let results = match rt.block_on(brave::brave_news_search(&key, &query, Some("pw"), 5)) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Ok(conn) = Connection::open(&self.db_path) {
            for item in &results {
                let source = item.source.as_deref().unwrap_or("Brave");
                let _ = crate::db::news_cache::insert_news_with_source_type(
                    &conn,
                    &item.title,
                    &item.url,
                    source,
                    "brave",
                    Some(symbol),
                    "markets",
                    chrono::Utc::now().timestamp(),
                    Some(&item.description),
                    &item.extra_snippets,
                );
            }
            self.load_news();
        }
    }

    fn load_predictions(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            self.prediction_markets = crate::db::predictions_cache::get_cached_predictions(&conn, 10)
                .unwrap_or_default();
        }
    }

    fn load_allocation_targets(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            if let Ok(targets) = crate::db::allocation_targets::list_targets(&conn) {
                self.allocation_targets = targets
                    .into_iter()
                    .map(|t| (t.symbol.clone(), t))
                    .collect();
            }
        }
    }

    fn load_alerts(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            if let Ok(results) = crate::alerts::engine::check_alerts(&conn) {
                self.triggered_alert_count = results
                    .iter()
                    .filter(|r| r.newly_triggered || r.rule.status == crate::alerts::AlertStatus::Triggered)
                    .count();
            }
        }
    }

    fn load_sentiment(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            if let Ok(Some(reading)) = crate::db::sentiment_cache::get_latest(&conn, "crypto") {
                self.crypto_fng = Some((reading.value, reading.classification));
            }
            if let Ok(Some(reading)) = crate::db::sentiment_cache::get_latest(&conn, "traditional") {
                self.traditional_fng = Some((reading.value, reading.classification));
            }
        }
    }

    fn load_calendar(&mut self) {
        // Fetch 7 days ahead as per spec
        if let Ok(events) = crate::data::calendar::fetch_events(7) {
            self.calendar_events = events;
        }
    }

    fn load_bls_data(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            // Load latest data for each BLS series
            let series_ids = [
                crate::data::bls::SERIES_CPI_U,
                crate::data::bls::SERIES_UNEMPLOYMENT,
                crate::data::bls::SERIES_NFP,
                crate::data::bls::SERIES_HOURLY_EARNINGS,
            ];
            
            for series_id in &series_ids {
                if let Ok(Some(data)) = crate::db::bls_cache::get_latest_bls_data(&conn, series_id) {
                    self.bls_data.insert(series_id.to_string(), data);
                }
            }
        }
    }

    fn load_economic_data(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            if let Ok(entries) = crate::db::economic_data::get_all(&conn) {
                self.economic_data = entries
                    .into_iter()
                    .map(|e| (e.indicator.clone(), e))
                    .collect();
            }
        }
    }

    fn load_worldbank_data(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            self.worldbank_data.clear();
            // Load latest data for tracked countries and indicators
            let countries = [
                crate::data::worldbank::COUNTRY_US,
                crate::data::worldbank::COUNTRY_CHINA,
                crate::data::worldbank::COUNTRY_INDIA,
                crate::data::worldbank::COUNTRY_RUSSIA,
                crate::data::worldbank::COUNTRY_BRAZIL,
            ];
            
            let indicators = [
                crate::data::worldbank::INDICATOR_GDP_GROWTH,
                crate::data::worldbank::INDICATOR_DEBT_GDP,
                crate::data::worldbank::INDICATOR_RESERVES,
            ];

            let load_from_cache = |store: &mut HashMap<(String, String), crate::data::worldbank::WorldBankDataPoint>| {
                for country in &countries {
                    for indicator in &indicators {
                        if let Ok(data_points) = crate::db::worldbank_cache::get_cached_worldbank_data(&conn, &[country], indicator) {
                            // Take the most recent year for this country+indicator
                            if let Some(latest) = data_points.first() {
                                let key = (country.to_string(), indicator.to_string());
                                store.insert(key, latest.clone());
                            }
                        }
                    }
                }
            };

            load_from_cache(&mut self.worldbank_data);

            // Cache miss fallback: fetch once on-demand so Economy global macro
            // panel doesn't stay empty between scheduled refresh runs.
            if self.worldbank_data.is_empty() {
                if let Ok(rt) = tokio::runtime::Runtime::new() {
                    if let Ok(points) = rt.block_on(crate::data::worldbank::fetch_all_indicators()) {
                        if !points.is_empty() {
                            let _ = crate::db::worldbank_cache::upsert_worldbank_data(&conn, &points);
                            load_from_cache(&mut self.worldbank_data);
                        }
                    }
                }
            }
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

    /// Fetch Fear & Greed sentiment indices and cache them.
    fn request_sentiment_data(&mut self) {
        use std::thread;
        use std::time::Duration;
        
        let db_path = self.db_path.clone();
        
        // Spawn thread to fetch sentiment without blocking
        thread::spawn(move || {
            // Fetch crypto F&G
            if let Ok(index) = crate::data::sentiment::fetch_crypto_fng() {
                if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                    let reading = crate::db::sentiment_cache::SentimentReading {
                        index_type: index.index_type,
                        value: index.value,
                        classification: index.classification,
                        timestamp: index.timestamp,
                        fetched_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    };
                    let _ = crate::db::sentiment_cache::upsert_reading(&conn, &reading);
                }
            }
            
            // Fetch traditional F&G
            thread::sleep(Duration::from_millis(500)); // rate limiting
            if let Ok(index) = crate::data::sentiment::fetch_traditional_fng() {
                if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                    let reading = crate::db::sentiment_cache::SentimentReading {
                        index_type: index.index_type,
                        value: index.value,
                        classification: index.classification,
                        timestamp: index.timestamp,
                        fetched_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    };
                    let _ = crate::db::sentiment_cache::upsert_reading(&conn, &reading);
                }
            }
        });
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
            // Also fetch SPY benchmark when overlay is enabled
            if self.benchmark_overlay {
                self.request_history_if_needed("^GSPC", AssetCategory::Equity, needed_days);
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
            // Update selected_symbol to propagate selection across views
            self.selected_symbol = self.display_positions.get(self.selected_index).map(|p| p.symbol.clone());
            // Load saved chart timeframe for this symbol
            self.load_chart_timeframe();
            self.refetch_chart_history();
        }
    }

    /// Load saved chart timeframe for currently selected symbol
    fn load_chart_timeframe(&mut self) {
        if let Some(pos) = self.selected_position() {
            if let Ok(conn) = Connection::open(&self.db_path) {
                if let Ok(Some(saved_tf)) = crate::db::chart_state::load_timeframe(&conn, &pos.symbol) {
                    if let Some(tf) = ChartTimeframe::from_label(&saved_tf) {
                        self.chart_timeframe = tf;
                    }
                }
            }
        }
    }

    /// Save current chart timeframe for currently selected symbol
    fn save_chart_timeframe(&mut self) {
        if let Some(pos) = self.selected_position() {
            if let Ok(conn) = Connection::open(&self.db_path) {
                let _ = crate::db::chart_state::save_timeframe(&conn, &pos.symbol, self.chart_timeframe.label());
            }
        }
    }

    pub fn recompute(&mut self) {
        match self.portfolio_mode {
            PortfolioMode::Full => {
                self.positions = compute_positions(&self.transactions, &self.prices, &self.fx_rates);
            }
            PortfolioMode::Percentage => {
                self.positions =
                    compute_positions_from_allocations(&self.allocations, &self.prices, &self.fx_rates);
            }
        }
        self.apply_filter_and_sort();
        self.compute_totals();
        self.compute_prev_day_cat_allocations();
        self.last_value_update_tick = self.tick_count;
    }

    /// Recompute regime score from current prices and history data.
    pub fn recompute_regime(&mut self) {
        self.regime_score = crate::regime::compute_regime(&self.prices, &self.price_history);
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
            let clamped = self.selected_index.min(self.display_positions.len() - 1);
            self.set_selected_index(clamped);
        } else {
            self.set_selected_index(0);
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

    fn selected_watchlist_entry(&self) -> Option<&db_watchlist::WatchlistEntry> {
        self.watchlist_entries.get(self.watchlist_selected_index)
    }

    /// Update selected_index and sync selected_symbol to the position at that index.
    pub fn set_selected_index(&mut self, new_index: usize) {
        self.selected_index = new_index;
        self.selected_symbol = self.display_positions.get(new_index).map(|p| p.symbol.clone());
    }

    fn home_views(&self) -> (ViewMode, ViewMode) {
        if self.last_saved_home_tab == ViewMode::Watchlist {
            (ViewMode::Watchlist, ViewMode::Positions)
        } else {
            (ViewMode::Positions, ViewMode::Watchlist)
        }
    }

    fn switch_to_home_default(&mut self) {
        let (default_view, _) = self.home_views();
        self.view_mode = default_view;
        self.detail_open = false;
        self.detail_popup_open = false;
        if matches!(self.view_mode, ViewMode::Watchlist) {
            self.load_watchlist();
            self.request_watchlist_data();
        }
    }

    fn toggle_home_subtab(&mut self) {
        let (default_view, secondary_view) = self.home_views();
        self.view_mode = if self.view_mode == default_view {
            secondary_view
        } else {
            default_view
        };
        self.detail_open = false;
        self.detail_popup_open = false;
        if matches!(self.view_mode, ViewMode::Watchlist) {
            self.load_watchlist();
            self.request_watchlist_data();
        }
    }

    fn set_watchlist_group(&mut self, group_id: i64) {
        self.watchlist_active_group = crate::db::watchlist_groups::clamp_group_id(group_id);
        self.load_watchlist();
        self.request_watchlist_data();
        self.watchlist_group_pending = false;
    }

    fn watchlist_inline_open_chart(&mut self) {
        let Some(entry) = self.selected_watchlist_entry().cloned() else {
            return;
        };
        let category: AssetCategory = entry.category.parse().unwrap_or(AssetCategory::Equity);
        let symbol = watchlist_view::yahoo_symbol_for(&entry.symbol, category);

        if let Some(svc) = &self.price_service {
            if !self.prices.contains_key(&symbol) {
                svc.send_command(PriceCommand::FetchAll(vec![(symbol.clone(), category)]));
            }
            if !self.price_history.contains_key(&symbol) {
                svc.send_command(PriceCommand::FetchHistory(
                    symbol.clone(),
                    category,
                    370,
                ));
            }
        }
        self.search_chart_popup = Some(
            crate::tui::views::search_chart_popup::SearchChartPopupState { symbol },
        );
    }

    fn watchlist_inline_remove(&mut self) {
        let Some(entry) = self.selected_watchlist_entry().cloned() else {
            return;
        };
        if let Ok(conn) = Connection::open(&self.db_path) {
            let _ = db_watchlist::remove_from_watchlist(&conn, &entry.symbol);
        }
        self.load_watchlist();
        if self.watchlist_selected_index >= self.watchlist_entries.len() && self.watchlist_selected_index > 0 {
            self.watchlist_selected_index -= 1;
        }
    }

    fn watchlist_inline_add_alert(&mut self) {
        let Some(entry) = self.selected_watchlist_entry().cloned() else {
            return;
        };
        let category: AssetCategory = entry.category.parse().unwrap_or(AssetCategory::Equity);
        let alert_symbol = watchlist_view::yahoo_symbol_for(&entry.symbol, category);

        let (direction, threshold) = if let (Some(tp), Some(dir)) = (entry.target_price.clone(), entry.target_direction.clone()) {
            (dir, tp)
        } else {
            let Some(price) = self.prices.get(&alert_symbol).copied() else {
                return;
            };
            let above = (price * dec!(1.05)).round_dp(2);
            ("above".to_string(), above.to_string())
        };
        let rule_text = format!("{} {} {}", alert_symbol, direction, threshold);

        if let Ok(conn) = Connection::open(&self.db_path) {
            let _ = crate::db::alerts::add_alert(
                &conn,
                "price",
                &alert_symbol,
                &direction,
                &threshold,
                &rule_text,
            );
            self.load_alerts();
        }
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
            ViewMode::Analytics => "Analytics",
            ViewMode::News => "News",
            ViewMode::Journal => "Journal",
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
                ChartVariant::ratio("DXY/Gold", "DX-Y.NYB", AssetCategory::Forex, "GC=F", AssetCategory::Commodity),
                ChartVariant::ratio("DXY/SPX", "DX-Y.NYB", AssetCategory::Forex, "^GSPC", AssetCategory::Equity),
                ChartVariant::ratio("DXY/BTC", "DX-Y.NYB", AssetCategory::Forex, "BTC-USD", AssetCategory::Equity),
            ]
        } else if is_cash {
            let pair = format!("{}USD=X", sym);
            let pair_label = format!("{}/USD", sym);
            vec![
                ChartVariant::single(&pair, &pair_label, AssetCategory::Forex),
                ChartVariant::ratio(&format!("{}/DXY", sym), &pair, AssetCategory::Forex, "DX-Y.NYB", AssetCategory::Forex),
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
                    // Equity, Fund, non-Gold Commodity: {SYM}/SPX, {SYM}/QQQ, {SYM}/BTC
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
                    if !is_btc {
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
        self.persist_home_tab_preference_if_needed();

        // Check background refresh completion
        if self.is_background_refreshing {
            if let Some(ref rx) = self.background_refresh_complete_rx {
                if rx.try_recv().is_ok() {
                    self.is_background_refreshing = false;
                    // Reload all data after refresh
                    self.load_cached_prices();
                    self.load_cached_history();
                    self.load_watchlist();
                    self.load_predictions();
                    self.load_sentiment();
                    self.load_calendar();
                    self.load_bls_data();
                    self.load_economic_data();
                    self.load_worldbank_data();
                    self.recompute();
                    self.recompute_regime();
                }
            }
        }

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
            if updated || history_updated {
                if updated {
                    self.recompute();
                    self.load_alerts(); // re-check alerts after price update
                }
                if history_updated && self.portfolio_mode == PortfolioMode::Full {
                    self.compute_portfolio_value_history();
                }
                self.recompute_regime();
            }
        }

        if self.auto_refresh_enabled {
            if let Some(last) = self.last_refresh {
                if last.elapsed().as_secs() >= self.refresh_interval_secs {
                    self.force_refresh();
                }
            }
        }
    }

    fn persist_home_tab_preference_if_needed(&mut self) {
        let desired = match self.view_mode {
            ViewMode::Positions => Some(ViewMode::Positions),
            ViewMode::Watchlist => Some(ViewMode::Watchlist),
            _ => None,
        };
        let Some(desired) = desired else { return; };
        if desired == self.last_saved_home_tab {
            return;
        }

        if let Ok(mut cfg) = config::load_config() {
            cfg.home_tab = if desired == ViewMode::Watchlist {
                "watchlist".to_string()
            } else {
                "positions".to_string()
            };
            let _ = config::save_config(&cfg);
            self.last_saved_home_tab = desired;
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
        self.request_sentiment_data();
        self.load_sentiment(); // reload from cache after background fetch
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Search chart popup (sits on top of search overlay)
        if self.search_chart_popup.is_some() {
            self.handle_search_chart_popup_key(key);
            return;
        }

        // Asset detail popup (legacy, sits on top of search overlay)
        if self.asset_detail.is_some() {
            self.handle_asset_detail_key(key);
            return;
        }

        if self.command_palette_open {
            self.handle_command_palette_key(key);
            return;
        }

        if self.scan_builder_open {
            self.handle_scan_builder_key(key);
            return;
        }

        // Global asset search overlay (must be checked first)
        if self.search_overlay_open {
            self.handle_search_overlay_key(key);
            return;
        }

        // Context menu mode
        if self.context_menu.is_some() {
            self.handle_context_menu_key(key);
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
            KeyCode::Char(':') => {
                self.open_command_palette();
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
            KeyCode::Esc if self.alerts_open => {
                self.alerts_open = false;
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

        if self.alerts_open {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.alerts_scroll = self.alerts_scroll.saturating_add(1);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.alerts_scroll = self.alerts_scroll.saturating_sub(1);
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.alerts_scroll = self.alerts_scroll.saturating_add(self.half_page());
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.alerts_scroll = self.alerts_scroll.saturating_sub(self.half_page());
                }
                KeyCode::Char('G') => {
                    self.alerts_scroll = usize::MAX;
                }
                _ => {
                    if key.code == KeyCode::Char('g') {
                        if self.g_pending {
                            self.alerts_scroll = 0;
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

        // Handle watchlist group key chord: W then 1/2/3
        if self.watchlist_group_pending {
            self.watchlist_group_pending = false;
            match key.code {
                KeyCode::Char('1') => {
                    self.set_watchlist_group(1);
                    self.view_mode = ViewMode::Watchlist;
                    return;
                }
                KeyCode::Char('2') => {
                    self.set_watchlist_group(2);
                    self.view_mode = ViewMode::Watchlist;
                    return;
                }
                KeyCode::Char('3') => {
                    self.set_watchlist_group(3);
                    self.view_mode = ViewMode::Watchlist;
                    return;
                }
                _ => {}
            }
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
                self.switch_to_home_default();
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
            KeyCode::Char('5') | KeyCode::Char('w') => {
                self.view_mode = ViewMode::Watchlist;
                self.detail_open = false;
                self.detail_popup_open = false;
                self.load_watchlist();
                self.request_watchlist_data();
            }
            KeyCode::Char('W') => {
                self.view_mode = ViewMode::Watchlist;
                self.watchlist_group_pending = true;
                self.detail_open = false;
                self.detail_popup_open = false;
            }
            KeyCode::Char('6') => {
                self.view_mode = ViewMode::Analytics;
                self.detail_open = false;
                self.detail_popup_open = false;
            }
            KeyCode::Char('7') => {
                self.view_mode = ViewMode::News;
                self.detail_open = false;
                self.detail_popup_open = false;
                self.load_news();
            }
            KeyCode::Char('8') => {
                self.view_mode = ViewMode::Journal;
                self.detail_open = false;
                self.detail_popup_open = false;
                self.load_journal();
            }

            // Alerts overlay toggle (Ctrl+A)
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.alerts_open = !self.alerts_open;
                if self.alerts_open {
                    self.alerts_scroll = 0;
                    self.load_alerts(); // refresh alerts when opening
                }
            }

            // Privacy toggle
            KeyCode::Char('p') => {
                if self.portfolio_mode == PortfolioMode::Full {
                    self.show_percentages_only = !self.show_percentages_only;
                }
            }

            // Drift columns toggle
            KeyCode::Char('D') => {
                self.show_drift_columns = !self.show_drift_columns;
            }

            // Category grouping toggle (group headers with aggregate alloc/perf)
            KeyCode::Char('Z') if matches!(self.view_mode, ViewMode::Positions) => {
                self.show_sector_grouping = !self.show_sector_grouping;
                if self.show_sector_grouping {
                    self.sort_field = SortField::Category;
                    self.sort_ascending = true;
                    self.last_sort_change_tick = self.tick_count;
                }
                self.recompute();
            }

            // Split-pane toggle (bottom 30% detail pane for selected position)
            KeyCode::Char('S') if matches!(self.view_mode, ViewMode::Positions) => {
                self.split_pane_open = !self.split_pane_open;
            }

            // Change timeframe cycling (updates both table % and portfolio chart)
            KeyCode::Char('T') if matches!(self.view_mode, ViewMode::Positions) => {
                self.change_timeframe = self.change_timeframe.next();
                // Sync portfolio chart timeframe to match
                self.sparkline_timeframe = match self.change_timeframe {
                    ChangeTimeframe::OneHour => ChartTimeframe::OneWeek, // 1h fits in 1W context
                    ChangeTimeframe::TwentyFourHour => ChartTimeframe::OneWeek, // 24h = 1D fits in 1W
                    ChangeTimeframe::SevenDay => ChartTimeframe::OneMonth, // 7d fits in 1M
                    ChangeTimeframe::ThirtyDay => ChartTimeframe::ThreeMonths, // 30d fits in 3M
                    ChangeTimeframe::YearToDate => ChartTimeframe::OneYear, // YTD uses 1Y view
                };
            }
            // Markets correlation window (7d/30d/90d)
            KeyCode::Char('M') if matches!(self.view_mode, ViewMode::Markets) => {
                self.markets_correlation_window = self.markets_correlation_window.next();
            }

            // Detail popup toggle (chart is always visible in right pane)
            KeyCode::Enter if matches!(self.view_mode, ViewMode::Positions) => {
                if let Some(pos) = self.selected_position().cloned() {
                    self.detail_popup_open = !self.detail_popup_open;
                    if self.detail_popup_open {
                        self.fetch_asset_brave_news(&pos.symbol);
                    }
                }
            }

            // Home sub-tabs (default + secondary)
            KeyCode::Tab | KeyCode::Left | KeyCode::Right
                if matches!(self.view_mode, ViewMode::Positions | ViewMode::Watchlist) =>
            {
                self.toggle_home_subtab();
            }

            // Toggle rich preview in News view
            KeyCode::Enter if matches!(self.view_mode, ViewMode::News) => {
                self.news_preview_expanded = !self.news_preview_expanded;
            }

            // Open selected news URL in browser
            KeyCode::Char('o') if matches!(self.view_mode, ViewMode::News) => {
                if self.news_selected_index < self.news_entries.len() {
                    let url = &self.news_entries[self.news_selected_index].url;
                    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
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
            // Chart render mode toggle with C (Positions view only)
            KeyCode::Char('C') if matches!(self.view_mode, ViewMode::Positions) => {
                self.chart_render_mode = self.chart_render_mode.toggle();
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
            KeyCode::Char('h') if matches!(self.view_mode, ViewMode::Positions) => {
                if self.crosshair_mode {
                    self.crosshair_x = self.crosshair_x.saturating_sub(1);
                } else {
                    self.chart_timeframe = self.chart_timeframe.prev();
                    self.save_chart_timeframe();
                    self.refetch_chart_history();
                }
            }
            KeyCode::Char('l') if matches!(self.view_mode, ViewMode::Positions) => {
                if self.crosshair_mode {
                    self.crosshair_x = self.crosshair_x.saturating_add(1);
                    // clamped during render
                } else {
                    self.chart_timeframe = self.chart_timeframe.next();
                    self.save_chart_timeframe();
                    self.refetch_chart_history();
                }
            }

            // Analytics scenario scaling
            KeyCode::Char('+') | KeyCode::Char('=') if matches!(self.view_mode, ViewMode::Analytics) => {
                self.analytics_shock_scale_pct = (self.analytics_shock_scale_pct + 5).min(200);
            }
            KeyCode::Char('-') if matches!(self.view_mode, ViewMode::Analytics) => {
                self.analytics_shock_scale_pct = (self.analytics_shock_scale_pct - 5).max(0);
            }
            KeyCode::Char('0') if matches!(self.view_mode, ViewMode::Analytics) => {
                self.analytics_shock_scale_pct = 100;
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

            // Watchlist inline actions
            KeyCode::Char('a') if matches!(self.view_mode, ViewMode::Watchlist) => {
                self.watchlist_inline_add_alert();
            }
            KeyCode::Char('c') if matches!(self.view_mode, ViewMode::Watchlist) => {
                self.watchlist_inline_open_chart();
            }
            KeyCode::Char('r') if matches!(self.view_mode, ViewMode::Watchlist) => {
                self.watchlist_inline_remove();
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
            KeyCode::Char('G') if matches!(self.view_mode, ViewMode::Positions) => {
                self.show_sector_grouping = true;
                self.sort_field = SortField::Category;
                self.sort_ascending = true;
                self.last_sort_change_tick = self.tick_count;
                self.recompute();
            }
            KeyCode::Char('G') => {
                self.jump_to_bottom();
            }
            KeyCode::End => {
                self.jump_to_bottom();
            }
            KeyCode::Char('A') if matches!(self.view_mode, ViewMode::Positions) => {
                self.show_sector_grouping = false;
                self.sort_field = SortField::Allocation;
                self.sort_ascending = false;
                self.last_sort_change_tick = self.tick_count;
                self.recompute();
            }
            KeyCode::Char('P') if matches!(self.view_mode, ViewMode::Positions) => {
                if !is_privacy_view(self) {
                    self.show_sector_grouping = false;
                    self.sort_field = SortField::GainPct;
                    self.sort_ascending = false;
                    self.last_sort_change_tick = self.tick_count;
                    self.recompute();
                }
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

            // Global asset search overlay
            KeyCode::Char('/') => {
                self.search_overlay_open = true;
                self.search_overlay_query.clear();
                self.search_overlay_selected = 0;
                self.search_overlay_requested_symbols.clear();
                self.search_chart_popup = None;
                self.command_palette_open = false;
                self.command_palette_input.clear();
                self.command_palette_selected = 0;
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

            // Add transaction (i) — opens inline form for selected position
            KeyCode::Char('i') if matches!(self.view_mode, ViewMode::Positions) => {
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

            // Toggle benchmark overlay on chart (Shift+B)
            KeyCode::Char('B') if matches!(self.view_mode, ViewMode::Positions) => {
                self.benchmark_overlay = !self.benchmark_overlay;
            }

            // Toggle volume sub-chart (Shift+V)
            KeyCode::Char('V') if matches!(self.view_mode, ViewMode::Positions) => {
                self.volume_overlay = !self.volume_overlay;
            }

            _ => {}
        }
    }

    /// Handle mouse events: scroll wheel, tab clicks, row selection.
    ///
    /// Layout is recomputed from terminal dimensions and the same constants
    /// used in `ui.rs` and `widgets/header.rs`, so hit-testing stays in sync
    /// without storing mutable rects.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        match mouse.kind {
            // Scroll wheel — navigate up/down in current view
            MouseEventKind::ScrollUp => {
                // Dismiss overlays with scroll if active
                if self.show_help
                    || self.command_palette_open
                    || self.scan_builder_open
                    || self.search_overlay_open
                    || self.detail_popup_open
                {
                    return;
                }
                self.move_up();
            }
            MouseEventKind::ScrollDown => {
                if self.show_help
                    || self.command_palette_open
                    || self.scan_builder_open
                    || self.search_overlay_open
                    || self.detail_popup_open
                {
                    return;
                }
                self.move_down();
            }

            MouseEventKind::Down(MouseButton::Left) => {
                // If context menu is open, click outside to dismiss
                if self.context_menu.is_some() {
                    self.context_menu = None;
                    return;
                }
                // If help overlay is open, click anywhere to dismiss
                if self.show_help {
                    self.show_help = false;
                    return;
                }
                // If command palette is open, click anywhere to dismiss
                if self.command_palette_open {
                    self.command_palette_open = false;
                    self.command_palette_input.clear();
                    self.command_palette_selected = 0;
                    return;
                }
                if self.scan_builder_open {
                    self.scan_builder_open = false;
                    self.scan_builder_clause_input.clear();
                    self.scan_builder_name_input.clear();
                    self.scan_builder_message = None;
                    self.scan_builder_mode = ScanBuilderMode::Edit;
                    return;
                }
                // If search overlay is open, click outside to dismiss
                if self.search_overlay_open {
                    self.search_overlay_open = false;
                    self.search_overlay_query.clear();
                    self.search_overlay_selected = 0;
                    self.search_overlay_requested_symbols.clear();
                    self.search_chart_popup = None;
                    return;
                }
                // If detail popup is open, click outside to dismiss
                if self.detail_popup_open {
                    self.detail_popup_open = false;
                    return;
                }

                // Compute header height (same logic as widgets/header.rs)
                let compact = self.terminal_width < crate::tui::ui::COMPACT_WIDTH;
                let header_h = if !compact
                    && matches!(self.view_mode, ViewMode::Positions | ViewMode::Watchlist)
                {
                    3u16
                } else {
                    2u16
                };

                // Click in header area → tab switching
                if row < header_h {
                    self.handle_header_click(col);
                    return;
                }

                // Click in status bar (last 2 rows) → ignore for now
                if row >= self.terminal_height.saturating_sub(2) {
                    return;
                }

                // Click in allocation bars → category filter
                if self.handle_alloc_bar_click(col, row) {
                    return;
                }

                // Click in timeframe selector → change timeframe
                if self.handle_timeframe_selector_click(col, row) {
                    return;
                }

                // Click in main content area → row selection
                let content_y = row.saturating_sub(header_h);
                self.handle_content_click(col, content_y);
            }

            MouseEventKind::Down(MouseButton::Right) => {
                // Dismiss context menu if already open
                if self.context_menu.is_some() {
                    self.context_menu = None;
                    return;
                }

                // Only open context menu in Positions view on data rows
                if !matches!(self.view_mode, ViewMode::Positions) {
                    return;
                }

                // Don't open over other overlays
                if self.show_help || self.search_overlay_open || self.detail_popup_open {
                    return;
                }

                // Compute header height
                let compact = self.terminal_width < crate::tui::ui::COMPACT_WIDTH;
                let header_h = if !compact
                    && matches!(self.view_mode, ViewMode::Positions | ViewMode::Watchlist)
                {
                    3u16
                } else {
                    2u16
                };

                // Must be in content area
                if row < header_h || row >= self.terminal_height.saturating_sub(2) {
                    return;
                }

                let content_y = row.saturating_sub(header_h);
                let wide = self.terminal_width >= crate::tui::ui::COMPACT_WIDTH;
                let data_start: u16 = if wide { 3 } else { 2 };
                if content_y < data_start {
                    return;
                }

                let clicked_row = (content_y - data_start) as usize;
                if clicked_row >= self.display_positions.len() {
                    return;
                }

                // Select the clicked row first
                let old_idx = self.selected_index;
                self.selected_index = clicked_row;
                if self.selected_index != old_idx {
                    self.on_position_selection_changed();
                }

                // Open context menu at click position
                let symbol = self.display_positions[clicked_row].symbol.clone();
                let is_pct = self.portfolio_mode == PortfolioMode::Percentage;
                let actions = ContextMenuAction::for_positions(is_pct);
                self.context_menu = Some(ContextMenuState {
                    col,
                    row,
                    selected: 0,
                    actions,
                    symbol,
                });
            }

            _ => {}
        }
    }

    /// Handle a click in the header area. Detect which tab label was clicked.
    ///
    /// Tab layout (non-compact): " pftui  [1]Pos [2]Tx [3]Mkt [4]Econ [5]Watch [6]Analytics [7]News [8]Journal ..."
    /// Character offsets are approximate; we use generous hit zones.
    fn handle_header_click(&mut self, col: u16) {
        let col = col as usize;
        let compact = self.terminal_width < crate::tui::ui::COMPACT_WIDTH;
        let pct_mode = self.portfolio_mode == PortfolioMode::Percentage;

        // Tab label positions (0-indexed character columns).
        // Layout: " pf" (3) + "tui" (3) + "  " (2) = 8 chars before [1]
        // [1]Pos  = cols ~8..14
        // [2]Tx   = cols ~15..20 (hidden in pct mode)
        // [3]Mkt  = cols ~21..27
        // [4]Econ = cols ~28..35
        // [5]Watch = cols ~36..45
        //
        // These are rough ranges — generous to make clicking easy.
        // When [2]Tx is hidden, subsequent tabs shift left by ~5 chars.

        let tx_visible = !pct_mode;

        // [1]Pos — always starts around col 8
        if (8..14).contains(&col) {
            self.switch_to_home_default();
            return;
        }

        // [2]Tx — only in full mode
        if tx_visible && (15..20).contains(&col) {
            self.view_mode = ViewMode::Transactions;
            self.detail_open = false;
            self.detail_popup_open = false;
            return;
        }

        // Offset for remaining tabs depends on whether [2] is visible
        let base = if tx_visible { 20 } else { 15 };

        // [3]Mkt
        if (base..base + 7).contains(&col) {
            self.view_mode = ViewMode::Markets;
            self.detail_open = false;
            self.detail_popup_open = false;
            self.request_market_data();
            return;
        }

        let econ_label_len: usize = if compact { 5 } else { 7 }; // "[4]Ec" or "[4]Econ"
        let base2 = base + 7;

        // [4]Econ
        if (base2..base2 + econ_label_len + 1).contains(&col) {
            self.view_mode = ViewMode::Economy;
            self.detail_open = false;
            self.detail_popup_open = false;
            self.request_economy_data();
            return;
        }

        let base3 = base2 + econ_label_len + 1;
        let watch_label_len: usize = if compact { 4 } else { 8 }; // "[5]W" or "[5]Watch"

        // [5]Watch
        if (base3..base3 + watch_label_len + 1).contains(&col) {
            self.view_mode = ViewMode::Watchlist;
            self.detail_open = false;
            self.detail_popup_open = false;
            self.load_watchlist();
            self.request_watchlist_data();
            return;
        }

        let base4 = base3 + watch_label_len + 1;
        let analytics_label_len: usize = if compact { 5 } else { 12 }; // "[6]An" or "[6]Analytics"
        if (base4..base4 + analytics_label_len + 1).contains(&col) {
            self.view_mode = ViewMode::Analytics;
            self.detail_open = false;
            self.detail_popup_open = false;
            return;
        }

        let base5 = base4 + analytics_label_len + 1;
        let news_label_len: usize = if compact { 4 } else { 7 }; // "[7]N" or "[7]News"
        if (base5..base5 + news_label_len + 1).contains(&col) {
            self.view_mode = ViewMode::News;
            self.detail_open = false;
            self.detail_popup_open = false;
            self.load_news();
            return;
        }

        let base6 = base5 + news_label_len + 1;
        let journal_label_len: usize = if compact { 4 } else { 10 }; // "[8]J" or "[8]Journal"
        if (base6..base6 + journal_label_len + 1).contains(&col) {
            self.view_mode = ViewMode::Journal;
            self.detail_open = false;
            self.detail_popup_open = false;
            self.load_journal();
            return;
        }

        // Theme indicator click — cycle theme (non-compact only)
        if let Some((start, end)) = self.header_theme_col_range {
            if (start as usize..end as usize).contains(&col) {
                self.cycle_theme();
                return;
            }
        }

        // Privacy/percentage-view indicator click — toggle privacy mode
        if let Some((start, end)) = self.header_privacy_col_range {
            if (start as usize..end as usize).contains(&col)
                && self.portfolio_mode == PortfolioMode::Full
            {
                self.show_percentages_only = !self.show_percentages_only;
            }
        }
    }

    /// Handle a click on the allocation bars widget.
    /// Returns `true` if the click was consumed (hit an allocation bar).
    ///
    /// The allocation bars widget is a Block with Borders::ALL.
    /// Inside the block, each line corresponds to one category bar, in the
    /// same order as `alloc_bar_categories`.
    /// Clicking a bar sets the category filter to that category.
    /// Clicking the already-active filter category clears the filter.
    fn handle_alloc_bar_click(&mut self, col: u16, row: u16) -> bool {
        // Only in Positions view
        if !matches!(self.view_mode, ViewMode::Positions) {
            return false;
        }

        let area = match self.alloc_bar_area {
            Some(a) => a,
            None => return false,
        };

        // Check if click is within the allocation bars widget area
        if col < area.x || col >= area.x + area.width || row < area.y || row >= area.y + area.height
        {
            return false;
        }

        // Skip top border row
        if row <= area.y {
            return false;
        }

        // Row within the block: subtract area.y and 1 for top border
        let inner_row = (row - area.y - 1) as usize;

        if inner_row < self.alloc_bar_categories.len() {
            let clicked_cat = self.alloc_bar_categories[inner_row];
            if self.category_filter == Some(clicked_cat) {
                // Already filtering this category — clear filter
                self.category_filter = None;
                self.filter_cycle_index = 0;
            } else {
                self.category_filter = Some(clicked_cat);
                // Sync filter_cycle_index with the category position in AssetCategory::all()
                let all_cats = AssetCategory::all();
                self.filter_cycle_index = all_cats
                    .iter()
                    .position(|c| *c == clicked_cat)
                    .unwrap_or(0);
            }
            self.recompute();
            return true;
        }

        false
    }

    /// Handle a click in the timeframe selector bar above the portfolio chart.
    /// Returns true if the click was handled.
    fn handle_timeframe_selector_click(&mut self, col: u16, row: u16) -> bool {
        // Only in Positions view
        if !matches!(self.view_mode, ViewMode::Positions) {
            return false;
        }

        // Check if we have a timeframe selector row set
        let selector_row = match self.timeframe_selector_row {
            Some(r) => r,
            None => return false,
        };

        // Must click on the selector row
        if row != selector_row {
            return false;
        }

        // Check each button's column range
        for &(timeframe, (col_start, col_end)) in &self.timeframe_selector_buttons {
            if col >= col_start && col <= col_end {
                // Clicked this button - update both change_timeframe and sparkline_timeframe
                self.change_timeframe = timeframe;
                // Sync portfolio chart timeframe to match
                self.sparkline_timeframe = match timeframe {
                    ChangeTimeframe::OneHour => ChartTimeframe::OneWeek,
                    ChangeTimeframe::TwentyFourHour => ChartTimeframe::OneWeek,
                    ChangeTimeframe::SevenDay => ChartTimeframe::OneMonth,
                    ChangeTimeframe::ThirtyDay => ChartTimeframe::ThreeMonths,
                    ChangeTimeframe::YearToDate => ChartTimeframe::OneYear,
                };
                return true;
            }
        }

        false
    }

    /// Handle a click in the main content area. `content_y` is relative to
    /// the top of the content area (below header).
    fn handle_content_click(&mut self, col: u16, content_y: u16) {
        // In list views, each row in the table has:
        //   Row 0: section header (SECTION_HEADER_HEIGHT = 1 in wide mode)
        //   Row 1: table top border
        //   Row 2: table header row
        //   Row 3+: data rows
        //
        // The exact offsets vary by view and layout. For simplicity we use
        // the common case: section header (1) + border (1) + header row (1)
        // = data starts at content_y == 3.

        let wide = self.terminal_width >= crate::tui::ui::COMPACT_WIDTH;
        let data_start: u16 = if wide {
            // section header (1) + top border (1) + column header (1) = 3
            3
        } else {
            // no section header; top border (1) + column header (1) = 2
            2
        };

        // Check if click is on the column header row (one row before data start)
        let header_row = data_start.saturating_sub(1);
        if content_y == header_row && matches!(self.view_mode, ViewMode::Positions) {
            self.handle_column_header_click(col);
            return;
        }

        if content_y < data_start {
            return;
        }

        let clicked_row = (content_y - data_start) as usize;
        let old_pos_idx = self.selected_index;

        match self.view_mode {
            ViewMode::Positions => {
                if clicked_row < self.display_positions.len() {
                    self.selected_index = clicked_row;
                    if self.selected_index != old_pos_idx {
                        self.on_position_selection_changed();
                    }
                }
            }
            ViewMode::Watchlist => {
                if clicked_row < self.watchlist_entries.len() {
                    self.watchlist_selected_index = clicked_row;
                }
            }
            ViewMode::Transactions => {
                if clicked_row < self.display_transactions.len() {
                    self.tx_selected_index = clicked_row;
                }
            }
            ViewMode::Markets => {
                let count = markets::market_symbols().len();
                if clicked_row < count {
                    self.markets_selected_index = clicked_row;
                }
            }
            ViewMode::Economy => {
                let count = economy::economy_symbols().len();
                if clicked_row < count {
                    self.economy_selected_index = clicked_row;
                }
            }
            ViewMode::Analytics => {
                let count = self.analytics_scenario_count();
                if clicked_row < count {
                    self.analytics_selected_index = clicked_row;
                }
            }
            ViewMode::News => {
                if clicked_row < self.news_entries.len() {
                    self.news_selected_index = clicked_row;
                }
            }
            ViewMode::Journal => {
                if clicked_row < self.journal_entries.len() {
                    self.journal_selected_index = clicked_row;
                }
            }
        }
    }

    /// Handle a click on the column header row in the Positions view.
    /// Maps the clicked column to a SortField and toggles sort direction
    /// if clicking the already-active sort column.
    fn handle_column_header_click(&mut self, col: u16) {
        let wide = self.terminal_width >= crate::tui::ui::COMPACT_WIDTH;
        let privacy = is_privacy_view(self);

        // Compute the table content area start X.
        // In wide mode, the positions table is in the left 57% panel.
        // The table Block has Borders::ALL, so content starts 1 cell in.
        let table_area_width = if wide {
            (self.terminal_width * 57) / 100
        } else {
            self.terminal_width
        };
        // Table content starts at x=1 (left border), ends at table_area_width-2
        let content_start_x: u16 = 1;
        let content_width = table_area_width.saturating_sub(2); // minus left+right border

        // Column widths and their sort field mappings.
        // Must match the constraints in positions.rs render_full_table / render_privacy_table.
        // ratatui default column_spacing = 1.
        let (col_widths, sort_fields): (Vec<u16>, Vec<Option<SortField>>) = if privacy {
            // Privacy: Asset, Price, Day%, Alloc%, RSI, Trend
            let fixed: u16 = 12 + 7 + 8 + 6 + 8; // all fixed-width columns
            let gaps: u16 = 5; // 6 columns → 5 gaps
            let asset_w = content_width.saturating_sub(fixed + gaps).max(18);
            (
                vec![asset_w, 12, 7, 8, 6, 8],
                vec![
                    Some(SortField::Name),       // Asset
                    None,                         // Price (no sort field)
                    None,                         // Day% (no sort field)
                    Some(SortField::Allocation),  // Alloc%
                    None,                         // RSI
                    None,                         // Trend
                ],
            )
        } else {
            // Full: Asset, Price, Day%, Day$, P&L, Value, Alloc%, RSI, Trend
            let fixed: u16 = 16 + 7 + 9 + 8 + 10 + 7 + 6 + 8; // all fixed-width columns
            let gaps: u16 = 8; // 9 columns → 8 gaps
            let asset_w = content_width.saturating_sub(fixed + gaps).max(14);
            (
                vec![asset_w, 16, 7, 9, 8, 10, 7, 6, 8],
                vec![
                    Some(SortField::Name),       // Asset
                    None,                         // Price (no sort field)
                    None,                         // Day% (no sort field)
                    None,                         // Day$ (no sort field)
                    Some(SortField::GainPct),     // P&L
                    None,                         // Value
                    Some(SortField::Allocation),  // Alloc%
                    None,                         // RSI
                    None,                         // Trend
                ],
            )
        };

        // Find which column was clicked
        let rel_col = col.saturating_sub(content_start_x);
        let mut cumulative: u16 = 0;
        for (i, &w) in col_widths.iter().enumerate() {
            let col_end = cumulative + w;
            if rel_col < col_end {
                // Clicked in column i
                if let Some(field) = &sort_fields[i] {
                    if self.sort_field == *field {
                        self.sort_ascending = !self.sort_ascending;
                    } else {
                        self.sort_field = *field;
                        // Default direction: Name/Category ascending, others descending
                        self.sort_ascending = matches!(field, SortField::Name | SortField::Category);
                    }
                    self.last_sort_change_tick = self.tick_count;
                    self.recompute();
                }
                return;
            }
            cumulative = col_end + 1; // +1 for column spacing
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
            ViewMode::Watchlist => {
                if !self.watchlist_entries.is_empty() {
                    self.watchlist_selected_index =
                        (self.watchlist_selected_index + 1).min(self.watchlist_entries.len() - 1);
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
            ViewMode::Analytics => {
                let count = self.analytics_scenario_count();
                if count > 0 {
                    self.analytics_selected_index =
                        (self.analytics_selected_index + 1).min(count - 1);
                }
            }
            ViewMode::News => {
                if !self.news_entries.is_empty() {
                    self.news_selected_index =
                        (self.news_selected_index + 1).min(self.news_entries.len() - 1);
                }
            }
            ViewMode::Journal => {
                if !self.journal_entries.is_empty() {
                    self.journal_selected_index =
                        (self.journal_selected_index + 1).min(self.journal_entries.len() - 1);
                }
            }
        }
        if matches!(self.view_mode, ViewMode::Positions)
            && self.selected_index != old_pos_idx
        {
            self.on_position_selection_changed();
        }
    }

    fn move_up(&mut self) {
        let old_pos_idx = self.selected_index;
        match self.view_mode {
            ViewMode::Positions => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            ViewMode::Watchlist => {
                self.watchlist_selected_index = self.watchlist_selected_index.saturating_sub(1);
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
            ViewMode::Analytics => {
                self.analytics_selected_index = self.analytics_selected_index.saturating_sub(1);
            }
            ViewMode::News => {
                self.news_selected_index = self.news_selected_index.saturating_sub(1);
            }
            ViewMode::Journal => {
                self.journal_selected_index = self.journal_selected_index.saturating_sub(1);
            }
        }
        if matches!(self.view_mode, ViewMode::Positions)
            && self.selected_index != old_pos_idx
        {
            self.on_position_selection_changed();
        }
    }

    fn jump_to_top(&mut self) {
        let old_pos_idx = self.selected_index;
        match self.view_mode {
            ViewMode::Positions => {
                self.selected_index = 0;
            }
            ViewMode::Watchlist => {
                self.watchlist_selected_index = 0;
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
            ViewMode::Analytics => {
                self.analytics_selected_index = 0;
            }
            ViewMode::News => {
                self.news_selected_index = 0;
            }
            ViewMode::Journal => {
                self.journal_selected_index = 0;
            }
        }
        if matches!(self.view_mode, ViewMode::Positions)
            && self.selected_index != old_pos_idx
        {
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
            ViewMode::Watchlist => {
                if !self.watchlist_entries.is_empty() {
                    self.watchlist_selected_index = self.watchlist_entries.len() - 1;
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
            ViewMode::Analytics => {
                let count = self.analytics_scenario_count();
                if count > 0 {
                    self.analytics_selected_index = count - 1;
                }
            }
            ViewMode::News => {
                if !self.news_entries.is_empty() {
                    self.news_selected_index = self.news_entries.len() - 1;
                }
            }
            ViewMode::Journal => {
                if !self.journal_entries.is_empty() {
                    self.journal_selected_index = self.journal_entries.len() - 1;
                }
            }
        }
        if matches!(self.view_mode, ViewMode::Positions)
            && self.selected_index != old_pos_idx
        {
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

    fn analytics_scenario_count(&self) -> usize {
        5
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
            ViewMode::Watchlist => {
                if !self.watchlist_entries.is_empty() {
                    self.watchlist_selected_index =
                        (self.watchlist_selected_index + step).min(self.watchlist_entries.len() - 1);
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
            ViewMode::Analytics => {
                let count = self.analytics_scenario_count();
                if count > 0 {
                    self.analytics_selected_index =
                        (self.analytics_selected_index + step).min(count - 1);
                }
            }
            ViewMode::News => {
                if !self.news_entries.is_empty() {
                    self.news_selected_index =
                        (self.news_selected_index + step).min(self.news_entries.len() - 1);
                }
            }
            ViewMode::Journal => {
                if !self.journal_entries.is_empty() {
                    self.journal_selected_index =
                        (self.journal_selected_index + step).min(self.journal_entries.len() - 1);
                }
            }
        }
        if matches!(self.view_mode, ViewMode::Positions)
            && self.selected_index != old_pos_idx
        {
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
            ViewMode::Watchlist => {
                self.watchlist_selected_index = self.watchlist_selected_index.saturating_sub(step);
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
            ViewMode::Analytics => {
                self.analytics_selected_index = self.analytics_selected_index.saturating_sub(step);
            }
            ViewMode::News => {
                self.news_selected_index = self.news_selected_index.saturating_sub(step);
            }
            ViewMode::Journal => {
                self.journal_selected_index = self.journal_selected_index.saturating_sub(step);
            }
        }
        if matches!(self.view_mode, ViewMode::Positions)
            && self.selected_index != old_pos_idx
        {
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
            cfg.home_tab = if self.view_mode == ViewMode::Watchlist {
                "watchlist".to_string()
            } else {
                "positions".to_string()
            };
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
    /// Handle key input when the context menu is open.
    fn handle_context_menu_key(&mut self, key: KeyEvent) {
        let menu = match &self.context_menu {
            Some(m) => m,
            None => return,
        };
        let action_count = menu.actions.len();

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.context_menu = None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(ref mut menu) = self.context_menu {
                    if menu.selected + 1 < action_count {
                        menu.selected += 1;
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(ref mut menu) = self.context_menu {
                    menu.selected = menu.selected.saturating_sub(1);
                }
            }
            KeyCode::Enter => {
                let action = menu.actions.get(menu.selected).copied();
                self.context_menu = None;
                if let Some(action) = action {
                    self.execute_context_action(action);
                }
            }
            _ => {
                // Any other key dismisses the menu
                self.context_menu = None;
            }
        }
    }

    /// Execute a context menu action on the currently selected position.
    fn execute_context_action(&mut self, action: ContextMenuAction) {
        match action {
            ContextMenuAction::ViewDetail => {
                if let Some(pos) = self.selected_position().cloned() {
                    self.detail_popup_open = true;
                    self.fetch_asset_brave_news(&pos.symbol);
                }
            }
            ContextMenuAction::AddTransaction => {
                if self.portfolio_mode == PortfolioMode::Full {
                    self.open_tx_form();
                }
            }
            ContextMenuAction::Delete => {
                if self.portfolio_mode == PortfolioMode::Full {
                    self.open_delete_confirm();
                }
            }
            ContextMenuAction::CopySymbol => {
                // Copy symbol to clipboard via OSC 52 escape sequence.
                // This works in terminals that support OSC 52 (most modern terminals).
                if let Some(pos) = self.selected_position() {
                    let symbol = pos.symbol.clone();
                    let encoded = base64_encode_symbol(&symbol);
                    // The actual clipboard write happens in the TUI event loop
                    // by emitting the OSC 52 sequence to stdout.
                    self.clipboard_osc52 = Some(encoded);
                }
            }
        }
    }

    fn open_tx_form(&mut self) {
        if let Some(pos) = self.selected_position().cloned() {
            self.tx_form = Some(TxFormState::new(pos.symbol, pos.category));
        }
    }

    fn open_tx_form_for_symbol(&mut self, symbol: String) {
        let category = crate::models::asset_names::infer_category(&symbol);
        self.tx_form = Some(TxFormState::new(symbol, category));
    }

    fn open_command_palette(&mut self) {
        self.command_palette_open = true;
        self.command_palette_input.clear();
        self.command_palette_selected = 0;
        self.search_overlay_open = false;
    }

    fn handle_command_palette_key(&mut self, key: KeyEvent) {
        let matches = crate::tui::views::command_palette::matching_commands(
            &self.command_palette_input,
        );
        match key.code {
            KeyCode::Esc => {
                self.command_palette_open = false;
                self.command_palette_input.clear();
                self.command_palette_selected = 0;
            }
            KeyCode::Backspace => {
                self.command_palette_input.pop();
                self.command_palette_selected = 0;
            }
            KeyCode::Down => {
                if self.command_palette_selected + 1 < matches.len() {
                    self.command_palette_selected += 1;
                }
            }
            KeyCode::Up => {
                self.command_palette_selected =
                    self.command_palette_selected.saturating_sub(1);
            }
            KeyCode::Tab => {
                if let Some(entry) = matches.get(self.command_palette_selected) {
                    self.command_palette_input = entry.command.to_string();
                }
            }
            KeyCode::Enter => {
                let command = if let Some(entry) = matches.get(self.command_palette_selected) {
                    entry.command.to_string()
                } else {
                    self.command_palette_input.trim().to_string()
                };
                self.execute_palette_command(&command);
                self.command_palette_open = false;
                self.command_palette_input.clear();
                self.command_palette_selected = 0;
            }
            KeyCode::Char(c) => {
                self.command_palette_input.push(c);
                self.command_palette_selected = 0;
            }
            _ => {}
        }
    }

    fn execute_palette_command(&mut self, command: &str) {
        let cmd = command.trim().to_lowercase();
        match cmd.as_str() {
            "quit" | "q" | "exit" => self.should_quit = true,
            "help" | "?" => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            "refresh" | "r" => self.force_refresh(),
            "theme next" | "theme" => self.cycle_theme(),
            "split toggle" => {
                if matches!(self.view_mode, ViewMode::Positions) {
                    self.split_pane_open = !self.split_pane_open;
                }
            }
            "layout compact" => self.set_workspace_layout(WorkspaceLayout::Compact),
            "layout split" => self.set_workspace_layout(WorkspaceLayout::Split),
            "layout analyst" => self.set_workspace_layout(WorkspaceLayout::Analyst),
            "view positions" => self.view_mode = ViewMode::Positions,
            "view transactions" => {
                if self.portfolio_mode == PortfolioMode::Full {
                    self.view_mode = ViewMode::Transactions;
                }
            }
            "view markets" => self.view_mode = ViewMode::Markets,
            "view economy" => self.view_mode = ViewMode::Economy,
            "view watchlist" => {
                self.view_mode = ViewMode::Watchlist;
                self.load_watchlist();
                self.request_watchlist_data();
            }
            "view analytics" => self.view_mode = ViewMode::Analytics,
            "view news" => self.view_mode = ViewMode::News,
            "view journal" => self.view_mode = ViewMode::Journal,
            "scan" => self.open_scan_builder(),
            _ => {}
        }
    }

    fn open_scan_builder(&mut self) {
        self.scan_builder_open = true;
        self.scan_builder_mode = ScanBuilderMode::Edit;
        self.scan_builder_clause_input.clear();
        self.scan_builder_name_input.clear();
        self.scan_builder_clauses.clear();
        self.scan_builder_selected = 0;
        self.scan_builder_message = None;
    }

    fn current_scan_filter_expr(&self) -> String {
        self.scan_builder_clauses.join(" and ")
    }

    fn load_scan_builder_query(&mut self, name: &str) {
        match Connection::open(&self.db_path)
            .ok()
            .and_then(|conn| scan_queries::get_scan_query(&conn, name).ok())
            .flatten()
        {
            Some(row) => {
                self.scan_builder_clauses = row
                    .filter_expr
                    .split(" and ")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                self.scan_builder_selected = 0;
                self.scan_builder_message = Some(format!("Loaded '{}'", name));
            }
            None => {
                self.scan_builder_message = Some(format!("No saved scan named '{}'", name));
            }
        }
    }

    fn save_scan_builder_query(&mut self, name: &str) {
        if self.scan_builder_clauses.is_empty() {
            self.scan_builder_message = Some("No clauses to save".to_string());
            return;
        }
        let expr = self.current_scan_filter_expr();
        let res = Connection::open(&self.db_path)
            .ok()
            .and_then(|conn| scan_queries::upsert_scan_query(&conn, name, &expr).ok());
        if res.is_some() {
            self.scan_builder_message = Some(format!("Saved '{}' ({})", name, expr));
        } else {
            self.scan_builder_message = Some("Failed to save scan".to_string());
        }
    }

    fn handle_scan_builder_key(&mut self, key: KeyEvent) {
        match self.scan_builder_mode {
            ScanBuilderMode::Edit => match key.code {
                KeyCode::Esc => {
                    self.scan_builder_open = false;
                    self.scan_builder_clause_input.clear();
                    self.scan_builder_name_input.clear();
                }
                KeyCode::Backspace => {
                    self.scan_builder_clause_input.pop();
                }
                KeyCode::Char('a') | KeyCode::Enter => {
                    let clause = self.scan_builder_clause_input.trim();
                    if !clause.is_empty() {
                        self.scan_builder_clauses.push(clause.to_string());
                        self.scan_builder_clause_input.clear();
                        self.scan_builder_selected = self.scan_builder_clauses.len().saturating_sub(1);
                        self.scan_builder_message = Some("Clause added".to_string());
                    }
                }
                KeyCode::Char('r') => {
                    if self.scan_builder_selected < self.scan_builder_clauses.len() {
                        self.scan_builder_clauses.remove(self.scan_builder_selected);
                        if self.scan_builder_selected >= self.scan_builder_clauses.len() {
                            self.scan_builder_selected = self.scan_builder_clauses.len().saturating_sub(1);
                        }
                        self.scan_builder_message = Some("Clause removed".to_string());
                    }
                }
                KeyCode::Char('c') => {
                    self.scan_builder_clauses.clear();
                    self.scan_builder_selected = 0;
                    self.scan_builder_message = Some("Cleared clauses".to_string());
                }
                KeyCode::Char('s') => {
                    self.scan_builder_mode = ScanBuilderMode::SaveName;
                    self.scan_builder_name_input.clear();
                    self.scan_builder_message = Some("Enter name, then press Enter to save".to_string());
                }
                KeyCode::Char('l') => {
                    self.scan_builder_mode = ScanBuilderMode::LoadName;
                    self.scan_builder_name_input.clear();
                    self.scan_builder_message = Some("Enter name, then press Enter to load".to_string());
                }
                KeyCode::Down => {
                    if self.scan_builder_selected + 1 < self.scan_builder_clauses.len() {
                        self.scan_builder_selected += 1;
                    }
                }
                KeyCode::Up => {
                    self.scan_builder_selected = self.scan_builder_selected.saturating_sub(1);
                }
                KeyCode::Char(ch) => {
                    self.scan_builder_clause_input.push(ch);
                }
                _ => {}
            },
            ScanBuilderMode::SaveName => match key.code {
                KeyCode::Esc => {
                    self.scan_builder_mode = ScanBuilderMode::Edit;
                    self.scan_builder_name_input.clear();
                }
                KeyCode::Backspace => {
                    self.scan_builder_name_input.pop();
                }
                KeyCode::Enter => {
                    let name = self.scan_builder_name_input.trim().to_string();
                    if !name.is_empty() {
                        self.save_scan_builder_query(&name);
                    }
                    self.scan_builder_mode = ScanBuilderMode::Edit;
                    self.scan_builder_name_input.clear();
                }
                KeyCode::Char(ch) => {
                    self.scan_builder_name_input.push(ch);
                }
                _ => {}
            },
            ScanBuilderMode::LoadName => match key.code {
                KeyCode::Esc => {
                    self.scan_builder_mode = ScanBuilderMode::Edit;
                    self.scan_builder_name_input.clear();
                }
                KeyCode::Backspace => {
                    self.scan_builder_name_input.pop();
                }
                KeyCode::Enter => {
                    let name = self.scan_builder_name_input.trim().to_string();
                    if !name.is_empty() {
                        self.load_scan_builder_query(&name);
                    }
                    self.scan_builder_mode = ScanBuilderMode::Edit;
                    self.scan_builder_name_input.clear();
                }
                KeyCode::Char(ch) => {
                    self.scan_builder_name_input.push(ch);
                }
                _ => {}
            },
        }
    }

    fn set_workspace_layout(&mut self, layout: WorkspaceLayout) {
        self.workspace_layout = layout;
        if let Ok(mut cfg) = config::load_config() {
            cfg.layout = layout;
            let _ = config::save_config(&cfg);
        }
    }

    /// Handle key input in the global asset search overlay.
    fn handle_search_overlay_key(&mut self, key: KeyEvent) {
        use crate::tui::views::search_overlay::build_results;

        match key.code {
            KeyCode::Esc => {
                self.search_overlay_open = false;
                self.search_overlay_query.clear();
                self.search_overlay_selected = 0;
                self.search_overlay_requested_symbols.clear();
                self.search_chart_popup = None;
            }
            KeyCode::Backspace => {
                self.search_overlay_query.pop();
                self.search_overlay_selected = 0;
                self.request_search_overlay_live_data();
            }
            KeyCode::Char(c) => {
                self.search_overlay_query.push(c);
                self.search_overlay_selected = 0;
                self.request_search_overlay_live_data();
            }
            KeyCode::Down => {
                // Navigate results (capped at max 19 since results are limited to 20)
                if self.search_overlay_selected < 19 {
                    self.search_overlay_selected += 1;
                }
            }
            KeyCode::Up => {
                self.search_overlay_selected = self.search_overlay_selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                // Open full-screen search chart popup for the selected result
                let results = build_results(self, &self.search_overlay_query.clone());
                if let Some(result) = results.get(self.search_overlay_selected) {
                    let symbol = result.symbol.clone();
                    let category = crate::models::asset_names::infer_category(&symbol);
                    // Request price data if we don't have it
                    if let Some(svc) = &self.price_service {
                        if !self.prices.contains_key(&symbol) {
                            svc.send_command(PriceCommand::FetchAll(vec![(
                                symbol.clone(),
                                category,
                            )]));
                        }
                        if !self.price_history.contains_key(&symbol) {
                            svc.send_command(PriceCommand::FetchHistory(
                                symbol.clone(),
                                category,
                                370,
                            ));
                        }
                    }
                    self.search_chart_popup = Some(
                        crate::tui::views::search_chart_popup::SearchChartPopupState { symbol },
                    );
                    // Keep search overlay open underneath so Esc returns to it
                }
            }
            _ => {}
        }
    }

    fn handle_search_chart_popup_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_chart_popup = None;
            }
            KeyCode::Char('w') | KeyCode::Char('W') => {
                if let Some(state) = &self.search_chart_popup {
                    let symbol = state.symbol.clone();
                    let category = crate::models::asset_names::infer_category(&symbol);
                    if let Ok(conn) = Connection::open(&self.db_path) {
                        let _ = db_watchlist::add_to_watchlist_in_group(
                            &conn,
                            &symbol,
                            category,
                            self.watchlist_active_group,
                        );
                    }
                    self.load_watchlist();
                    self.request_watchlist_data();
                }
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if let Some(state) = &self.search_chart_popup {
                    let symbol = state.symbol.clone();
                    self.search_chart_popup = None;
                    self.search_overlay_open = false;
                    self.search_overlay_query.clear();
                    self.search_overlay_selected = 0;
                    self.search_overlay_requested_symbols.clear();
                    self.open_tx_form_for_symbol(symbol);
                }
            }
            _ => {}
        }
    }

    fn request_search_overlay_live_data(&mut self) {
        use crate::tui::views::search_overlay::build_results;

        if self.search_overlay_query.trim().is_empty() {
            return;
        }
        let Some(svc) = &self.price_service else { return };

        let mut quote_batch: Vec<(String, AssetCategory)> = Vec::new();
        let mut history_batch: Vec<(String, AssetCategory, u32)> = Vec::new();

        let results = build_results(self, &self.search_overlay_query);
        for result in results.into_iter().take(8) {
            if result.in_portfolio || result.in_watchlist {
                continue;
            }
            if self.search_overlay_requested_symbols.contains(&result.symbol) {
                continue;
            }

            let category = crate::models::asset_names::infer_category(&result.symbol);
            if !self.prices.contains_key(&result.symbol) {
                quote_batch.push((result.symbol.clone(), category));
            }
            if !self.price_history.contains_key(&result.symbol) {
                history_batch.push((result.symbol.clone(), category, 370));
            }
            self.search_overlay_requested_symbols
                .insert(result.symbol.clone());
        }

        if !quote_batch.is_empty() {
            svc.send_command(PriceCommand::FetchAll(quote_batch));
        }
        if !history_batch.is_empty() {
            svc.send_command(PriceCommand::FetchHistoryBatch(history_batch));
        }
    }

    /// Handle key input in the asset detail popup (opened from search overlay).
    fn handle_asset_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                // Close popup, return to search overlay (which is still open underneath)
                self.asset_detail = None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(ref mut state) = self.asset_detail {
                    state.scroll = state.scroll.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(ref mut state) = self.asset_detail {
                    state.scroll = state.scroll.saturating_sub(1);
                }
            }
            KeyCode::Char('G') => {
                // Jump to bottom
                if let Some(ref mut state) = self.asset_detail {
                    state.scroll = usize::MAX; // Will be clamped during render
                }
            }
            KeyCode::Char('g') => {
                // gg — jump to top (simplified: single g jumps to top in this context)
                if let Some(ref mut state) = self.asset_detail {
                    state.scroll = 0;
                }
            }
            _ => {}
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
            native_currency: None,
            fx_rate: None,
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

        assert_eq!(variants.len(), 5); // All + DXY single + DXY/Gold + DXY/SPX + DXY/BTC
        assert_eq!(variants[0].label, "All");
        assert_eq!(variants[1].label, "Dollar Index (DXY)");
        assert_eq!(variants[2].label, "DXY/Gold");
        assert_eq!(variants[3].label, "DXY/SPX");
        assert_eq!(variants[4].label, "DXY/BTC");

        // DXY should be single chart for USD
        match &variants[1].kind {
            ChartKind::Single { symbol, .. } => assert_eq!(symbol, "DX-Y.NYB"),
            _ => panic!("Expected Single chart for DXY"),
        }

        // DXY/SPX should be a ratio (DXY / ^GSPC)
        match &variants[3].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "DX-Y.NYB");
                assert_eq!(den_symbol, "^GSPC");
            }
            _ => panic!("Expected Ratio chart for DXY/SPX"),
        }

        // DXY/BTC should be a ratio (DXY / BTC-USD)
        match &variants[4].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "DX-Y.NYB");
                assert_eq!(den_symbol, "BTC-USD");
            }
            _ => panic!("Expected Ratio chart for DXY/BTC"),
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

        // Equities get All + single + /SPX + /QQQ + /BTC = 5 variants
        assert_eq!(variants.len(), 5);
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

        // {SYM}/BTC ratio
        assert_eq!(variants[4].label, "AAPL/BTC");
        match &variants[4].kind {
            ChartKind::Ratio { num_symbol, den_symbol, .. } => {
                assert_eq!(num_symbol, "AAPL");
                assert_eq!(den_symbol, "BTC-USD");
            }
            _ => panic!("Expected Ratio chart for AAPL/BTC"),
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

        assert_eq!(variants.len(), 5); // All + single + /SPX + /QQQ + /BTC
        assert_eq!(variants[0].label, "All");
        assert_eq!(variants[2].label, "VTI/SPX");
        assert_eq!(variants[3].label, "VTI/QQQ");
        assert_eq!(variants[4].label, "VTI/BTC");
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
    fn test_equity_has_btc_ratio() {
        // All equities, funds, and commodities get /BTC ratio
        let pos = make_position("AAPL", AssetCategory::Equity);
        let variants = App::chart_variants_for_position(&pos);

        let labels = variant_labels(&variants);
        assert!(labels.contains(&"AAPL/BTC".to_string()), "Equities should have /BTC ratio");
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
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(110), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-02-01".into(), close: dec!(110), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-03-01".into(), close: dec!(120), volume: None, open: None, high: None, low: None },
        ]);
        // New fetch returns only last month (shorter range)
        let new_records = vec![
            HistoryRecord { date: "2025-03-01".into(), close: dec!(125), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
        ]);
        let new_records = vec![
            HistoryRecord { date: "2025-01-02".into(), close: dec!(105), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(110), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
                native_currency: None,
                fx_rate: None,
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
    fn test_end_jumps_to_bottom() {
        let mut app = make_test_app(10);
        app.selected_index = 0;

        app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
                native_currency: None,
                fx_rate: None,
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
                native_currency: None,
                fx_rate: None,
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
                native_currency: None,
                fx_rate: None,
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
    fn test_slash_opens_search_overlay() {
        let mut app = make_search_app();
        assert!(!app.search_overlay_open);

        app.handle_key(key('/'));
        assert!(app.search_overlay_open);
        assert!(app.search_overlay_query.is_empty());
    }

    #[test]
    fn test_search_overlay_typing_updates_query() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        app.handle_key(key('c'));

        assert!(app.search_overlay_open);
        assert_eq!(app.search_overlay_query, "btc");
    }

    #[test]
    fn test_search_overlay_esc_closes_and_clears() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));

        app.handle_key(esc_key());
        assert!(!app.search_overlay_open);
        assert!(app.search_overlay_query.is_empty());
        assert_eq!(app.search_overlay_selected, 0);
    }

    #[test]
    fn test_search_overlay_enter_opens_search_chart_popup() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        app.handle_key(key('c'));
        app.handle_key(enter_key());

        // Enter opens chart popup, keeping search overlay open underneath
        assert!(app.search_overlay_open);
        assert!(app.search_chart_popup.is_some());
        assert_eq!(app.search_chart_popup.as_ref().unwrap().symbol, "BTC");
    }

    #[test]
    fn test_search_overlay_backspace_removes_char() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        app.handle_key(key('c'));
        assert_eq!(app.search_overlay_query, "btc");

        app.handle_key(backspace_key());
        assert_eq!(app.search_overlay_query, "bt");

        app.handle_key(backspace_key());
        app.handle_key(backspace_key());
        assert!(app.search_overlay_query.is_empty());
    }

    #[test]
    fn test_search_overlay_arrow_down_increments_selected() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('a'));

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);
        assert_eq!(app.search_overlay_selected, 1);
        app.handle_key(down);
        assert_eq!(app.search_overlay_selected, 2);
    }

    #[test]
    fn test_search_overlay_arrow_up_decrements_selected() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('a'));

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        app.handle_key(down);
        app.handle_key(down);
        assert_eq!(app.search_overlay_selected, 2);

        app.handle_key(up);
        assert_eq!(app.search_overlay_selected, 1);
    }

    #[test]
    fn test_search_overlay_blocks_normal_keys() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        // Typing 'q' in overlay should NOT quit
        app.handle_key(key('q'));
        assert!(!app.should_quit);
        assert_eq!(app.search_overlay_query, "q");
    }

    #[test]
    fn test_search_overlay_enter_opens_chart_for_portfolio_position() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('B'));
        app.handle_key(key('T'));
        app.handle_key(key('C'));
        app.handle_key(enter_key());

        // BTC is in portfolio — chart popup still opens
        assert!(app.search_chart_popup.is_some());
        assert_eq!(app.search_chart_popup.as_ref().unwrap().symbol, "BTC");
        // Search overlay stays open underneath
        assert!(app.search_overlay_open);
    }

    #[test]
    fn test_search_chart_popup_esc_returns_to_search() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        app.handle_key(key('c'));
        app.handle_key(enter_key());

        assert!(app.search_chart_popup.is_some());
        assert!(app.search_overlay_open);

        // Esc closes chart popup, returns to search overlay
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.search_chart_popup.is_none());
        assert!(app.search_overlay_open);
    }

    #[test]
    fn test_search_chart_popup_a_opens_tx_form() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        app.handle_key(key('b'));
        app.handle_key(key('t'));
        app.handle_key(key('c'));
        app.handle_key(enter_key());
        assert!(app.search_chart_popup.is_some());

        app.handle_key(key('a'));
        assert!(app.search_chart_popup.is_none());
        assert!(!app.search_overlay_open);
        assert!(app.tx_form.is_some());
        assert_eq!(app.tx_form.as_ref().unwrap().symbol, "BTC");
    }

    #[test]
    fn test_search_overlay_typing_resets_selection() {
        let mut app = make_search_app();

        app.handle_key(key('/'));
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);
        app.handle_key(down);
        assert_eq!(app.search_overlay_selected, 2);

        // Typing a char should reset selection to 0
        app.handle_key(key('a'));
        assert_eq!(app.search_overlay_selected, 0);
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
        };
        let app = App::new(&config, PathBuf::from("/tmp/pftui_test_tf.db"));
        assert_eq!(app.chart_timeframe, ChartTimeframe::ThreeMonths);
    }

    fn make_tf_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
            native_currency: None,
            fx_rate: None,
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
                native_currency: None,
                fx_rate: None,
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
                native_currency: None,
                fx_rate: None,
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
            native_currency: None,
            fx_rate: None,
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
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(148), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
        ]);
        app.price_history.insert("GOOG".to_string(), vec![
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(2750), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: today, close: dec!(158), volume: None, open: None, high: None, low: None },
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
            native_currency: None,
            fx_rate: None,
        }
    }

    #[test]
    fn test_locf_fills_missing_dates() {
        // The core bug: if AAPL has price on day 1 and 3 but not day 2,
        // day 2 should use AAPL's day-1 price (not contribute $0).
        let mut app = make_app();
        app.positions = vec![make_position("AAPL", dec!(10), AssetCategory::Equity)];
        app.price_history.insert("AAPL".to_string(), vec![
            HistoryRecord { date: "2026-01-01".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
            // No record for 2026-01-02
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(155), volume: None, open: None, high: None, low: None },
        ]);
        // Add a second symbol that has data on day 2 to create the date
        app.positions.push(make_position("GOOG", dec!(5), AssetCategory::Equity));
        app.price_history.insert("GOOG".to_string(), vec![
            HistoryRecord { date: "2026-01-02".to_string(), close: dec!(2800), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(2850), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-01-01".to_string(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-05".to_string(), close: dec!(100), volume: None, open: None, high: None, low: None },
        ]);
        app.price_history.insert("GOOG".to_string(), vec![
            HistoryRecord { date: "2026-01-02".to_string(), close: dec!(200), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-04".to_string(), close: dec!(200), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-01-01".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-02".to_string(), close: dec!(155), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-01-01".to_string(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-02".to_string(), close: dec!(105), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(110), volume: None, open: None, high: None, low: None },
        ]);
        // NEW only gets a price on day 3
        app.price_history.insert("NEW".to_string(), vec![
            HistoryRecord { date: "2026-01-03".to_string(), close: dec!(50), volume: None, open: None, high: None, low: None },
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
            native_currency: None,
            fx_rate: None,
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
    fn test_breadcrumb_watchlist_tab() {
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
mod watchlist_tab_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_app() -> App {
        let config = Config::default();
        App::new(&config, std::path::PathBuf::from(":memory:"))
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn test_default_view_is_positions() {
        let app = make_app();
        assert_eq!(app.view_mode, ViewMode::Positions);
    }

    #[test]
    fn test_w_switches_to_watchlist_view() {
        let mut app = make_app();
        assert_eq!(app.view_mode, ViewMode::Positions);
        app.handle_key(key('w'));
        assert_eq!(app.view_mode, ViewMode::Watchlist);
    }

    #[test]
    fn test_5_switches_to_watchlist_view() {
        let mut app = make_app();
        app.handle_key(key('5'));
        assert_eq!(app.view_mode, ViewMode::Watchlist);
    }

    #[test]
    fn test_key_1_returns_to_positions_from_watchlist() {
        let mut app = make_app();
        app.handle_key(key('w'));
        assert_eq!(app.view_mode, ViewMode::Watchlist);
        app.handle_key(key('1'));
        assert_eq!(app.view_mode, ViewMode::Positions);
    }

    #[test]
    fn test_w_from_any_view_goes_to_watchlist() {
        let mut app = make_app();
        app.view_mode = ViewMode::Markets;
        app.handle_key(key('w'));
        assert_eq!(app.view_mode, ViewMode::Watchlist);
    }

    #[test]
    fn test_watchlist_then_markets_then_back() {
        let mut app = make_app();
        app.handle_key(key('5'));
        assert_eq!(app.view_mode, ViewMode::Watchlist);
        app.handle_key(key('3'));
        assert_eq!(app.view_mode, ViewMode::Markets);
        app.handle_key(key('5'));
        assert_eq!(app.view_mode, ViewMode::Watchlist);
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
    fn test_tx_form_opens_on_i() {
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
            native_currency: None,
            fx_rate: None,
        }];
        app.display_positions = app.positions.clone();
        app.selected_index = 0;

        assert!(app.tx_form.is_none());
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
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
            native_currency: None,
            fx_rate: None,
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
                native_currency: None,
                fx_rate: None,
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
        // Set view to Transactions so Tab toggles sort (not home sub-tabs)
        app.view_mode = ViewMode::Transactions;
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
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
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
            native_currency: None,
            fx_rate: None,
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
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(140), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
        ]);
        app.price_history.insert("BTC".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(50000), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(60000), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(100), volume: None, open: None, high: None, low: None },
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
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(140), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(150), volume: None, open: None, high: None, low: None },
        ]);
        app.price_history.insert("GOOG".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(190), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(200), volume: None, open: None, high: None, low: None },
        ]);
        app.price_history.insert("BTC".to_string(), vec![
            HistoryRecord { date: "2026-02-27".to_string(), close: dec!(48000), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2026-02-28".to_string(), close: dec!(50000), volume: None, open: None, high: None, low: None },
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

#[cfg(test)]
mod base64_tests {
    use super::base64_encode_symbol;

    #[test]
    fn encode_aapl() {
        assert_eq!(base64_encode_symbol("AAPL"), "QUFQTA==");
    }

    #[test]
    fn encode_btc() {
        assert_eq!(base64_encode_symbol("BTC-USD"), "QlRDLVVTRA==");
    }

    #[test]
    fn encode_empty() {
        assert_eq!(base64_encode_symbol(""), "");
    }
}

#[cfg(test)]
mod mouse_tests {
    use super::*;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use std::path::PathBuf;

    fn make_app() -> App {
        let config = crate::config::Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "midnight".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
        };
        let mut app = App::new(&config, PathBuf::from("/tmp/pftui_test_mouse.db"));
        app.terminal_width = 120;
        app.terminal_height = 40;
        app
    }

    fn make_position(symbol: &str) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category: AssetCategory::Equity,
            quantity: dec!(10),
            avg_cost: dec!(100),
            total_cost: dec!(1000),
            currency: "USD".to_string(),
            current_price: Some(dec!(150)),
            current_value: Some(dec!(1500)),
            gain: Some(dec!(500)),
            gain_pct: Some(dec!(50)),
            allocation_pct: Some(dec!(50)),
            native_currency: None,
            fx_rate: None,
        }
    }

    fn mouse_event(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: crossterm::event::KeyModifiers::empty(),
        }
    }

    #[test]
    fn scroll_down_moves_selection_down() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG"), make_position("MSFT")];
        assert_eq!(app.selected_index, 0);

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 10, 10));
        assert_eq!(app.selected_index, 1);

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 10, 10));
        assert_eq!(app.selected_index, 2);
    }

    #[test]
    fn scroll_up_moves_selection_up() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG"), make_position("MSFT")];
        app.selected_index = 2;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, 10, 10));
        assert_eq!(app.selected_index, 1);

        app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, 10, 10));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn scroll_up_clamps_at_zero() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.selected_index = 0;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, 10, 10));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn scroll_ignored_when_help_open() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.show_help = true;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 10, 10));
        assert_eq!(app.selected_index, 0); // unchanged
    }

    #[test]
    fn scroll_ignored_when_search_overlay_open() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.search_overlay_open = true;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 10, 10));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn click_dismisses_help_overlay() {
        let mut app = make_app();
        app.show_help = true;

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 10, 10));
        assert!(!app.show_help);
    }

    #[test]
    fn click_dismisses_search_overlay() {
        let mut app = make_app();
        app.search_overlay_open = true;

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 10, 10));
        assert!(!app.search_overlay_open);
    }

    #[test]
    fn click_dismisses_detail_popup() {
        let mut app = make_app();
        app.detail_popup_open = true;

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 10, 10));
        assert!(!app.detail_popup_open);
    }

    #[test]
    fn click_header_tab_1_switches_to_positions() {
        let mut app = make_app();
        app.view_mode = ViewMode::Markets;

        // Click on [1]Pos area (col ~8-13)
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 10, 0));
        assert_eq!(app.view_mode, ViewMode::Positions);
    }

    #[test]
    fn click_header_tab_2_switches_to_transactions() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;

        // Click on [2]Tx area (col ~15-19)
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 16, 0));
        assert_eq!(app.view_mode, ViewMode::Transactions);
    }

    #[test]
    fn click_header_tab_2_area_in_percentage_mode_hits_mkt() {
        let mut app = make_app();
        app.portfolio_mode = PortfolioMode::Percentage;
        app.view_mode = ViewMode::Positions;

        // In percentage mode, [2]Tx is hidden. Col 15-21 is now [3]Mkt
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 16, 0));
        assert_eq!(app.view_mode, ViewMode::Markets);
    }

    #[test]
    fn click_position_row_selects_it() {
        let mut app = make_app();
        app.display_positions = vec![
            make_position("AAPL"),
            make_position("GOOG"),
            make_position("MSFT"),
        ];
        app.selected_index = 0;

        // In wide mode (120 cols), data starts at content_y=3 (section header + border + header row)
        // Header is 3 rows (positions view, non-compact), so row 0 for positions = absolute row 3+3=6
        // Click on second row (index 1) = absolute row 3 (header) + 3 (data_start) + 1 = 7
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 5, 7));
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn click_out_of_bounds_row_does_not_crash() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.selected_index = 0;

        // Click well beyond the last row
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 5, 30));
        // Should stay at 0 (out of bounds is ignored)
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn scroll_works_in_markets_view() {
        let mut app = make_app();
        app.view_mode = ViewMode::Markets;
        app.markets_selected_index = 0;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 10, 10));
        assert_eq!(app.markets_selected_index, 1);
    }

    #[test]
    fn scroll_works_in_economy_view() {
        let mut app = make_app();
        app.view_mode = ViewMode::Economy;
        app.economy_selected_index = 0;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 10, 10));
        assert_eq!(app.economy_selected_index, 1);
    }

    #[test]
    fn right_click_out_of_bounds_no_context_menu() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.selected_index = 0;

        // Right click on a row beyond positions count — no context menu
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Right), 10, 10));
        assert_eq!(app.selected_index, 0);
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn right_click_on_position_opens_context_menu() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.selected_index = 0;

        // Row 6 in wide mode (header=3, section_header=1, border=1, col_header=1, data row 0 = row 6)
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Right), 10, 6));
        assert!(app.context_menu.is_some());
        let menu = app.context_menu.as_ref().unwrap();
        assert_eq!(menu.symbol, "AAPL");
        assert_eq!(menu.selected, 0);
        assert_eq!(menu.actions.len(), 4); // Full mode: View Detail, Add Tx, Delete, Copy Symbol
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn right_click_selects_position_and_opens_menu() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.selected_index = 0;

        // Click on row 7 → second position (GOOG)
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Right), 10, 7));
        assert!(app.context_menu.is_some());
        let menu = app.context_menu.as_ref().unwrap();
        assert_eq!(menu.symbol, "GOOG");
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn right_click_in_non_positions_view_ignored() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.view_mode = ViewMode::Markets;

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Right), 10, 6));
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn left_click_dismisses_context_menu() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.context_menu = Some(ContextMenuState {
            col: 10,
            row: 6,
            selected: 0,
            actions: ContextMenuAction::for_positions(false),
            symbol: "AAPL".to_string(),
        });

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 5, 5));
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn context_menu_j_k_navigates() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.context_menu = Some(ContextMenuState {
            col: 10,
            row: 6,
            selected: 0,
            actions: ContextMenuAction::for_positions(false),
            symbol: "AAPL".to_string(),
        });

        // j moves down
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty()));
        assert_eq!(app.context_menu.as_ref().unwrap().selected, 1);

        // k moves back up
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::empty()));
        assert_eq!(app.context_menu.as_ref().unwrap().selected, 0);

        // k at 0 stays at 0
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::empty()));
        assert_eq!(app.context_menu.as_ref().unwrap().selected, 0);
    }

    #[test]
    fn context_menu_esc_dismisses() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.context_menu = Some(ContextMenuState {
            col: 10,
            row: 6,
            selected: 0,
            actions: ContextMenuAction::for_positions(false),
            symbol: "AAPL".to_string(),
        });

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn context_menu_enter_view_detail_opens_popup() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.selected_index = 0;
        app.context_menu = Some(ContextMenuState {
            col: 10,
            row: 6,
            selected: 0, // ViewDetail is first
            actions: ContextMenuAction::for_positions(false),
            symbol: "AAPL".to_string(),
        });

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.context_menu.is_none()); // menu closed
        assert!(app.detail_popup_open); // detail opened
    }

    #[test]
    fn context_menu_enter_copy_symbol_sets_clipboard() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.selected_index = 0;
        app.context_menu = Some(ContextMenuState {
            col: 10,
            row: 6,
            selected: 3, // CopySymbol is fourth
            actions: ContextMenuAction::for_positions(false),
            symbol: "AAPL".to_string(),
        });

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.context_menu.is_none());
        assert!(app.clipboard_osc52.is_some());
    }

    #[test]
    fn context_menu_percentage_mode_excludes_tx_actions() {
        let mut app = make_app();
        app.portfolio_mode = PortfolioMode::Percentage;
        app.display_positions = vec![make_position("AAPL")];

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Right), 10, 6));
        assert!(app.context_menu.is_some());
        let menu = app.context_menu.as_ref().unwrap();
        assert_eq!(menu.actions.len(), 2); // ViewDetail, CopySymbol only
    }

    #[test]
    fn scroll_down_clamps_at_last_position() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.selected_index = 1;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 10, 10));
        assert_eq!(app.selected_index, 1); // clamped
    }

    #[test]
    fn click_column_header_sorts_by_asset_name() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.view_mode = ViewMode::Positions;
        // Default sort is Allocation desc
        assert_eq!(app.sort_field, SortField::Allocation);

        // In wide mode (120 cols), header row is at content_y = 2 (section_header=1, border=1, then header).
        // With header_h = 3 (wide + Positions view with ticker), content_y = row - header_h.
        // Header row is at absolute row = header_h + section_header(1) + border(1) = 3 + 1 + 1 = 5.
        // content_y = 5 - 3 = 2, which is data_start - 1 = 3 - 1 = 2. ✓
        // Click in Asset column (col ~2, inside left border)
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 2, 5));
        assert_eq!(app.sort_field, SortField::Name);
        assert!(app.sort_ascending); // Name defaults ascending
    }

    #[test]
    fn click_column_header_toggles_direction_on_same_field() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.view_mode = ViewMode::Positions;
        app.sort_field = SortField::Name;
        app.sort_ascending = true;

        // Click Asset column again → should toggle to descending
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 2, 5));
        assert_eq!(app.sort_field, SortField::Name);
        assert!(!app.sort_ascending);
    }

    #[test]
    fn click_column_header_alloc_column() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.view_mode = ViewMode::Positions;
        app.sort_field = SortField::Name;
        app.sort_ascending = true;

        // In full mode (120 cols wide), Alloc% is the 7th fixed column:
        // Asset, Price, Day%, Day$, P&L, Value, Alloc%, RSI, Trend.
        // Click near the Alloc% start boundary.
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 71, 5));
        assert_eq!(app.sort_field, SortField::Allocation);
        assert!(!app.sort_ascending); // Allocation defaults descending
    }

    #[test]
    fn click_column_header_updates_sort_flash_tick() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.view_mode = ViewMode::Positions;
        app.tick_count = 100;

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 2, 5));
        assert_eq!(app.last_sort_change_tick, 100);
    }

    #[test]
    fn click_column_header_ignored_in_non_positions_view() {
        let mut app = make_app();
        app.display_positions = vec![make_position("AAPL")];
        app.view_mode = ViewMode::Markets;
        app.sort_field = SortField::Allocation;

        // Click where header would be — should NOT change sort
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 2, 5));
        assert_eq!(app.sort_field, SortField::Allocation);
    }

    #[test]
    fn click_theme_indicator_cycles_theme() {
        let mut app = make_app();
        app.theme_name = "midnight".to_string();
        // Simulate header render setting the click range
        app.header_theme_col_range = Some((80, 92));
        let old_theme = app.theme_name.clone();

        // Click within the theme indicator range (row 0 = header)
        app.handle_header_click(85);
        assert_ne!(app.theme_name, old_theme, "Theme should have cycled");
    }

    #[test]
    fn click_theme_indicator_outside_range_does_nothing() {
        let mut app = make_app();
        app.theme_name = "midnight".to_string();
        app.header_theme_col_range = Some((80, 92));
        let old_theme = app.theme_name.clone();

        // Click outside the range
        app.handle_header_click(95);
        assert_eq!(app.theme_name, old_theme, "Theme should not have changed");
    }

    #[test]
    fn click_privacy_indicator_toggles_privacy() {
        let mut app = make_app();
        assert!(!app.show_percentages_only);
        // Simulate header render setting the privacy click range
        // Use column 100 which is past all tabs (safer for click handling)
        app.header_privacy_col_range = Some((100, 110));

        // Click within the privacy indicator range
        app.handle_header_click(105);
        assert!(app.show_percentages_only, "Privacy should be toggled on");

        // Click again to toggle off
        app.handle_header_click(105);
        assert!(!app.show_percentages_only, "Privacy should be toggled off");
    }

    #[test]
    fn click_privacy_indicator_ignored_in_percentage_mode() {
        let mut app = make_app();
        app.portfolio_mode = PortfolioMode::Percentage;
        app.show_percentages_only = true; // already in pct mode
        app.header_privacy_col_range = Some((50, 60));

        app.handle_header_click(55);
        // In percentage mode, the toggle should not change (already percentage)
        assert!(app.show_percentages_only);
    }

    #[test]
    fn click_targets_none_by_default() {
        let app = make_app();
        assert!(app.header_theme_col_range.is_none());
        assert!(app.header_privacy_col_range.is_none());
    }

    #[test]
    fn click_theme_no_crash_when_range_is_none() {
        let mut app = make_app();
        app.header_theme_col_range = None;
        let old_theme = app.theme_name.clone();

        // Should not crash or change theme
        app.handle_header_click(85);
        assert_eq!(app.theme_name, old_theme);
    }

    #[test]
    fn click_alloc_bar_sets_category_filter() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        // Simulate allocation bars rendered at area (x=0, y=10, w=30, h=6)
        // with two categories: Equity at inner row 0, Crypto at inner row 1
        app.alloc_bar_area = Some(Rect::new(0, 10, 30, 6));
        app.alloc_bar_categories = vec![AssetCategory::Equity, AssetCategory::Crypto];
        app.category_filter = None;

        // Click row 11 (area.y=10 + 1 border + 0 = inner row 0 → Equity)
        let consumed = app.handle_alloc_bar_click(5, 11);
        assert!(consumed);
        assert_eq!(app.category_filter, Some(AssetCategory::Equity));
    }

    #[test]
    fn click_alloc_bar_second_category() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.alloc_bar_area = Some(Rect::new(0, 10, 30, 6));
        app.alloc_bar_categories = vec![AssetCategory::Equity, AssetCategory::Crypto];
        app.category_filter = None;

        // Click row 12 (inner row 1 → Crypto)
        let consumed = app.handle_alloc_bar_click(5, 12);
        assert!(consumed);
        assert_eq!(app.category_filter, Some(AssetCategory::Crypto));
    }

    #[test]
    fn click_alloc_bar_toggles_off_same_category() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.alloc_bar_area = Some(Rect::new(0, 10, 30, 6));
        app.alloc_bar_categories = vec![AssetCategory::Equity, AssetCategory::Crypto];
        app.category_filter = Some(AssetCategory::Equity);

        // Click Equity again → should clear filter
        let consumed = app.handle_alloc_bar_click(5, 11);
        assert!(consumed);
        assert_eq!(app.category_filter, None);
    }

    #[test]
    fn click_alloc_bar_outside_area_returns_false() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.alloc_bar_area = Some(Rect::new(0, 10, 30, 6));
        app.alloc_bar_categories = vec![AssetCategory::Equity];

        // Click above the area
        assert!(!app.handle_alloc_bar_click(5, 5));
        // Click below the area
        assert!(!app.handle_alloc_bar_click(5, 20));
        // Click to the right
        assert!(!app.handle_alloc_bar_click(35, 11));
    }

    #[test]
    fn click_alloc_bar_no_area_returns_false() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.alloc_bar_area = None;

        assert!(!app.handle_alloc_bar_click(5, 11));
    }

    #[test]
    fn click_alloc_bar_wrong_view_returns_false() {
        let mut app = make_app();
        app.view_mode = ViewMode::Markets;
        app.alloc_bar_area = Some(Rect::new(0, 10, 30, 6));
        app.alloc_bar_categories = vec![AssetCategory::Equity];

        assert!(!app.handle_alloc_bar_click(5, 11));
    }

    #[test]
    fn click_alloc_bar_on_border_row_returns_false() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.alloc_bar_area = Some(Rect::new(0, 10, 30, 6));
        app.alloc_bar_categories = vec![AssetCategory::Equity];

        // Click on top border (row 10 = area.y)
        assert!(!app.handle_alloc_bar_click(5, 10));
    }

    #[test]
    fn drift_columns_toggle_with_d() {
        let mut app = make_app();
        assert!(!app.show_drift_columns);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('D')));
        assert!(app.show_drift_columns);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('D')));
        assert!(!app.show_drift_columns);
    }

    #[test]
    fn sector_grouping_toggle_with_z() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.sort_field = SortField::Name;
        app.sort_ascending = false;
        assert!(!app.show_sector_grouping);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('Z')));
        assert!(app.show_sector_grouping);
        assert_eq!(app.sort_field, SortField::Category);
        assert!(app.sort_ascending);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('Z')));
        assert!(!app.show_sector_grouping);
    }

    #[test]
    fn positions_submode_g_groups_by_category() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.show_sector_grouping = false;
        app.sort_field = SortField::Name;
        app.sort_ascending = false;

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('G')));
        assert!(app.show_sector_grouping);
        assert_eq!(app.sort_field, SortField::Category);
        assert!(app.sort_ascending);
    }

    #[test]
    fn positions_submode_a_sorts_allocation() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.show_sector_grouping = true;
        app.sort_field = SortField::Name;
        app.sort_ascending = true;

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('A')));
        assert!(!app.show_sector_grouping);
        assert_eq!(app.sort_field, SortField::Allocation);
        assert!(!app.sort_ascending);
    }

    #[test]
    fn positions_submode_p_sorts_performance() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.show_sector_grouping = true;
        app.sort_field = SortField::Name;
        app.sort_ascending = true;

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('P')));
        assert!(!app.show_sector_grouping);
        assert_eq!(app.sort_field, SortField::GainPct);
        assert!(!app.sort_ascending);
    }

    #[test]
    fn positions_add_transaction_hotkey_is_i() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.portfolio_mode = PortfolioMode::Full;
        app.display_positions = vec![make_position("AAPL"), make_position("GOOG")];
        app.selected_index = 0;

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('i')));
        assert!(app.tx_form.is_some());
    }

    #[test]
    fn watchlist_inline_chart_opens_popup() {
        let mut app = make_app();
        app.db_path = PathBuf::from(format!("/tmp/pftui_test_watchlist_chart_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&app.db_path);
        app.view_mode = ViewMode::Watchlist;
        app.watchlist_entries = vec![crate::db::watchlist::WatchlistEntry {
            id: 1,
            symbol: "BTC".to_string(),
            category: "crypto".to_string(),
            group_id: 1,
            added_at: "2026-03-08T00:00:00Z".to_string(),
            target_price: None,
            target_direction: None,
        }];
        app.watchlist_selected_index = 0;

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('c')));
        assert!(app.search_chart_popup.is_some());
        assert_eq!(app.search_chart_popup.as_ref().unwrap().symbol, "BTC-USD");
    }

    #[test]
    fn watchlist_inline_remove_deletes_selected_symbol() {
        let mut app = make_app();
        app.db_path = PathBuf::from(format!("/tmp/pftui_test_watchlist_remove_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&app.db_path);
        app.view_mode = ViewMode::Watchlist;
        let conn = rusqlite::Connection::open(&app.db_path).unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        crate::db::watchlist::add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        drop(conn);
        app.load_watchlist();
        assert_eq!(app.watchlist_entries.len(), 1);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('r')));
        assert!(app.watchlist_entries.is_empty());
    }

    #[test]
    fn watchlist_inline_alert_creates_price_alert() {
        let mut app = make_app();
        app.db_path = PathBuf::from(format!("/tmp/pftui_test_watchlist_alert_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&app.db_path);
        app.view_mode = ViewMode::Watchlist;
        let conn = rusqlite::Connection::open(&app.db_path).unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        crate::db::watchlist::add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        crate::db::watchlist::set_watchlist_target(&conn, "AAPL", Some("190"), Some("above")).unwrap();
        drop(conn);
        app.load_watchlist();
        app.watchlist_selected_index = 0;

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('a')));
        let conn = rusqlite::Connection::open(&app.db_path).unwrap();
        let alerts = crate::db::alerts::list_alerts(&conn).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].symbol, "AAPL");
        assert_eq!(alerts[0].threshold, "190");
    }

    #[test]
    fn watchlist_group_switch_w_then_number() {
        let mut app = make_app();
        app.db_path = PathBuf::from(format!("/tmp/pftui_test_watchlist_groups_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&app.db_path);
        app.view_mode = ViewMode::Watchlist;
        let conn = rusqlite::Connection::open(&app.db_path).unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        crate::db::watchlist::add_to_watchlist_in_group(&conn, "AAPL", AssetCategory::Equity, 1).unwrap();
        crate::db::watchlist::add_to_watchlist_in_group(&conn, "BTC", AssetCategory::Crypto, 2).unwrap();
        drop(conn);

        app.load_watchlist();
        assert_eq!(app.watchlist_active_group, 1);
        assert_eq!(app.watchlist_entries.len(), 1);
        assert_eq!(app.watchlist_entries[0].symbol, "AAPL");

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('W')));
        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('2')));
        assert_eq!(app.watchlist_active_group, 2);
        assert_eq!(app.watchlist_entries.len(), 1);
        assert_eq!(app.watchlist_entries[0].symbol, "BTC");
    }

    fn palette_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn palette_tab_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
    }

    fn palette_enter_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    #[test]
    fn command_palette_opens_with_colon() {
        let mut app = make_app();
        assert!(!app.command_palette_open);
        app.handle_key(palette_key(':'));
        assert!(app.command_palette_open);
    }

    #[test]
    fn command_palette_tab_autocomplete() {
        let mut app = make_app();
        app.handle_key(palette_key(':'));
        for c in "view mar".chars() {
            app.handle_key(palette_key(c));
        }
        app.handle_key(palette_tab_key());
        assert_eq!(app.command_palette_input, "view markets");
    }

    #[test]
    fn command_palette_executes_view_switch() {
        let mut app = make_app();
        app.view_mode = ViewMode::Positions;
        app.handle_key(palette_key(':'));
        for c in "view news".chars() {
            app.handle_key(palette_key(c));
        }
        app.handle_key(palette_enter_key());
        assert_eq!(app.view_mode, ViewMode::News);
        assert!(!app.command_palette_open);
    }

    #[test]
    fn command_palette_executes_layout_command() {
        let mut app = make_app();
        assert_eq!(app.workspace_layout, WorkspaceLayout::Split);
        app.handle_key(palette_key(':'));
        for c in "layout compact".chars() {
            app.handle_key(palette_key(c));
        }
        app.handle_key(palette_enter_key());
        assert_eq!(app.workspace_layout, WorkspaceLayout::Compact);
    }

    #[test]
    fn command_palette_scan_opens_builder_modal() {
        let mut app = make_app();
        assert!(!app.scan_builder_open);
        app.handle_key(palette_key(':'));
        for c in "scan".chars() {
            app.handle_key(palette_key(c));
        }
        app.handle_key(palette_enter_key());
        assert!(app.scan_builder_open);
        assert!(!app.command_palette_open);
    }

    #[test]
    fn allocation_targets_loaded_on_init() {
        use std::path::PathBuf;
        let db_path = PathBuf::from("/tmp/pftui_test_drift.db");
        
        // Clean up any existing test db
        let _ = std::fs::remove_file(&db_path);
        
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        
        // Add a target
        crate::db::allocation_targets::set_target(&conn, "BTC", dec!(15), dec!(2)).unwrap();
        drop(conn);

        let config = Config {
            base_currency: "USD".to_string(),
            refresh_interval: 60,
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Full,
            theme: "default".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
        };
        let mut app = App::new(&config, db_path);
        app.init_offline();

        assert!(app.allocation_targets.contains_key("BTC"));
        assert_eq!(app.allocation_targets.get("BTC").unwrap().target_pct, dec!(15));
        assert_eq!(app.allocation_targets.get("BTC").unwrap().drift_band_pct, dec!(2));
    }
}
