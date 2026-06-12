//! Thesis evidence-contract verifier (`pftui research verify-thesis`).
//!
//! Curated thesis sections (L4) carry numeric claims in an evidence format
//! DESIGNED for re-checking:
//!
//! - `[pftui]` — computed from a pftui table; verification SQL shown either
//!   as a fenced ```sql block labeled "Verification SQL" (expected values in
//!   a `-- →` comment) or as an inline backticked `SELECT ...`.
//! - `[derived]` — computed from a combination of `[pftui]` values; re-checked
//!   when the derivation is mechanically stated (the `X% ($A vs $B)` form).
//! - `[ext: N]` — externally cited; only reference PRESENCE is checked
//!   (a `[ext:N]` line with an http URL must exist in the section).
//!
//! This module re-extracts every tagged claim, re-runs the embedded SQL
//! read-only against the live DB, and classifies:
//!
//! | status        | meaning |
//! |---|---|
//! | `verified`    | re-run output matches the claimed value(s) within tolerance (±2% numeric, exact dates) |
//! | `drift`       | re-run output near the claimed value but outside tolerance — claimed vs current shown |
//! | `broken`      | the SQL errored (schema drift, repaired series) or an `[ext]` reference is missing |
//! | `unverifiable`| tagged claim with no runnable SQL / no mechanically-stated derivation |
//! | `ext-present` | `[ext]` citation whose reference + URL exist |
//! | `untagged`    | numeric claim in an evidence-contract section with NO tag — the contract-violation class |
//!
//! Drift is not always an error: claims are classified SNAPSHOT (dated,
//! "current/live/as-of" framing — aging is expected, drift reports staleness
//! at severity info) vs STRUCTURAL (cycle peak dates/values, anchors — drift
//! means an error or a data change, severity suspect).
//!
//! Read-only by design: only single-statement `SELECT`s are executed.
//! Repair of wrong STRUCTURAL values stays a curated operation on the L4
//! row (`analytics thesis set` / reviewed SQL UPDATE + journal note).

use std::collections::HashSet;
use std::sync::OnceLock;

use anyhow::Result;
use regex::Regex;
use rusqlite::Connection;
use serde::Serialize;

/// Evidence-tag class of an extracted claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimKind {
    /// `[pftui]` — recomputable from a pftui table.
    Pftui,
    /// `[derived]` — combination of `[pftui]` values.
    Derived,
    /// `[ext: ...]` — externally cited.
    External,
    /// Numeric claim with no tag in a section that adopted the contract.
    Untagged,
}

/// Aging class: does drift mean "stale" or "wrong"?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimClass {
    /// Dated/current/live reading — drift is expected aging (severity info).
    Snapshot,
    /// Cycle anchor, historical peak/bottom, definition threshold — drift is
    /// an error or a data change (severity suspect).
    Structural,
}

/// Verification outcome for one claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimStatus {
    Verified,
    Drift,
    Broken,
    Unverifiable,
    ExtPresent,
    Untagged,
}

impl ClaimStatus {
    pub fn label(self) -> &'static str {
        match self {
            ClaimStatus::Verified => "verified",
            ClaimStatus::Drift => "drift",
            ClaimStatus::Broken => "broken",
            ClaimStatus::Unverifiable => "unverifiable",
            ClaimStatus::ExtPresent => "ext-present",
            ClaimStatus::Untagged => "untagged",
        }
    }
}

/// One claim extracted from thesis content.
#[derive(Debug, Clone, Serialize)]
pub struct Claim {
    /// 1-based line number in the section content.
    pub line: usize,
    /// Trimmed source text (truncated for display).
    pub text: String,
    pub kind: ClaimKind,
    pub class: ClaimClass,
    /// `as of YYYY-MM-DD` date found on the line or its governing heading.
    pub as_of: Option<String>,
    /// Verification SQL (fenced block or inline backticked SELECT).
    pub sql: Option<String>,
    /// Claimed numeric values parsed from the claim text.
    pub nums: Vec<f64>,
    /// Claimed ISO dates parsed from the claim text.
    pub dates: Vec<String>,
    /// `[ext:N]` reference ids cited on the line.
    pub ext_refs: Vec<u32>,
}

/// A claim plus its verification outcome.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    #[serde(flatten)]
    pub claim: Claim,
    pub status: ClaimStatus,
    /// `ok` | `info` | `suspect`
    pub severity: &'static str,
    pub detail: String,
}

/// Per-section verification report.
#[derive(Debug, Serialize)]
pub struct SectionReport {
    pub section: String,
    pub verified: usize,
    pub drift: usize,
    pub broken: usize,
    pub unverifiable: usize,
    pub ext_checked: usize,
    pub untagged: usize,
    pub findings: Vec<Finding>,
}

