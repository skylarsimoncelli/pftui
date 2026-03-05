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
use std::io::Cursor;

/// Metals tracked by COMEX inventory scraper.
pub const COMEX_METALS: &[ComexMetal] = &[
    ComexMetal {
        metal: "Gold",
        symbol: "GC=F",
        url: "https://www.cmegroup.com/delivery_reports/Gold_Stocks.xls",
        unit: "troy ounces",
    },
    ComexMetal {
        metal: "Silver",
        symbol: "SI=F",
        url: "https://www.cmegroup.com/delivery_reports/Silver_stocks.xls",
        unit: "troy ounces",
    },
];

/// Metadata for a tracked COMEX metal.
#[derive(Debug, Clone)]
pub struct ComexMetal {
    pub metal: &'static str,
    pub symbol: &'static str,
    pub url: &'static str,
    pub unit: &'static str,
}

/// COMEX warehouse inventory snapshot.
#[derive(Debug, Clone)]
pub struct ComexInventory {
    pub symbol: String,       // GC=F or SI=F
    pub date: String,          // YYYY-MM-DD
    pub registered: f64,       // Registered stocks (troy oz)
    pub eligible: f64,         // Eligible stocks (troy oz)
    pub total: f64,            // Total (registered + eligible)
    pub reg_ratio: f64,        // Registered / Total (%)
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

    let client = reqwest::blocking::Client::builder()
        .user_agent("pftui/0.4.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(metal.url).send()?;
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
            // Look for row with "TOTAL" in first column
            for row in range.rows() {
                if let Some(cell) = row.first() {
                    let cell_str = format!("{:?}", cell).to_uppercase();
                    if cell_str.contains("TOTAL") {
                        // Registered typically in col 1, Eligible in col 2
                        if let (Some(reg_cell), Some(elig_cell)) = (row.get(1), row.get(2)) {
                            if let Ok(reg_val) = parse_cell_as_float(reg_cell) {
                                total_registered += reg_val;
                            }
                            if let Ok(elig_val) = parse_cell_as_float(elig_cell) {
                                total_eligible += elig_val;
                            }
                        }
                        break;
                    }
                }
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

/// Parse a calamine Data as f64.
fn parse_cell_as_float(cell: &calamine::Data) -> Result<f64> {
    match cell {
        calamine::Data::Int(i) => Ok(*i as f64),
        calamine::Data::Float(f) => Ok(*f),
        calamine::Data::String(s) => {
            let cleaned = s.replace([',', '$'], "").trim().to_string();
            cleaned
                .parse::<f64>()
                .map_err(|e| anyhow!("Failed to parse '{}' as float: {}", s, e))
        }
        _ => Err(anyhow!("Cell type not numeric: {:?}", cell)),
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
}
