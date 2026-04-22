//! COMEX warehouse inventory scraper.
//!
//! Fetches daily registered and eligible inventory data from CME Group's
//! publicly-available XLS files. Updated daily after market close (~5pm ET).
//!
//! Data sources:
//! - Gold: https://www.cmegroup.com/delivery_reports/Gold_Stocks.xls
//! - Silver: https://www.cmegroup.com/delivery_reports/Silver_stocks.xls
//!
//! Format: XLS files with multiple sheets per depository.
//! Strategy: Download XLS, parse with calamine, extract "TOTAL" row.

use anyhow::{anyhow, Result};
use calamine::{open_workbook_from_rs, Reader, Xls};
use regex::Regex;
use std::io::Cursor;

/// Metals tracked by COMEX inventory scraper.
pub const COMEX_METALS: &[ComexMetal] = &[
    ComexMetal {
        metal: "Gold",
        symbol: "GC=F",
        url: "https://www.cmegroup.com/delivery_reports/Gold_Stocks.xls",
        fallback_url: "https://goldsilver.ai/metal-prices/comex-gold",
        unit: "troy ounces",
    },
    ComexMetal {
        metal: "Silver",
        symbol: "SI=F",
        url: "https://www.cmegroup.com/delivery_reports/Silver_stocks.xls",
        fallback_url: "https://goldsilver.ai/metal-prices/comex-silver",
        unit: "troy ounces",
    },
];

/// Metadata for a tracked COMEX metal.
#[derive(Debug, Clone)]
pub struct ComexMetal {
    pub metal: &'static str,
    pub symbol: &'static str,
    pub url: &'static str,
    pub fallback_url: &'static str,
    pub unit: &'static str,
}

/// COMEX warehouse inventory snapshot.
#[derive(Debug, Clone)]
pub struct ComexInventory {
    pub symbol: String,  // GC=F or SI=F
    pub date: String,    // YYYY-MM-DD
    pub registered: f64, // Registered stocks (troy oz)
    pub eligible: f64,   // Eligible stocks (troy oz)
    pub total: f64,      // Total (registered + eligible)
    pub reg_ratio: f64,  // Registered / Total (%)
}

impl ComexInventory {
    /// Coverage ratio: registered inventory / daily volume.
    /// Low ratio (<5 days) suggests tight physical market.
    pub fn coverage_days(&self, daily_volume_oz: f64) -> Option<f64> {
        if daily_volume_oz > 0.0 {
            Some(self.registered / daily_volume_oz)
        } else {
            None
        }
    }

    /// Trend signal vs previous day.
    pub fn trend_vs(&self, prev: &ComexInventory) -> &'static str {
        let change = self.registered - prev.registered;
        let pct_change = change / prev.registered * 100.0;
        if pct_change < -2.0 {
            "drawing down"
        } else if pct_change > 2.0 {
            "building"
        } else {
            "stable"
        }
    }
}

/// Fetch COMEX inventory for a single metal.
///
/// Downloads XLS, parses TOTAL row across all sheets, sums registered/eligible.
pub fn fetch_inventory(symbol: &str) -> Result<ComexInventory> {
    let metal = COMEX_METALS
        .iter()
        .find(|m| m.symbol == symbol)
        .ok_or_else(|| anyhow!("Unknown COMEX symbol: {}", symbol))?;

    match fetch_inventory_from_cme(metal) {
        Ok(inventory) => Ok(inventory),
        Err(primary_err) => fetch_inventory_from_goldsilver_ai(metal).map_err(|fallback_err| {
            anyhow!(
                "COMEX primary fetch failed ({}); fallback fetch failed ({})",
                primary_err,
                fallback_err
            )
        }),
    }
}

fn fetch_inventory_from_cme(metal: &ComexMetal) -> Result<ComexInventory> {
    fetch_inventory_from_xls(metal)
}

