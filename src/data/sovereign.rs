use anyhow::{anyhow, Context, Result};
use scraper::{Html, Selector};

use crate::data::comex;

const WGC_CBD_URL: &str =
    "https://fsapi.gold.org/api/cbd/v11/charts/getPage?page=snapshot&periodicity=quarterly";
const BTC_GOV_URL: &str = "https://bitcointreasuries.net/governments";

#[derive(Debug, Clone, serde::Serialize)]
pub struct SovereignSnapshot {
    pub fetched_at: String,
    pub source_urls: SourceUrls,
    pub cb_gold_as_of: Option<String>,
    pub cb_gold_tonnes: Vec<CentralBankGold>,
    pub government_btc_holdings: Vec<GovernmentBtcHolding>,
    pub comex_silver: ComexSilverSnapshot,
    /// Non-fatal warnings from data sources that partially failed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceUrls {
    pub wgc_cbd_api: String,
    pub btc_governments_page: String,
    pub comex_silver_xls: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CentralBankGold {
    pub country: String,
    pub tonnes: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GovernmentBtcHolding {
    pub name: String,
    pub slug: String,
    pub btc: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComexSilverSnapshot {
    pub date: String,
    pub registered_oz: f64,
    pub eligible_oz: f64,
    pub total_oz: f64,
    pub registered_ratio_pct: f64,
}

/// Optional cached COMEX silver data to use as fallback when live fetch fails.
pub struct CachedComexSilver {
    pub date: String,
    pub registered_oz: f64,
    pub eligible_oz: f64,
    pub total_oz: f64,
    pub registered_ratio_pct: f64,
}

pub fn fetch_snapshot(cached_silver: Option<CachedComexSilver>) -> Result<SovereignSnapshot> {
    let mut warnings = Vec::new();

    // Fetch CB gold — non-fatal on failure
    let (cb_gold_as_of, cb_gold_tonnes) = match fetch_cb_gold() {
        Ok((as_of, tonnes)) => (as_of, tonnes),
        Err(e) => {
            warnings.push(format!("WGC central-bank gold: {}", e));
            (None, Vec::new())
        }
    };

    // Fetch government BTC — non-fatal on failure
    let government_btc_holdings = match fetch_government_btc() {
        Ok(btc) => btc,
        Err(e) => {
            warnings.push(format!("Government BTC holdings: {}", e));
            Vec::new()
        }
    };

    // Fetch COMEX silver — fall back to cached data if live fetch fails
    let comex_silver = match comex::fetch_inventory("SI=F") {
        Ok(silver) => ComexSilverSnapshot {
            date: silver.date,
            registered_oz: silver.registered,
            eligible_oz: silver.eligible,
            total_oz: silver.total,
            registered_ratio_pct: silver.reg_ratio,
        },
        Err(e) => {
            if let Some(cached) = cached_silver {
                warnings.push(format!(
                    "COMEX silver live fetch failed ({}), using cached data from {}",
                    e, cached.date
                ));
                ComexSilverSnapshot {
                    date: cached.date,
                    registered_oz: cached.registered_oz,
                    eligible_oz: cached.eligible_oz,
                    total_oz: cached.total_oz,
                    registered_ratio_pct: cached.registered_ratio_pct,
                }
            } else {
                warnings.push(format!("COMEX silver: {}", e));
                ComexSilverSnapshot {
                    date: "unavailable".to_string(),
                    registered_oz: 0.0,
                    eligible_oz: 0.0,
                    total_oz: 0.0,
                    registered_ratio_pct: 0.0,
                }
            }
        }
    };

    Ok(SovereignSnapshot {
        fetched_at: chrono::Utc::now().to_rfc3339(),
        source_urls: SourceUrls {
            wgc_cbd_api: WGC_CBD_URL.to_string(),
            btc_governments_page: BTC_GOV_URL.to_string(),
            comex_silver_xls: "https://www.cmegroup.com/delivery_reports/Silver_stocks.xls"
                .to_string(),
        },
        cb_gold_as_of,
        cb_gold_tonnes,
        government_btc_holdings,
        comex_silver,
        warnings,
    })
}

fn fetch_cb_gold() -> Result<(Option<String>, Vec<CentralBankGold>)> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("pftui/0.6.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;

    let payload: serde_json::Value = client
        .get(WGC_CBD_URL)
        .send()
        .context("failed to fetch WGC central-bank data")?
        .error_for_status()
        .context("WGC central-bank endpoint returned non-success status")?
        .json()
        .context("failed to decode WGC central-bank JSON")?;

    parse_cb_gold_from_wgc_json(&payload)
}

fn parse_cb_gold_from_wgc_json(
    payload: &serde_json::Value,
) -> Result<(Option<String>, Vec<CentralBankGold>)> {
    let as_of = payload
        .pointer("/chartData/options/maxDateAvailable")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);

    let date_key = as_of
        .as_deref()
        .ok_or_else(|| anyhow!("missing chartData.options.maxDateAvailable in WGC payload"))?;

    let data = payload
        .pointer(&format!(
            "/chartData/treeMap/LAST_YEAR_END/{}/gold_reserves_tns/data",
            date_key
        ))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow!("missing gold_reserves_tns dataset in WGC payload"))?;

    let mut out = Vec::new();

    for row in data {
        let Some(country) = row.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };

        // Keep only leaf country rows with numeric holdings.
        if row
            .get("parent")
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            continue;
        }