impl SectionReport {
    pub fn total_claims(&self) -> usize {
        self.findings.len()
    }
}

// ---------------------------------------------------------------------------
// Regexes (lazily compiled once)
// ---------------------------------------------------------------------------

fn re(slot: &'static OnceLock<Regex>, pattern: &'static str) -> &'static Regex {
    slot.get_or_init(|| Regex::new(pattern).expect("static regex pattern must compile"))
}

fn iso_date_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"\d{4}-\d{2}-\d{2}")
}

fn ext_ref_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"\[ext:\s*(\d+)\]")
}

fn ext_def_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"^\[ext:\s*(\d+)\].*https?://\S")
}

fn tag_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"\[(pftui|derived|ext:)[^\]]*\]")
}

fn inline_code_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"`[^`]*`")
}

fn url_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"https?://\S+")
}

fn num_token_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    // $-prefixed, comma-grouped, decimal, with optional k/%/× suffix.
    re(
        &SLOT,
        r"(?P<cur>\$)?(?P<sign>-)?(?P<body>\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:\.\d+)?)(?P<suf>[kK%×x])?",
    )
}

fn as_of_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"(?i)as of (\d{4}-\d{2}-\d{2})")
}

fn derived_pct_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    // "+1.1% ($62,447 vs $61,744)" — the mechanically restatable derivation.
    re(
        &SLOT,
        r"(?P<pct>[-+]?\d+(?:\.\d+)?)%\s*\(\$?(?P<a>\d[\d,]*(?:\.\d+)?)\s+vs\s+\$?(?P<b>\d[\d,]*(?:\.\d+)?)\)",
    )
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

const SNAPSHOT_KEYWORDS: [&str; 7] = [
    "as of", "current", "live", "today", "to-date", "(open)", "this week",
];

fn is_snapshot(line: &str, heading: &str) -> bool {
    let l = line.to_lowercase();
    let h = heading.to_lowercase();
    SNAPSHOT_KEYWORDS
        .iter()
        .any(|k| l.contains(k) || h.contains(k))
}

fn find_as_of(line: &str, heading: &str) -> Option<String> {
    as_of_re()
        .captures(line)
        .or_else(|| as_of_re().captures(heading))
        .map(|c| c[1].to_string())
}

/// Parse one numeric token to f64 (handles `$`, commas, `k` suffix; the
/// `%`/`×` suffixes keep the raw magnitude).
fn parse_num(caps: &regex::Captures) -> Option<f64> {
    let body = caps["body"].replace(',', "");
    let mut v: f64 = body.parse().ok()?;
    if caps.name("sign").is_some() {
        v = -v;
    }
    let suf = caps.name("suf").map(|m| m.as_str()).unwrap_or("");
    if suf.eq_ignore_ascii_case("k") {
        v *= 1000.0;
    }
    // Plain 4-digit integers without $ / suffix are almost always years in
    // prose ("the 2017 top") — skip them; dates are handled separately.
    if caps.name("cur").is_none() && suf.is_empty() && v.fract() == 0.0 && (1900.0..2100.0).contains(&v)
    {
        return None;
    }
    Some(v)
}

fn year_month_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    re(&SLOT, r"\b\d{4}-\d{2}\b")
}

fn range_dash_re() -> &'static Regex {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    // "40k-53k" / "$60-65k": the dash is a range, not a minus sign.
    re(&SLOT, r"(?P<l>[\dkK%])-(?P<r>\d)")
}

/// Extract claimed numeric values + ISO dates from claim text, after
/// stripping tags, inline code, and URLs (their numbers are not claims).
fn extract_values(line: &str) -> (Vec<f64>, Vec<String>) {
    let stripped = tag_re().replace_all(line, " ");
    let stripped = inline_code_re().replace_all(&stripped, " ");
    let stripped = url_re().replace_all(&stripped, " ");

    let mut dates: Vec<String> = Vec::new();
    for m in iso_date_re().find_iter(&stripped) {
        dates.push(m.as_str().to_string());
    }
    // Blank out dates (and bare year-month tokens like "2026-03") so their
    // components aren't re-parsed as negative numbers, then split range
    // dashes ("40k-53k") so they aren't read as minus signs.
    let no_dates = iso_date_re().replace_all(&stripped, " ");
    let no_dates = year_month_re().replace_all(&no_dates, " ");
    let no_dates = range_dash_re().replace_all(&no_dates, "$l $r");

    let mut nums: Vec<f64> = Vec::new();
    for caps in num_token_re().captures_iter(&no_dates) {
        if let Some(v) = parse_num(&caps) {
            nums.push(v);
        }
    }
    (nums, dates)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}

