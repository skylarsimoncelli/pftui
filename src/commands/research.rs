use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::config::load_config;
use crate::data::brave;

pub fn run(
    _conn: &Connection,
    query: &str,
    news: bool,
    freshness: Option<&str>,
    count: usize,
    json: bool,
) -> Result<()> {
    let cfg = load_config()?;
    let key = cfg
        .brave_api_key
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Brave API key is required. Set with: pftui config set brave_api_key <key>"))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    if news {
        let results = rt.block_on(brave::brave_news_search(&key, query, freshness, count))?;
        if json {
            println!("{}", serde_json::to_string_pretty(&results)?);
            return Ok(());
        }
        if results.is_empty() {
            println!("No results.");
            return Ok(());
        }
        for (i, r) in results.iter().enumerate() {
            println!("{}. {}", i + 1, r.title);
            println!("   {}", r.url);
            if !r.description.trim().is_empty() {
                println!("   {}", r.description);
            }
            println!();
        }
        return Ok(());
    }

    let results = rt.block_on(brave::brave_web_search(&key, query, freshness, count))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }
    if results.is_empty() {
        println!("No results.");
        return Ok(());
    }
    for (i, r) in results.iter().enumerate() {
        println!("{}. {}", i + 1, r.title);
        println!("   {}", r.url);
        if !r.description.trim().is_empty() {
            println!("   {}", r.description);
        }
        println!();
    }

    Ok(())
}

pub fn validate_freshness(value: &str) -> Result<String> {
    match value {
        "pd" | "pw" | "pm" | "py" => Ok(value.to_string()),
        _ => bail!("Invalid freshness '{}'. Use one of: pd, pw, pm, py", value),
    }
}

