//! Capital-flow ingestion provider scaffold (F59).
//!
//! The SEC EDGAR 13F path is implemented against the free public
//! submissions JSON + per-filing infoTable XML. The ETF.com flow path
//! is implemented against the public fund-flows-tool HTML table
//! (scraped). This module ships the provider contract, the working
//! `NoopProvider`, the live `EtfComCsvProvider` (HTML scraper despite
//! the historical name), and the live `SecEdgar13fProvider`.
//!
//! Selection contract
//! ------------------
//!
//! The environment variable `PFTUI_FLOWS_PROVIDER` selects which provider
//! the refresh path uses:
//!
//! - `noop` (default) — no-op provider, logs "capital flows provider not
//!   configured" and returns zero flows. Always safe.
//! - `etf_com_csv` — live ETF.com fund-flows HTML scraper. Pulls the
//!   public `https://www.etf.com/etfanalytics/etf-fund-flows-tool` page,
//!   discovers the flows table by header content ("Ticker" + "Net
//!   Flow"), and emits one `CapitalFlow` row per ETF using the daily
//!   net-flow column (falling back to the weekly column when daily is
//!   blank). Positive net flow → `flow_type = "etf_creation"`; negative
//!   → `flow_type = "etf_redemption"`. `amount_usd` is the absolute
//!   value (sign is implicit in flow_type). Malformed rows are silently
//!   dropped; the provider only `bail!`s when ZERO rows parse. The
//!   refresh hook enforces a 12-hour cadence throttle keyed on
//!   `MAX(capital_flows.fetched_at WHERE source LIKE 'etf.com/%')`.
//!   Scraping is fragile — the table structure can change without
//!   notice; treat failed runs as a "page changed" signal and inspect.
//! - `sec_edgar_13f` — live SEC EDGAR ingest. Walks a SMALL canonical
//!   list of well-known filers (`TRACKED_CIKS`), pulls each filer's most
//!   recent 13F-HR filing via the public `data.sec.gov` submissions
//!   feed, fetches the holdings `infoTable` XML, and emits one
//!   `CapitalFlow { flow_type: "institutional_13f" }` row per
//!   (filer + issuer-CUSIP + quarter). Per-filer HTTP/parse errors are
//!   collected and logged in the result `note`; the call only
//!   `bail!`s if EVERY tracked filer fails. Runs no more than once
//!   per quarter automatically when invoked from `data refresh` — the
//!   refresh hook checks the most-recent `capital_flows.fetched_at`
//!   for any `institutional_13f` row and short-circuits with a
//!   "throttled" note if the last successful fetch landed within
//!   ~80 days.
//!
//! `amount_usd` is stored as a `rust_decimal::Decimal` because these are
//! money values (per the project `CLAUDE.md` standards) and serialised to
//! the SQLite `capital_flows.amount_usd TEXT` column as a decimal string.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;
use rust_decimal::Decimal;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

/// Allowed values for the `capital_flows.flow_type` column.
pub const FLOW_TYPES: &[&str] = &[
    "etf_creation",
    "etf_redemption",
    "institutional_13f",
    "crypto_exchange_inflow",
    "crypto_exchange_outflow",
];

/// Canonical SEC-required `User-Agent` for EDGAR requests. SEC EDGAR
/// rejects requests without a `User-Agent` identifying the requester.
/// Generic placeholder contact — operators who want to attribute their
/// pftui install can override by editing this string locally.
pub const EDGAR_USER_AGENT: &str = "pftui-bot/0.28 contact@example.com";

/// Polite `User-Agent` for the ETF.com fund-flows HTML scraper. Includes
/// a contact URL per scraper etiquette so the etf.com operators can
/// identify pftui if they need to.
pub const ETF_COM_USER_AGENT: &str =
    "pftui-bot/0.28 https://github.com/skylarsimoncelli/pftui";

/// Public fund-flows-tool page on ETF.com. Renders an HTML table of
/// daily/weekly/monthly net flows per ETF.
pub const ETF_COM_FLOWS_URL: &str =
    "https://www.etf.com/etfanalytics/etf-fund-flows-tool";

/// Canonical `source` string written to `capital_flows.source` for rows
/// originating from the ETF.com scraper. Used by the refresh hook to
/// detect prior-fetch freshness for the 12-hour cadence throttle.
pub const ETF_COM_SOURCE: &str = "etf.com/etfanalytics";

/// Small canonical roster of well-known 13F filers. Tuples are
/// `(CIK as 10-digit zero-padded string, human-readable filer name)`.
/// These CIKs are publicly listed in SEC EDGAR's filer search and never
/// change. Keeping the list small bounds the network walk and the
/// downstream `capital_flows` row count per quarter.
pub const TRACKED_CIKS: &[(&str, &str)] = &[
    ("0001067983", "Berkshire Hathaway Inc"),
    ("0001350694", "Bridgewater Associates LP"),
    ("0001037389", "Renaissance Technologies LLC"),
    ("0001423053", "Citadel Advisors LLC"),
];

/// One canonical capital-flow observation, agnostic to the upstream provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapitalFlow {
    pub asset: String,
    pub flow_type: String,
    pub amount_usd: Decimal,
    /// ISO-8601 date (YYYY-MM-DD) for the period start.
    pub period_start: String,
    /// ISO-8601 date (YYYY-MM-DD) for the period end (inclusive).
    pub period_end: String,
    pub source: String,
}

/// Result returned by a provider's `fetch` call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowFetchResult {
    pub flows: Vec<CapitalFlow>,
    /// Human-readable note suitable for logging into the refresh DAG.
    pub note: String,
}

/// Provider contract — anything that can yield `CapitalFlow` rows.
pub trait FlowProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Fetch flows for an optional asset filter.
    ///
    /// Implementations MUST NOT panic. Return `Ok(FlowFetchResult { flows:
    /// vec![], note: "..." })` for the gracefully-degraded path, and
    /// `Err(...)` only when a real upstream failure means the caller needs
    /// to surface an error.
    fn fetch(&self, asset_filter: Option<&str>) -> Result<FlowFetchResult>;
}

/// Default provider — does nothing, returns no flows, never errors.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopProvider;

