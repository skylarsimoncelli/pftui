//! `pftui data series status` — freshness of every registered canonical
//! series, driven entirely by the `series_registry` table (R3).

use anyhow::Result;
use chrono::Utc;

use crate::db::backend::BackendConnection;
use crate::db::series_registry::{self, SeriesStatus};

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let conn = match backend {
        BackendConnection::Sqlite { conn } => conn,
        BackendConnection::Postgres { .. } => {
            anyhow::bail!(
                "data series status reads the local SQLite series_registry; \
                 not available on the postgres backend"
            );
        }
    };
    let statuses = series_registry::status_all(conn, Utc::now())?;

    if json_output {
        let stale = statuses.iter().filter(|s| s.stale).count();
        let past_2x = statuses.iter().filter(|s| s.past_2x_sla).count();
        let doc = serde_json::json!({
            "generated_at": Utc::now().to_rfc3339(),
            "total_series": statuses.len(),
            "stale": stale,
            "past_2x_sla": past_2x,
            "series": statuses,
        });
        println!("{}", serde_json::to_string_pretty(&doc)?);
        return Ok(());
    }

    print_table(&statuses);
    Ok(())
}

fn glyph(s: &SeriesStatus) -> &'static str {
    if s.past_2x_sla {
        "✗"
    } else if s.stale {
        "⚠"
    } else {
        "✓"
    }
}

fn age_label(s: &SeriesStatus) -> String {
    match s.age_hours {
        None => "no data".to_string(),
        Some(h) if h < 48.0 => format!("{h:.0}h"),
        Some(h) => format!("{:.1}d", h / 24.0),
    }
}

fn print_table(statuses: &[SeriesStatus]) {
    println!("Canonical series freshness ({} registered)", statuses.len());
    println!(
        "{:<2} {:<24} {:<12} {:<22} {:>9} {:>8}  Storage",
        "", "Series", "Kind", "Last datapoint", "Age", "SLA"
    );
    println!("{}", "-".repeat(100));
    for s in statuses {
        let storage = match &s.entry.storage_filter {
            Some(f) => format!("{} [{}]", s.entry.storage_table, f),
            None => s.entry.storage_table.clone(),
        };
        println!(
            "{:<2} {:<24} {:<12} {:<22} {:>9} {:>7}h  {}",
            glyph(s),
            s.entry.series_id,
            s.entry.kind,
            s.last_datapoint.as_deref().unwrap_or("—"),
            age_label(s),
            s.entry.freshness_sla_hours,
            storage,
        );
    }
    let stale: Vec<_> = statuses.iter().filter(|s| s.stale).collect();
    println!();
    if stale.is_empty() {
        println!("All registered series within SLA.");
    } else {
        println!(
            "{} of {} series stale ({} past 2x SLA — flagged by `pftui system doctor`).",
            stale.len(),
            statuses.len(),
            stale.iter().filter(|s| s.past_2x_sla).count()
        );
    }
}
