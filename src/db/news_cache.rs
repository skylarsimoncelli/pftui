//! SQLite cache for RSS news items.
//!
//! Stores news items with 48-hour retention.
//! Deduplicates by URL.
//! Query by source, category, search term, or time range.

use anyhow::Result;
use rusqlite::{params, Connection};

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

/// Insert a news item with an explicit source type ("rss" or "brave").
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

/// Delete news older than 48 hours.
pub fn cleanup_old_news(conn: &Connection) -> Result<usize> {
    let cutoff = chrono::Utc::now().timestamp() - (48 * 3600);
    let deleted = conn.execute(
        "DELETE FROM news_cache WHERE published_at < ?1",
        params![cutoff],
    )?;
    Ok(deleted)
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
