use anyhow::Result;
use rusqlite::{params, Connection, Row as SqliteRow};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use crate::db::backend::BackendConnection;
use crate::db::query;

fn split_tags(tag_value: &str) -> Vec<String> {
    tag_value
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

fn aggregate_tags<I>(tag_values: I) -> Vec<(String, usize)>
where
    I: IntoIterator<Item = String>,
{
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for value in tag_values {
        for tag in split_tags(&value) {
            *counts.entry(tag).or_insert(0) += 1;
        }
    }

    let mut tags: Vec<(String, usize)> = counts.into_iter().collect();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    tags
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: i64,
    pub timestamp: String,
    pub content: String,
    pub tag: Option<String>,
    pub symbol: Option<String>,
    pub conviction: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewJournalEntry {
    pub timestamp: String,
    pub content: String,
    pub tag: Option<String>,
    pub symbol: Option<String>,
    pub conviction: Option<String>,
    pub status: String,
}

impl JournalEntry {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            content: row.get(2)?,
            tag: row.get(3)?,
            symbol: row.get(4)?,
            conviction: row.get(5)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
        })
    }
}

pub fn add_entry(conn: &Connection, entry: &NewJournalEntry) -> Result<i64> {
    conn.execute(
        "INSERT INTO journal (timestamp, content, tag, symbol, conviction, status)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![
            &entry.timestamp,
            &entry.content,
            &entry.tag,
            &entry.symbol,
            &entry.conviction,
            &entry.status,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn add_entry_backend(backend: &BackendConnection, entry: &NewJournalEntry) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_entry(conn, entry),
        |pool| add_entry_postgres(pool, entry),
    )
}

pub fn get_entry(conn: &Connection, id: i64) -> Result<Option<JournalEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at
         FROM journal WHERE id = ?",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(JournalEntry::from_row(row)?))
    } else {
        Ok(None)
    }
}

pub fn get_entry_backend(backend: &BackendConnection, id: i64) -> Result<Option<JournalEntry>> {
    query::dispatch(
        backend,
        |conn| get_entry(conn, id),
        |pool| get_entry_postgres(pool, id),
    )
}