fn fetch_inventory_from_xls(metal: &ComexMetal) -> Result<ComexInventory> {
    let symbol = metal.symbol;

    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(12))
        // CME blocks HTTP/2 with INTERNAL_ERROR; force HTTP/1.1
        .http1_only()
        .build()?;

    let resp = client
        .get(metal.url)
        .header("Accept", "application/vnd.ms-excel,*/*")
        .header("Referer", "https://www.cmegroup.com/clearing/operations-and-deliveries/nymex-delivery-notices.html")
        .send()?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "COMEX fetch failed: {} (status {})",
            metal.metal,
            resp.status()
        ));
    }

    let bytes = resp.bytes()?;
    let cursor = Cursor::new(bytes.to_vec());

    let mut workbook: Xls<_> = open_workbook_from_rs(cursor)?;

    let mut total_registered = 0.0;
    let mut total_eligible = 0.0;

    // Iterate through all sheets, sum TOTAL rows
    for sheet_name in workbook.sheet_names() {
        if let Ok(range) = workbook.worksheet_range(&sheet_name) {
            // Find header row to determine column indices
            let mut reg_col: Option<usize> = None;
            let mut elig_col: Option<usize> = None;

            // First pass: find header columns
            for row in range.rows() {
                for (idx, cell) in row.iter().enumerate() {
                    let cell_str = cell_to_text(cell).to_uppercase();
                    if cell_str.contains("REGISTERED") {
                        reg_col = Some(idx);
                    }
                    if cell_str.contains("ELIGIBLE") {
                        elig_col = Some(idx);
                    }
                }
                if reg_col.is_some() || elig_col.is_some() {
                    break;
                }
            }

            // Second pass: find TOTAL / GRAND TOTAL rows and extract values.
            // We look for the last TOTAL-like row in each sheet since that
            // is typically the summary row.  Earlier TOTAL rows (sub-totals)
            // may also appear but the grand total is the one we want.
            let mut sheet_reg = 0.0_f64;
            let mut sheet_elig = 0.0_f64;
            let mut found_total = false;

            for row in range.rows() {
                let row_text: String = row
                    .iter()
                    .map(|c| cell_to_text(c).to_uppercase())
                    .collect::<Vec<_>>()
                    .join(" ");

                // Skip rows that are clearly headers (contain both REGISTERED
                // and TOTAL/ELIGIBLE in the same row as text labels).
                let is_header = row_text.contains("REGISTERED") && row_text.contains("ELIGIBLE");
                if is_header {
                    continue;
                }

                // Match TOTAL, GRAND TOTAL, or similar summary rows
                let has_total = row_text.contains("TOTAL")
                    || row_text.contains("GRAND")
                    || row_text.contains("COMBINED");

                if !has_total {
                    continue;
                }

                // Use discovered columns or try common fallback indices
                let r_idx = reg_col.unwrap_or(2);
                let e_idx = elig_col.unwrap_or(3);

                let mut row_reg = 0.0_f64;
                let mut row_elig = 0.0_f64;
                let mut got_reg = false;
                let mut got_elig = false;

                // Try the header-discovered column first
                if let Some(reg_cell) = row.get(r_idx) {
                    if let Ok(v) = parse_cell_as_float(reg_cell) {
                        row_reg = v;
                        got_reg = true;
                    }
                }
                if let Some(elig_cell) = row.get(e_idx) {
                    if let Ok(v) = parse_cell_as_float(elig_cell) {
                        row_elig = v;
                        got_elig = true;
                    }
                }

                // If header columns didn't work, scan all numeric cells in
                // the row and pick the two largest as registered/eligible.
                if !got_reg && !got_elig {
                    let mut nums: Vec<(usize, f64)> = Vec::new();
                    for (idx, cell) in row.iter().enumerate() {
                        if let Ok(v) = parse_cell_as_float(cell) {
                            if v > 0.0 {
                                nums.push((idx, v));
                            }
                        }
                    }
                    nums.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    if nums.len() >= 2 {
                        // Larger value is typically eligible, smaller is registered
                        row_elig = nums[0].1;
                        row_reg = nums[1].1;
                        got_reg = true;
                        got_elig = true;
                    } else if nums.len() == 1 {
                        row_reg = nums[0].1;
                        got_reg = true;
                    }
                } else if !got_reg || !got_elig {
                    // One column worked, try fallback indices for the other
                    for &fi in &[1_usize, 2, 3, 4] {
                        if fi == r_idx || fi == e_idx {
                            continue;
                        }
                        if let Some(cell) = row.get(fi) {
                            if let Ok(v) = parse_cell_as_float(cell) {
                                if !got_reg {
                                    row_reg = v;
                                    got_reg = true;
                                    break;
                                } else if !got_elig {
                                    row_elig = v;
                                    got_elig = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                if got_reg || got_elig {
                    // Use the last (grand) total we find — overwrite, don't accumulate
                    sheet_reg = row_reg;
                    sheet_elig = row_elig;
                    found_total = true;
                }
            }

            if found_total {
                total_registered += sheet_reg;
                total_eligible += sheet_elig;
            }
        }
    }

    if total_registered == 0.0 && total_eligible == 0.0 {
        return Err(anyhow!("No TOTAL rows found in COMEX {} XLS", metal.metal));
    }

    let total = total_registered + total_eligible;
    let reg_ratio = if total > 0.0 {
        (total_registered / total) * 100.0
    } else {
        0.0
    };

    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

    Ok(ComexInventory {
        symbol: symbol.to_string(),
        date,
        registered: total_registered,
        eligible: total_eligible,
        total,
        reg_ratio,
    })
}

fn fetch_inventory_from_goldsilver_ai(metal: &ComexMetal) -> Result<ComexInventory> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(12))
        .build()?;

    let body = client
        .get(metal.fallback_url)
        .send()?
        .error_for_status()?
        .text()?;

    parse_goldsilver_ai_inventory(&body, metal)
}

fn parse_goldsilver_ai_inventory(body: &str, metal: &ComexMetal) -> Result<ComexInventory> {
    let normalized = body.replace("\\\"", "\"");
    let (registered, registered_timestamp) = latest_series_point(&normalized, "registeredData")?;
    let (eligible, latest_timestamp) = latest_series_point(&normalized, "eligibleData")?;
    let date = timestamp_ms_to_date(registered_timestamp.max(latest_timestamp))?;

    let total = registered + eligible;
    let reg_ratio = if total > 0.0 {
        (registered / total) * 100.0
    } else {
        0.0
    };

    Ok(ComexInventory {
        symbol: metal.symbol.to_string(),
        date,
        registered,
        eligible,
        total,
        reg_ratio,
    })
}

fn latest_series_point(body: &str, series_name: &str) -> Result<(f64, i64)> {
    let series = extract_series_payload(body, series_name)?;

    let point_re =
        Regex::new(r#"\{\s*"x"\s*:\s*(\d+)\s*,\s*"y"\s*:\s*([0-9]+(?:\.[0-9]+)?)\s*\}"#)?;
    let mut latest: Option<(f64, i64)> = None;
    for caps in point_re.captures_iter(series) {
        let timestamp = caps[1]
            .parse::<i64>()
            .map_err(|e| anyhow!("failed to parse {} timestamp '{}': {}", series_name, &caps[1], e))?;
        let value = caps[2]
            .parse::<f64>()
            .map_err(|e| anyhow!("failed to parse {} value '{}': {}", series_name, &caps[2], e))?;
        latest = Some((value, timestamp));
    }

    latest.ok_or_else(|| anyhow!("missing data points in {} fallback payload", series_name))
}

fn extract_series_payload<'a>(body: &'a str, series_name: &str) -> Result<&'a str> {
    let marker = format!(r#""{}":"#, series_name);
    let marker_with_space = format!(r#""{}" :"#, series_name);
    let series_start = body
        .find(&marker)
        .or_else(|| body.find(&marker_with_space))
        .ok_or_else(|| anyhow!("missing {} series in fallback payload", series_name))?;

    let array_start = body[series_start..]
        .find('[')
        .map(|offset| series_start + offset + 1)
        .ok_or_else(|| anyhow!("missing opening bracket for {} series", series_name))?;

    let mut depth = 1_i32;
    for (offset, ch) in body[array_start..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&body[array_start..array_start + offset]);
                }
            }
            _ => {}
        }
    }

    Err(anyhow!(
        "missing closing bracket for {} series in fallback payload",
        series_name
    ))
}

