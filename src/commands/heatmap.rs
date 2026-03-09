//! `pftui heatmap` — treemap-style sector performance view.

use std::cmp::Reverse;
use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;

use crate::commands::sector::SECTOR_ETFS;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::{get_all_cached_prices_backend, upsert_price_backend};
use crate::db::price_history::get_history_backend;
use crate::price::yahoo;

#[derive(Debug, Clone)]
struct HeatCell {
    symbol: String,
    name: String,
    day_change_pct: f64,
}

#[derive(Debug, Clone)]
struct Tile {
    cell: HeatCell,
    width: usize,
}

fn missing_symbols(price_map: &HashMap<String, Decimal>) -> Vec<&'static str> {
    SECTOR_ETFS
        .iter()
        .map(|(symbol, _)| *symbol)
        .filter(|symbol| !price_map.contains_key(*symbol))
        .collect()
}

fn backfill_prices(
    backend: &BackendConnection,
    price_map: &mut HashMap<String, Decimal>,
    symbols: &[&str],
) -> Result<()> {
    if symbols.is_empty() {
        return Ok(());
    }

    let rt = tokio::runtime::Runtime::new()?;
    for symbol in symbols {
        if let Ok(quote) = rt.block_on(yahoo::fetch_price(symbol)) {
            upsert_price_backend(backend, &quote)?;
            price_map.insert(symbol.to_string(), quote.price);
        }
    }

    Ok(())
}

