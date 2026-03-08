use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::{Config, DatabaseBackend};

#[derive(Debug)]
pub enum BackendConnection {
    Sqlite(Connection),
    Postgres { _pool: PgPool },
}

impl BackendConnection {
    pub fn require_sqlite(self) -> Result<Connection> {
        match self {
            BackendConnection::Sqlite(conn) => Ok(conn),
            BackendConnection::Postgres { .. } => bail!(
                "PostgreSQL backend is configured but the query layer is still SQLite-only. \
                 PostgreSQL storage is in progress (TODO Phase 2). Set `database_backend = \"sqlite\"` to continue."
            ),
        }
    }
}

pub fn open_from_config(config: &Config, sqlite_path: &Path) -> Result<BackendConnection> {
    match config.database_backend {
        DatabaseBackend::Sqlite => {
            let conn = super::open_db(sqlite_path)?;
            Ok(BackendConnection::Sqlite(conn))
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
            let pool = PgPoolOptions::new()
                .max_connections(5)
                .connect_lazy(url)
                .context("Failed to create PostgreSQL pool from database_url")?;
            Ok(BackendConnection::Postgres { _pool: pool })
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
            BackendConnection::Sqlite(_) => {}
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
}
