use anyhow::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WatchlistGroup {
    pub id: i64,
    pub name: String,
}

pub fn list_groups(conn: &Connection) -> Result<Vec<WatchlistGroup>> {
    let mut stmt = conn.prepare("SELECT id, name FROM watchlist_groups ORDER BY id ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok(WatchlistGroup {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    })?;
    let mut groups = Vec::new();
    for row in rows {
        groups.push(row?);
    }
    Ok(groups)
}

pub fn get_group_name(conn: &Connection, group_id: i64) -> Result<Option<String>> {
    let gid = clamp_group_id(group_id);
    let mut stmt = conn.prepare("SELECT name FROM watchlist_groups WHERE id = ?1")?;
    let mut rows = stmt.query(params![gid])?;
    if let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        Ok(Some(name))
    } else {
        Ok(None)
    }
}

pub fn clamp_group_id(group_id: i64) -> i64 {
    group_id.clamp(1, 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn lists_default_groups() {
        let conn = open_in_memory();
        let groups = list_groups(&conn).unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].id, 1);
    }

    #[test]
    fn reads_group_name() {
        let conn = open_in_memory();
        let name = get_group_name(&conn, 2).unwrap();
        assert_eq!(name.as_deref(), Some("Opportunistic"));
    }
}