pub fn list_entries(
    conn: &Connection,
    limit: Option<usize>,
    since: Option<&str>,
    tag: Option<&str>,
    symbol: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<JournalEntry>> {
    let mut query = String::from(
        "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at
         FROM journal WHERE 1=1",
    );

    if let Some(since_date) = since {
        query.push_str(&format!(" AND timestamp >= '{}'", since_date));
    }
    if let Some(tag_filter) = tag {
        let tags: Vec<&str> = tag_filter
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .collect();
        if !tags.is_empty() {
            let clauses = tags
                .iter()
                .map(|_| "(',' || COALESCE(tag, '') || ',') LIKE ?")
                .collect::<Vec<_>>()
                .join(" OR ");
            query.push_str(&format!(" AND ({})", clauses));
        }
    }
    if let Some(sym) = symbol {
        query.push_str(&format!(" AND symbol = '{}'", sym));
    }
    if let Some(st) = status {
        query.push_str(&format!(" AND status = '{}'", st));
    }

    query.push_str(" ORDER BY timestamp DESC");

    if let Some(lim) = limit {
        query.push_str(&format!(" LIMIT {}", lim));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = if let Some(tag_filter) = tag {
        let tag_patterns: Vec<String> = tag_filter
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(|tag| format!("%,{},%", tag))
            .collect();
        let params: Vec<&dyn rusqlite::ToSql> = tag_patterns
            .iter()
            .map(|pattern| pattern as &dyn rusqlite::ToSql)
            .collect();
        stmt.query_map(&params[..], JournalEntry::from_row)?
    } else {
        stmt.query_map([], JournalEntry::from_row)?
    };

    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn list_entries_backend(
    backend: &BackendConnection,
    limit: Option<usize>,
    since: Option<&str>,
    tag: Option<&str>,
    symbol: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<JournalEntry>> {
    query::dispatch(
        backend,
        |conn| list_entries(conn, limit, since, tag, symbol, status),
        |pool| list_entries_postgres(pool, limit, since, tag, symbol, status),
    )
}

pub fn search_entries(
    conn: &Connection,
    query: &str,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<JournalEntry>> {
    let mut sql = String::from(
        "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at
         FROM journal WHERE content LIKE ?",
    );

    if let Some(since_date) = since {
        sql.push_str(&format!(" AND timestamp >= '{}'", since_date));
    }

    sql.push_str(" ORDER BY timestamp DESC");

    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }

    let search_pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![search_pattern], JournalEntry::from_row)?;

    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn search_entries_backend(
    backend: &BackendConnection,
    query_text: &str,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<JournalEntry>> {
    query::dispatch(
        backend,
        |conn| search_entries(conn, query_text, since, limit),
        |pool| search_entries_postgres(pool, query_text, since, limit),
    )
}

pub fn update_entry(
    conn: &Connection,
    id: i64,
    content: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    if let Some(c) = content {
        conn.execute(
            "UPDATE journal SET content = ? WHERE id = ?",
            params![c, id],
        )?;
    }
    if let Some(s) = status {
        conn.execute("UPDATE journal SET status = ? WHERE id = ?", params![s, id])?;
    }
    Ok(())
}

pub fn update_entry_backend(
    backend: &BackendConnection,
    id: i64,
    content: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_entry(conn, id, content, status),
        |pool| update_entry_postgres(pool, id, content, status),
    )
}

pub fn remove_entry(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM journal WHERE id = ?", params![id])?;
    Ok(())
}

pub fn remove_entry_backend(backend: &BackendConnection, id: i64) -> Result<()> {
    query::dispatch(
        backend,
        |conn| remove_entry(conn, id),
        |pool| remove_entry_postgres(pool, id),
    )
}

pub fn get_all_tags(conn: &Connection) -> Result<Vec<(String, usize)>> {
    let mut stmt = conn.prepare("SELECT tag FROM journal WHERE tag IS NOT NULL")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    let mut tag_values = Vec::new();
    for tag in rows {
        tag_values.push(tag?);
    }
    Ok(aggregate_tags(tag_values))
}

pub fn get_all_tags_backend(backend: &BackendConnection) -> Result<Vec<(String, usize)>> {
    query::dispatch(backend, get_all_tags, get_all_tags_postgres)
}

#[derive(Debug, Serialize)]
pub struct JournalStats {
    pub total_entries: usize,
    pub entries_by_tag: Vec<(String, usize)>,
    pub entries_by_month: Vec<(String, usize)>,
}

pub fn get_stats(conn: &Connection) -> Result<JournalStats> {
    let total: usize = conn.query_row("SELECT COUNT(*) FROM journal", [], |row| row.get(0))?;

    let tags = get_all_tags(conn)?;

    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m', timestamp) as month, COUNT(*) as count
         FROM journal GROUP BY month ORDER BY month DESC",
    )?;
    let months = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let mut entries_by_month = Vec::new();
    for month in months {
        entries_by_month.push(month?);
    }

    Ok(JournalStats {
        total_entries: total,
        entries_by_tag: tags,
        entries_by_month,
    })
}

pub fn get_stats_backend(backend: &BackendConnection) -> Result<JournalStats> {
    query::dispatch(backend, get_stats, get_stats_postgres)
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS journal (
                id BIGSERIAL PRIMARY KEY,
                timestamp TEXT NOT NULL,
                content TEXT NOT NULL,
                tag TEXT,
                symbol TEXT,
                conviction TEXT,
                status TEXT NOT NULL DEFAULT 'open',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type JournalRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
);

fn to_journal_entry(r: JournalRow) -> JournalEntry {
    JournalEntry {
        id: r.0,
        timestamp: r.1,
        content: r.2,
        tag: r.3,
        symbol: r.4,
        conviction: r.5,
        status: r.6,
        created_at: r.7,
    }
}

fn add_entry_postgres(pool: &PgPool, entry: &NewJournalEntry) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO journal (timestamp, content, tag, symbol, conviction, status)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(&entry.timestamp)
        .bind(&entry.content)
        .bind(&entry.tag)
        .bind(&entry.symbol)
        .bind(&entry.conviction)
        .bind(&entry.status)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn get_entry_postgres(pool: &PgPool, id: i64) -> Result<Option<JournalEntry>> {
    ensure_tables_postgres(pool)?;
    let row: Option<JournalRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at::text
             FROM journal
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(to_journal_entry))
}

fn list_entries_postgres(
    pool: &PgPool,
    limit: Option<usize>,
    since: Option<&str>,
    tag: Option<&str>,
    symbol: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<JournalEntry>> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at::text
             FROM journal
             WHERE TRUE",
        );

        if let Some(since_date) = since {
            qb.push(" AND timestamp >= ").push_bind(since_date);
        }
        if let Some(sym) = symbol {
            qb.push(" AND symbol = ").push_bind(sym);
        }
        if let Some(st) = status {
            qb.push(" AND status = ").push_bind(st);
        }
        if let Some(tag_filter) = tag {
            let tags: Vec<&str> = tag_filter
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            if !tags.is_empty() {
                qb.push(" AND (");
                let mut separated = qb.separated(" OR ");
                for t in tags {
                    separated.push("(',' || COALESCE(tag, '') || ',') LIKE ");
                    separated.push_bind(format!("%,{},%", t));
                }
                qb.push(")");
            }
        }

        qb.push(" ORDER BY timestamp DESC");
        if let Some(limit) = limit {
            qb.push(" LIMIT ").push_bind(limit as i64);
        }

        qb.build().fetch_all(pool).await
    })?;

    rows.into_iter()
        .map(|row| {
            Ok(to_journal_entry((
                row.try_get(0)?,
                row.try_get(1)?,
                row.try_get(2)?,
                row.try_get(3)?,
                row.try_get(4)?,
                row.try_get(5)?,
                row.try_get(6)?,
                row.try_get(7)?,
            )))
        })
        .collect()
}