fn timestamp_ms_to_date(timestamp_ms: i64) -> Result<String> {
    let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms)
        .ok_or_else(|| anyhow!("invalid fallback timestamp: {}", timestamp_ms))?;
    Ok(datetime.format("%Y-%m-%d").to_string())
}

fn parse_ounces(raw: &str) -> Result<f64> {
    let cleaned = raw.replace(',', "").trim().to_string();
    let number = cleaned
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("missing numeric value in '{}'", raw))?;
    let multiplier = if number.ends_with('M') {
        1_000_000.0
    } else if number.ends_with('K') {
        1_000.0
    } else {
        1.0
    };
    let value = number
        .trim_end_matches(['M', 'K'])
        .parse::<f64>()
        .map_err(|e| anyhow!("failed to parse '{}' as ounces: {}", raw, e))?;
    Ok(value * multiplier)
}

fn parse_month_day_year(raw: &str) -> Result<String> {
    let parsed = chrono::NaiveDate::parse_from_str(raw, "%b %e, %Y")
        .or_else(|_| chrono::NaiveDate::parse_from_str(raw, "%b %d, %Y"))?;
    Ok(parsed.format("%Y-%m-%d").to_string())
}

/// Parse a calamine Data as f64.
fn parse_cell_as_float(cell: &calamine::Data) -> Result<f64> {
    match cell {
        calamine::Data::Int(i) => Ok(*i as f64),
        calamine::Data::Float(f) => Ok(*f),
        calamine::Data::String(s) => {
            let cleaned = s.replace([',', '$', '*'], "").trim().to_string();
            cleaned
                .parse::<f64>()
                .map_err(|e| anyhow!("Failed to parse '{}' as float: {}", s, e))
        }
        _ => Err(anyhow!("Cell type not numeric: {:?}", cell)),
    }
}

