use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use super::allocation::Allocation;
use super::asset::AssetCategory;
use super::asset_names;
use super::transaction::{Transaction, TxType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub name: String,
    pub category: AssetCategory,
    pub quantity: Decimal,
    pub avg_cost: Decimal,
    pub total_cost: Decimal,
    pub currency: String,
    pub current_price: Option<Decimal>,
    pub current_value: Option<Decimal>,
    pub gain: Option<Decimal>,
    pub gain_pct: Option<Decimal>,
    pub allocation_pct: Option<Decimal>,
}

pub fn compute_positions(
    transactions: &[Transaction],
    prices: &HashMap<String, Decimal>,
) -> Vec<Position> {
    // Group transactions by symbol
    let mut groups: HashMap<String, Vec<&Transaction>> = HashMap::new();
    for tx in transactions {
        groups.entry(tx.symbol.clone()).or_default().push(tx);
    }

    let mut positions = Vec::new();

    for (symbol, txs) in &groups {
        let mut qty = dec!(0);
        let mut total_cost = dec!(0);
        let mut currency = String::from("USD");
        let mut category = AssetCategory::Equity;

        for tx in txs {
            category = tx.category;
            currency = tx.currency.clone();
            match tx.tx_type {
                TxType::Buy => {
                    total_cost += tx.quantity * tx.price_per;
                    qty += tx.quantity;
                }
                TxType::Sell => {
                    if qty > dec!(0) {
                        // Reduce cost basis proportionally
                        let avg = total_cost / qty;
                        qty -= tx.quantity;
                        total_cost = avg * qty;
                    }
                }
            }
        }

        if qty <= dec!(0) {
            continue;
        }

        let avg_cost = total_cost / qty;
        let current_price = if category == AssetCategory::Cash {
            Some(dec!(1)) // 1 unit of cash = 1 unit of cash
        } else {
            prices.get(symbol.as_str()).copied()
        };
        let current_value = current_price.map(|p| p * qty);
        let gain = current_value.map(|v| v - total_cost);
        let gain_pct = if total_cost > dec!(0) {
            gain.map(|g| (g / total_cost) * dec!(100))
        } else {
            None
        };

        positions.push(Position {
            symbol: symbol.clone(),
            name: asset_names::resolve_name(symbol),
            category,
            quantity: qty,
            avg_cost,
            total_cost,
            currency,
            current_price,
            current_value,
            gain,
            gain_pct,
            allocation_pct: None, // computed after all positions
        });
    }

    // Compute allocation percentages
    let total_value: Decimal = positions
        .iter()
        .filter_map(|p| p.current_value)
        .sum();

    if total_value > dec!(0) {
        for pos in &mut positions {
            if let Some(val) = pos.current_value {
                pos.allocation_pct = Some((val / total_value) * dec!(100));
            }
        }
    }

    positions
}

pub fn compute_positions_from_allocations(
    allocations: &[Allocation],
    prices: &HashMap<String, Decimal>,
) -> Vec<Position> {
    allocations
        .iter()
        .map(|alloc| {
            let current_price = if alloc.category == AssetCategory::Cash {
                Some(dec!(1))
            } else {
                prices.get(&alloc.symbol).copied()
            };
            Position {
                symbol: alloc.symbol.clone(),
                name: asset_names::resolve_name(&alloc.symbol),
                category: alloc.category,
                quantity: dec!(0),
                avg_cost: dec!(0),
                total_cost: dec!(0),
                currency: "USD".to_string(),
                current_price,
                current_value: None,
                gain: None,
                gain_pct: None,
                allocation_pct: Some(alloc.allocation_pct),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_tx(symbol: &str, tx_type: TxType, qty: Decimal, price: Decimal) -> Transaction {
        Transaction {
            id: 0,
            symbol: symbol.to_string(),
            category: AssetCategory::Equity,
            tx_type,
            quantity: qty,
            price_per: price,
            currency: "USD".to_string(),
            date: "2025-01-01".to_string(),
            notes: None,
            created_at: "2025-01-01T00:00:00".to_string(),
        }
    }

    #[test]
    fn test_single_buy() {
        let txs = vec![make_tx("AAPL", TxType::Buy, dec!(10), dec!(150))];
        let prices = HashMap::from([("AAPL".to_string(), dec!(200))]);
        let positions = compute_positions(&txs, &prices);

        assert_eq!(positions.len(), 1);
        let p = &positions[0];
        assert_eq!(p.quantity, dec!(10));
        assert_eq!(p.avg_cost, dec!(150));
        assert_eq!(p.total_cost, dec!(1500));
        assert_eq!(p.current_value, Some(dec!(2000)));
        assert_eq!(p.gain, Some(dec!(500)));
    }

    #[test]
    fn test_buy_sell() {
        let txs = vec![
            make_tx("AAPL", TxType::Buy, dec!(10), dec!(100)),
            make_tx("AAPL", TxType::Sell, dec!(5), dec!(150)),
        ];
        let prices = HashMap::from([("AAPL".to_string(), dec!(200))]);
        let positions = compute_positions(&txs, &prices);

        assert_eq!(positions.len(), 1);
        let p = &positions[0];
        assert_eq!(p.quantity, dec!(5));
        assert_eq!(p.avg_cost, dec!(100));
        assert_eq!(p.total_cost, dec!(500));
    }

    #[test]
    fn test_fully_sold() {
        let txs = vec![
            make_tx("AAPL", TxType::Buy, dec!(10), dec!(100)),
            make_tx("AAPL", TxType::Sell, dec!(10), dec!(150)),
        ];
        let prices = HashMap::from([("AAPL".to_string(), dec!(200))]);
        let positions = compute_positions(&txs, &prices);

        assert_eq!(positions.len(), 0);
    }

    #[test]
    fn test_allocation_pct() {
        let txs = vec![
            make_tx("AAPL", TxType::Buy, dec!(10), dec!(100)),
            make_tx("GOOG", TxType::Buy, dec!(10), dec!(100)),
        ];
        let prices = HashMap::from([
            ("AAPL".to_string(), dec!(100)),
            ("GOOG".to_string(), dec!(300)),
        ]);
        let positions = compute_positions(&txs, &prices);

        assert_eq!(positions.len(), 2);
        let total_alloc: Decimal = positions
            .iter()
            .filter_map(|p| p.allocation_pct)
            .sum();
        assert_eq!(total_alloc, dec!(100));
    }
}
