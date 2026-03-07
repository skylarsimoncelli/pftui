use anyhow::Result;
use rusqlite::Connection;

use crate::db::transactions::list_transactions;

pub fn run(conn: &Connection, show_notes: bool, json_output: bool) -> Result<()> {
    let txs = list_transactions(conn)?;
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

    if show_notes {
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