fn search_entries_postgres(
    pool: &PgPool,
    query_text: &str,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<JournalEntry>> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at::text
             FROM journal
             WHERE content ILIKE ",
        );
        qb.push_bind(format!("%{}%", query_text));
        if let Some(since_date) = since {
            qb.push(" AND timestamp >= ").push_bind(since_date);
        }
        qb.push(" ORDER BY timestamp DESC");
        if let Some(limit) = limit {
            qb.push(" LIMIT ").push_bind(limit as i64);
        }
        qb.build().fetch_all(pool).await
    })?;

    rows.into_iter()
        .map(|row| {
            Ok(to_journal_entry((
                row.try_get(0)?,
                row.try_get(1)?,
                row.try_get(2)?,
                row.try_get(3)?,
                row.try_get(4)?,
                row.try_get(5)?,
                row.try_get(6)?,
                row.try_get(7)?,
            )))
        })
        .collect()
}

fn update_entry_postgres(
    pool: &PgPool,
    id: i64,
    content: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        if let Some(content) = content {
            sqlx::query("UPDATE journal SET content = $1 WHERE id = $2")
                .bind(content)
                .bind(id)
                .execute(pool)
                .await?;
        }
        if let Some(status) = status {
            sqlx::query("UPDATE journal SET status = $1 WHERE id = $2")
                .bind(status)
                .bind(id)
                .execute(pool)
                .await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn remove_entry_postgres(pool: &PgPool, id: i64) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM journal WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_all_tags_postgres(pool: &PgPool) -> Result<Vec<(String, usize)>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<String> = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT tag FROM journal WHERE tag IS NOT NULL")
            .fetch_all(pool)
            .await
    })?;
    Ok(aggregate_tags(rows))
}

