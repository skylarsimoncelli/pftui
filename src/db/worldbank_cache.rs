use crate::data::worldbank::WorldBankDataPoint;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Initialize World Bank cache table.
pub fn init_worldbank_cache(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS worldbank_cache (
            country_code TEXT NOT NULL,
            country_name TEXT NOT NULL,
            indicator_code TEXT NOT NULL,
            indicator_name TEXT NOT NULL,
            year INTEGER NOT NULL,
            value TEXT,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (country_code, indicator_code, year)
        )",
        [],
    )
    .context("Failed to create worldbank_cache table")?;

    // Index for querying by country + indicator
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_worldbank_country_indicator 
         ON worldbank_cache (country_code, indicator_code, year)",
        [],
    )
    .context("Failed to create worldbank_cache index")?;

    Ok(())
}

/// Insert or replace World Bank data points.
pub fn upsert_worldbank_data(conn: &Connection, data: &[WorldBankDataPoint]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO worldbank_cache 
         (country_code, country_name, indicator_code, indicator_name, year, value, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
    )?;

    for point in data {
        let value_str = point.value.map(|v| v.to_string());
        stmt.execute(params![
            point.country_code,
            point.country_name,
            point.indicator_code,
            point.indicator_name,
            point.year,
            value_str,
        ])?;
    }

    Ok(())
}

/// Get cached World Bank data for specific countries and indicator.
pub fn get_cached_worldbank_data(
    conn: &Connection,
    countries: &[&str],
    indicator: &str,
) -> Result<Vec<WorldBankDataPoint>> {
    let placeholders = countries
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");

    let query = format!(
        "SELECT country_code, country_name, indicator_code, indicator_name, year, value
         FROM worldbank_cache
         WHERE country_code IN ({}) AND indicator_code = ?
         ORDER BY country_code, year DESC",
        placeholders
    );

    let mut stmt = conn.prepare(&query)?;

    let mut params: Vec<&dyn rusqlite::ToSql> = countries
        .iter()
        .map(|c| c as &dyn rusqlite::ToSql)
        .collect();
    params.push(&indicator);

    let rows = stmt.query_map(params.as_slice(), |row| {
        let value_str: Option<String> = row.get(5)?;
        let value = value_str.and_then(|s| Decimal::from_str(&s).ok());

        Ok(WorldBankDataPoint {
            country_code: row.get(0)?,
            country_name: row.get(1)?,
            indicator_code: row.get(2)?,
            indicator_name: row.get(3)?,
            year: row.get(4)?,
            value,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }

    Ok(result)
}

/// Get all cached World Bank data (all countries, all indicators).
pub fn get_all_cached_worldbank_data(conn: &Connection) -> Result<Vec<WorldBankDataPoint>> {
    let mut stmt = conn.prepare(
        "SELECT country_code, country_name, indicator_code, indicator_name, year, value
         FROM worldbank_cache
         ORDER BY country_code, indicator_code, year DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        let value_str: Option<String> = row.get(5)?;
        let value = value_str.and_then(|s| Decimal::from_str(&s).ok());

        Ok(WorldBankDataPoint {
            country_code: row.get(0)?,
            country_name: row.get(1)?,
            indicator_code: row.get(2)?,
            indicator_name: row.get(3)?,
            year: row.get(4)?,
            value,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }

    Ok(result)
}

/// Check if cache needs refresh (empty or older than 30 days).
pub fn needs_refresh(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM worldbank_cache 
         WHERE updated_at > datetime('now', '-30 days')",
        [],
        |row| row.get(0),
    )?;

    Ok(count == 0)
}

/// Get latest indicators for all tracked countries (most recent year per country/indicator).
pub fn get_latest_indicators(conn: &Connection) -> Result<Vec<WorldBankDataPoint>> {
    let mut stmt = conn.prepare(
        "SELECT wb.country_code, wb.country_name, wb.indicator_code, wb.indicator_name, wb.year, wb.value
         FROM worldbank_cache wb
         INNER JOIN (
             SELECT country_code, indicator_code, MAX(year) as max_year
             FROM worldbank_cache
             GROUP BY country_code, indicator_code
         ) latest
         ON wb.country_code = latest.country_code 
         AND wb.indicator_code = latest.indicator_code 
         AND wb.year = latest.max_year
         ORDER BY wb.country_code, wb.indicator_code",
    )?;

    let rows = stmt.query_map([], |row| {
        let value_str: Option<String> = row.get(5)?;
        let value = value_str.and_then(|s| Decimal::from_str(&s).ok());

        Ok(WorldBankDataPoint {
            country_code: row.get(0)?,
            country_name: row.get(1)?,
            indicator_code: row.get(2)?,
            indicator_name: row.get(3)?,
            year: row.get(4)?,
            value,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::worldbank::{COUNTRY_US, INDICATOR_GDP_GROWTH};

    #[test]
    fn test_worldbank_cache_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        init_worldbank_cache(&conn).unwrap();

        let data = vec![WorldBankDataPoint {
            country_code: COUNTRY_US.to_string(),
            country_name: "United States".to_string(),
            indicator_code: INDICATOR_GDP_GROWTH.to_string(),
            indicator_name: "GDP growth (annual %)".to_string(),
            year: 2023,
            value: Some(Decimal::new(25, 1)), // 2.5
        }];

        upsert_worldbank_data(&conn, &data).unwrap();

        let cached = get_cached_worldbank_data(&conn, &[COUNTRY_US], INDICATOR_GDP_GROWTH).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].country_code, COUNTRY_US);
        assert_eq!(cached[0].year, 2023);
    }

    #[test]
    fn test_needs_refresh() {
        let conn = Connection::open_in_memory().unwrap();
        init_worldbank_cache(&conn).unwrap();

        // Empty cache should need refresh
        assert!(needs_refresh(&conn).unwrap());

        // Add some data
        let data = vec![WorldBankDataPoint {
            country_code: COUNTRY_US.to_string(),
            country_name: "United States".to_string(),
            indicator_code: INDICATOR_GDP_GROWTH.to_string(),
            indicator_name: "GDP growth (annual %)".to_string(),
            year: 2023,
            value: Some(Decimal::new(25, 1)),
        }];
        upsert_worldbank_data(&conn, &data).unwrap();

        // Now should not need refresh
        assert!(!needs_refresh(&conn).unwrap());
    }
}