        let Some(tonnes) = row.get("value").and_then(serde_json::Value::as_f64) else {
            continue;
        };

        if tonnes <= 0.0 {
            continue;
        }

        out.push(CentralBankGold {
            country: country.to_string(),
            tonnes,
        });
    }

    out.sort_by(|a, b| {
        b.tonnes
            .partial_cmp(&a.tonnes)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok((as_of, out))
}

fn fetch_government_btc() -> Result<Vec<GovernmentBtcHolding>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;

    let html = client
        .get(BTC_GOV_URL)
        .send()
        .context("failed to fetch government BTC page")?
        .error_for_status()
        .context("government BTC page returned non-success status")?
        .text()
        .context("failed to decode government BTC page")?;

    parse_government_btc_html(&html)
}

fn parse_government_btc_html(html: &str) -> Result<Vec<GovernmentBtcHolding>> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("a[href^='/governments/']").expect("valid selector");

    let mut by_slug = std::collections::BTreeMap::<String, GovernmentBtcHolding>::new();

    for a in doc.select(&sel) {
        let Some(href) = a.value().attr("href") else {
            continue;
        };

        let slug = href
            .strip_prefix("/governments/")
            .unwrap_or_default()
            .trim()
            .to_string();

        if slug.is_empty() {
            continue;
        }

        let text = a.text().collect::<Vec<_>>().join(" ");
        let Some(btc) = parse_btc_amount(&text) else {
            continue;
        };

        let name = a
            .value()
            .attr("aria-label")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| slug_to_name(&slug));

        let candidate = GovernmentBtcHolding {
            name,
            slug: slug.clone(),
            btc,
        };

        match by_slug.get(&slug) {
            Some(existing) if existing.btc >= candidate.btc => {}
            _ => {
                by_slug.insert(slug, candidate);
            }
        }
    }

    let mut out: Vec<_> = by_slug.into_values().collect();
    out.sort_by(|a, b| {
        b.btc
            .partial_cmp(&a.btc)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

fn parse_btc_amount(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    for i in 1..parts.len() {
        if parts[i].eq_ignore_ascii_case("BTC") {
            let raw = parts[i - 1].replace(',', "");
            if let Ok(v) = raw.parse::<f64>() {
                return Some(v);
            }
        }
    }
    None
}

fn slug_to_name(slug: &str) -> String {
    slug.split('-')
        .filter(|s| !s.is_empty())
        .map(|p| {
            let mut ch = p.chars();
            match ch.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), ch.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wgc_json_gold_rows() {
        let payload = serde_json::json!({
            "chartData": {
                "options": { "maxDateAvailable": "2025-12-31" },
                "treeMap": {
                    "LAST_YEAR_END": {
                        "2025-12-31": {
                            "gold_reserves_tns": {
                                "data": [
                                    { "name": "Region Total", "value": 10000.0 },
                                    { "name": "Italy", "parent": "Western Europe", "value": 2451.87 },
                                    { "name": "United States of America", "parent": "North America", "value": null },
                                    { "name": "China", "parent": "East Asia", "value": 2306.3 }
                                ]
                            }
                        }
                    }
                }
            }
        });

        let (as_of, rows) = parse_cb_gold_from_wgc_json(&payload).expect("parse should succeed");
        assert_eq!(as_of.as_deref(), Some("2025-12-31"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].country, "Italy");
        assert_eq!(rows[1].country, "China");
    }

    #[test]
    fn parses_government_btc_rows() {
        let html = r#"
        <html><body>
          <a href="/governments/united-states" aria-label="United States">
            <text>United States</text><text>198,109 BTC</text>
          </a>
          <a href="/governments/north-korea" aria-label="North Korea">
            <text>North Korea</text><text>803 BTC</text>
          </a>
          <a href="/governments/finland" aria-label="Finland">
            <text>Finland</text><text>90 BTC</text>
          </a>
        </body></html>
        "#;

        let rows = parse_government_btc_html(html).expect("parse should succeed");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].slug, "united-states");
        assert_eq!(rows[0].btc, 198_109.0);
        assert_eq!(rows[2].slug, "finland");
        assert_eq!(rows[2].btc, 90.0);
    }
}
