use anyhow::Result;

use crate::db::backend::BackendConnection;
use crate::db::transactions::list_transactions_backend;

pub fn run(
    backend: &BackendConnection,
    show_notes: bool,
    show_paired: bool,
    json_output: bool,
) -> Result<()> {
    let txs = list_transactions_backend(backend)?;
    if txs.is_empty() {
        if json_output {
            println!("[]");
        } else {
            println!("No transactions found. Add one with: pftui add-tx");
        }
        return Ok(());
    }

    if json_output {
        let json = serde_json::to_string_pretty(&txs)?;
        println!("{}", json);
        return Ok(());
    }

    if show_notes && show_paired {
        println!(
            "{:<5} {:<8} {:<10} {:<5} {:>10} {:>12} {:<5} {:<12} {:<7} Notes",
            "ID", "Symbol", "Category", "Type", "Qty", "Price", "Ccy", "Date", "Pair"
        );
        println!("{}", "-".repeat(105));

        for tx in &txs {
            println!(
                "{:<5} {:<8} {:<10} {:<5} {:>10} {:>12} {:<5} {:<12} {:<7} {}",
                tx.id,
                tx.symbol,
                tx.category,
                tx.tx_type,
                tx.quantity,
                tx.price_per,
                tx.currency,
                tx.date,
                tx.paired_tx_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                tx.notes.as_deref().unwrap_or(""),
            );
        }
    } else if show_notes {
        println!(
            "{:<5} {:<8} {:<10} {:<5} {:>10} {:>12} {:<5} {:<12} Notes",
            "ID", "Symbol", "Category", "Type", "Qty", "Price", "Ccy", "Date"
        );
        println!("{}", "-".repeat(95));

        for tx in &txs {
            println!(
                "{:<5} {:<8} {:<10} {:<5} {:>10} {:>12} {:<5} {:<12} {}",
                tx.id,
                tx.symbol,
                tx.category,
                tx.tx_type,
                tx.quantity,
                tx.price_per,
                tx.currency,
                tx.date,
                tx.notes.as_deref().unwrap_or(""),
            );
        }
    } else if show_paired {
        println!(
            "{:<5} {:<8} {:<10} {:<5} {:>10} {:>12} {:<5} {:<12} {:<7}",
            "ID", "Symbol", "Category", "Type", "Qty", "Price", "Ccy", "Date", "Pair"
        );
        println!("{}", "-".repeat(85));

        for tx in &txs {
            println!(
                "{:<5} {:<8} {:<10} {:<5} {:>10} {:>12} {:<5} {:<12} {:<7}",
                tx.id,
                tx.symbol,
                tx.category,
                tx.tx_type,
                tx.quantity,
                tx.price_per,
                tx.currency,
                tx.date,
                tx.paired_tx_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
        }
    } else {
        println!(
            "{:<5} {:<8} {:<10} {:<5} {:>10} {:>12} {:<5} {:<12}",
            "ID", "Symbol", "Category", "Type", "Qty", "Price", "Ccy", "Date"
        );
        println!("{}", "-".repeat(75));

        for tx in &txs {
            println!(
                "{:<5} {:<8} {:<10} {:<5} {:>10} {:>12} {:<5} {:<12}",
                tx.id,
                tx.symbol,
                tx.category,
                tx.tx_type,
                tx.quantity,
                tx.price_per,
                tx.currency,
                tx.date,
            );
        }
    }
    println!("\nTotal: {} transactions", txs.len());
    Ok(())
}
