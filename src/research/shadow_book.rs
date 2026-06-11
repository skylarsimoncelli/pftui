//! Shadow book — the counterfactual portfolio that mechanically executes
//! every recommendations-ledger row, so "does following the desk beat
//! ignoring it?" becomes a number instead of a feeling.
//!
//! Three books, all seeded with the OPERATOR'S ACTUAL holdings at inception
//! (the first ledger row's run_date), so shadow-vs-actual is a pure
//! *decisions-since-inception* comparison:
//!
//! - **SHADOW** — executes every ledger row under the mechanical policy.
//! - **ACTUAL** — the operator's real transactions as they happened,
//!   valued daily.
//! - **HOLD** — inception holdings frozen; no trades ever.
//!
//! ## Mechanical policy (POLICY V1 — version any change)
//!
//! - `add`  → move **+1.0pp of total NAV** from cash into the symbol at the
//!   row's `entry_price`. Skipped (with a warning) when cash < 1pp of NAV.
//! - `trim` → move **−1.0pp of total NAV** from the symbol to cash at the
//!   row's `entry_price` (capped at the held value; skipped when the
//!   position is empty).
//! - `wait` / `hold` / `avoid` → no trade (counted as waits).
//! - Multiple same-day rows apply in `id` order; NAV is re-marked before
//!   each trade so same-day rows see the cash consumed by earlier ones.
//! - Unpriced ledger rows (no `entry_price`) are skipped with a warning.
//!
//! Everything is computed on demand from `recommendations` +
//! `price_history` + `transactions` — no state tables. Inception is derived
//! from the ledger, never pinned. Daily marks use the closest close on or
//! before each date (LOCF), with the `SYM` → `SYM-USD` deep-series
//! fallback. Cash (category `cash` transactions) is valued at face value;
//! external flows (deposits/withdrawals after inception) are NOT adjusted
//! out of the ACTUAL return in policy v1 — they are listed as a caveat.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use anyhow::{anyhow, Result};
use chrono::NaiveDate;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::recommendations::{self, Recommendation, LEDGER_ACTIONS};
use crate::models::asset::AssetCategory;
use crate::models::transaction::{Transaction, TxType};

/// The mechanical execution policy version. Bump on ANY policy change —
/// published shadow-book numbers bind to this string.
pub const POLICY_VERSION: &str = "policy v1";

/// Trade size as a fraction of total NAV (1.0pp).
const TRADE_FRACTION: Decimal = Decimal::from_parts(1, 0, 0, false, 2); // 0.01

/// Days of ledger history before the verdict stops carrying the
/// "benchmark accruing" banner.
pub const ACCRUING_DAYS: i64 = 90;

