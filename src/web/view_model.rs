use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::config::Config;
use crate::models::transaction::Transaction;

pub const WEB_TABS_FULL: &[&str] = &[
    "Positions",
    "Transactions",
    "Markets",
    "Economy",
    "Watchlist",
    "Alerts",
    "News",
    "Journal",
];

pub const WEB_TABS_PERCENTAGE: &[&str] = &[
    "Positions",
    "Markets",
    "Economy",
    "Watchlist",
    "Alerts",
    "News",
    "Journal",
];

pub fn tabs_for_config(config: &Config) -> Vec<&'static str> {
    if config.is_percentage_mode() {
        WEB_TABS_PERCENTAGE.to_vec()
    } else {
        WEB_TABS_FULL.to_vec()
    }
}

#[derive(Serialize, Clone)]
pub struct ResponseMeta {
    pub last_refresh_at: String,
    pub stale_after_sec: u64,
    pub source_status: String,
    pub auth_required: bool,
    pub transport: String,
}

pub fn fresh_meta(stale_after_sec: u64) -> ResponseMeta {
    ResponseMeta {
        last_refresh_at: Utc::now().to_rfc3339(),
        stale_after_sec,
        source_status: "ok".to_string(),
        auth_required: true,
        transport: "polling".to_string(),
    }
}

pub fn compute_watchlist_proximity(
    current: Option<Decimal>,
    target: Option<Decimal>,
    direction: Option<&str>,
) -> (Option<Decimal>, bool) {
    let (cur, tgt, dir) = match (current, target, direction) {
        (Some(cur), Some(tgt), Some(dir)) if tgt > dec!(0) => (cur, tgt, dir.to_lowercase()),
        _ => return (None, false),
    };
    let dist_pct = ((cur - tgt).abs() / tgt) * dec!(100);
    let hit = match dir.as_str() {
        "above" => cur >= tgt,
        "below" => cur <= tgt,
        _ => false,
    };
    (Some(dist_pct), hit)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TxSortField {
    Date,
    Symbol,
    Type,
    Quantity,
    Price,
    Fee,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl TxSortField {
    pub fn from_str(s: &str) -> Self {
        match s {
            "symbol" => Self::Symbol,
            "type" => Self::Type,
            "qty" => Self::Quantity,
            "price" => Self::Price,
            "fee" => Self::Fee,
            _ => Self::Date,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Symbol => "symbol",
            Self::Type => "type",
            Self::Quantity => "qty",
            Self::Price => "price",
            Self::Fee => "fee",
        }
    }
}

impl SortOrder {
    pub fn from_str(s: &str) -> Self {
        match s {
            "asc" => Self::Asc,
            _ => Self::Desc,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

pub fn apply_transaction_filters(
    txs: Vec<Transaction>,
    symbol: Option<&str>,
    tx_type: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Vec<Transaction> {
    txs.into_iter()
        .filter(|tx| {
            if let Some(sym) = symbol {
                if !tx.symbol.eq_ignore_ascii_case(sym) {
                    return false;
                }
            }
            if let Some(kind) = tx_type {
                if tx.tx_type.to_string() != kind.to_lowercase() {
                    return false;
                }
            }
            if let Some(from_date) = from {
                if tx.date.as_str() < from_date {
                    return false;
                }
            }
            if let Some(to_date) = to {
                if tx.date.as_str() > to_date {
                    return false;
                }
            }
            true
        })
        .collect()
}

pub fn sort_transactions(
    txs: &mut [Transaction],
    sort_by: TxSortField,
    sort_order: SortOrder,
) {
    txs.sort_by(|a, b| {
        let ord = match sort_by {
            TxSortField::Date => a.date.cmp(&b.date).then(a.id.cmp(&b.id)),
            TxSortField::Symbol => a.symbol.cmp(&b.symbol).then(a.id.cmp(&b.id)),
            TxSortField::Type => a
                .tx_type
                .to_string()
                .cmp(&b.tx_type.to_string())
                .then(a.id.cmp(&b.id)),
            TxSortField::Quantity => a.quantity.cmp(&b.quantity).then(a.id.cmp(&b.id)),
            TxSortField::Price => a.price_per.cmp(&b.price_per).then(a.id.cmp(&b.id)),
            TxSortField::Fee => {
                let af = tx_fee(a);
                let bf = tx_fee(b);
                af.cmp(&bf).then(a.id.cmp(&b.id))
            }
        };
        if sort_order == SortOrder::Desc {
            ord.reverse()
        } else {
            ord
        }
    });
}

fn tx_fee(tx: &Transaction) -> Decimal {
    tx.notes
        .as_deref()
        .and_then(|n| n.strip_prefix("fee:"))
        .and_then(|v| v.trim().parse::<Decimal>().ok())
        .unwrap_or(dec!(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::asset::AssetCategory;
    use crate::models::transaction::TxType;

    fn sample_tx(id: i64, symbol: &str, tx_type: TxType, qty: i64, price: i64, date: &str) -> Transaction {
        Transaction {
            id,
            symbol: symbol.to_string(),
            category: AssetCategory::Equity,
            tx_type,
            quantity: Decimal::from(qty),
            price_per: Decimal::from(price),
            currency: "USD".to_string(),
            date: date.to_string(),
            notes: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn watchlist_proximity_hit() {
        let (dist, hit) = compute_watchlist_proximity(Some(dec!(120)), Some(dec!(100)), Some("above"));
        assert!(hit);
        assert!(dist.is_some());
    }

    #[test]
    fn tx_sort_symbol_desc() {
        let mut txs = vec![
            sample_tx(1, "AAPL", TxType::Buy, 1, 100, "2026-01-01"),
            sample_tx(2, "TSLA", TxType::Buy, 1, 100, "2026-01-01"),
        ];
        sort_transactions(&mut txs, TxSortField::Symbol, SortOrder::Desc);
        assert_eq!(txs[0].symbol, "TSLA");
    }

    #[test]
    fn tx_filter_type() {
        let txs = vec![
            sample_tx(1, "AAPL", TxType::Buy, 1, 100, "2026-01-01"),
            sample_tx(2, "AAPL", TxType::Sell, 1, 100, "2026-01-02"),
        ];
        let filtered = apply_transaction_filters(txs, Some("AAPL"), Some("sell"), None, None);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].tx_type, TxType::Sell);
    }
}
