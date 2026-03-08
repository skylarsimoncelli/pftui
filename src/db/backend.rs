use std::path::{Path, PathBuf};

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
    Postgres(PostgresSqliteBridge),
}

impl BackendConnection {
    pub fn sqlite(&self) -> &Connection {
        match self {
            BackendConnection::Sqlite { conn } => conn,
            BackendConnection::Postgres(bridge) => &bridge.conn,
        }
    }

    pub fn flush(&self) -> Result<()> {
        match self {
            BackendConnection::Sqlite { .. } => Ok(()),
            BackendConnection::Postgres(bridge) => bridge.flush(),
        }
    }

    pub fn sqlite_native(&self) -> Option<&Connection> {
        match self {
            BackendConnection::Sqlite { conn } => Some(conn),
            BackendConnection::Postgres(_) => None,
        }
    }

    pub fn postgres_pool(&self) -> Option<&PgPool> {
        match self {
            BackendConnection::Sqlite { .. } => None,
            BackendConnection::Postgres(bridge) => Some(&bridge.pool),
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
            let bridge = PostgresSqliteBridge::new(url, sqlite_path)?;
            Ok(BackendConnection::Postgres(bridge))
        }
    }
}

#[derive(Debug)]
pub struct PostgresSqliteBridge {
    conn: Connection,
    pool: PgPool,
    sqlite_path: PathBuf,
    state_key: String,
}

impl PostgresSqliteBridge {
    fn new(url: &str, sqlite_path: &Path) -> Result<Self> {
        if let Some(parent) = sqlite_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let runtime = tokio::runtime::Runtime::new()
            .context("Failed to create Tokio runtime for PostgreSQL backend")?;
        let pool = runtime
            .block_on(async { PgPoolOptions::new().max_connections(5).connect(url).await })
            .context("Failed to connect to PostgreSQL using database_url")?;

        runtime
            .block_on(async {
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS pftui_sqlite_state (
                        state_key TEXT PRIMARY KEY,
                        sqlite_blob BYTEA NOT NULL,
                        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                    )",
                )
                .execute(&pool)
                .await
            })
            .context("Failed to initialize PostgreSQL state table")?;

        let state_key = sqlite_path.to_string_lossy().to_string();
        let sqlite_path_buf = sqlite_path.to_path_buf();

        let maybe_blob: Option<Vec<u8>> = runtime
            .block_on(async {
                sqlx::query_scalar("SELECT sqlite_blob FROM pftui_sqlite_state WHERE state_key = $1")
                    .bind(&state_key)
                    .fetch_optional(&pool)
                    .await
            })
            .context("Failed to load SQLite state from PostgreSQL")?;

        if let Some(blob) = maybe_blob {
            std::fs::write(&sqlite_path_buf, &blob).with_context(|| {
                format!(
                    "Failed to write hydrated SQLite state to {}",
                    sqlite_path_buf.display()
                )
            })?;
        }

        let conn = super::open_db(&sqlite_path_buf)?;
        let bridge = Self {
            conn,
            pool,
            sqlite_path: sqlite_path_buf,
            state_key,
        };

        // Ensure PostgreSQL always has a baseline snapshot after first open.
        bridge.flush()?;
        Ok(bridge)
    }

    fn flush(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .context("Failed to checkpoint SQLite WAL before PostgreSQL sync")?;
        let db_bytes = std::fs::read(&self.sqlite_path).with_context(|| {
            format!(
                "Failed to read SQLite working database at {} for PostgreSQL sync",
                self.sqlite_path.display()
            )
        })?;

        let runtime = tokio::runtime::Runtime::new()
            .context("Failed to create Tokio runtime for PostgreSQL sync")?;
        runtime
            .block_on(async {
                sqlx::query(
                    "INSERT INTO pftui_sqlite_state (state_key, sqlite_blob, updated_at)
                     VALUES ($1, $2, NOW())
                     ON CONFLICT (state_key)
                     DO UPDATE SET sqlite_blob = EXCLUDED.sqlite_blob, updated_at = NOW()",
                )
                .bind(&self.state_key)
                .bind(&db_bytes)
                .execute(&self.pool)
                .await
            })
            .context("Failed to persist SQLite state snapshot into PostgreSQL")?;
        Ok(())
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
            BackendConnection::Postgres(_) => panic!("expected sqlite backend"),
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
