//! `pftui research verify-thesis` — re-verify the thesis evidence contract.
//!
//! All extraction/verification logic lives in `research::thesis_verify`;
//! this module is presentation only (per-section summary table, detail
//! rows, `--json`, doctor one-liner).

use anyhow::Result;
use rusqlite::Connection;

use crate::db::backend::BackendConnection;
use crate::db::thesis;
use crate::research::thesis_verify::{self, ClaimClass, ClaimStatus, SectionReport};

fn sqlite(backend: &BackendConnection) -> Result<&Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow::anyhow!("research verify-thesis requires the SQLite backend"))
}

pub fn run(backend: &BackendConnection, section: Option<&str>, json: bool) -> Result<()> {
    let conn = sqlite(backend)?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    let entries: Vec<(String, String)> = match section {
        Some(name) => match thesis::get_thesis_section(conn, name)? {
            Some(e) => vec![(e.section, e.content)],
            None => anyhow::bail!("thesis section '{name}' not found"),
        },
        None => thesis::list_thesis(conn)?
            .into_iter()
            .map(|e| (e.section, e.content))
            .collect(),
    };

    let reports: Vec<SectionReport> = entries
        .iter()
        .map(|(name, content)| thesis_verify::verify_section(conn, name, content, &today))
        .collect();

    if json {
        print_json(&reports)?;
    } else {
        print_text(&reports);
    }
    Ok(())
}

fn totals(reports: &[SectionReport]) -> (usize, usize, usize, usize, usize, usize, usize) {
    let mut t = (0, 0, 0, 0, 0, 0, 0);
    for r in reports {
        t.0 += r.verified;
        t.1 += r.drift;
        t.2 += r.broken;
        t.3 += r.unverifiable;
        t.4 += r.ext_checked;
        t.5 += r.untagged;
        t.6 += structural_drift(r);
    }
    t
}

fn structural_drift(r: &SectionReport) -> usize {
    r.findings
        .iter()
        .filter(|f| f.status == ClaimStatus::Drift && f.claim.class == ClaimClass::Structural)
        .count()
}

fn doctor_line(reports: &[SectionReport]) -> Option<String> {
    let (_, _, broken, _, _, untagged, structural) = totals(reports);
    if broken + structural + untagged == 0 {
        return None;
    }
    Some(format!(
        "Doctor: {broken} broken, {structural} structural-drift, {untagged} untagged evidence claims — \
         re-verify and repair the affected thesis sections (curated L4 rows; journal every correction)"
    ))
}

fn print_json(reports: &[SectionReport]) -> Result<()> {
    let (verified, drift, broken, unverifiable, ext_checked, untagged, structural) =
        totals(reports);
    let payload = serde_json::json!({
        "sections": reports,
        "totals": {
            "verified": verified,
            "drift": drift,
            "structural_drift": structural,
            "broken": broken,
            "unverifiable": unverifiable,
            "ext_checked": ext_checked,
            "untagged": untagged,
        },
        "doctor": doctor_line(reports),
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn print_text(reports: &[SectionReport]) {
    println!("Thesis evidence verification");
    println!("════════════════════════════════════════════════════════════════════════");
    println!(
        "{:<32} {:>5} {:>5} {:>6} {:>6} {:>5} {:>5}",
        "section", "ok", "drift", "broken", "unver", "ext", "untag"
    );
    for r in reports {
        if r.total_claims() == 0 {
            continue; // doctrine/prose section — no evidence contract
        }
        println!(
            "{:<32} {:>5} {:>5} {:>6} {:>6} {:>5} {:>5}",
            r.section, r.verified, r.drift, r.broken, r.unverifiable, r.ext_checked, r.untagged
        );
    }
    let (verified, drift, broken, unverifiable, ext_checked, untagged, structural) =
        totals(reports);
    println!(
        "{:<32} {:>5} {:>5} {:>6} {:>6} {:>5} {:>5}",
        "TOTAL", verified, drift, broken, unverifiable, ext_checked, untagged
    );
    if structural > 0 {
        println!("({structural} of {drift} drift findings are STRUCTURAL — suspect, not aging)");
    }

    // Detail rows: everything that is not a clean pass.
    let mut printed_header = false;
    for r in reports {
        for f in r
            .findings
            .iter()
            .filter(|f| !matches!(f.status, ClaimStatus::Verified | ClaimStatus::ExtPresent))
        {
            if !printed_header {
                println!("\nFindings (non-verified):");
                printed_header = true;
            }
            println!(
                "  [{}|{}] {}:{} — {}",
                f.status.label(),
                f.severity,
                r.section,
                f.claim.line,
                f.claim.text
            );
            println!("      {}", f.detail);
        }
    }
    if !printed_header {
        println!("\nAll extracted claims verified.");
    }

    if let Some(line) = doctor_line(reports) {
        println!("\n{line}");
    }
}