fn get_stats_postgres(pool: &PgPool) -> Result<JournalStats> {
    ensure_tables_postgres(pool)?;
    let total: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM journal")
            .fetch_one(pool)
            .await
    })?;
    let entries_by_tag = get_all_tags_postgres(pool)?;
    let months: Vec<(String, i64)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT TO_CHAR(date_trunc('month', timestamp::timestamptz), 'YYYY-MM') AS month, COUNT(*)::bigint
             FROM journal
             GROUP BY month
             ORDER BY month DESC",
        )
        .fetch_all(pool)
        .await
    })?;
    let entries_by_month = months
        .into_iter()
        .map(|(month, count)| (month, count as usize))
        .collect();

    Ok(JournalStats {
        total_entries: total as usize,
        entries_by_tag,
        entries_by_month,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_add_and_get_entry() {
        let conn = setup_test_db();
        let entry = NewJournalEntry {
            timestamp: "2026-03-04T20:00:00Z".to_string(),
            content: "Test entry".to_string(),
            tag: Some("test".to_string()),
            symbol: Some("GC=F".to_string()),
            conviction: Some("high".to_string()),
            status: "open".to_string(),
        };

        let id = add_entry(&conn, &entry).unwrap();
        let retrieved = get_entry(&conn, id).unwrap().unwrap();

        assert_eq!(retrieved.content, "Test entry");
        assert_eq!(retrieved.tag, Some("test".to_string()));
        assert_eq!(retrieved.symbol, Some("GC=F".to_string()));
        assert_eq!(retrieved.conviction, Some("high".to_string()));
        assert_eq!(retrieved.status, "open");
    }

    #[test]
    fn test_list_entries() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Entry 1".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Entry 2".to_string(),
                tag: Some("thesis".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let entries = list_entries(&conn, None, None, None, None, None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].content, "Entry 1"); // Most recent first
    }

    #[test]
    fn test_list_entries_with_tag_filter() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Trade entry".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Thesis entry".to_string(),
                tag: Some("thesis".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let entries = list_entries(&conn, None, None, Some("trade"), None, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Trade entry");
    }

    #[test]
    fn test_list_entries_with_comma_separated_tags() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Macro oil entry".to_string(),
                tag: Some("macro,oil".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let entries = list_entries(&conn, None, None, Some("oil"), None, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Macro oil entry");
    }

    #[test]
    fn test_search_entries() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Gold thesis confirmed".to_string(),
                tag: None,
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Bitcoin pump".to_string(),
                tag: None,
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let entries = search_entries(&conn, "gold", None, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Gold thesis confirmed");
    }

    #[test]
    fn test_update_entry() {
        let conn = setup_test_db();
        let id = add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Original".to_string(),
                tag: None,
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        update_entry(&conn, id, Some("Updated"), Some("validated")).unwrap();
        let entry = get_entry(&conn, id).unwrap().unwrap();
        assert_eq!(entry.content, "Updated");
        assert_eq!(entry.status, "validated");
    }

    #[test]
    fn test_remove_entry() {
        let conn = setup_test_db();
        let id = add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "To be deleted".to_string(),
                tag: None,
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        remove_entry(&conn, id).unwrap();
        let entry = get_entry(&conn, id).unwrap();
        assert!(entry.is_none());
    }

    #[test]
    fn test_get_all_tags() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Entry 1".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Entry 2".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-02T20:00:00Z".to_string(),
                content: "Entry 3".to_string(),
                tag: Some("thesis".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let tags = get_all_tags(&conn).unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], ("trade".to_string(), 2));
        assert_eq!(tags[1], ("thesis".to_string(), 1));
    }

    #[test]
    fn test_get_all_tags_splits_comma_separated_values() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Entry 1".to_string(),
                tag: Some("macro,oil".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Entry 2".to_string(),
                tag: Some("oil,geopolitical".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let tags = get_all_tags(&conn).unwrap();
        assert_eq!(tags[0], ("oil".to_string(), 2));
        assert!(tags.contains(&("macro".to_string(), 1)));
        assert!(tags.contains(&("geopolitical".to_string(), 1)));
    }

    #[test]
    fn test_get_stats() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Entry 1".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-02-15T20:00:00Z".to_string(),
                content: "Entry 2".to_string(),
                tag: Some("thesis".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let stats = get_stats(&conn).unwrap();
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.entries_by_tag.len(), 2);
        assert_eq!(stats.entries_by_month.len(), 2);
    }
}
