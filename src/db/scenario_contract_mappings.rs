use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// Links a pftui scenario to a prediction market contract.
/// When contracts are refreshed, the contract's probability is auto-logged
/// as a data point in the scenario's probability history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Used in tests + F55.5 calibration infrastructure
pub struct ScenarioContractMapping {
    pub id: i64,
    pub scenario_id: i64,
    pub contract_id: String,
    pub created_at: String,
}

/// A mapping enriched with scenario and contract details for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedMapping {
    pub mapping_id: i64,
    pub scenario_id: i64,
    pub scenario_name: String,
    pub scenario_probability: f64,
    pub contract_id: String,
    pub contract_question: String,
    pub contract_probability: f64,
    pub contract_category: String,
    pub divergence_pp: f64,
}

// ── SQLite ──────────────────────────────────────────────────────────

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS scenario_contract_mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            contract_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(scenario_id, contract_id)
        );
        CREATE INDEX IF NOT EXISTS idx_scm_scenario ON scenario_contract_mappings(scenario_id);
        CREATE INDEX IF NOT EXISTS idx_scm_contract ON scenario_contract_mappings(contract_id);",
    )?;
    Ok(())
}

/// Add a mapping between a scenario and a prediction market contract.
pub fn add_mapping(conn: &Connection, scenario_id: i64, contract_id: &str) -> Result<()> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO scenario_contract_mappings (scenario_id, contract_id) VALUES (?, ?)",
        params![scenario_id, contract_id],
    )?;
    Ok(())
}

/// Remove a mapping by scenario_id + contract_id.
pub fn remove_mapping(conn: &Connection, scenario_id: i64, contract_id: &str) -> Result<bool> {
    ensure_table(conn)?;
    let changed = conn.execute(
        "DELETE FROM scenario_contract_mappings WHERE scenario_id = ? AND contract_id = ?",
        params![scenario_id, contract_id],
    )?;
    Ok(changed > 0)
}

/// Remove all mappings for a scenario.
pub fn remove_all_for_scenario(conn: &Connection, scenario_id: i64) -> Result<usize> {
    ensure_table(conn)?;
    let changed = conn.execute(
        "DELETE FROM scenario_contract_mappings WHERE scenario_id = ?",
        params![scenario_id],
    )?;
    Ok(changed)
}