fn line_ext_refs(line: &str) -> Vec<u32> {
    ext_ref_re()
        .captures_iter(line)
        .filter_map(|c| c[1].parse().ok())
        .collect()
}

/// Parse `[ext:N] ... http...` reference definitions out of the content.
pub fn defined_references(content: &str) -> HashSet<u32> {
    content
        .lines()
        .filter_map(|l| ext_def_re().captures(l.trim()))
        .filter_map(|c| c[1].parse().ok())
        .collect()
}

/// Does this section carry the evidence contract at all?
pub fn has_evidence_tags(content: &str) -> bool {
    tag_re().is_match(content)
}

/// Is this line a data-bearing numeric claim (for the untagged pass)?
/// Requires a `$`-number or a percent figure — bare integers in prose are
/// not flagged.
fn has_data_number(line: &str) -> bool {
    static SLOT: OnceLock<Regex> = OnceLock::new();
    let r = re(&SLOT, r"\$\d|\d(?:\.\d+)?%");
    let stripped = inline_code_re().replace_all(line, " ");
    let stripped = url_re().replace_all(&stripped, " ");
    r.is_match(&stripped)
}

/// Extract every claim from one section's markdown content.
pub fn extract_claims(content: &str) -> Vec<Claim> {
    let contract = has_evidence_tags(content);
    let mut claims: Vec<Claim> = Vec::new();

    let mut heading = String::new();
    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut fence_buf: Vec<String> = Vec::new();
    let mut fence_start = 0usize;
    let mut fence_label = String::new();
    let mut last_nonempty = String::new();

    for (idx, raw) in content.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();

        if line.starts_with("```") {
            if in_fence {
                // Closing fence.
                if fence_lang == "sql" {
                    if let Some(claim) =
                        fenced_sql_claim(&fence_buf, fence_start, &fence_label, &heading)
                    {
                        claims.push(claim);
                    }
                }
                in_fence = false;
                fence_buf.clear();
            } else {
                in_fence = true;
                fence_lang = line.trim_start_matches('`').trim().to_lowercase();
                fence_start = line_no;
                fence_label = last_nonempty.clone();
            }
            continue;
        }
        if in_fence {
            fence_buf.push(raw.to_string());
            continue;
        }
        if !line.is_empty() {
            last_nonempty = line.to_string();
        }
        if line.starts_with('#') {
            heading = line.to_string();
            continue;
        }
        // Contract prose / table scaffolding — never claims.
        if line.is_empty() || line.starts_with('>') {
            continue;
        }
        if line.starts_with('|') && line.replace(['|', '-', ':', ' '], "").is_empty() {
            continue;
        }
        // Reference definitions are checked, not extracted as claims.
        if ext_def_re().is_match(line) {
            continue;
        }

        let has_pftui = line.contains("[pftui");
        let has_derived = line.contains("[derived");
        let ext_refs = line_ext_refs(line);

        if has_pftui || has_derived || !ext_refs.is_empty() {
            let (nums, dates) = extract_values(line);
            let sql = inline_sql(line);
            let kind = if has_pftui {
                ClaimKind::Pftui
            } else if has_derived {
                ClaimKind::Derived
            } else {
                ClaimKind::External
            };
            let class = if is_snapshot(line, &heading) {
                ClaimClass::Snapshot
            } else {
                ClaimClass::Structural
            };
            claims.push(Claim {
                line: line_no,
                text: truncate(line, 160),
                kind,
                class,
                as_of: find_as_of(line, &heading),
                sql,
                nums,
                dates,
                ext_refs,
            });
        } else if contract && has_data_number(line) && !line.contains('❌') {
            // Contract violation class: a number presented as data, no tag.
            // (❌-prefixed lines are FORBIDDEN-claim examples — negated, not
            // asserted — so they are exempt from the untagged pass.)
            let (nums, dates) = extract_values(line);
            let class = if is_snapshot(line, &heading) {
                ClaimClass::Snapshot
            } else {
                ClaimClass::Structural
            };
            claims.push(Claim {
                line: line_no,
                text: truncate(line, 160),
                kind: ClaimKind::Untagged,
                class,
                as_of: find_as_of(line, &heading),
                sql: None,
                nums,
                dates,
                ext_refs: Vec::new(),
            });
        }
    }
    claims
}

/// First inline backticked span that looks like a SELECT.
fn inline_sql(line: &str) -> Option<String> {
    for m in inline_code_re().find_iter(line) {
        let inner = m.as_str().trim_matches('`').trim();
        if inner.to_uppercase().starts_with("SELECT") {
            return Some(inner.to_string());
        }
    }
    None
}

