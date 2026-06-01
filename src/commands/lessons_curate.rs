//! `pftui analytics lessons curate|revive|health` — lesson half-life
//! curation routine. Marks stale uncited lessons as `retired` so the
//! analyst lesson book stays high-signal.

use anyhow::Result;
use serde::Serialize;

use crate::db::agent_messages;
use crate::db::backend::BackendConnection;
use crate::db::prediction_lessons::{self, CurateSummary, LibraryHealth};

const AGENT_FROM: &str = "system";

#[derive(Debug, Serialize)]
struct CurateJson<'a> {
    summary: &'a CurateSummary,
}

#[derive(Debug, Serialize)]
struct ReviveJson {
    lesson_id: i64,
    updated: bool,
}

#[derive(Debug, Serialize)]
struct HealthJson<'a> {
    library: &'a LibraryHealth,
}

pub fn run_curate(
    backend: &BackendConnection,
    dry_run: bool,
    retire_after_days: i64,
    json_output: bool,
) -> Result<()> {
    let summary = prediction_lessons::curate_backend(backend, retire_after_days, dry_run)?;

    // Journal an agent message so analysts can see the substrate was pruned.
    if !dry_run && summary.retired > 0 {
        let content = format!(
            "Lesson half-life curate: retired {} lesson(s) (retire-after-days={}; cutoff={}). Lesson IDs: {}",
            summary.retired,
            summary.retire_after_days,
            summary.cutoff_iso,
            summary
                .actions
                .iter()
                .filter(|a| a.action == "retire")
                .map(|a| a.lesson_id.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        // Best-effort journal; do not fail the curate run if agent_messages
        // is missing or write fails.
        let _ = agent_messages::send_message_backend(
            backend,
            AGENT_FROM,
            None,
            Some("normal"),
            &content,
            Some("lesson-curate"),
            None,
            None,
            None,
        );
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&CurateJson { summary: &summary })?
        );
    } else {
        print_curate_text(&summary);
    }
    Ok(())
}

pub fn run_revive(backend: &BackendConnection, id: i64, json_output: bool) -> Result<()> {
    let updated = prediction_lessons::revive_backend(backend, id)?;
    if updated {
        let _ = agent_messages::send_message_backend(
            backend,
            AGENT_FROM,
            None,
            Some("normal"),
            &format!("Lesson #{} revived (status → active)", id),
            Some("lesson-revive"),
            None,
            None,
            None,
        );
    }
    let payload = ReviveJson {
        lesson_id: id,
        updated,
    };
    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if updated {
        println!("Lesson #{} revived (status → active)", id);
    } else {
        println!("No lesson with id {}", id);
    }
    Ok(())
}

pub fn run_health(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let library = prediction_lessons::library_health_backend(backend)?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&HealthJson { library: &library })?
        );
    } else {
        println!("Lesson Library Health");
        println!("---------------------");
        println!("Total:              {}", library.total);
        println!("Active:             {}", library.active);
        println!("Retired:            {}", library.retired);
        println!("Superseded:         {}", library.superseded);
        println!("Citations total:    {}", library.citations_total);
        println!(
            "Avg cites / active: {:.2}",
            library.avg_citations_per_active
        );
    }
    Ok(())
}

fn print_curate_text(summary: &CurateSummary) {
    println!("Lesson Curate");
    println!("-------------");
    println!(
        "{} considered | {} retired | {} skipped | dry-run={} | retire-after-days={} | cutoff={}",
        summary.considered,
        summary.retired,
        summary.skipped,
        summary.dry_run,
        summary.retire_after_days,
        summary.cutoff_iso,
    );
    let retire_actions: Vec<_> = summary
        .actions
        .iter()
        .filter(|a| a.action == "retire")
        .collect();
    if !retire_actions.is_empty() {
        println!();
        println!("Retired:");
        for action in retire_actions.iter().take(20) {
            println!(
                "  #{:>4} ({:>10}) — {}",
                action.lesson_id, action.miss_type, action.reason
            );
        }
        if retire_actions.len() > 20 {
            println!("  ... and {} more", retire_actions.len() - 20);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::db;
    use crate::db::backend::BackendConnection;
    use crate::db::prediction_lessons;
    use crate::db::user_predictions;
    use rusqlite::params;

    fn fresh_backend() -> BackendConnection {
        let conn = db::open_in_memory();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn curate_and_health_round_trip_via_backend() {
        let backend = fresh_backend();
        user_predictions::add_prediction_backend(
            &backend,
            "stale claim",
            Some("BTC"),
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
        .unwrap();

        // Seed a 120-day-old lesson directly via the underlying sqlite
        // connection.
        let ts = chrono::Utc::now() - chrono::Duration::days(120);
        let created_at = ts.format("%Y-%m-%d %H:%M:%S").to_string();
        {
            let conn = backend.sqlite_native().expect("sqlite backend");
            conn.execute(
                "INSERT INTO prediction_lessons
                    (prediction_id, miss_type, what_predicted, what_happened,
                     why_wrong, signal_misread, created_at)
                 VALUES (1, 'directional', 'pred', 'happened', 'why', NULL, ?)",
                params![created_at],
            )
            .unwrap();
        }

        // dry-run first: nothing mutated.
        let dry = prediction_lessons::curate_backend(&backend, 60, true).unwrap();
        assert_eq!(dry.retired, 1);
        let health = prediction_lessons::library_health_backend(&backend).unwrap();
        assert_eq!(health.active, 1);
        assert_eq!(health.retired, 0);

        // real run: retires it.
        let real = prediction_lessons::curate_backend(&backend, 60, false).unwrap();
        assert_eq!(real.retired, 1);
        let health = prediction_lessons::library_health_backend(&backend).unwrap();
        assert_eq!(health.active, 0);
        assert_eq!(health.retired, 1);

        // revive un-retires it.
        let revived = prediction_lessons::revive_backend(&backend, 1).unwrap();
        assert!(revived);
        let health = prediction_lessons::library_health_backend(&backend).unwrap();
        assert_eq!(health.active, 1);
        assert_eq!(health.retired, 0);
    }
}
