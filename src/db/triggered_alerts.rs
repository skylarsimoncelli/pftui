use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggeredAlert {
    pub id: i64,
    pub alert_id: i64,
    pub triggered_at: String,
    pub trigger_data: String,
    pub acknowledged: bool,
}

impl TriggeredAlert {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            alert_id: row.get(1)?,
            triggered_at: row.get(2)?,
            trigger_data: row.get(3)?,
            acknowledged: row.get::<_, i64>(4)? != 0,
        })
    }
}

pub fn add_triggered_alert(
    conn: &Connection,
    alert_id: i64,
    triggered_at: &str,
    trigger_data: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO triggered_alerts (alert_id, triggered_at, trigger_data, acknowledged)
         VALUES (?1, ?2, ?3, 0)",
        params![alert_id, triggered_at, trigger_data],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_triggered_alerts(
    conn: &Connection,
    since_hours: Option<i64>,
    only_unacknowledged: bool,
) -> Result<Vec<TriggeredAlert>> {
    let mut query = String::from(
        "SELECT id, alert_id, triggered_at, trigger_data, acknowledged
         FROM triggered_alerts
         WHERE 1=1",
    );

    if let Some(hours) = since_hours {
        query.push_str(&format!(
            " AND triggered_at >= datetime('now', '-{} hours')",
            hours.max(0)
        ));
    }
    if only_unacknowledged {
        query.push_str(" AND acknowledged = 0");
    }
    query.push_str(" ORDER BY triggered_at DESC");

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], TriggeredAlert::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn acknowledge_for_alert(conn: &Connection, alert_id: i64) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE triggered_alerts SET acknowledged = 1 WHERE alert_id = ?1",
        params![alert_id],
    )?)
}

pub fn add_triggered_alert_backend(
    backend: &BackendConnection,
    alert_id: i64,
    triggered_at: &str,
    trigger_data: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_triggered_alert(conn, alert_id, triggered_at, trigger_data),
        |pool| add_triggered_alert_postgres(pool, alert_id, triggered_at, trigger_data),
    )
}

pub fn list_triggered_alerts_backend(
    backend: &BackendConnection,
    since_hours: Option<i64>,
    only_unacknowledged: bool,
) -> Result<Vec<TriggeredAlert>> {
    query::dispatch(
        backend,
        |conn| list_triggered_alerts(conn, since_hours, only_unacknowledged),
        |pool| list_triggered_alerts_postgres(pool, since_hours, only_unacknowledged),
    )
}

pub fn acknowledge_for_alert_backend(backend: &BackendConnection, alert_id: i64) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| acknowledge_for_alert(conn, alert_id),
        |pool| acknowledge_for_alert_postgres(pool, alert_id),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS triggered_alerts (
                id BIGSERIAL PRIMARY KEY,
                alert_id BIGINT NOT NULL,
                triggered_at TIMESTAMPTZ NOT NULL,
                trigger_data TEXT NOT NULL DEFAULT '{}',
                acknowledged BOOLEAN NOT NULL DEFAULT FALSE
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_triggered_alerts_triggered_at
             ON triggered_alerts(triggered_at)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_triggered_alert_postgres(
    pool: &PgPool,
    alert_id: i64,
    triggered_at: &str,
    trigger_data: &str,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    Ok(crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO triggered_alerts (alert_id, triggered_at, trigger_data, acknowledged)
             VALUES ($1, $2::timestamptz, $3, FALSE)
             RETURNING id",
        )
        .bind(alert_id)
        .bind(triggered_at)
        .bind(trigger_data)
        .fetch_one(pool)
        .await
    })?)
}

type TriggeredAlertRow = (i64, i64, String, String, bool);

fn list_triggered_alerts_postgres(
    pool: &PgPool,
    since_hours: Option<i64>,
    only_unacknowledged: bool,
) -> Result<Vec<TriggeredAlert>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<TriggeredAlertRow> = crate::db::pg_runtime::block_on(async {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT id, alert_id, triggered_at::text, trigger_data, acknowledged
             FROM triggered_alerts
             WHERE 1=1",
        );
        if let Some(hours) = since_hours {
            qb.push(" AND triggered_at >= NOW() - (")
                .push_bind(hours.max(0))
                .push(" * INTERVAL '1 hour')");
        }
        if only_unacknowledged {
            qb.push(" AND acknowledged = FALSE");
        }
        qb.push(" ORDER BY triggered_at DESC");
        qb.build_query_as().fetch_all(pool).await
    })?;
    Ok(rows
        .into_iter()
        .map(|row| TriggeredAlert {
            id: row.0,
            alert_id: row.1,
            triggered_at: row.2,
            trigger_data: row.3,
            acknowledged: row.4,
        })
        .collect())
}

fn acknowledge_for_alert_postgres(pool: &PgPool, alert_id: i64) -> Result<usize> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query("UPDATE triggered_alerts SET acknowledged = TRUE WHERE alert_id = $1")
            .bind(alert_id)
            .execute(pool)
            .await
    })?;
    Ok(rows.rows_affected() as usize)
}