fn collect_cells(backend: &BackendConnection) -> Result<Vec<HeatCell>> {
    let all_prices = get_all_cached_prices_backend(backend)?;
    let mut price_map: HashMap<String, Decimal> = all_prices
        .iter()
        .map(|p| (p.symbol.clone(), p.price))
        .collect();

    let missing = missing_symbols(&price_map);
    backfill_prices(backend, &mut price_map, &missing)?;

    let mut cells = Vec::new();
    for (symbol, name) in SECTOR_ETFS {
        let Some(price) = price_map.get(*symbol) else {
            continue;
        };
        let history = get_history_backend(backend, symbol, 2)?;
        let chg = if history.len() >= 2 {
            let yesterday = history[history.len() - 2].close;
            if yesterday > Decimal::ZERO {
                ((*price - yesterday) / yesterday * Decimal::from(100))
                    .to_string()
                    .parse::<f64>()
                    .unwrap_or(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        cells.push(HeatCell {
            symbol: symbol.to_string(),
            name: name.to_string(),
            day_change_pct: chg,
        });
    }

    Ok(cells)
}

fn weight(change: f64) -> usize {
    // Ensure every tile is visible while giving bigger moves larger area.
    let mag = change.abs();
    if mag >= 4.0 {
        8
    } else if mag >= 3.0 {
        7
    } else if mag >= 2.0 {
        6
    } else if mag >= 1.0 {
        5
    } else if mag >= 0.5 {
        4
    } else {
        3
    }
}

fn pack_rows(cells: &[HeatCell], rows: usize) -> Vec<Vec<HeatCell>> {
    let mut sorted = cells.to_vec();
    sorted.sort_by_key(|c| Reverse(weight(c.day_change_pct)));

    let mut buckets: Vec<Vec<HeatCell>> = vec![Vec::new(); rows];
    let mut bucket_weights = vec![0usize; rows];

    for cell in sorted {
        let (idx, _) = bucket_weights
            .iter()
            .enumerate()
            .min_by_key(|(_, w)| **w)
            .unwrap_or((0, &0));
        bucket_weights[idx] += weight(cell.day_change_pct);
        buckets[idx].push(cell);
    }

    buckets
}

fn allocate_widths(cells: &[HeatCell], total_width: usize, min_width: usize) -> Vec<Tile> {
    if cells.is_empty() {
        return Vec::new();
    }

    let weights: Vec<usize> = cells.iter().map(|c| weight(c.day_change_pct)).collect();
    let sum: usize = weights.iter().sum::<usize>().max(1);

    let mut widths = vec![min_width; cells.len()];
    let remaining = total_width.saturating_sub(min_width * cells.len());

    if remaining > 0 {
        for (i, w) in weights.iter().enumerate() {
            widths[i] += remaining * *w / sum;
        }

        // Distribute rounding remainder from left to right.
        let used: usize = widths.iter().sum();
        let mut extra = total_width.saturating_sub(used);
        let mut i = 0usize;
        while extra > 0 && !widths.is_empty() {
            widths[i] += 1;
            extra -= 1;
            i = (i + 1) % widths.len();
        }
    }

    cells
        .iter()
        .cloned()
        .zip(widths)
        .map(|(cell, width)| Tile { cell, width })
        .collect()
}

fn bg_code(change: f64) -> u8 {
    if change >= 3.0 {
        46
    } else if change >= 1.5 {
        40
    } else if change > 0.0 {
        34
    } else if change <= -3.0 {
        196
    } else if change <= -1.5 {
        160
    } else if change < 0.0 {
        124
    } else {
        240
    }
}

fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for ch in text.chars().take(width) {
        out.push(ch);
    }
    let used = out.chars().count();
    if used < width {
        out.push_str(&" ".repeat(width - used));
    }
    out
}

fn render_row(row: &[Tile]) {
    let mut line1 = String::new();
    let mut line2 = String::new();

    for tile in row {
        let code = bg_code(tile.cell.day_change_pct);
        let prefix = format!("\x1b[48;5;{}m\x1b[97m", code);
        let reset = "\x1b[0m";
        let l1 = fit(
            &format!(" {} {:+.2}%", tile.cell.symbol, tile.cell.day_change_pct),
            tile.width,
        );
        let l2 = fit(&format!(" {}", tile.cell.name), tile.width);
        line1.push_str(&format!("{}{}{}", prefix, l1, reset));
        line2.push_str(&format!("{}{}{}", prefix, l2, reset));
    }

    println!("{}", line1);
    println!("{}", line2);
}

fn print_terminal(cells: &[HeatCell]) {
    println!("\nSECTOR HEATMAP (Treemap-Style, 1D % Change)\n");

    let rows = pack_rows(cells, 3);
    for row in rows {
        let tiles = allocate_widths(&row, 96, 14);
        render_row(&tiles);
        println!();
    }

    println!("Legend: green = gain, red = loss, gray = flat. Larger tiles = larger absolute move.\n");
}

fn print_json(cells: &[HeatCell]) -> Result<()> {
    let mut out = serde_json::Map::new();
    let payload: Vec<_> = cells
        .iter()
        .map(|c| {
            serde_json::json!({
                "symbol": c.symbol,
                "name": c.name,
                "day_change_pct": c.day_change_pct,
                "weight": weight(c.day_change_pct),
            })
        })
        .collect();

    out.insert("heatmap".to_string(), serde_json::Value::Array(payload));
    println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(out))?);
    Ok(())
}

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    let mut cells = collect_cells(backend)?;
    cells.sort_by(|a, b| {
        b.day_change_pct
            .partial_cmp(&a.day_change_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if json {
        print_json(&cells)?;
    } else {
        print_terminal(&cells);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_increases_with_magnitude() {
        assert!(weight(4.5) > weight(1.0));
        assert_eq!(weight(0.0), 3);
        assert_eq!(weight(-3.2), 7);
    }

    #[test]
    fn allocate_widths_respects_total_width() {
        let cells = vec![
            HeatCell {
                symbol: "A".to_string(),
                name: "A".to_string(),
                day_change_pct: 2.0,
            },
            HeatCell {
                symbol: "B".to_string(),
                name: "B".to_string(),
                day_change_pct: -1.0,
            },
            HeatCell {
                symbol: "C".to_string(),
                name: "C".to_string(),
                day_change_pct: 0.2,
            },
        ];

        let tiles = allocate_widths(&cells, 60, 10);
        assert_eq!(tiles.iter().map(|t| t.width).sum::<usize>(), 60);
        assert!(tiles.iter().all(|t| t.width >= 10));
    }

    #[test]
    fn pack_rows_distributes_cells() {
        let cells = vec![
            HeatCell {
                symbol: "A".to_string(),
                name: "A".to_string(),
                day_change_pct: 4.0,
            },
            HeatCell {
                symbol: "B".to_string(),
                name: "B".to_string(),
                day_change_pct: 3.0,
            },
            HeatCell {
                symbol: "C".to_string(),
                name: "C".to_string(),
                day_change_pct: 2.0,
            },
            HeatCell {
                symbol: "D".to_string(),
                name: "D".to_string(),
                day_change_pct: 1.0,
            },
        ];

        let rows = pack_rows(&cells, 3);
        let total = rows.iter().map(|r| r.len()).sum::<usize>();
        assert_eq!(total, 4);
        assert!(rows.iter().any(|r| !r.is_empty()));
    }
}
