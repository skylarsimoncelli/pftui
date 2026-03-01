use anyhow::Result;
use rusqlite::Connection;

use crate::db::transactions::list_transactions;

pub fn run(conn: &Connection) -> Result<()> {
    let txs = list_transactions(conn)?;
    if txs.is_empty() {
        println!("No transactions found. Add one with: pftui add-tx");
        return Ok(());
    }

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
    println!("\nTotal: {} transactions", txs.len());
    Ok(())
}
