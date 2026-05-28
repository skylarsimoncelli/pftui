use anyhow::{bail, Result};
use serde::Serialize;
use std::io::{self, Write};

use crate::commands::transaction_summary::{
    print_summary, remove_summary, removed_cash_delta, TransactionChangeSummary,
};
use crate::db::backend::BackendConnection;
use crate::db::transactions::{
    delete_transaction_backend, get_transaction_backend, set_paired_transaction_backend,
};
use crate::models::transaction::Transaction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveOutcome {
    pub removed_ids: Vec<i64>,
}

#[derive(Debug, Clone)]
struct RemovePlan {
    transaction: Transaction,
    removed_transactions: Vec<Transaction>,
    summary: TransactionChangeSummary,
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

pub fn run(
    backend: &BackendConnection,
    id: i64,
    unpaired: bool,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let plan = build_remove_plan(backend, id, unpaired)?;
    let removed_ids: Vec<i64> = plan.removed_transactions.iter().map(|tx| tx.id).collect();

    if dry_run {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&RemoveTransactionOutput::dry_run(
                    removed_ids,
                    plan.summary
                ))?
            );
        } else {
            println!(
                "Dry run: would delete transaction #{}: {} {} {} @ {} on {}",
                plan.transaction.id,
                plan.transaction.tx_type,
                plan.transaction.quantity,
                plan.transaction.symbol,
                plan.transaction.price_per,
                plan.transaction.date
            );
            for paired in plan
                .removed_transactions
                .iter()
                .filter(|tx| tx.id != plan.transaction.id)
            {
                println!(
                    "Dry run: would also delete paired transaction #{}",
                    paired.id
                );
            }
            print_summary("Post-remove summary:", &plan.summary);
        }
        return Ok(());
    }

    print_confirmation_prompt(&plan, json)?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if input.trim().to_lowercase() == "y" {
        let outcome = delete_with_pair(backend, id, unpaired)?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&RemoveTransactionOutput::removed(
                    outcome.removed_ids,
                    plan.summary
                ))?
            );
            return Ok(());
        }
        if outcome.removed_ids.len() == 1 {
            println!("Deleted transaction #{}", id);
        } else {
            println!("Deleted transactions {:?}", outcome.removed_ids);
        }
        print_summary("Post-remove summary:", &plan.summary);
    } else {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&RemoveTransactionOutput::cancelled())?
            );
            return Ok(());
        }
        println!("Cancelled");
    }
    Ok(())
}

fn print_confirmation_prompt(plan: &RemovePlan, json: bool) -> Result<()> {
    let message = format!(
        "Transaction #{}: {} {} {} @ {} on {}",
        plan.transaction.id,
        plan.transaction.tx_type,
        plan.transaction.quantity,
        plan.transaction.symbol,
        plan.transaction.price_per,
        plan.transaction.date
    );
    if json {
        eprintln!("{message}");
    } else {
        println!("{message}");
    }

    for paired in plan
        .removed_transactions
        .iter()
        .filter(|tx| tx.id != plan.transaction.id)
    {
        if json {
            eprintln!("Paired transaction #{} will also be deleted.", paired.id);
        } else {
            println!("Paired transaction #{} will also be deleted.", paired.id);
        }
    }

    if json {
        eprint!("Delete this transaction? [y/N] ");
        io::stderr().flush()?;
    } else {
        print!("Delete this transaction? [y/N] ");
        io::stdout().flush()?;
    }
    Ok(())
}

fn build_remove_plan(backend: &BackendConnection, id: i64, unpaired: bool) -> Result<RemovePlan> {
    let tx = get_transaction_backend(backend, id)?;
    match tx {
        None => bail!("Transaction #{} not found", id),
        Some(tx) => {
            let paired_tx_id = tx.paired_tx_id.filter(|paired_id| *paired_id != id);
            let mut removed_transactions = vec![tx.clone()];
            if !unpaired {
                if let Some(paired_id) = paired_tx_id {
                    if let Some(paired) = get_transaction_backend(backend, paired_id)? {
                        removed_transactions.push(paired);
                    }
                }
            }
            let removed_ids: Vec<i64> = removed_transactions.iter().map(|tx| tx.id).collect();
            let (cash_delta, cash_currency) = removed_cash_delta(&removed_transactions);
            let summary =
                remove_summary(backend, &removed_ids, &tx.symbol, cash_delta, cash_currency)?;

            Ok(RemovePlan {
                transaction: tx,
                removed_transactions,
                summary,
            })
        }
    }
}

#[derive(Debug, Serialize)]
struct RemoveTransactionOutput {
    status: &'static str,
    removed_ids: Vec<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_remove_summary: Option<TransactionChangeSummary>,
}

impl RemoveTransactionOutput {
    fn dry_run(removed_ids: Vec<i64>, post_remove_summary: TransactionChangeSummary) -> Self {
        Self {
            status: "dry_run",
            removed_ids,
            post_remove_summary: Some(post_remove_summary),
        }
    }

    fn removed(removed_ids: Vec<i64>, post_remove_summary: TransactionChangeSummary) -> Self {
        Self {
            status: "removed",
            removed_ids,
            post_remove_summary: Some(post_remove_summary),
        }
    }

    fn cancelled() -> Self {
        Self {
            status: "cancelled",
            removed_ids: Vec::new(),
            post_remove_summary: None,
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

    #[test]
    fn dry_run_remove_does_not_delete_transactions() {
        let backend = backend();
        let (asset_id, cash_id) = paired_transactions(&backend);

        run(&backend, asset_id, false, true, true).unwrap();

        let txs = list_transactions_backend(&backend).unwrap();
        assert_eq!(txs.len(), 2);
        assert!(txs.iter().any(|tx| tx.id == asset_id));
        assert!(txs.iter().any(|tx| tx.id == cash_id));
    }
}
