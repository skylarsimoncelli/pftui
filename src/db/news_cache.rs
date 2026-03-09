//! SQLite cache for RSS news items.
//!
//! Stores news items with 48-hour retention.
//! Deduplicates by URL.
//! Query by source, category, search term, or time range.

use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct NewsEntry {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub source: String,
    pub source_type: String,
    pub symbol_tag: Option<String>,
    pub description: String,
    pub extra_snippets: Vec<String>,
    pub category: String,
    pub published_at: i64,
    pub fetched_at: String,
}

/// Insert a news item into the cache.
///
/// Deduplicates by URL (ignores duplicates).
pub fn insert_news(
    conn: &Connection,
    title: &str,
    url: &str,
    source: &str,
    category: &str,
    published_at: i64,
) -> Result<()> {
    insert_news_with_source_type(
        conn,
        title,
        url,
        source,
        "rss",
        None,
        category,
        published_at,
        None,
        &[],
    )
}

pub fn insert_news_backend(
    backend: &BackendConnection,
    title: &str,
    url: &str,
    source: &str,
    category: &str,
    published_at: i64,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| insert_news(conn, title, url, source, category, published_at),
        |pool| insert_news_postgres(pool, title, url, source, category, published_at),
    )
}

/// Insert a news item with an explicit source type ("rss" or "brave").
#[allow(clippy::too_many_arguments)]
pub fn insert_news_with_source_type(
    conn: &Connection,
    title: &str,
    url: &str,
    source: &str,
    source_type: &str,
    symbol_tag: Option<&str>,
    category: &str,
    published_at: i64,
    description: Option<&str>,
    extra_snippets: &[String],
) -> Result<()> {
    let snippets_json = serde_json::to_string(extra_snippets).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT OR IGNORE INTO news_cache
         (title, url, source, source_type, symbol_tag, description, extra_snippets, category, published_at, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
        params![
            title,
            url,
            source,
            source_type,
            symbol_tag,
            description.unwrap_or(""),
            snippets_json,
            category,
            published_at
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_news_with_source_type_backend(
    backend: &BackendConnection,
    title: &str,
    url: &str,
    source: &str,
    source_type: &str,
    symbol_tag: Option<&str>,
    category: &str,
    published_at: i64,
    description: Option<&str>,
    extra_snippets: &[String],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| {
            insert_news_with_source_type(
                conn,
                title,
                url,
                source,
                source_type,
                symbol_tag,
                category,
                published_at,
                description,
                extra_snippets,
            )
        },
        |pool| {
            insert_news_with_source_type_postgres(
                pool,
                title,
                url,
                source,
                source_type,
                symbol_tag,
                category,
                published_at,
                description,
                extra_snippets,
            )
        },
    )
}

/// Get latest N news items, optionally filtered.
///
/// Filters can be combined (AND logic).
pub fn get_latest_news(
    conn: &Connection,
    limit: usize,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
    search_term: Option<&str>,
    hours_back: Option<i64>,
) -> Result<Vec<NewsEntry>> {
    let mut sql = "SELECT id, title, url, source, source_type, symbol_tag, description, extra_snippets, category, published_at, fetched_at
                   FROM news_cache
                   WHERE 1=1".to_string();

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(source) = source_filter {
        sql.push_str(" AND source = ?");
        params_vec.push(Box::new(source.to_string()));
    }

    if let Some(category) = category_filter {
        sql.push_str(" AND category = ?");
        params_vec.push(Box::new(category.to_string()));
    }

    if let Some(term) = search_term {
        sql.push_str(" AND title LIKE ?");
        params_vec.push(Box::new(format!("%{}%", term)));
    }

    if let Some(hours) = hours_back {
        sql.push_str(" AND published_at > ?");
        let cutoff = chrono::Utc::now().timestamp() - (hours * 3600);
        params_vec.push(Box::new(cutoff));
    }

    sql.push_str(" ORDER BY published_at DESC LIMIT ?");
    params_vec.push(Box::new(limit as i64));

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(NewsEntry {
            id: row.get(0)?,
            title: row.get(1)?,
            url: row.get(2)?,
            source: row.get(3)?,
            source_type: row.get(4)?,
            symbol_tag: row.get(5)?,
            description: row.get(6)?,
            extra_snippets: serde_json::from_str::<Vec<String>>(&row.get::<_, String>(7)?)
                .unwrap_or_default(),
            category: row.get(8)?,
            published_at: row.get(9)?,
            fetched_at: row.get(10)?,
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }

    Ok(entries)
}

pub fn get_latest_news_backend(
    backend: &BackendConnection,
    limit: usize,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
    search_term: Option<&str>,
    hours_back: Option<i64>,
) -> Result<Vec<NewsEntry>> {
    query::dispatch(
        backend,
        |conn| get_latest_news(conn, limit, source_filter, category_filter, search_term, hours_back),
        |pool| {
            get_latest_news_postgres(
                pool,
                limit,
                source_filter,
                category_filter,
                search_term,
                hours_back,
            )
        },
    )
}

/// Delete news older than 48 hours.
pub fn cleanup_old_news(conn: &Connection) -> Result<usize> {
    let cutoff = chrono::Utc::now().timestamp() - (48 * 3600);
    let deleted = conn.execute(
        "DELETE FROM news_cache WHERE published_at < ?1",
        params![cutoff],
    )?;
    Ok(deleted)
}

pub fn cleanup_old_news_backend(backend: &BackendConnection) -> Result<usize> {
    query::dispatch(backend, cleanup_old_news, cleanup_old_news_postgres)
}

/// Get unique sources currently in cache.
pub fn get_sources(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT source FROM news_cache ORDER BY source")?;
    let rows = stmt.query_map([], |row| row.get(0))?;

    let mut sources = Vec::new();
    for row in rows {
        sources.push(row?);
    }

    Ok(sources)
}

pub fn get_sources_backend(backend: &BackendConnection) -> Result<Vec<String>> {
    query::dispatch(backend, get_sources, get_sources_postgres)
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS news_cache (
                id BIGSERIAL PRIMARY KEY,
                title TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                source TEXT NOT NULL,
                source_type TEXT NOT NULL DEFAULT 'rss',
                symbol_tag TEXT,
                description TEXT NOT NULL DEFAULT '',
                extra_snippets TEXT NOT NULL DEFAULT '[]',
                category TEXT NOT NULL DEFAULT 'general',
                published_at BIGINT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn insert_news_postgres(
    pool: &PgPool,
    title: &str,
    url: &str,
    source: &str,
    category: &str,
    published_at: i64,
) -> Result<()> {
    insert_news_with_source_type_postgres(
        pool,
        title,
        url,
        source,
        "rss",
        None,
        category,
        published_at,
        None,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_news_with_source_type_postgres(
    pool: &PgPool,
    title: &str,
    url: &str,
    source: &str,
    source_type: &str,
    symbol_tag: Option<&str>,
    category: &str,
    published_at: i64,
    description: Option<&str>,
    extra_snippets: &[String],
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let snippets_json = serde_json::to_string(extra_snippets).unwrap_or_else(|_| "[]".to_string());
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO news_cache
             (title, url, source, source_type, symbol_tag, description, extra_snippets, category, published_at, fetched_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
             ON CONFLICT (url) DO NOTHING",
        )
        .bind(title)
        .bind(url)
        .bind(source)
        .bind(source_type)
        .bind(symbol_tag)
        .bind(description.unwrap_or(""))
        .bind(snippets_json)
        .bind(category)
        .bind(published_at)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_latest_news_postgres(
    pool: &PgPool,
    limit: usize,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
    search_term: Option<&str>,
    hours_back: Option<i64>,
) -> Result<Vec<NewsEntry>> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT id, title, url, source, source_type, symbol_tag, description, extra_snippets, category, published_at, fetched_at::text
             FROM news_cache
             WHERE TRUE",
        );

        if let Some(source) = source_filter {
            qb.push(" AND source = ").push_bind(source);
        }
        if let Some(category) = category_filter {
            qb.push(" AND category = ").push_bind(category);
        }
        if let Some(term) = search_term {
            qb.push(" AND title ILIKE ").push_bind(format!("%{}%", term));
        }
        if let Some(hours) = hours_back {
            let cutoff = chrono::Utc::now().timestamp() - (hours * 3600);
            qb.push(" AND published_at > ").push_bind(cutoff);
        }

        qb.push(" ORDER BY published_at DESC LIMIT ").push_bind(limit as i64);
        qb.build().fetch_all(pool).await
    })?;

    rows.into_iter()
        .map(|row| {
            let snippets_json: String = row.try_get(7)?;
            Ok(NewsEntry {
                id: row.try_get(0)?,
                title: row.try_get(1)?,
                url: row.try_get(2)?,
                source: row.try_get(3)?,
                source_type: row.try_get(4)?,
                symbol_tag: row.try_get(5)?,
                description: row.try_get(6)?,
                extra_snippets: serde_json::from_str::<Vec<String>>(&snippets_json).unwrap_or_default(),
                category: row.try_get(8)?,
                published_at: row.try_get(9)?,
                fetched_at: row.try_get(10)?,
            })
        })
        .collect()
}

fn cleanup_old_news_postgres(pool: &PgPool) -> Result<usize> {
    ensure_tables_postgres(pool)?;
    let cutoff = chrono::Utc::now().timestamp() - (48 * 3600);
    let result = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM news_cache WHERE published_at < $1")
            .bind(cutoff)
            .execute(pool)
            .await
    })?;
    Ok(result.rows_affected() as usize)
}

fn get_sources_postgres(pool: &PgPool) -> Result<Vec<String>> {
    ensure_tables_postgres(pool)?;
    let values = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT source
             FROM news_cache
             ORDER BY source",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::run_migrations;

    #[test]
    fn test_insert_and_query_news() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news(
            &conn,
            "Bitcoin hits $100k",
            "https://example.com/btc-100k",
            "CoinDesk",
            "crypto",
            1709610000,
        )
        .unwrap();

        let items = get_latest_news(&conn, 10, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Bitcoin hits $100k");
        assert_eq!(items[0].source, "CoinDesk");
    }

    #[test]
    fn test_deduplication() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news(
            &conn,
            "News 1",
            "https://example.com/a",
            "Reuters",
            "macro",
            1709610000,
        )
        .unwrap();

        // Try inserting same URL again
        insert_news(
            &conn,
            "News 1 (duplicate)",
            "https://example.com/a",
            "Reuters",
            "macro",
            1709610001,
        )
        .unwrap();

        let items = get_latest_news(&conn, 10, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1); // Only one entry, duplicate ignored
    }

    #[test]
    fn test_filter_by_source() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news(
            &conn,
            "Reuters headline",
            "https://example.com/r1",
            "Reuters",
            "macro",
            1709610000,
        )
        .unwrap();
        insert_news(
            &conn,
            "CoinDesk headline",
            "https://example.com/c1",
            "CoinDesk",
            "crypto",
            1709610000,
        )
        .unwrap();

        let items = get_latest_news(&conn, 10, Some("CoinDesk"), None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, "CoinDesk");
    }

    #[test]
    fn test_search_term() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news(
            &conn,
            "Bitcoin rally continues",
            "https://example.com/btc",
            "CoinDesk",
            "crypto",
            1709610000,
        )
        .unwrap();
        insert_news(
            &conn,
            "Gold prices drop",
            "https://example.com/gold",
            "Reuters",
            "commodities",
            1709610000,
        )
        .unwrap();

        let items = get_latest_news(&conn, 10, None, None, Some("Bitcoin"), None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Bitcoin rally continues");
    }

    #[test]
    fn test_cleanup_old_news() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let now = chrono::Utc::now().timestamp();
        let three_days_ago = now - (3 * 24 * 3600);

        insert_news(
            &conn,
            "Old news",
            "https://example.com/old",
            "Reuters",
            "macro",
            three_days_ago,
        )
        .unwrap();

        insert_news(
            &conn,
            "Fresh news",
            "https://example.com/fresh",
            "Reuters",
            "macro",
            now,
        )
        .unwrap();

        let deleted = cleanup_old_news(&conn).unwrap();
        assert_eq!(deleted, 1);

        let items = get_latest_news(&conn, 10, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Fresh news");
    }
}
