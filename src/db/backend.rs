use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::{Config, DatabaseBackend};

#[derive(Debug)]
pub enum BackendConnection {
    Sqlite {
        conn: Connection,
    },
    Postgres {
        pool: PgPool,
    },
}

impl BackendConnection {
    #[allow(dead_code)]
    pub fn sqlite(&self) -> &Connection {
        match self {
            BackendConnection::Sqlite { conn } => conn,
            BackendConnection::Postgres { .. } => {
                panic!("sqlite() is unavailable when database_backend=postgres")
            }
        }
    }

    pub fn flush(&self) -> Result<()> {
        Ok(())
    }

    pub fn sqlite_native(&self) -> Option<&Connection> {
        match self {
            BackendConnection::Sqlite { conn } => Some(conn),
            BackendConnection::Postgres { .. } => None,
        }
    }

    pub fn postgres_pool(&self) -> Option<&PgPool> {
        match self {
            BackendConnection::Sqlite { .. } => None,
            BackendConnection::Postgres { pool } => Some(pool),
        }
    }
}

pub fn open_from_config(config: &Config, sqlite_path: &Path) -> Result<BackendConnection> {
    match config.database_backend {
        DatabaseBackend::Sqlite => {
            let conn = super::open_db(sqlite_path)?;
            Ok(BackendConnection::Sqlite { conn })
        }
        DatabaseBackend::Postgres => {
            let url = config
                .database_url
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "database_backend is set to postgres but database_url is not set"
                    )
                })?;
            let runtime = tokio::runtime::Runtime::new()
                .context("Failed to create Tokio runtime for PostgreSQL backend")?;
            let pool = runtime
                .block_on(async { PgPoolOptions::new().max_connections(5).connect(url).await })
                .context("Failed to connect to PostgreSQL using database_url")?;
            crate::db::postgres_schema::run_migrations(&pool)
                .context("Failed to run PostgreSQL schema migrations")?;
            let _ = sqlite_path; // retained for signature parity/callsites
            Ok(BackendConnection::Postgres { pool })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_backend_opens_sqlite_conn() {
        let cfg = Config::default();
        let path = std::path::PathBuf::from("/tmp/pftui_backend_test.db");
        let result = open_from_config(&cfg, &path).unwrap();
        match result {
            BackendConnection::Sqlite { .. } => {}
            BackendConnection::Postgres { .. } => panic!("expected sqlite backend"),
        }
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn postgres_backend_requires_url() {
        let cfg = Config {
            database_backend: DatabaseBackend::Postgres,
            database_url: None,
            ..Default::default()
        };
        let err = open_from_config(&cfg, Path::new("/tmp/unused.db"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("database_url is not set"));
    }

    #[test]
    fn sqlite_backend_flush_noop() {
        let cfg = Config::default();
        let path = std::path::PathBuf::from("/tmp/pftui_backend_flush_noop.db");
        let result = open_from_config(&cfg, &path).unwrap();
        result.flush().unwrap();
        std::fs::remove_file(path).ok();
    }
}
