use anyhow::{bail, Result};

use crate::db::backend::BackendConnection;
use crate::db::annotations::{self, Annotation};

pub struct AnnotateArgs<'a> {
    pub symbol: Option<&'a str>,
    pub thesis: Option<&'a str>,
    pub invalidation: Option<&'a str>,
    pub review_date: Option<&'a str>,
    pub target: Option<&'a str>,
    pub show: bool,
    pub list: bool,
    pub remove: bool,
    pub json: bool,
}

pub fn run(backend: &BackendConnection, args: AnnotateArgs<'_>) -> Result<()> {
    if args.list {
        return run_list(backend, args.json);
    }

    let symbol = args
        .symbol
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing symbol. Usage: pftui annotate SYMBOL --thesis \"...\""))?;

    if args.remove {
        let removed = annotations::remove_annotation_backend(backend, &symbol)?;
        if removed {
            println!("Removed annotation for {}", symbol);
        } else {
            println!("No annotation found for {}", symbol);
        }
        return Ok(());
    }

    if let Some(date) = args.review_date {
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("Invalid --review-date '{}'. Use YYYY-MM-DD.", date))?;
    }

    if let Some(target) = args.target {
        let cleaned = target.replace(['$', ','], "");
        let parsed = rust_decimal::Decimal::from_str_exact(&cleaned)
            .map_err(|_| anyhow::anyhow!("Invalid --target '{}'. Use a number.", target))?;
        if parsed <= rust_decimal::Decimal::ZERO {
            bail!("--target must be > 0");
        }
    }

    let has_update_fields = args.thesis.is_some()
        || args.invalidation.is_some()
        || args.review_date.is_some()
        || args.target.is_some();

    if has_update_fields {
        let existing = annotations::get_annotation_backend(backend, &symbol)?;
        let ann = Annotation {
            symbol: symbol.clone(),
            thesis: args
                .thesis
                .map(str::to_string)
                .or_else(|| existing.as_ref().map(|e| e.thesis.clone()))
                .unwrap_or_default(),
            invalidation: args
                .invalidation
                .map(str::to_string)
                .or_else(|| existing.as_ref().and_then(|e| e.invalidation.clone())),
            review_date: args
                .review_date
                .map(str::to_string)
                .or_else(|| existing.as_ref().and_then(|e| e.review_date.clone())),
            target_price: args
                .target
                .map(|t| t.replace(['$', ','], ""))
                .or_else(|| existing.as_ref().and_then(|e| e.target_price.clone())),
            updated_at: String::new(),
        };
        annotations::upsert_annotation_backend(backend, &ann)?;
    } else if !args.show {
        bail!("No update fields provided. Use --thesis/--invalidation/--review-date/--target, or --show/--list/--remove.");
    }

    run_show(backend, &symbol, args.json)
}

fn run_show(backend: &BackendConnection, symbol: &str, json: bool) -> Result<()> {
    let Some(ann) = annotations::get_annotation_backend(backend, symbol)? else {
        println!("No annotation for {}", symbol);
        return Ok(());
    };

    if json {
        let out = serde_json::json!({
            "symbol": ann.symbol,
            "thesis": ann.thesis,
            "invalidation": ann.invalidation,
            "review_date": ann.review_date,
            "target_price": ann.target_price,
            "updated_at": ann.updated_at,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("Annotation: {}", ann.symbol);
    println!("  Thesis: {}", if ann.thesis.is_empty() { "(empty)" } else { &ann.thesis });
    println!(
        "  Invalidation: {}",
        ann.invalidation.as_deref().unwrap_or("—")
    );
    println!(
        "  Review date: {}",
        ann.review_date.as_deref().unwrap_or("—")
    );
    println!(
        "  Target: {}",
        ann.target_price.as_deref().unwrap_or("—")
    );
    println!("  Updated: {}", ann.updated_at);
    Ok(())
}

fn run_list(backend: &BackendConnection, json: bool) -> Result<()> {
    let rows = annotations::list_annotations_backend(backend)?;
    if rows.is_empty() {
        println!("No annotations saved.");
        return Ok(());
    }

    if json {
        let out: Vec<_> = rows
            .iter()
            .map(|a| {
                serde_json::json!({
                    "symbol": a.symbol,
                    "thesis": a.thesis,
                    "invalidation": a.invalidation,
                    "review_date": a.review_date,
                    "target_price": a.target_price,
                    "updated_at": a.updated_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!(
        "{:<10}  {:<12}  {:<10}  Thesis",
        "Symbol", "Review", "Target"
    );
    println!("{}", "─".repeat(80));
    for a in rows {
        let review = a.review_date.unwrap_or_else(|| "—".to_string());
        let target = a.target_price.unwrap_or_else(|| "—".to_string());
        let thesis = if a.thesis.is_empty() {
            "(empty)".to_string()
        } else {
            a.thesis
        };
        println!("{:<10}  {:<12}  {:<10}  {}", a.symbol, review, target, thesis);
    }
    Ok(())
}