/// List all mappings, enriched with scenario and contract details.
pub fn list_enriched(conn: &Connection) -> Result<Vec<EnrichedMapping>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT m.id, m.scenario_id, s.name, s.probability,
                m.contract_id, COALESCE(c.question, '(contract not found)'),
                COALESCE(c.last_price, 0.0), COALESCE(c.category, 'unknown')
         FROM scenario_contract_mappings m
         JOIN scenarios s ON s.id = m.scenario_id
         LEFT JOIN prediction_market_contracts c ON c.contract_id = m.contract_id
         ORDER BY s.name",
    )?;

    let rows = stmt
        .query_map([], |row| {
            let scenario_prob: f64 = row.get(3)?;
            let contract_prob: f64 = row.get(6)?;
            Ok(EnrichedMapping {
                mapping_id: row.get(0)?,
                scenario_id: row.get(1)?,
                scenario_name: row.get(2)?,
                scenario_probability: scenario_prob,
                contract_id: row.get(4)?,
                contract_question: row.get(5)?,
                contract_probability: contract_prob,
                contract_category: row.get(7)?,
                divergence_pp: (scenario_prob - contract_prob) * 100.0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

/// Get all mappings as raw rows (for refresh sync — no joins needed).
#[allow(dead_code)] // Infrastructure for F55.5 (calibration analytics)
pub fn list_raw(conn: &Connection) -> Result<Vec<ScenarioContractMapping>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, scenario_id, contract_id, created_at FROM scenario_contract_mappings",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ScenarioContractMapping {
                id: row.get(0)?,
                scenario_id: row.get(1)?,
                contract_id: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get contract probability for a given contract_id (used during refresh sync).
pub fn get_contract_probability(conn: &Connection, contract_id: &str) -> Result<Option<f64>> {
    ensure_table(conn)?;
    let result: Option<f64> = conn
        .query_row(
            "SELECT last_price FROM prediction_market_contracts WHERE contract_id = ?",
            params![contract_id],
            |row| row.get(0),
        )
        .ok();
    Ok(result)
}

// ── Backend dispatch ────────────────────────────────────────────────

pub fn add_mapping_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    contract_id: &str,
) -> Result<()> {
    let cid1 = contract_id.to_string();
    let cid2 = contract_id.to_string();
    query::dispatch(
        backend,
        move |conn| add_mapping(conn, scenario_id, &cid1),
        move |pool| add_mapping_postgres(pool, scenario_id, &cid2),
    )
}

pub fn remove_mapping_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    contract_id: &str,
) -> Result<bool> {
    let cid1 = contract_id.to_string();
    let cid2 = contract_id.to_string();
    query::dispatch(
        backend,
        move |conn| remove_mapping(conn, scenario_id, &cid1),
        move |pool| remove_mapping_postgres(pool, scenario_id, &cid2),
    )
}

pub fn remove_all_for_scenario_backend(
    backend: &BackendConnection,
    scenario_id: i64,
) -> Result<usize> {
    query::dispatch(
        backend,
        move |conn| remove_all_for_scenario(conn, scenario_id),
        move |pool| remove_all_for_scenario_postgres(pool, scenario_id),
    )
}

pub fn list_enriched_backend(backend: &BackendConnection) -> Result<Vec<EnrichedMapping>> {
    query::dispatch(backend, list_enriched, list_enriched_postgres)
}

#[allow(dead_code)] // Infrastructure for F55.5 (calibration analytics)
pub fn list_raw_backend(backend: &BackendConnection) -> Result<Vec<ScenarioContractMapping>> {
    query::dispatch(backend, list_raw, list_raw_postgres)
}

#[allow(dead_code)] // Infrastructure for F55.5 (calibration analytics)
pub fn get_contract_probability_backend(
    backend: &BackendConnection,
    contract_id: &str,
) -> Result<Option<f64>> {
    let cid1 = contract_id.to_string();
    let cid2 = contract_id.to_string();
    query::dispatch(
        backend,
        move |conn| get_contract_probability(conn, &cid1),
        move |pool| get_contract_probability_postgres(pool, &cid2),
    )
}

/// After contracts are refreshed, sync mapped contract probabilities into scenario history.
/// Returns the number of scenario history entries logged.
pub fn sync_mapped_probabilities(backend: &BackendConnection) -> Result<usize> {
    query::dispatch(
        backend,
        sync_mapped_probabilities_sqlite,
        sync_mapped_probabilities_postgres,
    )
}

fn sync_mapped_probabilities_sqlite(conn: &Connection) -> Result<usize> {
    ensure_table(conn)?;

    // Get all mappings with current contract and scenario state
    let mut stmt = conn.prepare(
        "SELECT m.scenario_id, s.name, s.probability, c.last_price, c.question
         FROM scenario_contract_mappings m
         JOIN scenarios s ON s.id = m.scenario_id
         JOIN prediction_market_contracts c ON c.contract_id = m.contract_id
         WHERE s.status IN ('active', 'watching')",
    )?;

    let mappings: Vec<(i64, String, f64, f64, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut logged = 0;
    for (scenario_id, _scenario_name, _scenario_prob, contract_prob, contract_question) in &mappings
    {
        // Log the prediction market probability as a scenario history entry
        // Use a descriptive driver so agents can distinguish market-sourced vs manual updates
        let driver = format!(
            "Polymarket: {:.1}% — {}",
            contract_prob * 100.0,
            truncate_str(contract_question, 60)
        );
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver) VALUES (?, ?, ?)",
            params![scenario_id, contract_prob, driver],
        )?;
        logged += 1;
    }

    Ok(logged)
}

// ── Postgres ────────────────────────────────────────────────────────

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_contract_mappings (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                contract_id TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(scenario_id, contract_id)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_scm_scenario ON scenario_contract_mappings(scenario_id)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_scm_contract ON scenario_contract_mappings(contract_id)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_mapping_postgres(pool: &PgPool, scenario_id: i64, contract_id: &str) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO scenario_contract_mappings (scenario_id, contract_id)
             VALUES ($1, $2)
             ON CONFLICT (scenario_id, contract_id) DO NOTHING",
        )
        .bind(scenario_id)
        .bind(contract_id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn remove_all_for_scenario_postgres(pool: &PgPool, scenario_id: i64) -> Result<usize> {
    ensure_table_postgres(pool)?;
    let result = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "DELETE FROM scenario_contract_mappings WHERE scenario_id = $1",
        )
        .bind(scenario_id)
        .execute(pool)
        .await
    })?;
    Ok(result.rows_affected() as usize)
}

