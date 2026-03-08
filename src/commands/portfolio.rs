//! `pftui portfolio` — manage named portfolios.

use anyhow::{anyhow, bail, Result};

use crate::db;

fn print_list(json: bool) -> Result<()> {
    let active = db::read_active_portfolio();
    let names = db::list_portfolios();

    if json {
        let items: Vec<_> = names
            .iter()
            .map(|name| serde_json::json!({"name": name, "active": *name == active}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({"portfolios": items}))?);
        return Ok(());
    }

    println!("\nPortfolios:\n");
    for name in names {
        if name == active {
            println!("* {}", name);
        } else {
            println!("  {}", name);
        }
    }
    println!();
    Ok(())
}

fn print_current(json: bool) -> Result<()> {
    let active = db::read_active_portfolio();
    let path = db::db_path_for_portfolio(&active);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "active": active,
                "db_path": path,
            }))?
        );
        return Ok(());
    }

    println!("{}", active);
    println!("db: {}", path.display());
    Ok(())
}

fn create_portfolio(name: &str, json: bool) -> Result<()> {
    let safe = db::sanitize_portfolio_name(name)
        .ok_or_else(|| anyhow!("Invalid name '{}'. Use letters, numbers, '-' or '_'", name))?;
    let path = db::db_path_for_portfolio(&safe);
    let _ = db::open_db(&path)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "created": true,
                "name": safe,
                "db_path": path,
            }))?
        );
        return Ok(());
    }

    println!("Created portfolio '{}' at {}", safe, path.display());
    Ok(())
}

fn switch_portfolio(name: &str, json: bool) -> Result<()> {
    let safe = db::sanitize_portfolio_name(name)
        .ok_or_else(|| anyhow!("Invalid name '{}'. Use letters, numbers, '-' or '_'", name))?;
    let path = db::db_path_for_portfolio(&safe);

    // Ensure DB exists and migrations are applied before switching.
    let _ = db::open_db(&path)?;
    db::write_active_portfolio(&safe)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "switched": true,
                "active": safe,
                "db_path": path,
            }))?
        );
        return Ok(());
    }

    println!("Switched active portfolio to '{}'", safe);
    println!("db: {}", path.display());
    Ok(())
}

fn remove_portfolio(name: &str, json: bool) -> Result<()> {
    let safe = db::sanitize_portfolio_name(name)
        .ok_or_else(|| anyhow!("Invalid name '{}'. Use letters, numbers, '-' or '_'", name))?;
    if safe == "default" {
        bail!("Cannot remove 'default' portfolio")
    }
    let active = db::read_active_portfolio();
    if safe == active {
        bail!("Cannot remove active portfolio '{}'. Switch first.", safe)
    }

    let path = db::db_path_for_portfolio(&safe);
    let removed = if path.exists() {
        std::fs::remove_file(&path)?;
        true
    } else {
        false
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "removed": removed,
                "name": safe,
                "db_path": path,
            }))?
        );
        return Ok(());
    }

    if removed {
        println!("Removed portfolio '{}'", safe);
    } else {
        println!("Portfolio '{}' does not exist", safe);
    }
    Ok(())
}

pub fn run(action: &str, name: Option<&str>, json: bool) -> Result<()> {
    match action {
        "list" => print_list(json),
        "current" => print_current(json),
        "create" => {
            let name = name.ok_or_else(|| anyhow!("Usage: pftui portfolio create <name>"))?;
            create_portfolio(name, json)
        }
        "switch" => {
            let name = name.ok_or_else(|| anyhow!("Usage: pftui portfolio switch <name>"))?;
            switch_portfolio(name, json)
        }
        "remove" => {
            let name = name.ok_or_else(|| anyhow!("Usage: pftui portfolio remove <name>"))?;
            remove_portfolio(name, json)
        }
        _ => bail!(
            "Unknown action '{}'. Valid actions: list, current, create, switch, remove",
            action
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_rejects_unknown_action() {
        let err = run("bad", None, false).unwrap_err().to_string();
        assert!(err.contains("Unknown action"));
    }
}