/// Build a claim from a fenced ```sql block: SQL = non-comment lines,
/// expected values = the `-- →` / `-- ->` result comment.
fn fenced_sql_claim(
    buf: &[String],
    start_line: usize,
    label: &str,
    heading: &str,
) -> Option<Claim> {
    let sql: String = buf
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ");
    let sql = sql.trim().to_string();
    if sql.is_empty() {
        return None;
    }
    let expected_text: String = buf
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("--"))
        .filter(|l| l.contains('→') || l.contains("->"))
        .collect::<Vec<_>>()
        .join(" ");
    let (nums, dates) = extract_values_raw(&expected_text);

    let text = if label.is_empty() {
        format!("fenced verification SQL @ line {start_line}")
    } else {
        truncate(label.trim_matches(['*', ':', ' ']), 160)
    };
    let class = if is_snapshot(label, heading) {
        ClaimClass::Snapshot
    } else {
        ClaimClass::Structural
    };
    Some(Claim {
        line: start_line,
        text,
        kind: ClaimKind::Pftui,
        class,
        as_of: find_as_of(label, heading),
        sql: Some(sql),
        nums,
        dates,
        ext_refs: Vec::new(),
    })
}

/// Value extraction without code-span stripping (used on `-- →` comments,
/// where everything after the arrow IS the expected value).
fn extract_values_raw(text: &str) -> (Vec<f64>, Vec<String>) {
    let mut dates: Vec<String> = Vec::new();
    for m in iso_date_re().find_iter(text) {
        dates.push(m.as_str().to_string());
    }
    let no_dates = iso_date_re().replace_all(text, " ");
    let mut nums: Vec<f64> = Vec::new();
    for caps in num_token_re().captures_iter(&no_dates) {
        let body = caps["body"].replace(',', "");
        if let Ok(mut v) = body.parse::<f64>() {
            if caps.name("sign").is_some() {
                v = -v;
            }
            if caps
                .name("suf")
                .is_some_and(|s| s.as_str().eq_ignore_ascii_case("k"))
            {
                v *= 1000.0;
            }
            nums.push(v);
        }
    }
    (nums, dates)
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Relative tolerance for a numeric match.
const NUM_TOLERANCE: f64 = 0.02;
/// Same-order-of-magnitude band that turns a non-match into reported drift
/// (claimed vs current) instead of "not comparable".
const DRIFT_RATIO_LO: f64 = 0.2;
const DRIFT_RATIO_HI: f64 = 5.0;

struct Cell {
    text: String,
    num: Option<f64>,
}

/// Run a single read-only SELECT, returning up to 16 rows of cells.
fn run_select(conn: &Connection, sql: &str) -> Result<Vec<Vec<Cell>>> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if !trimmed.to_uppercase().starts_with("SELECT") || trimmed.contains(';') {
        anyhow::bail!("refusing non-SELECT / multi-statement verification SQL");
    }
    let mut stmt = conn.prepare(trimmed)?;
    let ncols = stmt.column_count();
    let mut rows = stmt.query([])?;
    let mut out: Vec<Vec<Cell>> = Vec::new();
    while let Some(row) = rows.next()? {
        let mut cells = Vec::with_capacity(ncols);
        for i in 0..ncols {
            let v = row.get_ref(i)?;
            let (text, num) = match v {
                rusqlite::types::ValueRef::Null => (String::from("NULL"), None),
                rusqlite::types::ValueRef::Integer(n) => (n.to_string(), Some(n as f64)),
                rusqlite::types::ValueRef::Real(f) => (format!("{f}"), Some(f)),
                rusqlite::types::ValueRef::Text(t) => {
                    let s = String::from_utf8_lossy(t).to_string();
                    let num = s.trim().parse::<f64>().ok();
                    (s, num)
                }
                rusqlite::types::ValueRef::Blob(_) => (String::from("<blob>"), None),
            };
            cells.push(Cell { text, num });
        }
        out.push(cells);
        if out.len() >= 16 {
            break;
        }
    }
    Ok(out)
}

fn rel_diff(claimed: f64, current: f64) -> f64 {
    (current - claimed).abs() / claimed.abs().max(1e-9)
}