fn remove_mapping_postgres(pool: &PgPool, scenario_id: i64, contract_id: &str) -> Result<bool> {
    ensure_table_postgres(pool)?;
    let result = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "DELETE FROM scenario_contract_mappings WHERE scenario_id = $1 AND contract_id = $2",
        )
        .bind(scenario_id)
        .bind(contract_id)
        .execute(pool)
        .await
    })?;
    Ok(result.rows_affected() > 0)
}

fn list_enriched_postgres(pool: &PgPool) -> Result<Vec<EnrichedMapping>> {
    ensure_table_postgres(pool)?;
    type Row = (i64, i64, String, f64, String, String, f64, String);
    let rows: Vec<Row> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT m.id, m.scenario_id, s.name, s.probability,
                    m.contract_id, COALESCE(c.question, '(contract not found)'),
                    COALESCE(c.last_price, 0.0), COALESCE(c.category, 'unknown')
             FROM scenario_contract_mappings m
             JOIN scenarios s ON s.id = m.scenario_id
             LEFT JOIN prediction_market_contracts c ON c.contract_id = m.contract_id
             ORDER BY s.name",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let divergence_pp = (r.3 - r.6) * 100.0;
            EnrichedMapping {
                mapping_id: r.0,
                scenario_id: r.1,
                scenario_name: r.2,
                scenario_probability: r.3,
                contract_id: r.4,
                contract_question: r.5,
                contract_probability: r.6,
                contract_category: r.7,
                divergence_pp,
            }
        })
        .collect())
}

fn list_raw_postgres(pool: &PgPool) -> Result<Vec<ScenarioContractMapping>> {
    ensure_table_postgres(pool)?;
    type Row = (i64, i64, String, String);
    let rows: Vec<Row> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, scenario_id, contract_id, created_at::text
             FROM scenario_contract_mappings",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| ScenarioContractMapping {
            id: r.0,
            scenario_id: r.1,
            contract_id: r.2,
            created_at: r.3,
        })
        .collect())
}

fn get_contract_probability_postgres(pool: &PgPool, contract_id: &str) -> Result<Option<f64>> {
    ensure_table_postgres(pool)?;
    let result: Option<(f64,)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT last_price FROM prediction_market_contracts WHERE contract_id = $1",
        )
        .bind(contract_id)
        .fetch_optional(pool)
        .await
    })?;
    Ok(result.map(|r| r.0))
}

fn sync_mapped_probabilities_postgres(pool: &PgPool) -> Result<usize> {
    ensure_table_postgres(pool)?;
    type Row = (i64, String, f64, f64, String);
    let mappings: Vec<Row> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT m.scenario_id, s.name, s.probability, c.last_price, c.question
             FROM scenario_contract_mappings m
             JOIN scenarios s ON s.id = m.scenario_id
             JOIN prediction_market_contracts c ON c.contract_id = m.contract_id
             WHERE s.status IN ('active', 'watching')",
        )
        .fetch_all(pool)
        .await
    })?;

    let mut logged = 0;
    for (scenario_id, _scenario_name, _scenario_prob, contract_prob, contract_question) in &mappings
    {
        let driver = format!(
            "Polymarket: {:.1}% — {}",
            contract_prob * 100.0,
            truncate_str(contract_question, 60)
        );
        crate::db::pg_runtime::block_on(async {
            sqlx::query(
                "INSERT INTO scenario_history (scenario_id, probability, driver) VALUES ($1, $2, $3)",
            )
            .bind(scenario_id)
            .bind(contract_prob)
            .bind(&driver)
            .execute(pool)
            .await
        })?;
        logged += 1;
    }

    Ok(logged)
}

