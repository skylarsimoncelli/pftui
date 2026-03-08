use anyhow::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
pub struct GroupRow {
    pub name: String,
    pub created_at: String,
}

pub fn create_group(conn: &Connection, name: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO groups (name) VALUES (?1)
         ON CONFLICT(name) DO NOTHING",
        params![name],
    )?;
    Ok(())
}

pub fn remove_group(conn: &Connection, name: &str) -> Result<bool> {
    let changed = conn.execute("DELETE FROM groups WHERE name = ?1", params![name])?;
    Ok(changed > 0)
}

pub fn list_groups(conn: &Connection) -> Result<Vec<GroupRow>> {
    let mut stmt = conn.prepare("SELECT name, created_at FROM groups ORDER BY name ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok(GroupRow {
            name: row.get(0)?,
            created_at: row.get(1)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn set_group_members(conn: &Connection, group_name: &str, symbols: &[String]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO groups (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
        params![group_name],
    )?;
    tx.execute("DELETE FROM group_members WHERE group_name = ?1", params![group_name])?;
    for sym in symbols {
        tx.execute(
            "INSERT INTO group_members (group_name, symbol) VALUES (?1, ?2)",
            params![group_name, sym.to_uppercase()],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn get_group_members(conn: &Connection, group_name: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT symbol FROM group_members WHERE group_name = ?1 ORDER BY symbol ASC",
    )?;
    let rows = stmt.query_map(params![group_name], |row| row.get(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_set_list_remove_group() {
        let conn = crate::db::open_in_memory();
        create_group(&conn, "hard-assets").unwrap();
        set_group_members(
            &conn,
            "hard-assets",
            &["GC=F".to_string(), "SI=F".to_string(), "BTC".to_string()],
        )
        .unwrap();

        let members = get_group_members(&conn, "hard-assets").unwrap();
        assert_eq!(members.len(), 3);
        assert!(members.contains(&"GC=F".to_string()));

        let groups = list_groups(&conn).unwrap();
        assert!(groups.iter().any(|g| g.name == "hard-assets"));

        assert!(remove_group(&conn, "hard-assets").unwrap());
        assert!(get_group_members(&conn, "hard-assets").unwrap().is_empty());
    }
}