/// Compare claimed values against query output.
fn compare(claim: &Claim, rows: &[Vec<Cell>]) -> (ClaimStatus, String) {
    let cells: Vec<&Cell> = rows.iter().flatten().collect();
    if cells.is_empty() {
        return (
            ClaimStatus::Drift,
            "query returned no rows (claimed values no longer present)".to_string(),
        );
    }
    let mut matched = 0usize;
    let mut drifts: Vec<String> = Vec::new();

    for &v in &claim.nums {
        let mut best: Option<(f64, f64)> = None; // (rel, current)
        for c in &cells {
            if let Some(r) = c.num {
                let rel = rel_diff(v, r);
                if best.is_none_or(|(b, _)| rel < b) {
                    best = Some((rel, r));
                }
            }
        }
        if let Some((rel, current)) = best {
            if rel <= NUM_TOLERANCE {
                matched += 1;
            } else {
                let ratio = if v.abs() > 1e-9 { current / v } else { f64::MAX };
                if (DRIFT_RATIO_LO..=DRIFT_RATIO_HI).contains(&ratio.abs()) {
                    drifts.push(format!("claimed {v} vs current {current:.4}"));
                }
                // else: not comparable to this claim value — ignore.
            }
        }
    }
    for d in &claim.dates {
        let present = cells.iter().any(|c| c.text.contains(d.as_str()));
        if present {
            matched += 1;
        } else if let Some(other) = cells
            .iter()
            .find(|c| iso_date_re().is_match(&c.text))
            .map(|c| c.text.clone())
        {
            drifts.push(format!("claimed date {d} vs current {other}"));
        }
    }

    if !drifts.is_empty() {
        drifts.truncate(3);
        (ClaimStatus::Drift, drifts.join("; "))
    } else if matched > 0 {
        (
            ClaimStatus::Verified,
            format!("{matched} claimed value(s) matched query output"),
        )
    } else {
        (
            ClaimStatus::Unverifiable,
            "query ran but no claimed value is comparable to its output".to_string(),
        )
    }
}

fn severity(status: ClaimStatus, class: ClaimClass) -> &'static str {
    match (status, class) {
        (ClaimStatus::Verified | ClaimStatus::ExtPresent, _) => "ok",
        (ClaimStatus::Drift, ClaimClass::Snapshot) => "info",
        (ClaimStatus::Drift, ClaimClass::Structural) => "suspect",
        (ClaimStatus::Broken, _) => "suspect",
        (ClaimStatus::Unverifiable | ClaimStatus::Untagged, _) => "info",
    }
}

fn staleness_note(claim: &Claim, today: &str) -> String {
    let Some(asof) = &claim.as_of else {
        return String::new();
    };
    let (Ok(a), Ok(t)) = (
        chrono::NaiveDate::parse_from_str(asof, "%Y-%m-%d"),
        chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d"),
    ) else {
        return String::new();
    };
    let days = (t - a).num_days();
    format!(" [snapshot dated {asof}, {days}d old]")
}

