use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerFlowEntry {
    pub id: i64,
    pub date: String,
    pub event: String,
    pub source_complex: String,
    pub direction: String,
    pub target_complex: Option<String>,
    pub evidence: String,
    pub magnitude: i32,
    pub agent_source: Option<String>,
    pub created_at: String,
}

impl PowerFlowEntry {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            date: row.get(1)?,
            event: row.get(2)?,
            source_complex: row.get(3)?,
            direction: row.get(4)?,
            target_complex: row.get(5)?,
            evidence: row.get(6)?,
            magnitude: row.get(7)?,
            agent_source: row.get(8)?,
            created_at: row.get(9)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerBalance {
    pub complex: String,
    pub net: i64,
    pub gaining_count: i64,
    pub losing_count: i64,
    pub gaining_magnitude: i64,
    pub losing_magnitude: i64,
}

const VALID_COMPLEXES: [&str; 3] = ["FIC", "MIC", "TIC"];
const VALID_DIRECTIONS: [&str; 2] = ["gaining", "losing"];

fn validate_complex(complex: &str) -> Result<()> {
    if !VALID_COMPLEXES.contains(&complex) {
        anyhow::bail!(
            "Invalid complex '{}'. Must be one of: FIC, MIC, TIC",
            complex
        );
    }
    Ok(())
}

fn validate_direction(direction: &str) -> Result<()> {
    if !VALID_DIRECTIONS.contains(&direction) {
        anyhow::bail!(
            "Invalid direction '{}'. Must be 'gaining' or 'losing'",
            direction
        );
    }
    Ok(())
}

// ── SQLite ──────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn add_power_flow(
    conn: &Connection,
    date: &str,
    event: &str,
    source_complex: &str,
    direction: &str,
    target_complex: Option<&str>,
    evidence: &str,
    magnitude: i32,
    agent_source: Option<&str>,
) -> Result<i64> {
    validate_complex(source_complex)?;
    validate_direction(direction)?;
    if let Some(tc) = target_complex {
        validate_complex(tc)?;
    }
    if !(1..=5).contains(&magnitude) {
        anyhow::bail!("Magnitude must be between 1 and 5, got {}", magnitude);
    }

    conn.execute(
        "INSERT INTO power_flows (date, event, source_complex, direction, target_complex, evidence, magnitude, agent_source)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![date, event, source_complex, direction, target_complex, evidence, magnitude, agent_source],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_power_flows(
    conn: &Connection,
    complex_filter: Option<&str>,
    direction_filter: Option<&str>,
    days: usize,
) -> Result<Vec<PowerFlowEntry>> {
    let mut conditions = vec![format!(
        "date >= date('now', '-{} days')",
        days
    )];

    if let Some(c) = complex_filter {
        validate_complex(c)?;
        conditions.push(format!(
            "(source_complex = '{}' OR target_complex = '{}')",
            c, c
        ));
    }
    if let Some(d) = direction_filter {
        validate_direction(d)?;
        conditions.push(format!("direction = '{}'", d));
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT id, date, event, source_complex, direction, target_complex, evidence, magnitude, agent_source, created_at
         FROM power_flows
         WHERE {}
         ORDER BY date DESC, id DESC",
        where_clause
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], PowerFlowEntry::from_row)?;
    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn compute_balance(conn: &Connection, days: usize) -> Result<Vec<PowerBalance>> {
    let mut balances = Vec::new();

    for complex in &VALID_COMPLEXES {
        let gaining_as_source: (i64, i64) = conn.query_row(
            &format!(
                "SELECT COALESCE(COUNT(*), 0), COALESCE(SUM(magnitude), 0)
                 FROM power_flows
                 WHERE source_complex = ? AND direction = 'gaining'
                 AND date >= date('now', '-{} days')",
                days
            ),
            params![complex],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let losing_as_source: (i64, i64) = conn.query_row(
            &format!(
                "SELECT COALESCE(COUNT(*), 0), COALESCE(SUM(magnitude), 0)
                 FROM power_flows
                 WHERE source_complex = ? AND direction = 'losing'
                 AND date >= date('now', '-{} days')",
                days
            ),
            params![complex],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        // When this complex is the target, the direction is inverted:
        // "FIC gaining, target MIC" means MIC is losing
        let gaining_as_target: (i64, i64) = conn.query_row(
            &format!(
                "SELECT COALESCE(COUNT(*), 0), COALESCE(SUM(magnitude), 0)
                 FROM power_flows
                 WHERE target_complex = ? AND direction = 'losing'
                 AND date >= date('now', '-{} days')",
                days
            ),
            params![complex],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let losing_as_target: (i64, i64) = conn.query_row(
            &format!(
                "SELECT COALESCE(COUNT(*), 0), COALESCE(SUM(magnitude), 0)
                 FROM power_flows
                 WHERE target_complex = ? AND direction = 'gaining'
                 AND date >= date('now', '-{} days')",
                days
            ),
            params![complex],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let gaining_count = gaining_as_source.0 + gaining_as_target.0;
        let gaining_magnitude = gaining_as_source.1 + gaining_as_target.1;
        let losing_count = losing_as_source.0 + losing_as_target.0;
        let losing_magnitude = losing_as_source.1 + losing_as_target.1;
        let net = gaining_magnitude - losing_magnitude;

        balances.push(PowerBalance {
            complex: complex.to_string(),
            net,
            gaining_count,
            losing_count,
            gaining_magnitude,
            losing_magnitude,
        });
    }

    Ok(balances)
}

// ── Postgres ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn add_power_flow_postgres(
    pool: &PgPool,
    date: &str,
    event: &str,
    source_complex: &str,
    direction: &str,
    target_complex: Option<&str>,
    evidence: &str,
    magnitude: i32,
    agent_source: Option<&str>,
) -> Result<i64> {
    validate_complex(source_complex)?;
    validate_direction(direction)?;
    if let Some(tc) = target_complex {
        validate_complex(tc)?;
    }
    if !(1..=5).contains(&magnitude) {
        anyhow::bail!("Magnitude must be between 1 and 5, got {}", magnitude);
    }

    let id: i64 = crate::db::pg_runtime::block_on(async {
        let row = sqlx::query_scalar::<_, i64>(
            "INSERT INTO power_flows (date, event, source_complex, direction, target_complex, evidence, magnitude, agent_source)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
        )
        .bind(date)
        .bind(event)
        .bind(source_complex)
        .bind(direction)
        .bind(target_complex)
        .bind(evidence)
        .bind(magnitude)
        .bind(agent_source)
        .fetch_one(pool)
        .await?;
        Ok::<i64, sqlx::Error>(row)
    })?;

    Ok(id)
}

fn list_power_flows_postgres(
    pool: &PgPool,
    complex_filter: Option<&str>,
    direction_filter: Option<&str>,
    days: usize,
) -> Result<Vec<PowerFlowEntry>> {
    let entries: Vec<PowerFlowEntry> = crate::db::pg_runtime::block_on(async {
        let mut conditions = vec![format!(
            "date >= (CURRENT_DATE - INTERVAL '{} days')::text",
            days
        )];

        if let Some(c) = complex_filter {
            conditions.push(format!(
                "(source_complex = '{}' OR target_complex = '{}')",
                c, c
            ));
        }
        if let Some(d) = direction_filter {
            conditions.push(format!("direction = '{}'", d));
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT id, date, event, source_complex, direction, target_complex, evidence, magnitude, agent_source, created_at::text
             FROM power_flows
             WHERE {}
             ORDER BY date DESC, id DESC",
            where_clause
        );

        let rows = sqlx::query_as::<_, (i64, String, String, String, String, Option<String>, String, i32, Option<String>, String)>(&sql)
            .fetch_all(pool)
            .await?;

        let entries: Vec<PowerFlowEntry> = rows
            .into_iter()
            .map(|r| PowerFlowEntry {
                id: r.0,
                date: r.1,
                event: r.2,
                source_complex: r.3,
                direction: r.4,
                target_complex: r.5,
                evidence: r.6,
                magnitude: r.7,
                agent_source: r.8,
                created_at: r.9,
            })
            .collect();

        Ok::<Vec<PowerFlowEntry>, sqlx::Error>(entries)
    })?;

    Ok(entries)
}

fn compute_balance_postgres(pool: &PgPool, days: usize) -> Result<Vec<PowerBalance>> {
    let balances: Vec<PowerBalance> = crate::db::pg_runtime::block_on(async {
        let mut result = Vec::new();

        for complex in &VALID_COMPLEXES {
            let sql = format!(
                "SELECT
                    COALESCE(SUM(CASE WHEN source_complex = $1 AND direction = 'gaining' THEN 1
                                      WHEN target_complex = $1 AND direction = 'losing' THEN 1
                                      ELSE 0 END), 0) as gaining_count,
                    COALESCE(SUM(CASE WHEN source_complex = $1 AND direction = 'gaining' THEN magnitude
                                      WHEN target_complex = $1 AND direction = 'losing' THEN magnitude
                                      ELSE 0 END), 0) as gaining_magnitude,
                    COALESCE(SUM(CASE WHEN source_complex = $1 AND direction = 'losing' THEN 1
                                      WHEN target_complex = $1 AND direction = 'gaining' THEN 1
                                      ELSE 0 END), 0) as losing_count,
                    COALESCE(SUM(CASE WHEN source_complex = $1 AND direction = 'losing' THEN magnitude
                                      WHEN target_complex = $1 AND direction = 'gaining' THEN magnitude
                                      ELSE 0 END), 0) as losing_magnitude
                 FROM power_flows
                 WHERE date >= (CURRENT_DATE - INTERVAL '{} days')::text",
                days
            );

            let row = sqlx::query_as::<_, (i64, i64, i64, i64)>(&sql)
                .bind(*complex)
                .fetch_one(pool)
                .await?;

            result.push(PowerBalance {
                complex: complex.to_string(),
                net: row.1 - row.3,
                gaining_count: row.0,
                losing_count: row.2,
                gaining_magnitude: row.1,
                losing_magnitude: row.3,
            });
        }

        Ok::<Vec<PowerBalance>, sqlx::Error>(result)
    })?;

    Ok(balances)
}

// ── Backend dispatch ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn add_power_flow_backend(
    backend: &BackendConnection,
    date: &str,
    event: &str,
    source_complex: &str,
    direction: &str,
    target_complex: Option<&str>,
    evidence: &str,
    magnitude: i32,
    agent_source: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            add_power_flow(
                conn,
                date,
                event,
                source_complex,
                direction,
                target_complex,
                evidence,
                magnitude,
                agent_source,
            )
        },
        |pool| {
            add_power_flow_postgres(
                pool,
                date,
                event,
                source_complex,
                direction,
                target_complex,
                evidence,
                magnitude,
                agent_source,
            )
        },
    )
}

pub fn list_power_flows_backend(
    backend: &BackendConnection,
    complex_filter: Option<&str>,
    direction_filter: Option<&str>,
    days: usize,
) -> Result<Vec<PowerFlowEntry>> {
    query::dispatch(
        backend,
        |conn| list_power_flows(conn, complex_filter, direction_filter, days),
        |pool| list_power_flows_postgres(pool, complex_filter, direction_filter, days),
    )
}

pub fn compute_balance_backend(
    backend: &BackendConnection,
    days: usize,
) -> Result<Vec<PowerBalance>> {
    query::dispatch(
        backend,
        |conn| compute_balance(conn, days),
        |pool| compute_balance_postgres(pool, days),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    /// Return today's date as YYYY-MM-DD so time-windowed queries always
    /// include the test data regardless of when the test suite runs.
    fn today() -> String {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    }

    #[test]
    fn test_add_and_list_power_flows() {
        let conn = db::open_in_memory();
        let date = today();
        let id = add_power_flow(
            &conn,
            &date,
            "Fed rate pause signals fiscal dominance",
            "FIC",
            "gaining",
            Some("MIC"),
            "Treasury yields falling while defense spending flat",
            4,
            Some("low-agent"),
        )
        .unwrap();
        assert!(id > 0);

        let entries = list_power_flows(&conn, None, None, 7).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_complex, "FIC");
        assert_eq!(entries[0].direction, "gaining");
        assert_eq!(entries[0].target_complex, Some("MIC".to_string()));
        assert_eq!(entries[0].magnitude, 4);
    }

    #[test]
    fn test_invalid_complex_rejected() {
        let conn = db::open_in_memory();
        let date = today();
        let result = add_power_flow(
            &conn,
            &date,
            "test",
            "INVALID",
            "gaining",
            None,
            "test",
            3,
            None,
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid complex"));
    }

    #[test]
    fn test_invalid_direction_rejected() {
        let conn = db::open_in_memory();
        let date = today();
        let result = add_power_flow(
            &conn,
            &date,
            "test",
            "FIC",
            "winning",
            None,
            "test",
            3,
            None,
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid direction"));
    }

    #[test]
    fn test_magnitude_bounds() {
        let conn = db::open_in_memory();
        let date = today();
        let too_low = add_power_flow(
            &conn,
            &date,
            "test",
            "FIC",
            "gaining",
            None,
            "test",
            0,
            None,
        );
        assert!(too_low.is_err());

        let too_high = add_power_flow(
            &conn,
            &date,
            "test",
            "FIC",
            "gaining",
            None,
            "test",
            6,
            None,
        );
        assert!(too_high.is_err());

        let ok = add_power_flow(
            &conn,
            &date,
            "test",
            "FIC",
            "gaining",
            None,
            "test",
            1,
            None,
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn test_filter_by_complex() {
        let conn = db::open_in_memory();
        let date = today();
        add_power_flow(
            &conn,
            &date,
            "FIC event",
            "FIC",
            "gaining",
            None,
            "evidence",
            3,
            None,
        )
        .unwrap();
        add_power_flow(
            &conn,
            &date,
            "MIC event",
            "MIC",
            "losing",
            None,
            "evidence",
            2,
            None,
        )
        .unwrap();

        let fic_only = list_power_flows(&conn, Some("FIC"), None, 7).unwrap();
        assert_eq!(fic_only.len(), 1);
        assert_eq!(fic_only[0].event, "FIC event");
    }

    #[test]
    fn test_filter_by_direction() {
        let conn = db::open_in_memory();
        let date = today();
        add_power_flow(
            &conn,
            &date,
            "gaining event",
            "FIC",
            "gaining",
            None,
            "evidence",
            3,
            None,
        )
        .unwrap();
        add_power_flow(
            &conn,
            &date,
            "losing event",
            "MIC",
            "losing",
            None,
            "evidence",
            2,
            None,
        )
        .unwrap();

        let gaining_only = list_power_flows(&conn, None, Some("gaining"), 7).unwrap();
        assert_eq!(gaining_only.len(), 1);
        assert_eq!(gaining_only[0].event, "gaining event");
    }

    #[test]
    fn test_compute_balance() {
        let conn = db::open_in_memory();
        let date = today();
        // FIC gaining +4 from MIC
        add_power_flow(
            &conn,
            &date,
            "event 1",
            "FIC",
            "gaining",
            Some("MIC"),
            "evidence 1",
            4,
            None,
        )
        .unwrap();
        // FIC gaining +3 (no target)
        add_power_flow(
            &conn,
            &date,
            "event 2",
            "FIC",
            "gaining",
            None,
            "evidence 2",
            3,
            None,
        )
        .unwrap();
        // TIC losing -2 to FIC
        add_power_flow(
            &conn,
            &date,
            "event 3",
            "TIC",
            "losing",
            Some("FIC"),
            "evidence 3",
            2,
            None,
        )
        .unwrap();

        let balances = compute_balance(&conn, 30).unwrap();
        assert_eq!(balances.len(), 3);

        let fic = balances.iter().find(|b| b.complex == "FIC").unwrap();
        // FIC: gaining_as_source(2 events, mag 7) + gaining_as_target(1 event from TIC losing, mag 2) = 3 gaining, mag 9
        // FIC: losing_as_source(0) + losing_as_target(0) = 0 losing
        assert_eq!(fic.gaining_count, 3);
        assert_eq!(fic.gaining_magnitude, 9);
        assert_eq!(fic.losing_count, 0);
        assert_eq!(fic.net, 9);

        let mic = balances.iter().find(|b| b.complex == "MIC").unwrap();
        // MIC: gaining_as_source(0) + gaining_as_target(0) = 0 gaining
        // MIC: losing_as_source(0) + losing_as_target(FIC gaining target MIC, 1 event mag 4) = 1 losing, mag 4
        assert_eq!(mic.gaining_count, 0);
        assert_eq!(mic.losing_count, 1);
        assert_eq!(mic.losing_magnitude, 4);
        assert_eq!(mic.net, -4);

        let tic = balances.iter().find(|b| b.complex == "TIC").unwrap();
        // TIC: losing_as_source(1 event, mag 2) = 1 losing, mag 2
        assert_eq!(tic.losing_count, 1);
        assert_eq!(tic.losing_magnitude, 2);
        assert_eq!(tic.net, -2);
    }

    #[test]
    fn test_target_complex_filter_includes_entries() {
        let conn = db::open_in_memory();
        let date = today();
        // FIC gaining, target = MIC. Filtering by MIC should include this.
        add_power_flow(
            &conn,
            &date,
            "FIC power grab",
            "FIC",
            "gaining",
            Some("MIC"),
            "evidence",
            3,
            None,
        )
        .unwrap();

        let mic_entries = list_power_flows(&conn, Some("MIC"), None, 7).unwrap();
        assert_eq!(mic_entries.len(), 1);
    }
}