#[derive(Debug, Clone, Serialize)]
pub struct ExecutedTrade {
    pub rec_id: i64,
    pub date: String,
    pub symbol: String,
    pub action: String,
    /// Units bought (add) or sold (trim).
    pub quantity: Decimal,
    pub entry_price: Decimal,
    /// NAV moved by the trade (1pp of NAV at execution, capped for trims).
    pub trade_value: Decimal,
    /// P&L of having executed vs not (cash assumed flat): for `add`,
    /// qty × (latest − entry); for `trim`, qty × (entry − latest).
    /// None when the symbol has no close on or before `as_of`.
    pub pnl_vs_skip: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedTrade {
    pub rec_id: i64,
    pub date: String,
    pub symbol: String,
    pub action: String,
    pub reason: String,
}

/// One ledger row's disposition under the mechanical policy.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerRowDisposition {
    pub rec_id: i64,
    pub date: String,
    pub symbol: String,
    pub action: String,
    /// `executed` | `skipped` | `no-trade`
    pub disposition: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NavPoint {
    pub date: String,
    pub shadow: Decimal,
    pub actual: Decimal,
    pub hold: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShadowBookReport {
    pub policy_version: String,
    pub inception: String,
    pub as_of: String,
    pub days: i64,
    /// True while the benchmark has under [`ACCRUING_DAYS`] of history.
    pub accruing: bool,
    pub inception_nav: Decimal,
    pub shadow_nav: Decimal,
    pub actual_nav: Decimal,
    pub hold_nav: Decimal,
    pub shadow_return_pct: f64,
    pub actual_return_pct: f64,
    pub hold_return_pct: f64,
    pub executed: Vec<ExecutedTrade>,
    pub skipped: Vec<SkippedTrade>,
    /// wait / hold / avoid rows (recorded, mechanically no-trade).
    pub waits: usize,
    /// Every ledger row in execution order with its policy disposition.
    pub ledger_rows: Vec<LedgerRowDisposition>,
    pub nav_series: Vec<NavPoint>,
    pub verdict: String,
    pub warnings: Vec<String>,
}

/// One symbol's chronological close map (date → close).
struct PriceSeries {
    closes: BTreeMap<String, Decimal>,
}

impl PriceSeries {
    /// Closest close on or before `date` (LOCF).
    fn at(&self, date: &str) -> Option<Decimal> {
        self.closes
            .range(..=date.to_string())
            .next_back()
            .map(|(_, v)| *v)
    }
}

/// Load the full close series for `symbol`, falling back to the
/// `SYMBOL-USD` deep series when the bare symbol has no history.
fn load_series(conn: &Connection, symbol: &str) -> Result<Option<PriceSeries>> {
    let fetch = |sym: &str| -> Result<BTreeMap<String, Decimal>> {
        let mut stmt =
            conn.prepare("SELECT date, close FROM price_history WHERE symbol = ?1 ORDER BY date")?;
        let rows = stmt.query_map([sym], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut closes = BTreeMap::new();
        for row in rows {
            let (date, close) = row?;
            if let Ok(c) = Decimal::from_str(&close) {
                if c != Decimal::ZERO {
                    closes.insert(date, c);
                }
            }
        }
        Ok(closes)
    };
    let closes = fetch(symbol)?;
    if !closes.is_empty() {
        return Ok(Some(PriceSeries { closes }));
    }
    if !symbol.to_uppercase().ends_with("-USD") {
        let twin = format!("{symbol}-USD");
        let closes = fetch(&twin)?;
        if !closes.is_empty() {
            return Ok(Some(PriceSeries { closes }));
        }
    }
    Ok(None)
}

/// A point-in-time book: per-symbol quantities plus a cash balance.
#[derive(Debug, Clone, Default)]
struct Book {
    qty: BTreeMap<String, Decimal>,
    cash: Decimal,
}

impl Book {
    fn mark(&self, prices: &HashMap<String, Option<PriceSeries>>, date: &str) -> Decimal {
        let mut nav = self.cash;
        for (sym, qty) in &self.qty {
            if qty.is_zero() {
                continue;
            }
            if let Some(Some(series)) = prices.get(sym).map(|s| s.as_ref()) {
                if let Some(close) = series.at(date) {
                    nav += *qty * close;
                }
            }
        }
        nav
    }
}

fn is_cash(tx: &Transaction) -> bool {
    tx.category == AssetCategory::Cash
}

/// Net book from `transactions` with `date <= cutoff` (inclusive).
fn book_at(transactions: &[Transaction], cutoff: &str) -> Book {
    let mut book = Book::default();
    for tx in transactions {
        if tx.date.as_str() > cutoff {
            continue;
        }
        let signed = match tx.tx_type {
            TxType::Buy => tx.quantity,
            TxType::Sell => -tx.quantity,
        };
        if is_cash(tx) {
            book.cash += signed;
        } else {
            *book.qty.entry(tx.symbol.to_uppercase()).or_default() += signed;
        }
    }
    book
}

/// Ledger rows in execution order: `report_date` asc, `id` asc.
fn ledger_rows(conn: &Connection) -> Result<Vec<Recommendation>> {
    recommendations::ensure_table(conn)?;
    let mut rows = recommendations::list(conn, None, None, None, None)?;
    rows.retain(|r| {
        LEDGER_ACTIONS.contains(&r.recommendation_type.as_str()) && r.asset.is_some()
    });
    rows.sort_by(|a, b| {
        a.report_date
            .cmp(&b.report_date)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(rows)
}

/// Compute the shadow book as of `today` (YYYY-MM-DD). Returns `None` when
/// the recommendations ledger has no action rows (no inception to derive).
pub fn compute(conn: &Connection, today: &str) -> Result<Option<ShadowBookReport>> {
    let today_date = NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .map_err(|_| anyhow!("invalid date '{today}': expected YYYY-MM-DD"))?;

    let ledger = ledger_rows(conn)?;
    let Some(first) = ledger.first() else {
        return Ok(None);
    };
    let inception = first.report_date.clone();
    let inception_date = NaiveDate::parse_from_str(&inception, "%Y-%m-%d")
        .map_err(|_| anyhow!("ledger inception date '{inception}' is not YYYY-MM-DD"))?;
    if inception_date > today_date {
        return Ok(None);
    }
    let days = (today_date - inception_date).num_days();

    let transactions = crate::db::transactions::list_transactions(conn)?;
    let mut warnings: Vec<String> = Vec::new();

    // Cash symbols (category `cash` in transactions, plus USD) are valued at
    // face — never priced from a series and never traded directly by the
    // policy (a ledger `hold USD` row is a stance on cash, not a trade).
    let mut cash_symbols: std::collections::HashSet<String> =
        std::collections::HashSet::from(["USD".to_string()]);
    for tx in &transactions {
        if is_cash(tx) {
            cash_symbols.insert(tx.symbol.to_uppercase());
        }
    }

    // Price series for every symbol the books or the ledger can touch.
    let mut prices: HashMap<String, Option<PriceSeries>> = HashMap::new();
    let want = |prices: &mut HashMap<String, Option<PriceSeries>>, sym: &str| -> Result<()> {
        let key = sym.to_uppercase();
        if let std::collections::hash_map::Entry::Vacant(slot) = prices.entry(key.clone()) {
            slot.insert(load_series(conn, &key)?);
        }
        Ok(())
    };
    for tx in &transactions {
        if !is_cash(tx) {
            want(&mut prices, &tx.symbol)?;
        }
    }
    for row in &ledger {
        // Prefer the series that priced the row (e.g. BTC-USD for BTC).
        if let Some(asset) = &row.asset {
            if !cash_symbols.contains(&asset.to_uppercase()) {
                want(&mut prices, asset)?;
            }
        }
    }
    for (sym, series) in &prices {
        if series.is_none() {
            warnings.push(format!(
                "{sym}: no price history (bare or -USD) — valued at 0 in every book"
            ));
        }
    }

    // Inception state: the operator's ACTUAL book at inception.
    let inception_book = book_at(&transactions, &inception);
    let inception_nav = inception_book.mark(&prices, &inception);
    if inception_nav <= Decimal::ZERO {
        warnings.push(
            "inception NAV is zero or negative — returns are not computable".to_string(),
        );
    }

    // SHADOW: replay the ledger mechanically.
    let mut shadow = inception_book.clone();
    let mut executed: Vec<ExecutedTrade> = Vec::new();
    let mut skipped: Vec<SkippedTrade> = Vec::new();
    let mut waits = 0usize;
    for row in &ledger {
        let symbol = row.asset.clone().unwrap_or_default().to_uppercase();
        let action = row.recommendation_type.as_str();
        match action {
            "wait" | "hold" | "avoid" => {
                waits += 1;
                continue;
            }
            "add" | "trim" => {}
            _ => continue,
        }
        if cash_symbols.contains(&symbol) {
            skipped.push(SkippedTrade {
                rec_id: row.id,
                date: row.report_date.clone(),
                symbol,
                action: action.to_string(),
                reason: "cash symbol — policy v1 does not trade cash directly".to_string(),
            });
            continue;
        }
        let Some(entry) = row
            .entry_price
            .as_deref()
            .and_then(|p| Decimal::from_str(p).ok())
            .filter(|p| !p.is_zero())
        else {
            skipped.push(SkippedTrade {
                rec_id: row.id,
                date: row.report_date.clone(),
                symbol,
                action: action.to_string(),
                reason: "no entry_price on ledger row (unpriced at record time)".to_string(),
            });
            continue;
        };
        let nav = shadow.mark(&prices, &row.report_date);
        let trade_value = nav * TRADE_FRACTION;
        if trade_value <= Decimal::ZERO {
            skipped.push(SkippedTrade {
                rec_id: row.id,
                date: row.report_date.clone(),
                symbol,
                action: action.to_string(),
                reason: "shadow NAV is zero — nothing to size against".to_string(),
            });
            continue;
        }
        match action {
            "add" => {
                if shadow.cash < trade_value {
                    skipped.push(SkippedTrade {
                        rec_id: row.id,
                        date: row.report_date.clone(),
                        symbol,
                        action: action.to_string(),
                        reason: format!(
                            "cash floor: cash {} < 1pp of NAV {}",
                            shadow.cash.round_dp(2),
                            trade_value.round_dp(2)
                        ),
                    });
                    continue;
                }
                let qty = trade_value / entry;
                *shadow.qty.entry(symbol.clone()).or_default() += qty;
                shadow.cash -= trade_value;
                executed.push(ExecutedTrade {
                    rec_id: row.id,
                    date: row.report_date.clone(),
                    symbol,
                    action: action.to_string(),
                    quantity: qty,
                    entry_price: entry,
                    trade_value,
                    pnl_vs_skip: None,
                });
            }
            "trim" => {
                let held = shadow.qty.get(&symbol).copied().unwrap_or_default();
                if held <= Decimal::ZERO {
                    skipped.push(SkippedTrade {
                        rec_id: row.id,
                        date: row.report_date.clone(),
                        symbol,
                        action: action.to_string(),
                        reason: "no shadow position to trim".to_string(),
                    });
                    continue;
                }
                let held_value = held * entry;
                let sell_value = trade_value.min(held_value);
                let qty = sell_value / entry;
                *shadow.qty.entry(symbol.clone()).or_default() -= qty;
                shadow.cash += sell_value;
                executed.push(ExecutedTrade {
                    rec_id: row.id,
                    date: row.report_date.clone(),
                    symbol,
                    action: action.to_string(),
                    quantity: qty,
                    entry_price: entry,
                    trade_value: sell_value,
                    pnl_vs_skip: None,
                });
            }
            _ => unreachable!(),
        }
    }

    // Per-row disposition listing (execution order).
    let executed_ids: std::collections::HashSet<i64> = executed.iter().map(|t| t.rec_id).collect();
    let skipped_ids: std::collections::HashSet<i64> = skipped.iter().map(|t| t.rec_id).collect();
    let ledger_dispositions: Vec<LedgerRowDisposition> = ledger
        .iter()
        .map(|row| LedgerRowDisposition {
            rec_id: row.id,
            date: row.report_date.clone(),
            symbol: row.asset.clone().unwrap_or_default().to_uppercase(),
            action: row.recommendation_type.clone(),
            disposition: if executed_ids.contains(&row.id) {
                "executed".to_string()
            } else if skipped_ids.contains(&row.id) {
                "skipped".to_string()
            } else {
                "no-trade".to_string()
            },
        })
        .collect();

    // Attribution: each executed trade's P&L vs not having done it,
    // marked at the latest close on or before `today` (cash assumed flat).
    for trade in &mut executed {
        let latest = prices
            .get(&trade.symbol)
            .and_then(|s| s.as_ref())
            .and_then(|s| s.at(today));
        trade.pnl_vs_skip = latest.map(|close| match trade.action.as_str() {
            "trim" => trade.quantity * (trade.entry_price - close),
            _ => trade.quantity * (close - trade.entry_price),
        });
    }

    // Daily NAV series for all three books.
    let mut nav_series: Vec<NavPoint> = Vec::new();
    let mut day = inception_date;
    while day <= today_date {
        let date = day.format("%Y-%m-%d").to_string();
        // Shadow NAV across the series requires replaying trades date by
        // date; rather than re-simulating, mark a book that contains only
        // trades executed on or before `date`.
        let mut shadow_at = inception_book.clone();
        for t in &executed {
            if t.date.as_str() > date.as_str() {
                continue;
            }
            match t.action.as_str() {
                "add" => {
                    *shadow_at.qty.entry(t.symbol.clone()).or_default() += t.quantity;
                    shadow_at.cash -= t.trade_value;
                }
                "trim" => {
                    *shadow_at.qty.entry(t.symbol.clone()).or_default() -= t.quantity;
                    shadow_at.cash += t.trade_value;
                }
                _ => {}
            }
        }
        nav_series.push(NavPoint {
            date: date.clone(),
            shadow: shadow_at.mark(&prices, &date),
            actual: book_at(&transactions, &date).mark(&prices, &date),
            hold: inception_book.mark(&prices, &date),
        });
        day += chrono::Duration::days(1);
    }

    let last = nav_series
        .last()
        .ok_or_else(|| anyhow!("empty NAV series despite valid inception"))?;
    let (shadow_nav, actual_nav, hold_nav) = (last.shadow, last.actual, last.hold);
    let ret = |nav: Decimal| -> f64 {
        if inception_nav <= Decimal::ZERO {
            return 0.0;
        }
        ((nav - inception_nav) / inception_nav * Decimal::from(100))
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0)
    };
    let shadow_return_pct = ret(shadow_nav);
    let actual_return_pct = ret(actual_nav);
    let hold_return_pct = ret(hold_nav);

    let verdict = format!(
        "Shadow {shadow_return_pct:+.2}% vs Actual {actual_return_pct:+.2}% vs Hold {hold_return_pct:+.2}% since {inception} (n={} executed trades, {waits} waits)",
        executed.len()
    );

    Ok(Some(ShadowBookReport {
        policy_version: POLICY_VERSION.to_string(),
        inception,
        as_of: today.to_string(),
        days,
        accruing: days < ACCRUING_DAYS,
        inception_nav,
        shadow_nav,
        actual_nav,
        hold_nav,
        shadow_return_pct,
        actual_return_pct,
        hold_return_pct,
        executed,
        skipped,
        waits,
        ledger_rows: ledger_dispositions,
        nav_series,
        verdict,
        warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                category TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                date TEXT NOT NULL,
                notes TEXT,
                paired_tx_id INTEGER REFERENCES transactions(id),
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'test',
                PRIMARY KEY (symbol, date)
            );",
        )
        .expect("schema");
        recommendations::ensure_table(&conn).expect("recommendations schema");
        conn
    }

    fn tx(conn: &Connection, symbol: &str, category: &str, tx_type: &str, qty: &str, date: &str) {
        conn.execute(
            "INSERT INTO transactions (symbol, category, tx_type, quantity, price_per, currency, date)
             VALUES (?1, ?2, ?3, ?4, '1', 'USD', ?5)",
            rusqlite::params![symbol, category, tx_type, qty, date],
        )
        .expect("insert tx");
    }

    fn close(conn: &Connection, symbol: &str, date: &str, price: &str) {
        conn.execute(
            "INSERT OR REPLACE INTO price_history (symbol, date, close, source)
             VALUES (?1, ?2, ?3, 'test')",
            rusqlite::params![symbol, date, price],
        )
        .expect("insert close");
    }

    fn ledger(conn: &Connection, date: &str, symbol: &str, action: &str) -> i64 {
        recommendations::record_ledger_entry(conn, date, symbol, action, None, "test")
            .expect("ledger row")
            .id
    }

    /// Fixture: GOLD held 10 units, cash 1000, inception 2026-01-02.
    /// GOLD: 100 at inception → 120 at as-of (+20%).
    fn basic_fixture(conn: &Connection) {
        tx(conn, "GOLD", "commodity", "buy", "10", "2025-12-01");
        tx(conn, "USD", "cash", "buy", "1000", "2025-12-01");
        close(conn, "GOLD", "2026-01-02", "100");
        close(conn, "GOLD", "2026-02-01", "110");
        close(conn, "GOLD", "2026-03-01", "120");
    }

    #[test]
    fn no_ledger_rows_means_no_report() {
        let conn = test_conn();
        assert!(compute(&conn, "2026-03-01").unwrap().is_none());
    }

    #[test]
    fn add_executes_one_pp_of_nav_and_beats_hold_on_rising_asset() {
        let conn = test_conn();
        basic_fixture(&conn);
        ledger(&conn, "2026-01-02", "GOLD", "add");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.inception, "2026-01-02");
        // Inception NAV: 10×100 + 1000 cash = 2000.
        assert_eq!(r.inception_nav, dec!(2000));
        assert_eq!(r.executed.len(), 1);
        let t = &r.executed[0];
        // 1pp of 2000 = 20 → 0.2 units at 100.
        assert_eq!(t.trade_value, dec!(20));
        assert_eq!(t.quantity, dec!(0.2));
        // Hold: 10×120 + 1000 = 2200 (+10%).
        assert_eq!(r.hold_nav, dec!(2200));
        assert!((r.hold_return_pct - 10.0).abs() < 1e-9);
        // Shadow: 10.2×120 + 980 = 2204 (+10.2%) — add helped.
        assert_eq!(r.shadow_nav, dec!(2204.0));
        assert!(r.shadow_return_pct > r.hold_return_pct);
        // Attribution: 0.2 × (120 − 100) = +4.
        assert_eq!(t.pnl_vs_skip, Some(dec!(4.0)));
        // No operator trades after inception → actual == hold.
        assert_eq!(r.actual_nav, r.hold_nav);
        assert!(r.verdict.contains("n=1 executed trades, 0 waits"));
    }

    #[test]
    fn waits_are_counted_not_traded() {
        let conn = test_conn();
        basic_fixture(&conn);
        ledger(&conn, "2026-01-02", "GOLD", "wait");
        ledger(&conn, "2026-01-02", "GOLD", "hold");
        ledger(&conn, "2026-01-02", "GOLD", "avoid");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.executed.len(), 0);
        assert_eq!(r.waits, 3);
        assert_eq!(r.shadow_nav, r.hold_nav);
    }

    #[test]
    fn cash_floor_skips_add_with_warning() {
        let conn = test_conn();
        // 10 GOLD @ 100, cash only 5 (< 1pp of 1005 NAV = 10.05).
        tx(&conn, "GOLD", "commodity", "buy", "10", "2025-12-01");
        tx(&conn, "USD", "cash", "buy", "5", "2025-12-01");
        close(&conn, "GOLD", "2026-01-02", "100");
        close(&conn, "GOLD", "2026-03-01", "120");
        ledger(&conn, "2026-01-02", "GOLD", "add");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.executed.len(), 0);
        assert_eq!(r.skipped.len(), 1);
        assert!(r.skipped[0].reason.contains("cash floor"));
        assert_eq!(r.shadow_nav, r.hold_nav);
    }

    #[test]
    fn same_day_rows_apply_in_id_order_and_drain_cash_sequentially() {
        let conn = test_conn();
        // NAV 2000, cash 25: first add (1pp = 20) executes, second add
        // sees cash 5 < 1pp of post-trade NAV and is skipped.
        tx(&conn, "GOLD", "commodity", "buy", "10", "2025-12-01");
        tx(&conn, "USD", "cash", "buy", "25", "2025-12-01");
        close(&conn, "GOLD", "2026-01-02", "197.5");
        close(&conn, "GOLD", "2026-03-01", "200");
        let first = ledger(&conn, "2026-01-02", "GOLD", "add");
        let second = ledger(&conn, "2026-01-02", "GOLD", "add");
        assert!(first < second);

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.executed.len(), 1);
        assert_eq!(r.executed[0].rec_id, first);
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].rec_id, second);
        assert!(r.skipped[0].reason.contains("cash floor"));
    }

    #[test]
    fn trim_moves_value_to_cash_and_caps_at_held_value() {
        let conn = test_conn();
        basic_fixture(&conn);
        ledger(&conn, "2026-01-02", "GOLD", "trim");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.executed.len(), 1);
        let t = &r.executed[0];
        // 1pp of 2000 = 20 → sells 0.2 units at 100.
        assert_eq!(t.trade_value, dec!(20));
        assert_eq!(t.quantity, dec!(0.2));
        // Shadow: 9.8×120 + 1020 = 2196 < hold 2200 (trim before a rally hurt).
        assert_eq!(r.shadow_nav, dec!(2196.0));
        assert!(r.shadow_return_pct < r.hold_return_pct);
        // Attribution: 0.2 × (100 − 120) = −4 (the trim cost 4 vs holding).
        assert_eq!(t.pnl_vs_skip, Some(dec!(-4.0)));
    }

    #[test]
    fn trim_on_absent_symbol_is_skipped() {
        let conn = test_conn();
        basic_fixture(&conn);
        close(&conn, "SILVER", "2026-01-02", "30");
        ledger(&conn, "2026-01-02", "SILVER", "trim");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.executed.len(), 0);
        assert_eq!(r.skipped.len(), 1);
        assert!(r.skipped[0].reason.contains("no shadow position"));
    }

    #[test]
    fn unpriced_ledger_row_is_skipped() {
        let conn = test_conn();
        basic_fixture(&conn);
        // No price history for MYSTERY → record_ledger_entry stores no entry_price.
        ledger(&conn, "2026-01-02", "MYSTERY", "add");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.executed.len(), 0);
        assert_eq!(r.skipped.len(), 1);
        assert!(r.skipped[0].reason.contains("no entry_price"));
    }

    #[test]
    fn actual_book_tracks_operator_trades_after_inception() {
        let conn = test_conn();
        basic_fixture(&conn);
        ledger(&conn, "2026-01-02", "GOLD", "wait");
        // Operator buys 5 more GOLD after inception (paired cash out 550).
        tx(&conn, "GOLD", "commodity", "buy", "5", "2026-02-01");
        tx(&conn, "USD", "cash", "sell", "550", "2026-02-01");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        // Hold: 10×120 + 1000 = 2200. Actual: 15×120 + 450 = 2250.
        assert_eq!(r.hold_nav, dec!(2200));
        assert_eq!(r.actual_nav, dec!(2250));
        assert_eq!(r.shadow_nav, r.hold_nav); // wait = no shadow trade
    }

    #[test]
    fn accruing_banner_under_90_days() {
        let conn = test_conn();
        basic_fixture(&conn);
        ledger(&conn, "2026-01-02", "GOLD", "add");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.days, 58);
        assert!(r.accruing);

        close(&conn, "GOLD", "2026-04-15", "125");
        let r = compute(&conn, "2026-04-15").unwrap().unwrap();
        assert_eq!(r.days, 103);
        assert!(!r.accruing);
    }

    #[test]
    fn deep_series_fallback_marks_bare_crypto_symbol() {
        let conn = test_conn();
        tx(&conn, "BTC", "crypto", "buy", "1", "2025-12-01");
        tx(&conn, "USD", "cash", "buy", "1000", "2025-12-01");
        // Only the deep series exists.
        close(&conn, "BTC-USD", "2026-01-02", "50000");
        close(&conn, "BTC-USD", "2026-03-01", "60000");
        ledger(&conn, "2026-01-02", "BTC", "add");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert_eq!(r.inception_nav, dec!(51000));
        assert_eq!(r.executed.len(), 1);
        assert!(r.warnings.is_empty());
        // Shadow gained on the added 1pp slice: shadow > hold.
        assert!(r.shadow_nav > r.hold_nav);
    }

    #[test]
    fn cash_ledger_rows_never_warn_and_never_trade() {
        let conn = test_conn();
        basic_fixture(&conn);
        // A hold on cash is a stance, not a trade — and an add on cash is
        // meaningless under policy v1 (cash → cash).
        ledger(&conn, "2026-01-02", "USD", "hold");
        ledger(&conn, "2026-01-03", "USD", "add");

        let r = compute(&conn, "2026-03-01").unwrap().unwrap();
        assert!(r.warnings.is_empty(), "warnings: {:?}", r.warnings);
        assert_eq!(r.waits, 1);
        assert_eq!(r.executed.len(), 0);
        assert_eq!(r.skipped.len(), 1);
        assert!(r.skipped[0].reason.contains("cash symbol"));
        assert_eq!(r.shadow_nav, r.hold_nav);
    }

    #[test]
    fn nav_series_spans_inception_to_as_of() {
        let conn = test_conn();
        basic_fixture(&conn);
        ledger(&conn, "2026-01-02", "GOLD", "add");
        let r = compute(&conn, "2026-01-05").unwrap().unwrap();
        assert_eq!(r.nav_series.len(), 4);
        assert_eq!(r.nav_series[0].date, "2026-01-02");
        assert_eq!(r.nav_series[3].date, "2026-01-05");
        // Day 0 marks: shadow == hold == actual == inception NAV (trade is
        // value-neutral at entry).
        assert_eq!(r.nav_series[0].shadow, r.nav_series[0].hold);
    }
}
