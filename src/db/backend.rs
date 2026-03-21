use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

use crate::config::{Config, DatabaseBackend};

#[derive(Debug)]
pub enum BackendConnection {
    Sqlite { conn: Connection },
    Postgres { pool: PgPool },
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

    /// Create a lightweight copy of the connection for the mobile API server.
    /// `PgPool` is `Clone` (internally `Arc`-wrapped), so this is cheap.
    /// For SQLite this is unsupported since the mobile API requires Postgres.
    pub fn clone_for_server(&self) -> Result<Self> {
        match self {
            BackendConnection::Postgres { pool } => {
                Ok(BackendConnection::Postgres { pool: pool.clone() })
            }
            BackendConnection::Sqlite { .. } => {
                anyhow::bail!("Mobile API server requires the Postgres backend")
            }
        }
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
            let max_connections = config.effective_postgres_max_connections();
            let connect_timeout =
                Duration::from_secs(config.effective_postgres_connect_timeout_secs());
            let pool = crate::db::pg_runtime::block_on(async {
                PgPoolOptions::new()
                    .max_connections(max_connections)
                    .acquire_timeout(connect_timeout)
                    .connect(url)
                    .await
            })
            .context("Failed to connect to PostgreSQL using database_url")?;
            if !config.effective_postgres_read_only() {
                crate::db::postgres_schema::run_migrations(&pool)
                    .context("Failed to run PostgreSQL schema migrations")?;
            }
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