impl FlowProvider for NoopProvider {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn fetch(&self, _asset_filter: Option<&str>) -> Result<FlowFetchResult> {
        Ok(FlowFetchResult {
            flows: Vec::new(),
            note: "capital flows provider not configured".to_string(),
        })
    }
}

/// Live ETF.com fund-flows provider.
///
/// Despite the legacy name (`etf_com_csv`), the upstream source is the
/// public HTML fund-flows-tool page rather than a CSV download. Each
/// row is mapped to one [`CapitalFlow`] keyed on the ETF's ticker, with
/// `flow_type` set from the sign of the net-flow column and
/// `amount_usd` carrying the absolute USD magnitude.
///
/// The fetch logic is intentionally defensive: malformed rows are
/// silently dropped, and the provider only returns an error when ZERO
/// rows parse (so the refresh DAG can distinguish "page changed" from
/// "no flows today"). Use `PFTUI_FLOWS_PROVIDER=etf_com_csv` to enable.
#[derive(Debug, Default, Clone, Copy)]
pub struct EtfComCsvProvider;

impl FlowProvider for EtfComCsvProvider {
    fn name(&self) -> &'static str {
        "etf_com_csv"
    }

    fn fetch(&self, asset_filter: Option<&str>) -> Result<FlowFetchResult> {
        let html = fetch_etf_com_flows_html()?;
        let mut flows = parse_etf_com_flows(&html)?;
        if let Some(filter) = asset_filter {
            flows.retain(|flow| flow.asset.eq_ignore_ascii_case(filter));
        }
        let note = format!(
            "etf.com/etfanalytics: {} rows (daily/weekly net flow scrape)",
            flows.len()
        );
        Ok(FlowFetchResult { flows, note })
    }
}

/// GET the ETF.com fund-flows-tool page using the polite scraper User-Agent.
fn fetch_etf_com_flows_html() -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(ETF_COM_USER_AGENT)
        .timeout(Duration::from_secs(20))
        .build()
        .context("build ETF.com HTTP client")?;
    client
        .get(ETF_COM_FLOWS_URL)
        .header("Accept", "text/html")
        .send()
        .with_context(|| format!("GET {ETF_COM_FLOWS_URL}"))?
        .error_for_status()
        .with_context(|| format!("ETF.com HTTP error for {ETF_COM_FLOWS_URL}"))?
        .text()
        .context("read ETF.com response body")
}

/// Parse the ETF.com fund-flows-tool HTML page into [`CapitalFlow`] rows.
///
/// Pure function — no I/O, no panics. Safe to call from tests against a
/// fixture file. The scraper is intentionally defensive:
///
/// 1. The flows table is located by scanning for the first `<table>`
///    whose header row contains BOTH "Ticker" and "Net Flow" cells
///    (case-insensitive). This survives most CSS class renames.
/// 2. The column indices for ticker, daily net flow, and weekly net
///    flow are resolved from the header row by name, not by position,
///    so column reordering does not silently corrupt data.
/// 3. Each row's cell extraction is wrapped in `.ok()` — malformed rows
///    are dropped without aborting the whole scrape.
/// 4. Per-row net flow defaults to the daily column; the weekly column
///    is used only when daily is blank/unparseable.
/// 5. If ZERO rows parse, the function returns `Err(...)` so the
///    refresh DAG records a Failed (rather than Ok-with-zero-rows)
///    result — this is the "page structure changed" signal.
///
/// `period_start`/`period_end` are `today` for daily flows, or the
/// most-recent Monday → today for weekly fallback flows.
pub fn parse_etf_com_flows(html: &str) -> Result<Vec<CapitalFlow>> {
    let today = Utc::now().date_naive();
    parse_etf_com_flows_at(html, today)
}

/// Date-injected variant of [`parse_etf_com_flows`] used by tests so
/// `period_start`/`period_end` are deterministic.
pub fn parse_etf_com_flows_at(html: &str, today: NaiveDate) -> Result<Vec<CapitalFlow>> {
    let document = Html::parse_document(html);
    let table_sel =
        Selector::parse("table").map_err(|e| anyhow!("invalid table selector: {e:?}"))?;
    let row_sel = Selector::parse("tr").map_err(|e| anyhow!("invalid row selector: {e:?}"))?;
    let header_cell_sel =
        Selector::parse("th, td").map_err(|e| anyhow!("invalid header-cell selector: {e:?}"))?;
    let body_cell_sel =
        Selector::parse("td").map_err(|e| anyhow!("invalid body-cell selector: {e:?}"))?;

    let table = locate_flows_table(&document, &table_sel, &row_sel, &header_cell_sel)
        .context("no ETF.com flows table found (page structure changed?)")?;

    let rows: Vec<ElementRef<'_>> = table.select(&row_sel).collect();
    let header_cells: Vec<String> = rows
        .first()
        .map(|r| {
            r.select(&header_cell_sel)
                .map(|c| cell_text(&c))
                .collect()
        })
        .unwrap_or_default();
    let columns = resolve_columns(&header_cells)
        .context("ETF.com flows header missing Ticker / Net Flow column")?;

    let today_iso = today.format("%Y-%m-%d").to_string();
    let monday_iso = most_recent_monday(today).format("%Y-%m-%d").to_string();

    let mut flows: Vec<CapitalFlow> = Vec::new();
    for row in rows.iter().skip(1) {
        if let Some(flow) =
            parse_row(row, &body_cell_sel, &columns, &today_iso, &monday_iso).ok().flatten()
        {
            flows.push(flow);
        }
    }

    if flows.is_empty() {
        bail!(
            "etf.com flows scrape returned zero rows — page structure may have changed (URL: {})",
            ETF_COM_FLOWS_URL
        );
    }
    Ok(flows)
}

/// Column-index resolution for the flows table. Position-by-name so a
/// future column reorder does not silently corrupt data.
#[derive(Debug, Clone, Copy)]
struct ColumnLayout {
    ticker: usize,
    daily: Option<usize>,
    weekly: Option<usize>,
}

