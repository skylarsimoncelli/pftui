use anyhow::{bail, Result};
use std::io::{self, Write};

use crate::db::backend::BackendConnection;
use crate::db::transactions::{delete_transaction_backend, get_transaction_backend};

pub fn run(backend: &BackendConnection, id: i64) -> Result<()> {
    let tx = get_transaction_backend(backend, id)?;
    match tx {
        None => bail!("Transaction #{} not found", id),
        Some(tx) => {
            println!(
                "Transaction #{}: {} {} {} @ {} on {}",
                tx.id, tx.tx_type, tx.quantity, tx.symbol, tx.price_per, tx.date
            );
            print!("Delete this transaction? [y/N] ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if input.trim().to_lowercase() == "y" {
                delete_transaction_backend(backend, id)?;
                println!("Deleted transaction #{}", id);
            } else {
                println!("Cancelled");
            }
            Ok(())
        }
    }
}