/// Verify one claim against the DB.
fn verify_claim(conn: &Connection, claim: &Claim, refs: &HashSet<u32>, today: &str) -> Finding {
    let (status, mut detail) = match claim.kind {
        ClaimKind::Pftui => match &claim.sql {
            Some(sql) => match run_select(conn, sql) {
                Ok(rows) => compare(claim, &rows),
                Err(e) => (ClaimStatus::Broken, format!("SQL error: {e}")),
            },
            None => (
                ClaimStatus::Unverifiable,
                "[pftui] claim carries no runnable SQL (contract: SQL shown)".to_string(),
            ),
        },
        ClaimKind::Derived => verify_derived(claim),
        ClaimKind::External => {
            let missing: Vec<u32> = claim
                .ext_refs
                .iter()
                .copied()
                .filter(|n| !refs.contains(n))
                .collect();
            if missing.is_empty() {
                (
                    ClaimStatus::ExtPresent,
                    "cited reference(s) present with URL".to_string(),
                )
            } else {
                (
                    ClaimStatus::Broken,
                    format!(
                        "missing reference definition(s): {}",
                        missing
                            .iter()
                            .map(|n| format!("[ext:{n}]"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                )
            }
        }
        ClaimKind::Untagged => (
            ClaimStatus::Untagged,
            "numeric claim without an evidence tag in a contract section".to_string(),
        ),
    };

    // [pftui] claims that also cite [ext:N]: fold a missing-ref note in.
    if claim.kind == ClaimKind::Pftui {
        let missing: Vec<u32> = claim
            .ext_refs
            .iter()
            .copied()
            .filter(|n| !refs.contains(n))
            .collect();
        if !missing.is_empty() {
            detail.push_str(&format!(
                "; missing [ext:{}] reference",
                missing
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
    }

    if status == ClaimStatus::Drift && claim.class == ClaimClass::Snapshot {
        detail.push_str(&staleness_note(claim, today));
    }

    Finding {
        severity: severity(status, claim.class),
        status,
        detail,
        claim: claim.clone(),
    }
}

/// Re-verify a `[derived]` claim when the derivation is mechanically stated
/// as `X% ($A vs $B)` (X = A/B − 1).
fn verify_derived(claim: &Claim) -> (ClaimStatus, String) {
    let Some(caps) = derived_pct_re().captures(&claim.text) else {
        return (
            ClaimStatus::Unverifiable,
            "derivation not mechanically stated (no `X% ($A vs $B)` form)".to_string(),
        );
    };
    let parse = |s: &str| s.replace(',', "").parse::<f64>().ok();
    let (Some(pct), Some(a), Some(b)) = (parse(&caps["pct"]), parse(&caps["a"]), parse(&caps["b"]))
    else {
        return (
            ClaimStatus::Unverifiable,
            "derivation values failed to parse".to_string(),
        );
    };
    if b.abs() < 1e-9 {
        return (ClaimStatus::Broken, "derivation divides by zero".to_string());
    }
    let computed = (a / b - 1.0) * 100.0;
    if (computed - pct).abs() <= f64::max(0.1, pct.abs() * NUM_TOLERANCE) {
        (
            ClaimStatus::Verified,
            format!("derivation recomputed: {computed:.2}% ≈ claimed {pct}%"),
        )
    } else {
        (
            ClaimStatus::Drift,
            format!("derivation recomputes to {computed:.2}%, claimed {pct}%"),
        )
    }
}

/// Verify one section's content against the live DB.
pub fn verify_section(
    conn: &Connection,
    section: &str,
    content: &str,
    today: &str,
) -> SectionReport {
    let refs = defined_references(content);
    let claims = extract_claims(content);
    let mut findings: Vec<Finding> = Vec::with_capacity(claims.len());
    for claim in &claims {
        findings.push(verify_claim(conn, claim, &refs, today));
    }
    let count = |s: ClaimStatus| findings.iter().filter(|f| f.status == s).count();
    SectionReport {
        section: section.to_string(),
        verified: count(ClaimStatus::Verified),
        drift: count(ClaimStatus::Drift),
        broken: count(ClaimStatus::Broken),
        unverifiable: count(ClaimStatus::Unverifiable),
        ext_checked: count(ClaimStatus::ExtPresent),
        untagged: count(ClaimStatus::Untagged),
        findings,
    }
}

/// One-line summary for `pftui system doctor`. Findings exist (broken /
/// structural drift / untagged contract violations) → failed (non-critical).
pub fn doctor_summary(conn: &Connection) -> Result<(bool, String)> {
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='thesis'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    if !table_exists {
        return Ok((true, "Thesis evidence: no thesis table — nothing to verify".into()));
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare("SELECT section, content FROM thesis ORDER BY section")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| {
            let section: String = r.get(0)?;
            // Tolerate BLOB-affinity content (rows written via readfile()).
            let content = match r.get_ref(1)? {
                rusqlite::types::ValueRef::Text(b) | rusqlite::types::ValueRef::Blob(b) => {
                    String::from_utf8_lossy(b).into_owned()
                }
                _ => String::new(),
            };
            Ok((section, content))
        })?
        .collect::<Result<_, _>>()?;

    let mut verified = 0usize;
    let mut broken = 0usize;
    let mut structural_drift = 0usize;
    let mut untagged = 0usize;
    let mut sections_with_claims = 0usize;
    for (section, content) in &rows {
        let report = verify_section(conn, section, content, &today);
        if report.total_claims() > 0 {
            sections_with_claims += 1;
        }
        verified += report.verified + report.ext_checked;
        broken += report.broken;
        untagged += report.untagged;
        structural_drift += report
            .findings
            .iter()
            .filter(|f| f.status == ClaimStatus::Drift && f.claim.class == ClaimClass::Structural)
            .count();
    }
    if broken + structural_drift + untagged == 0 {
        Ok((
            true,
            format!(
                "Thesis evidence: {verified} claims verified across {sections_with_claims} contract sections, none broken"
            ),
        ))
    } else {
        Ok((
            false,
            format!(
                "Thesis evidence: {broken} broken, {structural_drift} structural-drift, {untagged} untagged claims — pftui research verify-thesis"
            ),
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch(
            "CREATE TABLE price_history (symbol TEXT, date TEXT, close TEXT);
             INSERT INTO price_history VALUES
               ('BTC-USD','2025-10-06','124752.53'),
               ('BTC-USD','2026-06-05','77414.00'),
               ('BTC-USD','2026-06-11','62447.00');",
        )
        .expect("seed");
        conn
    }

    const FIXTURE: &str = r#"# Synthetic cycle framework

> Every numeric claim is labeled [pftui] / [ext: URL] / [derived].

## 1. Cycle history

| Cycle | Peak Date | Peak Close |
|---|---|---:|
| 4 | 2025-10-06 | $124,752 [pftui] |

Wrong anchor: peak close was **$99,999** [pftui] `SELECT CAST(close AS REAL) FROM price_history ORDER BY CAST(close AS REAL) DESC LIMIT 1`

**Verification SQL (Cycle 4 peak):**
```sql
SELECT date, CAST(close AS REAL) FROM price_history
 WHERE symbol='BTC-USD'
 ORDER BY CAST(close AS REAL) DESC LIMIT 1;
-- → 2025-10-06 | 124752.53
```

## 2. Current status (as of 2026-06-05)

| Metric | Value | Evidence |
|---|---:|---|
| Snapshot close | **$70,000 current** | [pftui] `SELECT CAST(close AS REAL) FROM price_history WHERE symbol='BTC-USD' ORDER BY date DESC LIMIT 1` |
| Broken metric | **$50,000** | [pftui] `SELECT close FROM missing_table` |
| Live vs MA | **+1.1% ($62,447 vs $61,744)** | [derived] |
| Bad derived | **+9.9% ($62,447 vs $61,744)** | [derived] |
| Vague derived | **roughly double** | [derived] |
| No-SQL metric | **$42,000** | [pftui] |

External fact: drawdown was -83% in cycle 2 [ext:1]. Missing cite [ext:9].

The untagged figure $13,337 sits here with no tag at all.

## References

[ext:1] Example source — https://example.com/cycles
"#;

    #[test]
    fn extracts_fenced_and_inline_claims() {
        let claims = extract_claims(FIXTURE);
        // Fenced block claim present, with SQL and expected values.
        let fenced = claims
            .iter()
            .find(|c| c.text.contains("Verification SQL"))
            .expect("fenced claim");
        assert_eq!(fenced.kind, ClaimKind::Pftui);
        assert!(fenced.sql.as_deref().unwrap().starts_with("SELECT"));
        assert!(fenced.dates.contains(&"2025-10-06".to_string()));
        assert!(fenced.nums.iter().any(|v| (v - 124752.53).abs() < 0.01));
        assert_eq!(fenced.class, ClaimClass::Structural);

        // Inline-SQL table row.
        let snap = claims
            .iter()
            .find(|c| c.text.contains("Snapshot close"))
            .expect("snapshot claim");
        assert!(snap.sql.is_some());
        assert_eq!(snap.class, ClaimClass::Snapshot);
        assert_eq!(snap.as_of.as_deref(), Some("2026-06-05"));
        assert!(snap.nums.contains(&70000.0));

        // Untagged contract violation captured.
        let untagged = claims
            .iter()
            .find(|c| c.kind == ClaimKind::Untagged)
            .expect("untagged claim");
        assert!(untagged.text.contains("13,337"));

        // The blockquote contract header and table separators are not claims.
        assert!(claims.iter().all(|c| !c.text.starts_with('>')));
    }

    #[test]
    fn classifies_verified_drift_broken_and_derived() {
        let conn = test_conn();
        let report = verify_section(&conn, "synthetic", FIXTURE, "2026-06-12");

        let by_text = |needle: &str| {
            report
                .findings
                .iter()
                .find(|f| f.claim.text.contains(needle))
                .unwrap_or_else(|| panic!("finding {needle}"))
        };

        // Fenced anchor still matches → verified.
        assert_eq!(by_text("Verification SQL").status, ClaimStatus::Verified);

        // Snapshot claim drifted (claimed 70k, current 62,447) → drift, info.
        let snap = by_text("Snapshot close");
        assert_eq!(snap.status, ClaimStatus::Drift);
        assert_eq!(snap.severity, "info");
        assert!(snap.detail.contains("snapshot dated 2026-06-05"));

        // Missing table → broken, suspect.
        let broken = by_text("Broken metric");
        assert_eq!(broken.status, ClaimStatus::Broken);
        assert_eq!(broken.severity, "suspect");

        // Structural value off by >2% within magnitude band → drift, suspect.
        let anchor = by_text("Wrong anchor");
        assert_eq!(anchor.status, ClaimStatus::Drift);
        assert_eq!(anchor.severity, "suspect");
        assert!(anchor.detail.contains("99999"));

        // Derived: good recompute verified, bad recompute drift, vague unverifiable.
        assert_eq!(by_text("Live vs MA").status, ClaimStatus::Verified);
        assert_eq!(by_text("Bad derived").status, ClaimStatus::Drift);
        assert_eq!(by_text("Vague derived").status, ClaimStatus::Unverifiable);

        // [pftui] with no SQL → unverifiable.
        assert_eq!(by_text("No-SQL metric").status, ClaimStatus::Unverifiable);

        // Ext present + ext missing.
        let ext = by_text("External fact");
        assert_eq!(ext.status, ClaimStatus::Broken);
        assert!(ext.detail.contains("[ext:9]"));

        // Untagged counted.
        assert_eq!(report.untagged, 1);
        assert!(report.broken >= 2);
        assert!(report.drift >= 2);
    }

    #[test]
    fn tolerance_within_two_percent_verifies() {
        let conn = test_conn();
        // Claimed 124,000 vs actual max 124,752.53 → 0.6% off → verified.
        let content = "x **$124,000** [pftui] `SELECT CAST(close AS REAL) FROM price_history ORDER BY CAST(close AS REAL) DESC LIMIT 1`";
        let report = verify_section(&conn, "s", content, "2026-06-12");
        assert_eq!(report.findings[0].status, ClaimStatus::Verified);
    }

    #[test]
    fn dates_must_match_exactly() {
        let conn = test_conn();
        let content = "peak on 2025-10-07 [pftui] `SELECT date FROM price_history ORDER BY CAST(close AS REAL) DESC LIMIT 1`";
        let report = verify_section(&conn, "s", content, "2026-06-12");
        assert_eq!(report.findings[0].status, ClaimStatus::Drift);
        assert!(report.findings[0].detail.contains("2025-10-06"));
    }

    #[test]
    fn rejects_non_select_sql() {
        let conn = test_conn();
        let content =
            "evil [pftui] `SELECT 1; DROP TABLE price_history` and also $5,000 claimed";
        let report = verify_section(&conn, "s", content, "2026-06-12");
        assert_eq!(report.findings[0].status, ClaimStatus::Broken);
        // Table survived.
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn untagged_pass_only_runs_in_contract_sections() {
        let conn = test_conn();
        // No tags anywhere → doctrine/prose section → zero claims.
        let content = "We believe $100,000 is plausible and 50% odds are fair.";
        let report = verify_section(&conn, "doctrine", content, "2026-06-12");
        assert_eq!(report.total_claims(), 0);
    }

    #[test]
    fn snapshot_vs_structural_classification() {
        let claims = extract_claims(
            "## Current status (as of 2026-06-01)\n\nlive read $1,000 [pftui]\n\n## Halving history\n\npeak close $2,000 on 2025-10-06 [pftui]\n",
        );
        assert_eq!(claims[0].class, ClaimClass::Snapshot);
        assert_eq!(claims[1].class, ClaimClass::Structural);
    }

    #[test]
    fn json_shape_is_stable() {
        let conn = test_conn();
        let report = verify_section(&conn, "synthetic", FIXTURE, "2026-06-12");
        let v = serde_json::to_value(&report).expect("serialize");
        assert_eq!(v["section"], "synthetic");
        for key in [
            "verified",
            "drift",
            "broken",
            "unverifiable",
            "ext_checked",
            "untagged",
        ] {
            assert!(v[key].is_u64(), "missing count {key}");
        }
        let f = &v["findings"][0];
        for key in ["line", "text", "kind", "class", "status", "severity", "detail"] {
            assert!(!f[key].is_null(), "missing finding field {key}");
        }
    }

    #[test]
    fn doctor_summary_reports_findings() {
        let conn = test_conn();
        conn.execute_batch(
            "CREATE TABLE thesis (id INTEGER PRIMARY KEY, section TEXT UNIQUE, content TEXT,
                                  conviction TEXT DEFAULT 'medium', updated_at TEXT, review_by TEXT);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thesis (section, content) VALUES ('synthetic', ?)",
            [FIXTURE],
        )
        .unwrap();
        let (passed, msg) = doctor_summary(&conn).expect("summary");
        assert!(!passed);
        assert!(msg.contains("broken"), "msg: {msg}");

        // Clean content → passes.
        conn.execute(
            "UPDATE thesis SET content = ? WHERE section='synthetic'",
            ["anchor $124,752 on 2025-10-06 [pftui] `SELECT date, CAST(close AS REAL) FROM price_history ORDER BY CAST(close AS REAL) DESC LIMIT 1`"],
        )
        .unwrap();
        let (passed, msg) = doctor_summary(&conn).expect("summary");
        assert!(passed, "msg: {msg}");
    }

    #[test]
    fn year_like_integers_are_not_claim_values() {
        let (nums, dates) = extract_values("the 2017 top and the 2021 top, plus $1,150");
        assert_eq!(nums, vec![1150.0]);
        assert!(dates.is_empty());
    }

    #[test]
    fn year_months_and_range_dashes_are_not_negative_numbers() {
        let (nums, _) = extract_values("min (window 2026-03→) was **8**");
        assert_eq!(nums, vec![8.0]);

        let (nums, _) = extract_values("the $40k-53k zone");
        assert_eq!(nums, vec![40000.0, 53000.0]);
    }

    #[test]
    fn forbidden_claim_examples_are_exempt_from_untagged_pass() {
        let content = "anchor $1 [pftui]\n\n- ❌ \"BTC will hit $250k by 2027\" — speculative\n";
        let claims = extract_claims(content);
        assert!(claims.iter().all(|c| c.kind != ClaimKind::Untagged));
    }
}