fn resolve_columns(header_cells: &[String]) -> Result<ColumnLayout> {
    let ticker = header_cells
        .iter()
        .position(|cell| cell.eq_ignore_ascii_case("ticker"))
        .context("no 'Ticker' header cell")?;
    let daily = header_cells.iter().position(|cell| {
        let lower = cell.to_ascii_lowercase();
        lower.contains("net flow") && (lower.contains("1d") || lower.contains("daily"))
    });
    let weekly = header_cells.iter().position(|cell| {
        let lower = cell.to_ascii_lowercase();
        lower.contains("net flow") && (lower.contains("1w") || lower.contains("week"))
    });
    if daily.is_none() && weekly.is_none() {
        bail!("no 'Net Flow' daily or weekly column found");
    }
    Ok(ColumnLayout {
        ticker,
        daily,
        weekly,
    })
}

/// Scan every `<table>` in the document for the one whose header row
/// contains BOTH "Ticker" and "Net Flow" cells (case-insensitive).
fn locate_flows_table<'a>(
    document: &'a Html,
    table_sel: &Selector,
    row_sel: &Selector,
    header_cell_sel: &Selector,
) -> Option<ElementRef<'a>> {
    for table in document.select(table_sel) {
        let Some(header_row) = table.select(row_sel).next() else {
            continue;
        };
        let headers: Vec<String> = header_row
            .select(header_cell_sel)
            .map(|c| cell_text(&c).to_ascii_lowercase())
            .collect();
        let has_ticker = headers.iter().any(|h| h == "ticker");
        let has_net_flow = headers.iter().any(|h| h.contains("net flow"));
        if has_ticker && has_net_flow {
            return Some(table);
        }
    }
    None
}

/// Parse a single table row into a [`CapitalFlow`], or `Ok(None)` if
/// the row should be dropped (e.g. malformed cells, blank ticker).
/// Returns `Err` only for unexpected selector failures — the caller
/// drops both `Err` and `Ok(None)`.
fn parse_row(
    row: &ElementRef<'_>,
    body_cell_sel: &Selector,
    columns: &ColumnLayout,
    today_iso: &str,
    monday_iso: &str,
) -> Result<Option<CapitalFlow>> {
    let cells: Vec<String> = row.select(body_cell_sel).map(|c| cell_text(&c)).collect();
    if cells.is_empty() {
        return Ok(None);
    }
    let Some(ticker_raw) = cells.get(columns.ticker) else {
        return Ok(None);
    };
    let ticker = ticker_raw.trim().to_ascii_uppercase();
    if ticker.is_empty() || ticker == "--" {
        return Ok(None);
    }

    let daily_flow = columns
        .daily
        .and_then(|idx| cells.get(idx))
        .and_then(|raw| parse_flow_usd(raw));
    let weekly_flow = columns
        .weekly
        .and_then(|idx| cells.get(idx))
        .and_then(|raw| parse_flow_usd(raw));

    let (amount_signed, period_start, period_end) = match (daily_flow, weekly_flow) {
        (Some(daily), _) => (daily, today_iso.to_string(), today_iso.to_string()),
        (None, Some(weekly)) => (weekly, monday_iso.to_string(), today_iso.to_string()),
        (None, None) => return Ok(None),
    };

    let flow_type = if amount_signed.is_sign_negative() {
        "etf_redemption"
    } else {
        "etf_creation"
    };
    Ok(Some(CapitalFlow {
        asset: ticker,
        flow_type: flow_type.to_string(),
        amount_usd: amount_signed.abs(),
        period_start,
        period_end,
        source: ETF_COM_SOURCE.to_string(),
    }))
}

/// Collapse all text descendants of an element to a single space-joined
/// string. Mirrors the helper used by other scrapers in `src/data/`.
fn cell_text(element: &ElementRef<'_>) -> String {
    let raw: String = element.text().collect::<Vec<_>>().join(" ");
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse a dollar-formatted net-flow cell like `"$1,234,567.89"`,
/// `"-$987,654,321.00"`, `"($45,000)"`, or `"45.2M"` into a signed
/// `Decimal`. Returns `None` on any parse failure so the caller can
/// silently drop the row.
pub fn parse_flow_usd(raw: &str) -> Option<Decimal> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "--" || trimmed == "n/a" || trimmed.eq_ignore_ascii_case("na")
    {
        return None;
    }
    // Accounting-style negatives: "($45,000)" → "-45000".
    let (negated_by_parens, body) = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        (true, &trimmed[1..trimmed.len() - 1])
    } else {
        (false, trimmed)
    };

    let mut sign = if negated_by_parens { -1i64 } else { 1i64 };
    let mut chars = body.chars().peekable();
    if let Some(&c) = chars.peek() {
        if c == '-' {
            sign *= -1;
            chars.next();
        } else if c == '+' {
            chars.next();
        }
    }
    let rest: String = chars.collect();
    let rest = rest.trim().trim_start_matches('$').trim();

    // Magnitude suffix (K/M/B/T) — common on ETF.com summary rows.
    let mut multiplier = Decimal::new(1, 0);
    let mut numeric_str = rest.to_string();
    if let Some(last) = rest.chars().last() {
        let mult = match last.to_ascii_uppercase() {
            'K' => Some(Decimal::new(1_000, 0)),
            'M' => Some(Decimal::new(1_000_000, 0)),
            'B' => Some(Decimal::new(1_000_000_000, 0)),
            'T' => Some(Decimal::new(1_000_000_000_000, 0)),
            _ => None,
        };
        if let Some(m) = mult {
            multiplier = m;
            numeric_str = rest[..rest.len() - last.len_utf8()].trim().to_string();
        }
    }

    let cleaned: String = numeric_str.chars().filter(|c| *c != ',' && *c != ' ').collect();
    if cleaned.is_empty() {
        return None;
    }
    let magnitude = Decimal::from_str_exact(&cleaned).ok()?;
    let signed = magnitude * multiplier;
    Some(if sign < 0 { -signed } else { signed })
}

/// The most recent Monday on or before `today`. Used for the weekly
/// flow `period_start` when daily data is unavailable.
fn most_recent_monday(today: NaiveDate) -> NaiveDate {
    let offset = today.weekday().num_days_from_monday() as i64;
    today - chrono::Duration::days(offset)
}

