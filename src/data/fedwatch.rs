use anyhow::{anyhow, Context, Result};
use scraper::{Html, Selector};
use std::sync::OnceLock;

use crate::data::predictions::{MarketCategory, PredictionMarket};

const FEDWATCH_URL: &str =
    "https://cmegroup-tools.quikstrike.net/User/QuikStrikeView.aspx?viewitemid=IntegratedFedWatchTool&userId=lwolf";
const FEDWATCH_REFERER: &str =
    "https://www.cmegroup.com/markets/interest-rates/cme-fedwatch-tool.html";

#[derive(Debug, Clone, serde::Serialize)]
pub struct FedWatchSnapshot {
    pub source_url: String,
    pub fetched_at: String,
    pub meetings: Vec<String>,
    pub meeting_info: MeetingInfo,
    pub summary: SummaryProbabilities,
    pub target_probabilities: Vec<TargetProbability>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MeetingInfo {
    pub meeting_date: String,
    pub contract: String,
    pub expires: String,
    pub mid_price: f64,
    pub prior_volume: u64,
    pub prior_open_interest: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SummaryProbabilities {
    pub ease_pct: f64,
    pub no_change_pct: f64,
    pub hike_pct: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TargetProbability {
    pub target_rate_bps: String,
    pub now_pct: f64,
    pub one_day_pct: f64,
    pub one_week_pct: f64,
    pub one_month_pct: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProbabilityConflict {
    pub metric: String,
    pub cme_value_pct: f64,
    pub alt_value_pct: f64,
    pub delta_pct_points: f64,
    pub recommended_source: String,
    pub rationale: String,
    pub alt_source_label: String,
}

pub fn fetch_snapshot() -> Result<FedWatchSnapshot> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("failed to build HTTP client")?;

    let html = client
        .get(FEDWATCH_URL)
        .header("Referer", FEDWATCH_REFERER)
        .send()
        .context("failed to fetch CME FedWatch data")?
        .error_for_status()
        .context("CME FedWatch returned non-success status")?
        .text()
        .context("failed to read CME FedWatch response")?;

    parse_snapshot(&html)
}

fn parse_snapshot(html: &str) -> Result<FedWatchSnapshot> {
    let doc = Html::parse_document(html);

    let meetings = parse_meetings(&doc)?;
    let (meeting_info, summary) = parse_meeting_and_summary_tables(&doc)?;
    let target_probabilities = parse_target_probabilities(&doc)?;

    Ok(FedWatchSnapshot {
        source_url: FEDWATCH_URL.to_string(),
        fetched_at: chrono::Utc::now().to_rfc3339(),
        meetings,
        meeting_info,
        summary,
        target_probabilities,
    })
}

fn cached_selector<'a>(slot: &'a OnceLock<Selector>, css: &str) -> Result<&'a Selector> {
    if slot.get().is_none() {
        let parsed =
            Selector::parse(css).map_err(|e| anyhow!("invalid CSS selector '{}': {:?}", css, e))?;
        let _ = slot.set(parsed);
    }
    slot.get()
        .ok_or_else(|| anyhow!("failed to initialize CSS selector '{}'", css))
}

fn parse_meetings(doc: &Html) -> Result<Vec<String>> {
    static LI_SEL: OnceLock<Selector> = OnceLock::new();
    static A_SEL: OnceLock<Selector> = OnceLock::new();

    let li_sel = cached_selector(&LI_SEL, "ul.inner-tabs li")?;
    let a_sel = cached_selector(&A_SEL, "a")?;
    let mut meetings = Vec::new();

    for li in doc.select(li_sel) {
        if !li
            .value()
            .attr("class")
            .unwrap_or_default()
            .contains("do-mobile")
        {
            continue;
        }
        let Some(a) = li.select(a_sel).next() else {
            continue;
        };
        let label = text_of(&a);
        if !label.is_empty() {
            meetings.push(label);
        }
    }

    Ok(meetings)
}

fn parse_meeting_and_summary_tables(doc: &Html) -> Result<(MeetingInfo, SummaryProbabilities)> {
    static TABLE_SEL: OnceLock<Selector> = OnceLock::new();
    static ROW_SEL: OnceLock<Selector> = OnceLock::new();
    static CELL_SEL: OnceLock<Selector> = OnceLock::new();

    let table_sel = cached_selector(&TABLE_SEL, "table.grid-thm.grid-thm-v2.no-shadow.w-lg")?;
    let row_sel = cached_selector(&ROW_SEL, "tr")?;
    let cell_sel = cached_selector(&CELL_SEL, "td")?;

    let mut meeting_info: Option<MeetingInfo> = None;
    let mut summary: Option<SummaryProbabilities> = None;

    for table in doc.select(table_sel) {
        let Some(header_row) = table.select(row_sel).next() else {
            continue;
        };
        let header_text = text_of(&header_row);

        if header_text.contains("Meeting Information") {
            for row in table.select(row_sel) {
                let cells: Vec<String> = row
                    .select(cell_sel)
                    .map(|c| text_of(&c))
                    .filter(|s| !s.is_empty())
                    .collect();
                if cells.len() == 6 && cells[0] != "Meeting Date" {
                    meeting_info = Some(MeetingInfo {
                        meeting_date: cells[0].clone(),
                        contract: cells[1].clone(),
                        expires: cells[2].clone(),
                        mid_price: parse_f64(&cells[3])?,
                        prior_volume: parse_u64(&cells[4])?,
                        prior_open_interest: parse_u64(&cells[5])?,
                    });
                    break;
                }
            }
        } else if header_text.contains("Probabilities") {
            for row in table.select(row_sel) {
                let cells: Vec<String> = row
                    .select(cell_sel)
                    .map(|c| text_of(&c))
                    .filter(|s| !s.is_empty())
                    .collect();
                if cells.len() == 3 && cells[0] != "Ease" {
                    summary = Some(SummaryProbabilities {
                        ease_pct: parse_percent(&cells[0])?,
                        no_change_pct: parse_percent(&cells[1])?,
                        hike_pct: parse_percent(&cells[2])?,
                    });
                    break;
                }
            }
        }
    }

    let meeting_info = meeting_info.ok_or_else(|| anyhow!("missing meeting information table"))?;
    let summary = summary.ok_or_else(|| anyhow!("missing summary probabilities table"))?;
    Ok((meeting_info, summary))
}

fn parse_target_probabilities(doc: &Html) -> Result<Vec<TargetProbability>> {
    static TABLE_SEL: OnceLock<Selector> = OnceLock::new();
    static ROW_SEL: OnceLock<Selector> = OnceLock::new();
    static CELL_SEL: OnceLock<Selector> = OnceLock::new();

    let table_sel = cached_selector(&TABLE_SEL, "table.grid-thm.grid-thm-v2.w-lg")?;
    let row_sel = cached_selector(&ROW_SEL, "tr")?;
    let cell_sel = cached_selector(&CELL_SEL, "td")?;

    for table in doc.select(table_sel) {
        let header_text = text_of(&table);
        if !header_text.contains("Target Rate (bps)") {
            continue;
        }

        let mut rows = Vec::new();
        for row in table.select(row_sel) {
            let cells: Vec<String> = row
                .select(cell_sel)
                .map(|c| text_of(&c))
                .filter(|s| !s.is_empty())
                .collect();

            if cells.len() != 5 {
                continue;
            }

            let now = parse_percent(&cells[1])?;
            rows.push(TargetProbability {
                target_rate_bps: cells[0].clone(),
                now_pct: now,
                one_day_pct: parse_percent(&cells[2])?,
                one_week_pct: parse_percent(&cells[3])?,
                one_month_pct: parse_percent(&cells[4])?,
            });
        }

        if rows.is_empty() {
            return Err(anyhow!(
                "target probability table was found but had no rows"
            ));
        }

        rows.sort_by(|a, b| {
            b.now_pct
                .partial_cmp(&a.now_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        return Ok(rows);
    }

    Err(anyhow!("missing target probability table"))
}

fn text_of(node: &scraper::ElementRef<'_>) -> String {
    node.text()
        .collect::<String>()
        .replace('\u{a0}', " ")
        .trim()
        .to_string()
}

fn parse_percent(input: &str) -> Result<f64> {
    parse_f64(&input.replace('%', ""))
}

fn parse_u64(input: &str) -> Result<u64> {
    let n = input.replace(',', "");
    let n = n.trim();
    n.parse::<u64>()
        .with_context(|| format!("failed to parse integer from '{}'", input))
}

fn parse_f64(input: &str) -> Result<f64> {
    let n = input.replace(',', "");
    let n = n.trim();
    n.parse::<f64>()
        .with_context(|| format!("failed to parse number from '{}'", input))
}

pub fn detect_no_change_conflict(
    snapshot: &FedWatchSnapshot,
    markets: &[PredictionMarket],
    threshold_pct_points: f64,
) -> Option<ProbabilityConflict> {
    let (alt_no_change, label, volume_24h) = infer_alt_no_change_probability(markets)?;
    let cme_no_change = snapshot.summary.no_change_pct;
    let delta = (cme_no_change - alt_no_change).abs();
    if delta < threshold_pct_points {
        return None;
    }

    let (recommended_source, rationale) = if volume_24h >= 50_000.0 && delta <= 20.0 {
        (
            "CME FedWatch + investigate divergence".to_string(),
            "Prediction market has meaningful liquidity, but CME FedWatch remains canonical for implied Fed path."
                .to_string(),
        )
    } else {
        (
            "CME FedWatch".to_string(),
            "CME contract-implied probabilities are generally more stable for near-term policy path than thin/noisy event markets."
                .to_string(),
        )
    };

    Some(ProbabilityConflict {
        metric: "next_fomc_no_change_probability".to_string(),
        cme_value_pct: cme_no_change,
        alt_value_pct: alt_no_change,
        delta_pct_points: delta,
        recommended_source,
        rationale,
        alt_source_label: label,
    })
}

fn infer_alt_no_change_probability(markets: &[PredictionMarket]) -> Option<(f64, String, f64)> {
    let mut best: Option<(f64, String, f64)> = None;

    for market in markets {
        if market.category != MarketCategory::Economics {
            continue;
        }
        let q = market.question.to_lowercase();
        if !(q.contains("fed") || q.contains("fomc") || q.contains("federal reserve")) {
            continue;
        }

        let no_change = if q.contains("no change")
            || q.contains("hold rates")
            || q.contains("rates unchanged")
            || q.contains("unchanged")
        {
            market.probability * 100.0
        } else if q.contains("rate cut")
            || q.contains("cut rates")
            || q.contains("rate hike")
            || q.contains("hike rates")
        {
            100.0 - (market.probability * 100.0)
        } else {
            continue;
        };

        match &best {
            Some((_, _, v)) if *v >= market.volume_24h => {}
            _ => {
                best = Some((no_change, market.question.clone(), market.volume_24h));
            }
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::predictions::MarketCategory;

    #[test]
    fn parses_cme_fedwatch_snapshot_snippet() {
        let html = r#"
        <ul class="qs-htabs inner-tabs">
          <li class="ui-state-active do-mobile"><a>18 Mar26</a></li>
          <li class="do-mobile"><a>29 Apr26</a></li>
        </ul>
        <table class="grid-thm grid-thm-v2 no-shadow w-lg">
          <tr><th colspan="6">Meeting Information</th></tr>
          <tr><th>Meeting Date</th><th>Contract</th><th>Expires</th><th>Mid Price</th><th>Prior Volume</th><th>Prior OI</th></tr>
          <tr><td>18 Mar 2026</td><td>ZQH6</td><td>31 Mar 2026</td><td>96.3625</td><td>99,278</td><td>288,405</td></tr>
        </table>
        <table class="grid-thm grid-thm-v2 no-shadow w-lg">
          <tr><th colspan="3">Probabilities</th></tr>
          <tr><th>Ease</th><th>No Change</th><th>Hike</th></tr>
          <tr><td>3.7 %</td><td>96.3 %</td><td>0.0 %</td></tr>
        </table>
        <table class="grid-thm grid-thm-v2 w-lg">
          <tr><th rowspan="2">Target Rate (bps)</th><th colspan="4">Probability(%)</th></tr>
          <tr><th>Now</th><th>1 Day</th><th>1 Week</th><th>1 Month</th></tr>
          <tr><td>325-350</td><td>3.7%</td><td>5.0%</td><td>7.0%</td><td>10.0%</td></tr>
          <tr><td>350-375</td><td>96.3%</td><td>95.0%</td><td>93.0%</td><td>90.0%</td></tr>
        </table>
        "#;

        let parsed = parse_snapshot(html).expect("snapshot should parse");
        assert_eq!(parsed.meetings.len(), 2);
        assert_eq!(parsed.meeting_info.contract, "ZQH6");
        assert_eq!(parsed.summary.no_change_pct, 96.3);
        assert_eq!(parsed.target_probabilities[0].target_rate_bps, "350-375");
        assert_eq!(parsed.target_probabilities[0].now_pct, 96.3);
        assert_eq!(parsed.target_probabilities[1].target_rate_bps, "325-350");
    }

    #[test]
    fn detects_no_change_conflict_against_prediction_markets() {
        let snapshot = FedWatchSnapshot {
            source_url: "cme".to_string(),
            fetched_at: "2026-03-13T00:00:00Z".to_string(),
            meetings: vec!["18 Mar26".to_string()],
            meeting_info: MeetingInfo {
                meeting_date: "18 Mar 2026".to_string(),
                contract: "ZQH6".to_string(),
                expires: "31 Mar 2026".to_string(),
                mid_price: 96.0,
                prior_volume: 1,
                prior_open_interest: 1,
            },
            summary: SummaryProbabilities {
                ease_pct: 1.0,
                no_change_pct: 92.0,
                hike_pct: 7.0,
            },
            target_probabilities: vec![],
        };
        let markets = vec![PredictionMarket {
            id: "1".to_string(),
            question: "Will the Fed cut rates at the next FOMC meeting?".to_string(),
            probability: 0.989,
            volume_24h: 80_000.0,
            category: MarketCategory::Economics,
            updated_at: 0,
        }];

        let conflict =
            detect_no_change_conflict(&snapshot, &markets, 5.0).expect("should detect conflict");
        assert!(conflict.delta_pct_points > 5.0);
        assert_eq!(conflict.metric, "next_fomc_no_change_probability");
    }
}