fn cell_to_text(cell: &calamine::Data) -> String {
    match cell {
        calamine::Data::String(s) => s.clone(),
        calamine::Data::Float(f) => format!("{}", f),
        calamine::Data::Int(i) => format!("{}", i),
        calamine::Data::Bool(b) => format!("{}", b),
        _ => String::new(),
    }
}

/// Fetch inventory for all tracked metals.
pub fn fetch_all_inventories() -> Vec<(String, Result<ComexInventory>)> {
    COMEX_METALS
        .iter()
        .map(|m| (m.symbol.to_string(), fetch_inventory(m.symbol)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coverage_days() {
        let inv = ComexInventory {
            symbol: "GC=F".to_string(),
            date: "2026-03-05".to_string(),
            registered: 10_000_000.0,
            eligible: 20_000_000.0,
            total: 30_000_000.0,
            reg_ratio: 33.33,
        };

        assert_eq!(inv.coverage_days(1_000_000.0), Some(10.0));
        assert_eq!(inv.coverage_days(0.0), None);
    }

    #[test]
    fn test_trend_vs() {
        let prev = ComexInventory {
            symbol: "GC=F".to_string(),
            date: "2026-03-04".to_string(),
            registered: 10_000_000.0,
            eligible: 20_000_000.0,
            total: 30_000_000.0,
            reg_ratio: 33.33,
        };

        let building = ComexInventory {
            symbol: "GC=F".to_string(),
            date: "2026-03-05".to_string(),
            registered: 10_500_000.0,
            eligible: 20_000_000.0,
            total: 30_500_000.0,
            reg_ratio: 34.43,
        };
        assert_eq!(building.trend_vs(&prev), "building");

        let drawing_down = ComexInventory {
            symbol: "GC=F".to_string(),
            date: "2026-03-05".to_string(),
            registered: 9_500_000.0,
            eligible: 20_000_000.0,
            total: 29_500_000.0,
            reg_ratio: 32.20,
        };
        assert_eq!(drawing_down.trend_vs(&prev), "drawing down");

        let stable = ComexInventory {
            symbol: "GC=F".to_string(),
            date: "2026-03-05".to_string(),
            registered: 10_100_000.0,
            eligible: 20_000_000.0,
            total: 30_100_000.0,
            reg_ratio: 33.55,
        };
        assert_eq!(stable.trend_vs(&prev), "stable");
    }

    #[test]
    fn test_parse_cell_as_float_string_with_commas() {
        let cell = calamine::Data::String("1,234,567".to_string());
        let parsed = parse_cell_as_float(&cell).unwrap();
        assert_eq!(parsed, 1_234_567.0);
    }

    #[test]
    fn parse_ounces_supports_suffixes() {
        assert_eq!(parse_ounces("15.7M oz").unwrap(), 15_700_000.0);
        assert_eq!(parse_ounces("178.5K oz").unwrap(), 178_500.0);
        assert_eq!(parse_ounces("42 oz").unwrap(), 42.0);
    }

    #[test]
    fn parse_goldsilver_ai_inventory_extracts_registered_and_eligible() {
        let html = r#"
            ["$","$L18",null,{
                "registeredData":[
                    {"x":1745020800000,"y":15700000.0},
                    {"x":1745193600000,"y":15800000.5}
                ],
                "eligibleData":[
                    {"x":1745020800000,"y":14100000.0},
                    {"x":1745193600000,"y":14250000.25}
                ]
            }]
        "#;
        let inventory = parse_goldsilver_ai_inventory(html, &COMEX_METALS[0]).unwrap();

        assert_eq!(inventory.symbol, "GC=F");
        assert_eq!(inventory.date, "2025-04-21");
        assert_eq!(inventory.registered, 15_800_000.5);
        assert_eq!(inventory.eligible, 14_250_000.25);
        assert_eq!(inventory.total, 30_050_000.75);
    }
}