/// Live SEC EDGAR 13F-HR provider. Walks the canonical filer roster in
/// [`TRACKED_CIKS`], parses each filer's most recent 13F-HR `infoTable`
/// XML, and emits one [`CapitalFlow`] row per (filer + issuer-CUSIP +
/// quarter).
///
/// HTTP / parse errors on a single filer are accumulated and surfaced in
/// the returned `FlowFetchResult.note`; the call only `bail!`s when
/// EVERY tracked filer fails. Use `PFTUI_FLOWS_PROVIDER=sec_edgar_13f`
/// to enable in the refresh pipeline.
#[derive(Debug, Default, Clone, Copy)]
pub struct SecEdgar13fProvider;

impl FlowProvider for SecEdgar13fProvider {
    fn name(&self) -> &'static str {
        "sec_edgar_13f"
    }

    fn fetch(&self, asset_filter: Option<&str>) -> Result<FlowFetchResult> {
        let client = build_edgar_client()?;
        let mut flows: Vec<CapitalFlow> = Vec::new();
        let mut filer_notes: Vec<String> = Vec::new();
        let mut filer_failures: Vec<String> = Vec::new();
        let mut filer_successes = 0usize;

        for (cik, filer_name) in TRACKED_CIKS {
            match fetch_latest_13f_for_filer(&client, cik, filer_name) {
                Ok(filer_flows) => {
                    filer_successes += 1;
                    let mut kept = 0usize;
                    for flow in filer_flows {
                        if let Some(filter) = asset_filter {
                            if !flow.asset.eq_ignore_ascii_case(filter) {
                                continue;
                            }
                        }
                        flows.push(flow);
                        kept += 1;
                    }
                    filer_notes.push(format!("{filer_name}: {kept} rows"));
                }
                Err(e) => {
                    filer_failures.push(format!("{filer_name}: {e}"));
                }
            }
        }

        if filer_successes == 0 {
            bail!(
                "sec_edgar_13f: all {} tracked filers failed ({})",
                TRACKED_CIKS.len(),
                filer_failures.join("; ")
            );
        }

        let mut note = filer_notes.join(", ");
        if !filer_failures.is_empty() {
            note.push_str("; failed: ");
            note.push_str(&filer_failures.join("; "));
        }

        Ok(FlowFetchResult { flows, note })
    }
}

/// Build the shared blocking HTTP client used for EDGAR requests. A
/// 20-second timeout is enough for the small JSON + XML payloads
/// involved and short enough that a hung filer can't stall the entire
/// refresh.
fn build_edgar_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(EDGAR_USER_AGENT)
        .timeout(Duration::from_secs(20))
        .build()
        .context("build EDGAR HTTP client")
}

/// Submissions JSON shape we care about — only the fields needed to
/// locate the most recent 13F-HR filing. The full document carries
/// hundreds of additional keys we deliberately ignore.
#[derive(Debug, Deserialize)]
struct SubmissionsResponse {
    filings: SubmissionsFilings,
}

#[derive(Debug, Deserialize)]
struct SubmissionsFilings {
    recent: SubmissionsRecent,
}

#[derive(Debug, Deserialize)]
struct SubmissionsRecent {
    #[serde(rename = "accessionNumber")]
    accession_number: Vec<String>,
    form: Vec<String>,
    #[serde(rename = "reportDate")]
    report_date: Vec<String>,
}

/// Filing-detail `index.json` shape — emitted by EDGAR for every
/// archive directory. We use it to find the actual `infoTable` XML
/// inside the filing without hardcoding a filename convention.
#[derive(Debug, Deserialize)]
struct FilingIndex {
    directory: FilingIndexDirectory,
}

#[derive(Debug, Deserialize)]
struct FilingIndexDirectory {
    item: Vec<FilingIndexItem>,
}

#[derive(Debug, Deserialize)]
struct FilingIndexItem {
    name: String,
}

/// Walk one filer's submissions feed and pull `CapitalFlow` rows from
/// the latest 13F-HR filing. Returns an error if any step in the walk
/// fails — the caller catches and continues with the next filer.
fn fetch_latest_13f_for_filer(
    client: &reqwest::blocking::Client,
    cik: &str,
    filer_name: &str,
) -> Result<Vec<CapitalFlow>> {
    let submissions_url = format!("https://data.sec.gov/submissions/CIK{cik}.json");
    let submissions: SubmissionsResponse = client
        .get(&submissions_url)
        .send()
        .with_context(|| format!("GET {submissions_url}"))?
        .error_for_status()
        .with_context(|| format!("EDGAR submissions HTTP error for CIK {cik}"))?
        .json()
        .with_context(|| format!("parse submissions JSON for CIK {cik}"))?;

    let (accession, period_of_report) = pick_latest_13fhr(&submissions.filings.recent)
        .with_context(|| format!("no 13F-HR filings found for {filer_name}"))?;

    let accession_no_dashes: String = accession.chars().filter(|c| *c != '-').collect();
    let cik_no_leading: String = cik.trim_start_matches('0').to_string();
    let filing_dir = format!(
        "https://www.sec.gov/Archives/edgar/data/{cik_no_leading}/{accession_no_dashes}"
    );
    let index_url = format!("{filing_dir}/index.json");
    let index: FilingIndex = client
        .get(&index_url)
        .send()
        .with_context(|| format!("GET {index_url}"))?
        .error_for_status()
        .with_context(|| format!("EDGAR filing index HTTP error for {accession}"))?
        .json()
        .with_context(|| format!("parse filing index JSON for {accession}"))?;

    let infotable_name = pick_infotable_xml_name(&index.directory.item)
        .with_context(|| format!("no infoTable XML found in {accession}"))?;
    let infotable_url = format!("{filing_dir}/{infotable_name}");

    let xml_bytes = client
        .get(&infotable_url)
        .send()
        .with_context(|| format!("GET {infotable_url}"))?
        .error_for_status()
        .with_context(|| format!("EDGAR infoTable HTTP error for {accession}"))?
        .bytes()
        .with_context(|| format!("read infoTable body for {accession}"))?;

    let (period_start, period_end) = quarter_window_for(&period_of_report)
        .with_context(|| format!("invalid periodOfReport '{period_of_report}'"))?;

    let source = format!("sec_edgar_13f:{filer_name} ({accession})");
    parse_infotable_xml(&xml_bytes, &source, &period_start, &period_end)
}