/// Truncate a string to max_len characters, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Create scenarios table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scenarios (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                probability REAL NOT NULL DEFAULT 0.0,
                description TEXT,
                asset_impact TEXT,
                triggers TEXT,
                historical_precedent TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS scenario_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                probability REAL NOT NULL,
                driver TEXT,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS prediction_market_contracts (
                contract_id TEXT PRIMARY KEY,
                exchange TEXT NOT NULL,
                event_id TEXT NOT NULL,
                event_title TEXT NOT NULL,
                question TEXT NOT NULL,
                category TEXT NOT NULL,
                last_price REAL NOT NULL,
                volume_24h REAL NOT NULL,
                liquidity REAL NOT NULL,
                end_date TEXT,
                updated_at INTEGER NOT NULL
            );",
        )
        .unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    fn insert_scenario(conn: &Connection, name: &str, probability: f64) -> i64 {
        conn.execute(
            "INSERT INTO scenarios (name, probability) VALUES (?, ?)",
            params![name, probability],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_contract(conn: &Connection, contract_id: &str, question: &str, price: f64) {
        conn.execute(
            "INSERT INTO prediction_market_contracts
             (contract_id, exchange, event_id, event_title, question, category,
              last_price, volume_24h, liquidity, end_date, updated_at)
             VALUES (?, 'polymarket', 'evt1', 'Event', ?, 'economics', ?, 100000.0, 500000.0, NULL, 1711670000)",
            params![contract_id, question, price],
        )
        .unwrap();
    }

    #[test]
    fn add_and_list_mapping() {
        let conn = setup();
        let sid = insert_scenario(&conn, "US Recession 2026", 0.35);
        insert_contract(&conn, "0xabc", "Will US enter recession in 2026?", 0.58);

        add_mapping(&conn, sid, "0xabc").unwrap();

        let enriched = list_enriched(&conn).unwrap();
        assert_eq!(enriched.len(), 1);
        assert_eq!(enriched[0].scenario_name, "US Recession 2026");
        assert_eq!(enriched[0].contract_id, "0xabc");
        assert!((enriched[0].scenario_probability - 0.35).abs() < 0.001);
        assert!((enriched[0].contract_probability - 0.58).abs() < 0.001);
        // Divergence: (0.35 - 0.58) * 100 = -23.0pp
        assert!((enriched[0].divergence_pp - (-23.0)).abs() < 0.1);
    }

    #[test]
    fn duplicate_mapping_ignored() {
        let conn = setup();
        let sid = insert_scenario(&conn, "Fed Cut", 0.20);
        insert_contract(&conn, "0xfed", "Will Fed cut rates?", 0.12);

        add_mapping(&conn, sid, "0xfed").unwrap();
        add_mapping(&conn, sid, "0xfed").unwrap(); // duplicate — should be ignored

        let raw = list_raw(&conn).unwrap();
        assert_eq!(raw.len(), 1);
    }

    #[test]
    fn remove_mapping_works() {
        let conn = setup();
        let sid = insert_scenario(&conn, "Iran Strike", 0.30);
        insert_contract(&conn, "0xiran", "Will US strike Iran?", 0.22);

        add_mapping(&conn, sid, "0xiran").unwrap();
        assert_eq!(list_raw(&conn).unwrap().len(), 1);

        let removed = remove_mapping(&conn, sid, "0xiran").unwrap();
        assert!(removed);
        assert_eq!(list_raw(&conn).unwrap().len(), 0);
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let conn = setup();
        let removed = remove_mapping(&conn, 999, "0xnope").unwrap();
        assert!(!removed);
    }

    #[test]
    fn remove_all_for_scenario_works() {
        let conn = setup();
        let sid = insert_scenario(&conn, "Multi-map", 0.50);
        insert_contract(&conn, "0xa", "Question A", 0.40);
        insert_contract(&conn, "0xb", "Question B", 0.60);

        add_mapping(&conn, sid, "0xa").unwrap();
        add_mapping(&conn, sid, "0xb").unwrap();
        assert_eq!(list_raw(&conn).unwrap().len(), 2);

        let removed = remove_all_for_scenario(&conn, sid).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(list_raw(&conn).unwrap().len(), 0);
    }

    #[test]
    fn enriched_shows_missing_contract() {
        let conn = setup();
        let sid = insert_scenario(&conn, "Ghost Contract", 0.50);

        // Map to a contract that doesn't exist in prediction_market_contracts
        add_mapping(&conn, sid, "0xghost").unwrap();

        let enriched = list_enriched(&conn).unwrap();
        assert_eq!(enriched.len(), 1);
        assert_eq!(enriched[0].contract_question, "(contract not found)");
        assert!((enriched[0].contract_probability - 0.0).abs() < 0.001);
    }

    #[test]
    fn sync_mapped_probabilities_logs_history() {
        let conn = setup();
        let sid = insert_scenario(&conn, "Recession Watch", 0.35);
        insert_contract(&conn, "0xrec", "Will US enter recession?", 0.58);
        add_mapping(&conn, sid, "0xrec").unwrap();

        let logged = sync_mapped_probabilities_sqlite(&conn).unwrap();
        assert_eq!(logged, 1);

        // Verify history entry was created
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM scenario_history WHERE scenario_id = ?",
                params![sid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify the logged probability matches the contract, not the scenario
        let prob: f64 = conn
            .query_row(
                "SELECT probability FROM scenario_history WHERE scenario_id = ?",
                params![sid],
                |row| row.get(0),
            )
            .unwrap();
        assert!((prob - 0.58).abs() < 0.001);

        // Verify the driver text contains "Polymarket"
        let driver: String = conn
            .query_row(
                "SELECT driver FROM scenario_history WHERE scenario_id = ?",
                params![sid],
                |row| row.get(0),
            )
            .unwrap();
        assert!(driver.contains("Polymarket"));
        assert!(driver.contains("58.0%"));
    }

    #[test]
    fn sync_skips_inactive_scenarios() {
        let conn = setup();
        let sid = insert_scenario(&conn, "Resolved Scenario", 0.80);
        conn.execute(
            "UPDATE scenarios SET status = 'resolved' WHERE id = ?",
            params![sid],
        )
        .unwrap();
        insert_contract(&conn, "0xres", "Resolved question?", 0.90);
        add_mapping(&conn, sid, "0xres").unwrap();

        let logged = sync_mapped_probabilities_sqlite(&conn).unwrap();
        assert_eq!(logged, 0);
    }

    #[test]
    fn sync_with_no_mappings_returns_zero() {
        let conn = setup();
        let logged = sync_mapped_probabilities_sqlite(&conn).unwrap();
        assert_eq!(logged, 0);
    }

    #[test]
    fn get_contract_probability_found() {
        let conn = setup();
        insert_contract(&conn, "0xtest", "Test question", 0.42);
        let prob = get_contract_probability(&conn, "0xtest").unwrap();
        assert_eq!(prob, Some(0.42));
    }

    #[test]
    fn get_contract_probability_not_found() {
        let conn = setup();
        let prob = get_contract_probability(&conn, "0xnope").unwrap();
        assert_eq!(prob, None);
    }

    #[test]
    fn enriched_mapping_serializes_to_json() {
        let mapping = EnrichedMapping {
            mapping_id: 1,
            scenario_id: 5,
            scenario_name: "Fed Rate Cut".to_string(),
            scenario_probability: 0.25,
            contract_id: "0xfed123".to_string(),
            contract_question: "Will the Fed cut rates?".to_string(),
            contract_probability: 0.12,
            contract_category: "economics".to_string(),
            divergence_pp: 13.0,
        };
        let json = serde_json::to_value(&mapping).unwrap();
        assert_eq!(json["scenario_name"], "Fed Rate Cut");
        assert_eq!(json["divergence_pp"], 13.0);
        assert_eq!(json["contract_probability"], 0.12);
    }

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_str_long() {
        assert_eq!(truncate_str("hello world this is long", 10), "hello w...");
    }

    #[test]
    fn multiple_scenarios_one_contract() {
        let conn = setup();
        let s1 = insert_scenario(&conn, "Scenario A", 0.30);
        let s2 = insert_scenario(&conn, "Scenario B", 0.60);
        insert_contract(&conn, "0xshared", "Shared question", 0.45);

        add_mapping(&conn, s1, "0xshared").unwrap();
        add_mapping(&conn, s2, "0xshared").unwrap();

        let enriched = list_enriched(&conn).unwrap();
        assert_eq!(enriched.len(), 2);
    }

    #[test]
    fn one_scenario_multiple_contracts() {
        let conn = setup();
        let sid = insert_scenario(&conn, "Big Scenario", 0.50);
        insert_contract(&conn, "0xc1", "Question 1", 0.40);
        insert_contract(&conn, "0xc2", "Question 2", 0.55);

        add_mapping(&conn, sid, "0xc1").unwrap();
        add_mapping(&conn, sid, "0xc2").unwrap();

        let raw = list_raw(&conn).unwrap();
        assert_eq!(raw.len(), 2);
    }
}
