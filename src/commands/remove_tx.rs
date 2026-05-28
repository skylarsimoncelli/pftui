use anyhow::{bail, Result};
use std::io::{self, Write};

use crate::db::backend::BackendConnection;
use crate::db::transactions::{
    delete_transaction_backend, get_transaction_backend, set_paired_transaction_backend,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveOutcome {
    pub removed_ids: Vec<i64>,
}

pub fn delete_with_pair(
    backend: &BackendConnection,
    id: i64,
    unpaired: bool,
) -> Result<RemoveOutcome> {
    let tx = get_transaction_backend(backend, id)?;
    match tx {
        None => bail!("Transaction #{} not found", id),
        Some(tx) => {
            let paired_tx_id = tx.paired_tx_id.filter(|paired_id| *paired_id != id);
            let mut removed_pair = None;
            if !unpaired {
                if let Some(paired_id) = paired_tx_id {
                    if get_transaction_backend(backend, paired_id)?.is_some() {
                        set_paired_transaction_backend(backend, id, None)?;
                        set_paired_transaction_backend(backend, paired_id, None)?;
                        delete_transaction_backend(backend, paired_id)?;
                        removed_pair = Some(paired_id);
                    }
                }
            } else if let Some(paired_id) = paired_tx_id {
                if get_transaction_backend(backend, paired_id)?.is_some() {
                    set_paired_transaction_backend(backend, paired_id, None)?;
                }
            }
            delete_transaction_backend(backend, id)?;
            let mut removed_ids = vec![id];
            if let Some(paired_id) = removed_pair {
                removed_ids.push(paired_id);
            }
            removed_ids.sort_unstable();
            Ok(RemoveOutcome { removed_ids })
        }
    }
}

pub fn run(backend: &BackendConnection, id: i64, unpaired: bool) -> Result<()> {
    let tx = get_transaction_backend(backend, id)?;
    match tx {
        None => bail!("Transaction #{} not found", id),
        Some(tx) => {
            let paired_tx_id = tx.paired_tx_id.filter(|paired_id| *paired_id != id);
            println!(
                "Transaction #{}: {} {} {} @ {} on {}",
                tx.id, tx.tx_type, tx.quantity, tx.symbol, tx.price_per, tx.date
            );
            if !unpaired {
                if let Some(paired_id) = paired_tx_id {
                    println!("Paired transaction #{} will also be deleted.", paired_id);
                }
            }
            print!("Delete this transaction? [y/N] ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if input.trim().to_lowercase() == "y" {
                let outcome = delete_with_pair(backend, id, unpaired)?;
                if outcome.removed_ids.len() == 1 {
                    println!("Deleted transaction #{}", id);
                } else {
                    println!("Deleted transactions {:?}", outcome.removed_ids);
                }
            } else {
                println!("Cancelled");
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;
    use crate::db::transactions::{
        insert_transaction_backend, list_transactions_backend, set_paired_transaction_backend,
    };
    use crate::models::asset::AssetCategory;
    use crate::models::transaction::{NewTransaction, TxType};
    use rust_decimal_macros::dec;

    fn backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: open_in_memory(),
        }
    }

    fn tx(
        symbol: &str,
        category: AssetCategory,
        tx_type: TxType,
        quantity: rust_decimal::Decimal,
    ) -> NewTransaction {
        NewTransaction {
            symbol: symbol.to_string(),
            category,
            tx_type,
            quantity,
            price_per: dec!(1),
            currency: "USD".to_string(),
            date: "2026-05-28".to_string(),
            notes: None,
        }
    }

    fn paired_transactions(backend: &BackendConnection) -> (i64, i64) {
        let asset_id = insert_transaction_backend(
            backend,
            &tx("AAPL", AssetCategory::Equity, TxType::Buy, dec!(10)),
        )
        .unwrap();
        let cash_id = insert_transaction_backend(
            backend,
            &tx("USD", AssetCategory::Cash, TxType::Sell, dec!(10)),
        )
        .unwrap();
        set_paired_transaction_backend(backend, asset_id, Some(cash_id)).unwrap();
        set_paired_transaction_backend(backend, cash_id, Some(asset_id)).unwrap();
        (asset_id, cash_id)
    }

    #[test]
    fn delete_with_pair_removes_both_legs() {
        let backend = backend();
        let (asset_id, cash_id) = paired_transactions(&backend);

        let outcome = delete_with_pair(&backend, asset_id, false).unwrap();

        assert_eq!(outcome.removed_ids, vec![asset_id, cash_id]);
        assert!(list_transactions_backend(&backend).unwrap().is_empty());
    }

    #[test]
    fn delete_with_pair_unpaired_keeps_other_leg() {
        let backend = backend();
        let (asset_id, cash_id) = paired_transactions(&backend);

        let outcome = delete_with_pair(&backend, asset_id, true).unwrap();

        assert_eq!(outcome.removed_ids, vec![asset_id]);
        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].id, cash_id);
        assert_eq!(txs[0].paired_tx_id, None);
    }
}
