use anyhow::{bail, Result};

use crate::config::load_config;
use crate::data::brave;

pub fn run(
    query: Option<&str>,
    news: bool,
    freshness: Option<&str>,
    count: usize,
    json: bool,
    preset: ResearchPresetArgs,
) -> Result<()> {
    let cfg = load_config()?;
    let key = cfg
        .brave_api_key
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Brave API key is required. Set with: pftui config set brave_api_key <key>"
            )
        })?;

    let query = resolve_query(query, &preset)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    if news {
        let results = rt.block_on(brave::brave_news_search(&key, &query, freshness, count))?;
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

    let results = rt.block_on(brave::brave_web_search(&key, &query, freshness, count))?;
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

#[derive(Debug, Clone, Default)]
pub struct ResearchPresetArgs {
    pub fed: bool,
    pub earnings: Option<String>,
    pub geopolitics: bool,
    pub cot: Option<String>,
    pub etf: Option<String>,
    pub opec: bool,
}

fn resolve_query(query: Option<&str>, preset: &ResearchPresetArgs) -> Result<String> {
    if let Some(q) = query {
        if !q.trim().is_empty() {
            return Ok(q.to_string());
        }
    }

    // If no explicit query, try exactly one preset.
    let mut preset_query: Option<String> = None;
    let mut set_count = 0;

    if preset.fed {
        preset_query = Some("latest Federal Reserve statements speeches minutes".to_string());
        set_count += 1;
    }
    if let Some(sym) = &preset.earnings {
        preset_query = Some(format!("latest earnings results {}", sym));
        set_count += 1;
    }
    if preset.geopolitics {
        preset_query = Some("latest geopolitics sanctions war trade tensions".to_string());
        set_count += 1;
    }
    if let Some(asset) = &preset.cot {
        preset_query = Some(format!("COT positioning report {}", asset));
        set_count += 1;
    }
    if let Some(asset) = &preset.etf {
        preset_query = Some(format!("ETF inflows outflows {}", asset));
        set_count += 1;
    }
    if preset.opec {
        preset_query = Some("latest OPEC production decisions output".to_string());
        set_count += 1;
    }

    if set_count == 0 {
        bail!("Provide a query or one preset flag (--fed/--earnings/--geopolitics/--cot/--etf/--opec)");
    }
    if set_count > 1 {
        bail!("Use only one preset flag at a time, or provide an explicit query");
    }

    Ok(preset_query.unwrap())
}

pub fn validate_freshness(value: &str) -> Result<String> {
    match value {
        "pd" | "pw" | "pm" | "py" => Ok(value.to_string()),
        _ => bail!("Invalid freshness '{}'. Use one of: pd, pw, pm, py", value),
    }
}