/// Find the index of the most recent 13F-HR entry in the submissions
/// `recent` block. EDGAR returns entries newest-first so the first
/// matching `form` slot wins. Returns `Some((accession, period_of_report))`.
fn pick_latest_13fhr(recent: &SubmissionsRecent) -> Option<(String, String)> {
    for (i, form) in recent.form.iter().enumerate() {
        if form == "13F-HR" {
            let accession = recent.accession_number.get(i)?.clone();
            let period = recent.report_date.get(i)?.clone();
            return Some((accession, period));
        }
    }
    None
}

/// Scan a filing's `index.json` directory listing for the holdings
/// info-table XML. EDGAR's filename convention varies (`infotable.xml`,
/// `form13fInfoTable.xml`, `wfXXX_infotable.xml`, etc.) so we match on
/// the substring `infotable` (case-insensitive) and reject the metadata
/// `primary_doc.xml`.
fn pick_infotable_xml_name(items: &[FilingIndexItem]) -> Option<String> {
    for item in items {
        let lower = item.name.to_ascii_lowercase();
        if lower.ends_with(".xml")
            && lower.contains("infotable")
            && !lower.contains("primary_doc")
        {
            return Some(item.name.clone());
        }
    }
    // Fallback: any `.xml` that isn't `primary_doc.xml`.
    for item in items {
        let lower = item.name.to_ascii_lowercase();
        if lower.ends_with(".xml") && !lower.contains("primary_doc") {
            return Some(item.name.clone());
        }
    }
    None
}

/// One `<infoTable>` row pulled out of the holdings XML before being
/// folded into a `CapitalFlow`. Public for tests.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoTableEntry {
    pub name_of_issuer: String,
    pub cusip: String,
    /// As-reported `<value>` element. Pre-2023 13F amendments report
    /// this in thousands of dollars; the May 2023 amendment switched
    /// to whole dollars. We disambiguate at convert-time via
    /// [`amount_usd_from_value`].
    pub value_raw: Decimal,
}

/// Convert a parsed `<value>` reading into whole dollars. The 13F-HR
/// `value` column was reported in thousands of dollars for filings up
/// through 2022-Q4 and in whole dollars from 2023-Q1 onward (SEC Final
/// Rule 33-11070). Heuristic: if the reading is < $1B treat it as
/// thousands (and multiply by 1000); otherwise treat as whole dollars.
/// $1B is a safe pivot because: (a) any institutional holding worth
/// $1B+ when reported in thousands would have been > 10^9 thousand-USD
/// (= $10^12, i.e. a trillion-dollar holding) which is impossible; and
/// (b) a $1B+ holding under the new whole-dollar regime is routine for
/// large filers.
pub fn amount_usd_from_value(value_raw: Decimal) -> Decimal {
    let one_billion = Decimal::new(1_000_000_000, 0);
    if value_raw < one_billion {
        value_raw * Decimal::new(1000, 0)
    } else {
        value_raw
    }
}

/// Parse a 13F-HR `infoTable` XML byte buffer into [`CapitalFlow`]
/// rows. Pure function — no I/O, no panics, suitable for fixture
/// testing. Each `<infoTable>` element produces one row keyed by CUSIP.
///
/// `period_start` / `period_end` are passed in by the caller because
/// the holdings XML itself does not carry the `periodOfReport` field
/// (that lives in the filing's `primary_doc.xml` companion).
pub fn parse_infotable_xml(
    xml: &[u8],
    source: &str,
    period_start: &str,
    period_end: &str,
) -> Result<Vec<CapitalFlow>> {
    let entries = parse_infotable_entries(xml)?;
    let mut flows = Vec::with_capacity(entries.len());
    for entry in entries {
        let amount_usd = amount_usd_from_value(entry.value_raw);
        flows.push(CapitalFlow {
            // Use the issuer's CUSIP as the canonical `asset` key because
            // 13F filings don't carry a ticker column; downstream views
            // can join CUSIP→ticker against the assets table later.
            asset: entry.cusip,
            flow_type: "institutional_13f".to_string(),
            amount_usd,
            period_start: period_start.to_string(),
            period_end: period_end.to_string(),
            source: source.to_string(),
        });
    }
    Ok(flows)
}

