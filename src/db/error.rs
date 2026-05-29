use anyhow::anyhow;

pub fn with_schema_repair_hint(err: anyhow::Error) -> anyhow::Error {
    let message = err.to_string();
    if let Some(column) = missing_column(&message) {
        anyhow!(
            "Schema migration appears incomplete: column `{}` is missing.\n\n\
             This usually means pftui was upgraded but a migration was skipped or only partly applied.\n\
             Run `pftui system schema verify` to see the drift, then `pftui system schema repair --dry-run` and `pftui system schema repair --confirm` to repair safe missing-table, missing-column, and missing-index drift.\n\n\
             Original error: {}",
            column,
            message
        )
    } else {
        err
    }
}

fn missing_column(message: &str) -> Option<String> {
    let marker = "no such column:";
    let start = message.find(marker)? + marker.len();
    let tail = message[start..].trim();
    let column = tail
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .next()?
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    if column.is_empty() {
        None
    } else {
        Some(column.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_hint_wraps_missing_column_errors() {
        let err = anyhow!("no such column: source_domain in CREATE INDEX");
        let wrapped = with_schema_repair_hint(err).to_string();
        assert!(wrapped.contains("Schema migration appears incomplete"));
        assert!(wrapped.contains("system schema verify"));
        assert!(wrapped.contains("source_domain"));
    }
}
