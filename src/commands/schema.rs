use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub default_value: Option<String>,
    pub primary_key_position: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct IndexSpec {
    name: String,
    table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sql: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TableSpec {
    name: String,
    columns: Vec<ColumnSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sql: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SchemaSpec {
    tables: BTreeMap<String, TableSpec>,
    indexes: BTreeMap<String, IndexSpec>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaIssue {
    MissingTable {
        table: String,
    },
    ExtraTable {
        table: String,
    },
    MissingColumn {
        table: String,
        column: String,
        expected: ColumnSpec,
    },
    ExtraColumn {
        table: String,
        column: String,
    },
    ColumnMismatch {
        table: String,
        column: String,
        field: String,
        expected: String,
        actual: String,
    },
    MissingIndex {
        index: String,
        table: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaReport {
    pub status: String,
    pub backend: String,
    pub target: String,
    pub expected_tables: usize,
    pub actual_tables: usize,
    pub issues: Vec<SchemaIssue>,
}

#[derive(Debug, Clone, Serialize)]
struct RepairPlan {
    status: String,
    target: String,
    repairable: Vec<String>,
    non_repairable: Vec<String>,
    dry_run: bool,
}

pub fn run_verify_sqlite(path: &Path, json: bool) -> Result<()> {
    let conn = open_raw_sqlite(path)?;
    let report = verify_sqlite_connection(&conn, &path.display().to_string())?;
    print_verify_report(&report, json)?;
    if report.issues.is_empty() {
        Ok(())
    } else {
        bail!(
            "schema verification failed: {} issue(s). Run `pftui system schema repair --dry-run`.",
            report.issues.len()
        )
    }
}

pub fn run_repair_sqlite(path: &Path, dry_run: bool, confirm: bool, json: bool) -> Result<()> {
    let conn = open_raw_sqlite(path)?;
    let report = verify_sqlite_connection(&conn, &path.display().to_string())?;
    let expected = expected_schema()?;
    let plan = build_repair_plan(&report, &expected)?;

    if plan.repairable.is_empty() && plan.non_repairable.is_empty() {
        let clean = RepairPlan {
            status: "clean".to_string(),
            target: path.display().to_string(),
            repairable: Vec::new(),
            non_repairable: Vec::new(),
            dry_run,
        };
        print_repair_plan(&clean, json)?;
        return Ok(());
    }

    let output_plan = RepairPlan {
        status: if dry_run {
            "dry_run".to_string()
        } else if !plan.non_repairable.is_empty() {
            "blocked".to_string()
        } else if !confirm {
            "confirm_required".to_string()
        } else {
            "repaired".to_string()
        },
        target: path.display().to_string(),
        repairable: plan.repairable.clone(),
        non_repairable: plan.non_repairable.clone(),
        dry_run,
    };
    print_repair_plan(&output_plan, json)?;

    if !plan.non_repairable.is_empty() {
        bail!(
            "schema repair blocked: {} non-repairable issue(s) require manual migration",
            plan.non_repairable.len()
        );
    }
    if dry_run {
        return Ok(());
    }
    if !confirm {
        bail!(
            "schema repair requires --confirm to execute {} statement(s)",
            plan.repairable.len()
        );
    }

    for sql in &plan.repairable {
        conn.execute_batch(sql)
            .with_context(|| format!("failed to execute repair statement: {sql}"))?;
    }
    Ok(())
}

fn open_raw_sqlite(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

pub fn verify_sqlite_connection(conn: &Connection, target: &str) -> Result<SchemaReport> {
    let expected = expected_schema()?;
    let actual = introspect_schema(conn)?;
    let mut issues = Vec::new();

    for table in expected.tables.keys() {
        if !actual.tables.contains_key(table) {
            issues.push(SchemaIssue::MissingTable {
                table: table.clone(),
            });
        }
    }
    for table in actual.tables.keys() {
        if !expected.tables.contains_key(table) {
            issues.push(SchemaIssue::ExtraTable {
                table: table.clone(),
            });
        }
    }

    for (table_name, expected_table) in &expected.tables {
        let Some(actual_table) = actual.tables.get(table_name) else {
            continue;
        };
        let expected_cols = columns_by_name(&expected_table.columns);
        let actual_cols = columns_by_name(&actual_table.columns);

        for (column_name, expected_column) in &expected_cols {
            let Some(actual_column) = actual_cols.get(column_name) else {
                issues.push(SchemaIssue::MissingColumn {
                    table: table_name.clone(),
                    column: column_name.to_string(),
                    expected: (*expected_column).clone(),
                });
                continue;
            };
            compare_column(table_name, expected_column, actual_column, &mut issues);
        }
        for column_name in actual_cols.keys() {
            if !expected_cols.contains_key(column_name) {
                issues.push(SchemaIssue::ExtraColumn {
                    table: table_name.clone(),
                    column: (*column_name).to_string(),
                });
            }
        }
    }

    for (index_name, expected_index) in &expected.indexes {
        if !actual.indexes.contains_key(index_name) {
            issues.push(SchemaIssue::MissingIndex {
                index: index_name.clone(),
                table: expected_index.table.clone(),
            });
        }
    }

    Ok(SchemaReport {
        status: if issues.is_empty() { "ok" } else { "drifted" }.to_string(),
        backend: "sqlite".to_string(),
        target: target.to_string(),
        expected_tables: expected.tables.len(),
        actual_tables: actual.tables.len(),
        issues,
    })
}

fn expected_schema() -> Result<SchemaSpec> {
    let conn = Connection::open_in_memory()?;
    crate::db::schema::run_migrations(&conn)?;
    introspect_schema(&conn)
}

fn introspect_schema(conn: &Connection) -> Result<SchemaSpec> {
    let mut tables = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT name, sql
         FROM sqlite_master
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    )?;
    let table_rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    for (name, sql) in table_rows {
        let columns = table_columns(conn, &name)?;
        tables.insert(name.clone(), TableSpec { name, columns, sql });
    }

    let mut indexes = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT name, tbl_name, sql
         FROM sqlite_master
         WHERE type = 'index'
           AND name NOT LIKE 'sqlite_autoindex_%'
         ORDER BY name",
    )?;
    let index_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    for (name, table, sql) in index_rows {
        indexes.insert(
            name.clone(),
            IndexSpec {
                name,
                table,
                sql,
            },
        );
    }

    Ok(SchemaSpec { tables, indexes })
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<ColumnSpec>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote_ident(table)))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ColumnSpec {
                name: row.get(1)?,
                data_type: row.get::<_, String>(2)?,
                not_null: row.get::<_, i64>(3)? != 0,
                default_value: row.get(4)?,
                primary_key_position: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn columns_by_name(columns: &[ColumnSpec]) -> BTreeMap<&str, &ColumnSpec> {
    columns.iter().map(|c| (c.name.as_str(), c)).collect()
}

fn compare_column(
    table: &str,
    expected: &ColumnSpec,
    actual: &ColumnSpec,
    issues: &mut Vec<SchemaIssue>,
) {
    if normalize_type(&expected.data_type) != normalize_type(&actual.data_type) {
        issues.push(SchemaIssue::ColumnMismatch {
            table: table.to_string(),
            column: expected.name.clone(),
            field: "type".to_string(),
            expected: expected.data_type.clone(),
            actual: actual.data_type.clone(),
        });
    }
    if expected.not_null != actual.not_null {
        issues.push(SchemaIssue::ColumnMismatch {
            table: table.to_string(),
            column: expected.name.clone(),
            field: "not_null".to_string(),
            expected: expected.not_null.to_string(),
            actual: actual.not_null.to_string(),
        });
    }
    if normalize_default(expected.default_value.as_deref())
        != normalize_default(actual.default_value.as_deref())
    {
        issues.push(SchemaIssue::ColumnMismatch {
            table: table.to_string(),
            column: expected.name.clone(),
            field: "default".to_string(),
            expected: expected.default_value.clone().unwrap_or_default(),
            actual: actual.default_value.clone().unwrap_or_default(),
        });
    }
    if expected.primary_key_position != actual.primary_key_position {
        issues.push(SchemaIssue::ColumnMismatch {
            table: table.to_string(),
            column: expected.name.clone(),
            field: "primary_key_position".to_string(),
            expected: expected.primary_key_position.to_string(),
            actual: actual.primary_key_position.to_string(),
        });
    }
}

fn build_repair_plan(report: &SchemaReport, expected: &SchemaSpec) -> Result<RepairPlan> {
    let mut repairable = Vec::new();
    let mut non_repairable = Vec::new();
    let mut seen = BTreeSet::new();

    for issue in &report.issues {
        match issue {
            SchemaIssue::MissingTable { table } => {
                if let Some(sql) = expected.tables.get(table).and_then(|t| t.sql.as_ref()) {
                    push_once(&mut repairable, &mut seen, sql.clone());
                } else {
                    non_repairable
                        .push(format!("missing table {table}: no CREATE TABLE SQL found"));
                }
            }
            SchemaIssue::MissingColumn {
                table,
                column,
                expected: column_spec,
            } => {
                if column_spec.primary_key_position != 0 {
                    non_repairable.push(format!(
                        "missing primary-key column {table}.{column}: table rebuild required"
                    ));
                    continue;
                }
                let table_sql = expected
                    .tables
                    .get(table)
                    .and_then(|t| t.sql.as_deref())
                    .ok_or_else(|| anyhow!("missing expected CREATE TABLE SQL for {table}"))?;
                let definition = column_definition_from_create_sql(table_sql, column)
                    .unwrap_or_else(|| fallback_column_definition(column_spec));
                push_once(
                    &mut repairable,
                    &mut seen,
                    format!(
                        "ALTER TABLE {} ADD COLUMN {};",
                        quote_ident(table),
                        definition
                    ),
                );
            }
            SchemaIssue::MissingIndex { index, .. } => {
                if let Some(sql) = expected.indexes.get(index).and_then(|i| i.sql.as_ref()) {
                    push_once(&mut repairable, &mut seen, sql.clone());
                } else {
                    non_repairable
                        .push(format!("missing index {index}: no CREATE INDEX SQL found"));
                }
            }
            SchemaIssue::ExtraTable { table } => {
                non_repairable.push(format!("extra table {table}: refusing destructive repair"));
            }
            SchemaIssue::ExtraColumn { table, column } => {
                non_repairable.push(format!(
                    "extra column {table}.{column}: refusing destructive repair"
                ));
            }
            SchemaIssue::ColumnMismatch {
                table,
                column,
                field,
                expected,
                actual,
            } => {
                non_repairable.push(format!(
                    "column mismatch {table}.{column} {field}: expected {expected:?}, got {actual:?}"
                ));
            }
        }
    }

    Ok(RepairPlan {
        status: if repairable.is_empty() && non_repairable.is_empty() {
            "clean".to_string()
        } else {
            "planned".to_string()
        },
        target: report.target.clone(),
        repairable,
        non_repairable,
        dry_run: true,
    })
}

fn push_once(values: &mut Vec<String>, seen: &mut BTreeSet<String>, value: String) {
    if seen.insert(value.clone()) {
        values.push(value);
    }
}

fn print_verify_report(report: &SchemaReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }
    println!("Schema: {}", report.status);
    println!("Backend: {}", report.backend);
    println!("Target: {}", report.target);
    println!(
        "Tables: {}/{}",
        report.actual_tables, report.expected_tables
    );
    if report.issues.is_empty() {
        println!("No schema drift detected.");
        return Ok(());
    }
    println!();
    println!("Issues:");
    for issue in &report.issues {
        println!("  - {}", describe_issue(issue));
    }
    Ok(())
}

fn print_repair_plan(plan: &RepairPlan, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(plan)?);
        return Ok(());
    }
    println!("Schema repair: {}", plan.status);
    println!("Target: {}", plan.target);
    if !plan.repairable.is_empty() {
        println!();
        println!("Repairable statements:");
        for sql in &plan.repairable {
            println!("  {sql}");
        }
    }
    if !plan.non_repairable.is_empty() {
        println!();
        println!("Manual repair required:");
        for issue in &plan.non_repairable {
            println!("  - {issue}");
        }
    }
    if plan.status == "confirm_required" {
        println!();
        println!("Re-run with `--confirm` to apply these schema repairs.");
    }
    Ok(())
}

fn describe_issue(issue: &SchemaIssue) -> String {
    match issue {
        SchemaIssue::MissingTable { table } => format!("missing table {table}"),
        SchemaIssue::ExtraTable { table } => format!("extra table {table}"),
        SchemaIssue::MissingColumn { table, column, .. } => {
            format!("missing column {table}.{column}")
        }
        SchemaIssue::ExtraColumn { table, column } => format!("extra column {table}.{column}"),
        SchemaIssue::ColumnMismatch {
            table,
            column,
            field,
            expected,
            actual,
        } => format!("{table}.{column} {field} mismatch: expected {expected:?}, got {actual:?}"),
        SchemaIssue::MissingIndex { index, table } => format!("missing index {index} on {table}"),
    }
}

fn column_definition_from_create_sql(create_sql: &str, column: &str) -> Option<String> {
    let open = create_sql.find('(')?;
    let close = create_sql.rfind(')')?;
    let body = &create_sql[open + 1..close];
    split_top_level_commas(body).into_iter().find_map(|part| {
        let trimmed = part.trim();
        let name = first_sql_token(trimmed)?;
        if unquote_ident(name).eq_ignore_ascii_case(column) {
            Some(trimmed.to_string())
        } else {
            None
        }
    })
}

fn split_top_level_commas(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    for ch in input.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double && depth > 0 => depth -= 1,
            ',' if !in_single && !in_double && depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn first_sql_token(input: &str) -> Option<&str> {
    input.split_whitespace().next()
}

fn fallback_column_definition(column: &ColumnSpec) -> String {
    let mut definition = quote_ident(&column.name);
    if !column.data_type.trim().is_empty() {
        definition.push(' ');
        definition.push_str(column.data_type.trim());
    }
    if column.not_null {
        definition.push_str(" NOT NULL");
    }
    if let Some(default) = &column.default_value {
        definition.push_str(" DEFAULT ");
        definition.push_str(default);
    }
    definition
}

fn normalize_type(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn normalize_default(value: Option<&str>) -> Option<String> {
    value.map(|v| v.trim().trim_matches('\'').trim_matches('"').to_string())
}

fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn unquote_ident(ident: &str) -> &str {
    ident
        .trim_matches('"')
        .trim_matches('`')
        .trim_matches('[')
        .trim_matches(']')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migrated_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn verify_clean_migrated_schema_has_no_issues() {
        let conn = migrated_conn();
        let report = verify_sqlite_connection(&conn, ":memory:").unwrap();
        assert_eq!(report.status, "ok");
        assert!(report.issues.is_empty());
    }

    #[test]
    fn verify_detects_missing_columns() {
        let conn = migrated_conn();
        conn.execute_batch(
            "ALTER TABLE alerts RENAME TO alerts_old;
             CREATE TABLE alerts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL DEFAULT 'price',
                symbol TEXT NOT NULL,
                direction TEXT NOT NULL,
                threshold TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'armed',
                rule_text TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                triggered_at TEXT
             );
             DROP TABLE alerts_old;",
        )
        .unwrap();

        let report = verify_sqlite_connection(&conn, ":memory:").unwrap();
        assert!(report.issues.iter().any(|issue| matches!(
            issue,
            SchemaIssue::MissingColumn { table, column, .. }
                if table == "alerts" && column == "condition"
        )));
        assert!(report.issues.iter().any(|issue| matches!(
            issue,
            SchemaIssue::MissingColumn { table, column, .. }
                if table == "alerts" && column == "cooldown_minutes"
        )));
    }

    #[test]
    fn repair_plan_adds_missing_columns_without_mutating() {
        let conn = migrated_conn();
        conn.execute_batch(
            "ALTER TABLE alerts RENAME TO alerts_old;
             CREATE TABLE alerts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL DEFAULT 'price',
                symbol TEXT NOT NULL,
                direction TEXT NOT NULL,
                threshold TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'armed',
                rule_text TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                triggered_at TEXT
             );
             DROP TABLE alerts_old;",
        )
        .unwrap();

        let report = verify_sqlite_connection(&conn, ":memory:").unwrap();
        let expected = expected_schema().unwrap();
        let plan = build_repair_plan(&report, &expected).unwrap();
        assert!(plan.non_repairable.is_empty());
        assert!(plan
            .repairable
            .iter()
            .any(|sql| sql.contains("ALTER TABLE \"alerts\" ADD COLUMN condition TEXT")));
        assert!(plan.repairable.iter().any(|sql| sql.contains(
            "ALTER TABLE \"alerts\" ADD COLUMN cooldown_minutes INTEGER NOT NULL DEFAULT 0"
        )));
    }

    #[test]
    fn column_definition_parser_handles_nested_defaults() {
        let sql = "CREATE TABLE sample (id INTEGER PRIMARY KEY, created_at TEXT DEFAULT (datetime('now')), notes TEXT)";
        assert_eq!(
            column_definition_from_create_sql(sql, "created_at").as_deref(),
            Some("created_at TEXT DEFAULT (datetime('now'))")
        );
    }
}