/// Pure-XML parse — extract every `<infoTable>` element from a 13F-HR
/// holdings XML payload. Public so tests can exercise the parser
/// without needing the full `CapitalFlow` wrapper.
pub fn parse_infotable_entries(xml: &[u8]) -> Result<Vec<InfoTableEntry>> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);

    let mut entries: Vec<InfoTableEntry> = Vec::new();
    let mut in_info_table = false;
    let mut current_tag: Option<String> = None;
    let mut current = ScratchEntry::default();
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .context("read infoTable XML event")?
        {
            Event::Start(e) => {
                let name = local_name(e.name().as_ref());
                if name == "infoTable" {
                    in_info_table = true;
                    current = ScratchEntry::default();
                } else if in_info_table {
                    current_tag = Some(name);
                }
            }
            Event::End(e) => {
                let name = local_name(e.name().as_ref());
                if name == "infoTable" {
                    let taken = std::mem::take(&mut current);
                    let entry = taken.finish().with_context(|| {
                        format!("incomplete infoTable entry at row {}", entries.len() + 1)
                    })?;
                    entries.push(entry);
                    in_info_table = false;
                    current_tag = None;
                } else if in_info_table && current_tag.as_deref() == Some(&name) {
                    current_tag = None;
                }
            }
            Event::Text(t) => {
                if !in_info_table {
                    continue;
                }
                let Some(tag) = current_tag.as_deref() else {
                    continue;
                };
                let text = t
                    .decode()
                    .context("decode XML text")?
                    .into_owned();
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match tag {
                    "nameOfIssuer" => current.name_of_issuer = Some(trimmed.to_string()),
                    "cusip" => current.cusip = Some(trimmed.to_string()),
                    "value" => {
                        // Strip commas defensively — most filings omit
                        // them, but a couple of historical filers
                        // include thousand-separators.
                        let clean: String = trimmed.chars().filter(|c| *c != ',').collect();
                        let dec = Decimal::from_str_exact(&clean).with_context(|| {
                            format!("parse <value>'{clean}' as decimal")
                        })?;
                        current.value_raw = Some(dec);
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

/// Local-name view of an XML element name, dropping any namespace
/// prefix (`ns1:infoTable` → `infoTable`).
fn local_name(raw: &[u8]) -> String {
    let s = std::str::from_utf8(raw).unwrap_or("");
    match s.rfind(':') {
        Some(idx) => s[idx + 1..].to_string(),
        None => s.to_string(),
    }
}

#[derive(Debug, Default)]
struct ScratchEntry {
    name_of_issuer: Option<String>,
    cusip: Option<String>,
    value_raw: Option<Decimal>,
}

impl ScratchEntry {
    fn finish(self) -> Result<InfoTableEntry> {
        let name_of_issuer = self
            .name_of_issuer
            .context("missing <nameOfIssuer>")?;
        let cusip = self.cusip.context("missing <cusip>")?;
        let value_raw = self.value_raw.context("missing <value>")?;
        Ok(InfoTableEntry {
            name_of_issuer,
            cusip,
            value_raw,
        })
    }
}

/// Map a 13F-HR `periodOfReport` (always a quarter-end date) to its
/// quarter's `(period_start, period_end)` window. SEC 13F filings are
/// always quarterly so the report date is always Mar/Jun/Sep/Dec end.
fn quarter_window_for(period_of_report: &str) -> Result<(String, String)> {
    let end = NaiveDate::parse_from_str(period_of_report, "%Y-%m-%d")
        .with_context(|| format!("invalid date '{period_of_report}'"))?;
    let quarter = match end.format("%m").to_string().as_str() {
        "03" => (1, 1),
        "06" => (4, 1),
        "09" => (7, 1),
        "12" => (10, 1),
        other => bail!("13F periodOfReport '{period_of_report}' month {other} not a quarter end"),
    };
    let start = NaiveDate::from_ymd_opt(end.format("%Y").to_string().parse()?, quarter.0, quarter.1)
        .context("compose quarter-start date")?;
    Ok((start.format("%Y-%m-%d").to_string(), end.format("%Y-%m-%d").to_string()))
}

/// Compute the elapsed days between `now` and the supplied RFC3339
/// timestamp. Returns `None` when the input fails to parse — callers
/// treat that as "no recent fetch" rather than panicking. Used by the
/// refresh hook's quarterly-cadence throttle.
pub fn days_since_rfc3339(rfc3339: &str) -> Option<i64> {
    let parsed = DateTime::parse_from_rfc3339(rfc3339).ok()?;
    let now = Utc::now();
    let delta = now.signed_duration_since(parsed.with_timezone(&Utc));
    Some(delta.num_days().max(0))
}

/// Compute the elapsed whole hours between `now` and the supplied
/// RFC3339 timestamp. Returns `None` when the input fails to parse.
/// Used by the refresh hook's daily-cadence throttle for the ETF.com
/// scraper.
pub fn hours_since_rfc3339(rfc3339: &str) -> Option<i64> {
    let parsed = DateTime::parse_from_rfc3339(rfc3339).ok()?;
    let now = Utc::now();
    let delta = now.signed_duration_since(parsed.with_timezone(&Utc));
    Some(delta.num_hours().max(0))
}

/// Resolve the configured provider from the environment.
///
/// Reads `PFTUI_FLOWS_PROVIDER`; defaults to `noop`. Unknown values fall
/// back to `noop` rather than erroring so a misconfigured env var never
/// breaks the refresh pipeline (the chosen provider's name is observable
/// via `FlowProvider::name`).
pub fn provider_from_env() -> Box<dyn FlowProvider> {
    let raw = std::env::var("PFTUI_FLOWS_PROVIDER").unwrap_or_default();
    provider_from_str(raw.trim())
}

/// Resolve a provider by name. Exposed for tests and CLI plumbing.
pub fn provider_from_str(name: &str) -> Box<dyn FlowProvider> {
    match name.to_ascii_lowercase().as_str() {
        "" | "noop" => Box::new(NoopProvider),
        "etf_com_csv" => Box::new(EtfComCsvProvider),
        "sec_edgar_13f" => Box::new(SecEdgar13fProvider),
        _ => Box::new(NoopProvider),
    }
}

/// Validate a flow_type string against the allowed enum values.
pub fn validate_flow_type(flow_type: &str) -> Result<()> {
    if FLOW_TYPES.contains(&flow_type) {
        Ok(())
    } else {
        bail!(
            "unknown flow_type '{}' — expected one of: {}",
            flow_type,
            FLOW_TYPES.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn noop_provider_returns_empty_flows_with_note() {
        let provider = NoopProvider;
        assert_eq!(provider.name(), "noop");
        let result = provider.fetch(None).expect("noop should never error");
        assert!(result.flows.is_empty());
        assert_eq!(result.note, "capital flows provider not configured");
    }

    #[test]
    fn etf_com_csv_provider_advertises_canonical_name() {
        // The live provider's name is part of the env-var contract and
        // must remain `etf_com_csv` even though the upstream switched
        // from CSV to HTML scraping.
        assert_eq!(EtfComCsvProvider.name(), "etf_com_csv");
    }

    #[test]
    fn parse_etf_com_flows_extracts_synthetic_rows() {
        let html = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/flows/etf_com_flows_sample.html"),
        )
        .expect("read fixture");
        let today = NaiveDate::from_ymd_opt(2026, 6, 3).expect("date");
        let flows = parse_etf_com_flows_at(&html, today).expect("parse fixture");
        // 4 well-formed ETFs (SPY, QQQ, IBIT, IWM) plus 2 malformed rows
        // dropped (blank ticker + n/a flows).
        assert_eq!(flows.len(), 4, "expected 4 well-formed rows, got {flows:?}");

        // SPY: positive 1D flow → creation, today's window.
        let spy = flows.iter().find(|f| f.asset == "SPY").expect("SPY row");
        assert_eq!(spy.flow_type, "etf_creation");
        assert_eq!(spy.amount_usd, dec!(1_234_567_890));
        assert_eq!(spy.period_start, "2026-06-03");
        assert_eq!(spy.period_end, "2026-06-03");
        assert_eq!(spy.source, "etf.com/etfanalytics");

        // QQQ: negative 1D flow → redemption with absolute amount.
        let qqq = flows.iter().find(|f| f.asset == "QQQ").expect("QQQ row");
        assert_eq!(qqq.flow_type, "etf_redemption");
        assert_eq!(qqq.amount_usd, dec!(987_654_321));

        // IBIT: small positive flow.
        let ibit = flows.iter().find(|f| f.asset == "IBIT").expect("IBIT row");
        assert_eq!(ibit.flow_type, "etf_creation");
        assert_eq!(ibit.amount_usd, dec!(45_250_000));

        // IWM: 1D blank → weekly fallback; period_start = most-recent
        // Monday before 2026-06-03 (which is a Wednesday) → 2026-06-01.
        let iwm = flows.iter().find(|f| f.asset == "IWM").expect("IWM row");
        assert_eq!(iwm.flow_type, "etf_creation");
        assert_eq!(iwm.amount_usd, dec!(310_500_000));
        assert_eq!(iwm.period_start, "2026-06-01");
        assert_eq!(iwm.period_end, "2026-06-03");
    }

    #[test]
    fn parse_etf_com_flows_bails_when_zero_rows() {
        // Page-structure-changed signal: no table with matching headers.
        let html = "<html><body><p>no flows table here</p></body></html>";
        let err = parse_etf_com_flows(html).expect_err("must bail on missing table");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no ETF.com flows table") || msg.contains("page structure"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn parse_etf_com_flows_bails_when_table_present_but_all_rows_malformed() {
        // The table exists with the right headers but every body row is
        // malformed (no parseable flow). Must bail rather than return
        // empty so the refresh DAG records Failed.
        let html = r#"
            <html><body>
              <table>
                <tr><th>Ticker</th><th>Net Flow (1D)</th></tr>
                <tr><td></td><td>$1.00</td></tr>
                <tr><td>BOGUS</td><td>n/a</td></tr>
              </table>
            </body></html>"#;
        let err = parse_etf_com_flows(html).expect_err("must bail on zero rows");
        assert!(format!("{err:#}").contains("zero rows"));
    }

    #[test]
    fn parse_flow_usd_handles_dollar_formats() {
        assert_eq!(parse_flow_usd("$1,234.56"), Some(dec!(1234.56)));
        assert_eq!(parse_flow_usd("-$1,000,000"), Some(dec!(-1000000)));
        assert_eq!(parse_flow_usd("($45,000)"), Some(dec!(-45000)));
        assert_eq!(parse_flow_usd("45.2M"), Some(dec!(45200000)));
        assert_eq!(parse_flow_usd("$1.5B"), Some(dec!(1_500_000_000)));
        assert_eq!(parse_flow_usd(""), None);
        assert_eq!(parse_flow_usd("--"), None);
        assert_eq!(parse_flow_usd("n/a"), None);
        assert_eq!(parse_flow_usd("not a number"), None);
    }

    #[test]
    fn most_recent_monday_handles_each_weekday() {
        // 2026-06-03 is a Wednesday → expect Monday 2026-06-01.
        let wed = NaiveDate::from_ymd_opt(2026, 6, 3).expect("date");
        assert_eq!(most_recent_monday(wed), NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
        // 2026-06-01 is itself Monday → unchanged.
        let mon = NaiveDate::from_ymd_opt(2026, 6, 1).expect("date");
        assert_eq!(most_recent_monday(mon), mon);
        // 2026-06-07 is Sunday → roll back to 2026-06-01.
        let sun = NaiveDate::from_ymd_opt(2026, 6, 7).expect("date");
        assert_eq!(most_recent_monday(sun), NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
    }

    #[test]
    fn provider_from_str_resolves_known_names() {
        assert_eq!(provider_from_str("noop").name(), "noop");
        assert_eq!(provider_from_str("").name(), "noop");
        assert_eq!(provider_from_str("NOOP").name(), "noop");
        assert_eq!(provider_from_str("etf_com_csv").name(), "etf_com_csv");
        assert_eq!(provider_from_str("sec_edgar_13f").name(), "sec_edgar_13f");
        // Unknown falls back to noop rather than panicking.
        assert_eq!(provider_from_str("not_a_real_provider").name(), "noop");
    }

    #[test]
    fn validate_flow_type_accepts_canonical_values() {
        for ty in FLOW_TYPES {
            validate_flow_type(ty).expect("canonical flow type should validate");
        }
        assert!(validate_flow_type("bogus").is_err());
    }

    #[test]
    fn tracked_ciks_are_well_formed() {
        // Every CIK must be exactly 10 digits and every name non-empty.
        for (cik, name) in TRACKED_CIKS {
            assert_eq!(cik.len(), 10, "CIK {cik} must be 10 digits");
            assert!(cik.chars().all(|c| c.is_ascii_digit()), "CIK {cik} must be digits");
            assert!(!name.is_empty(), "filer name must not be empty");
        }
        assert!(TRACKED_CIKS.len() >= 4);
    }

    #[test]
    fn amount_usd_from_value_promotes_thousands_for_small_readings() {
        // Pre-2023 13F: <value> reported in thousands → multiply by 1000.
        let thousands = dec!(150_000);
        assert_eq!(amount_usd_from_value(thousands), dec!(150_000_000));
    }

    #[test]
    fn amount_usd_from_value_passes_through_whole_dollars() {
        // Post-2023 13F: <value> reported in whole dollars → pass through.
        let whole = dec!(1_500_000_000);
        assert_eq!(amount_usd_from_value(whole), dec!(1_500_000_000));
    }

    #[test]
    fn parse_infotable_xml_extracts_synthetic_holdings() {
        let xml = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/flows/edgar_13f_sample.xml"),
        )
        .expect("read fixture");
        let flows = parse_infotable_xml(
            &xml,
            "sec_edgar_13f:Synthetic Filer (0000000000-00-000000)",
            "2026-01-01",
            "2026-03-31",
        )
        .expect("parse fixture");
        assert_eq!(flows.len(), 3);
        // Asset key = CUSIP, flow_type = institutional_13f.
        assert_eq!(flows[0].asset, "037833100");
        assert_eq!(flows[0].flow_type, "institutional_13f");
        // First row reported value=150000 (thousands) → $150_000_000.
        assert_eq!(flows[0].amount_usd, dec!(150_000_000));
        assert_eq!(flows[0].period_start, "2026-01-01");
        assert_eq!(flows[0].period_end, "2026-03-31");
        assert!(flows[0].source.starts_with("sec_edgar_13f:"));
        // Second row's CUSIP comes through verbatim.
        assert_eq!(flows[1].asset, "594918104");
        assert_eq!(flows[1].amount_usd, dec!(75_500_000));
        // Third row used the whole-dollar regime (value >= 1B).
        assert_eq!(flows[2].asset, "67066G104");
        assert_eq!(flows[2].amount_usd, dec!(2_400_000_000));
    }

    #[test]
    fn parse_infotable_entries_strips_namespace_prefixes() {
        // SEC 13F-HR XML is frequently emitted with namespace prefixes
        // (`ns1:infoTable`, `n1:infoTable`, etc.). The parser must
        // resolve these to local names so the substantive tags match.
        let xml = br#"<?xml version="1.0"?>
            <ns1:informationTable xmlns:ns1="http://www.sec.gov/edgar/document/thirteenf/informationtable">
              <ns1:infoTable>
                <ns1:nameOfIssuer>APPLE INC</ns1:nameOfIssuer>
                <ns1:cusip>037833100</ns1:cusip>
                <ns1:value>1000</ns1:value>
              </ns1:infoTable>
            </ns1:informationTable>"#;
        let entries = parse_infotable_entries(xml).expect("parse namespaced XML");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cusip, "037833100");
        assert_eq!(entries[0].value_raw, dec!(1000));
    }

    #[test]
    fn parse_infotable_entries_rejects_missing_required_fields() {
        let xml = br#"<?xml version="1.0"?>
            <informationTable>
              <infoTable>
                <nameOfIssuer>SOMETHING</nameOfIssuer>
                <!-- no cusip, no value -->
              </infoTable>
            </informationTable>"#;
        let err = parse_infotable_entries(xml).expect_err("must reject incomplete row");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("cusip") || msg.contains("value") || msg.contains("incomplete"),
            "error message did not mention missing field: {msg}"
        );
    }

    #[test]
    fn quarter_window_for_maps_period_of_report_to_quarter() {
        assert_eq!(
            quarter_window_for("2026-03-31").expect("Q1"),
            ("2026-01-01".to_string(), "2026-03-31".to_string())
        );
        assert_eq!(
            quarter_window_for("2026-06-30").expect("Q2"),
            ("2026-04-01".to_string(), "2026-06-30".to_string())
        );
        assert_eq!(
            quarter_window_for("2025-09-30").expect("Q3"),
            ("2025-07-01".to_string(), "2025-09-30".to_string())
        );
        assert_eq!(
            quarter_window_for("2024-12-31").expect("Q4"),
            ("2024-10-01".to_string(), "2024-12-31".to_string())
        );
        assert!(quarter_window_for("2026-05-15").is_err());
        assert!(quarter_window_for("not-a-date").is_err());
    }

    #[test]
    fn pick_latest_13fhr_takes_first_matching_form() {
        let recent = SubmissionsRecent {
            accession_number: vec![
                "0000000000-26-000001".to_string(),
                "0000000000-26-000002".to_string(),
                "0000000000-25-000010".to_string(),
            ],
            form: vec![
                "10-K".to_string(),
                "13F-HR".to_string(),
                "13F-HR".to_string(),
            ],
            report_date: vec![
                "2025-12-31".to_string(),
                "2026-03-31".to_string(),
                "2025-12-31".to_string(),
            ],
        };
        let pick = pick_latest_13fhr(&recent).expect("first 13F-HR");
        assert_eq!(pick.0, "0000000000-26-000002");
        assert_eq!(pick.1, "2026-03-31");
    }

    #[test]
    fn pick_latest_13fhr_returns_none_when_no_match() {
        let recent = SubmissionsRecent {
            accession_number: vec!["x".to_string()],
            form: vec!["10-K".to_string()],
            report_date: vec!["2026-03-31".to_string()],
        };
        assert!(pick_latest_13fhr(&recent).is_none());
    }

    #[test]
    fn pick_infotable_xml_name_prefers_named_infotable() {
        let items = vec![
            FilingIndexItem { name: "primary_doc.xml".to_string() },
            FilingIndexItem { name: "form13fInfoTable.xml".to_string() },
            FilingIndexItem { name: "filing-summary.xsl".to_string() },
        ];
        assert_eq!(
            pick_infotable_xml_name(&items).expect("infotable"),
            "form13fInfoTable.xml"
        );
    }

    #[test]
    fn pick_infotable_xml_name_falls_back_to_other_xml() {
        let items = vec![
            FilingIndexItem { name: "primary_doc.xml".to_string() },
            FilingIndexItem { name: "holdings.xml".to_string() },
        ];
        assert_eq!(
            pick_infotable_xml_name(&items).expect("fallback xml"),
            "holdings.xml"
        );
    }

    #[test]
    fn days_since_rfc3339_handles_recent_and_old_timestamps() {
        // 1d ago should report >= 0 and < 5.
        let one_day_ago = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        let d = days_since_rfc3339(&one_day_ago).expect("parse 1d");
        assert!((0..5).contains(&d), "expected 0..5 days, got {d}");

        // 200d ago should be well past the 80-day quarterly threshold.
        let long_ago = (Utc::now() - chrono::Duration::days(200)).to_rfc3339();
        let d2 = days_since_rfc3339(&long_ago).expect("parse 200d");
        assert!(d2 >= 199, "expected ~200 days, got {d2}");

        // Garbage parses to None rather than panicking.
        assert!(days_since_rfc3339("not-an-rfc3339-ts").is_none());
    }

    #[test]
    fn pick_infotable_xml_name_returns_none_when_only_metadata_present() {
        let items = vec![
            FilingIndexItem { name: "primary_doc.xml".to_string() },
            FilingIndexItem { name: "Financial_Report.xlsx".to_string() },
        ];
        assert!(pick_infotable_xml_name(&items).is_none());
    }
}
