use anyhow::{bail, Result};
use rusqlite::Connection;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;

pub fn dispatch<T, FSqlite, FPostgres>(
    backend: &BackendConnection,
    sqlite_fn: FSqlite,
    postgres_fn: FPostgres,
) -> Result<T>
where
    FSqlite: FnOnce(&Connection) -> Result<T>,
    FPostgres: FnOnce(&PgPool) -> Result<T>,
{
    if let Some(conn) = backend.sqlite_native() {
        return sqlite_fn(conn);
    }
    if let Some(pool) = backend.postgres_pool() {
        return postgres_fn(pool);
    }
    bail!("Unsupported database backend state")
}
