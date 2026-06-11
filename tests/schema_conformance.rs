//! Architecture-as-a-test: every table the code creates must be classified
//! in `docs/db-catalog.toml` with a valid layer (L0..L4 or DEAD).
//!
//! Two enumeration paths, both enforced:
//!   1. A fresh in-memory DB created by `db::schema::run_migrations` —
//!      catches every table the migration path creates.
//!   2. A regex scan of `CREATE TABLE` statements across `src/**/*.rs`
//!      (test modules excluded) — catches lazily-created tables that
//!      migrations don't touch (e.g. `analyst_views`, `mirror_sync_state`).
//!
//! If this test fails because you added a table: read
//! `docs/DATA-ARCHITECTURE.md`, pick the layer the table belongs to, and add
//! a `[tables.<name>]` entry to `docs/db-catalog.toml` with `layer`,
//! `purpose`, `writers`, `readers`, and the layer's required property
//! (freshness_sla_hours for L0/L1, rebuildable for L2, append_only for L3
//! ledgers). Do NOT weaken this test.
//!
//! TOML parsing uses the `toml` crate already in the dependency tree.
//! pftui has no lib target, so path 1 drives the built binary in an
//! isolated HOME (same pattern as tests/prior_release_schema.rs) and reads
//! the table list from `system db-info --json`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const VALID_LAYERS: &[&str] = &["L0", "L1", "L2", "L3", "L4", "DEAD"];

/// Tables that exist only transiently inside a single migration batch
/// (created, filled, renamed away). They never persist in any DB.
const TRANSIENT_TABLES: &[&str] = &["calibration_matrix_canonical_rebuild"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_catalog() -> BTreeMap<String, toml::Value> {
    let path = repo_root().join("docs/db-catalog.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let doc: toml::Value = text
        .parse()
        .unwrap_or_else(|e| panic!("docs/db-catalog.toml is not valid TOML: {e}"));
    let tables = doc
        .get("tables")
        .and_then(|t| t.as_table())
        .expect("docs/db-catalog.toml must have a [tables.*] section");
    tables
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Tables created by the migration path: run the binary against a fresh
/// isolated HOME (forcing a from-scratch migrated DB) and read the table
/// list from `system db-info --json`.
fn migrated_tables() -> BTreeSet<String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "pftui-schema-conformance-{}-{unique}",
        std::process::id()
    ));
    let home = root.join("home");
    let xdg_config = root.join("xdg_config");
    let xdg_data = root.join("xdg_data");
    fs::create_dir_all(xdg_config.join("pftui")).expect("create XDG config dir");
    fs::write(xdg_config.join("pftui/config.toml"), "").expect("write XDG config");
    fs::create_dir_all(home.join("Library/Application Support/pftui"))
        .expect("create home config dir");
    fs::write(
        home.join("Library/Application Support/pftui/config.toml"),
        "",
    )
    .expect("write home config");

    let output = Command::new(env!("CARGO_BIN_EXE_pftui"))
        .args(["--cached-only", "system", "db-info", "--json"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run pftui system db-info");
    assert!(
        output.status.success(),
        "system db-info failed on a fresh DB:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_str(
        String::from_utf8_lossy(&output.stdout).trim(),
    )
    .expect("system db-info --json did not emit valid JSON");
    let tables = value
        .get("tables")
        .and_then(|t| t.as_array())
        .expect("db-info JSON missing tables array");
    let names = tables
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .filter(|n| !n.starts_with("sqlite_"))
        .map(|n| n.to_string())
        .collect();
    let _ = fs::remove_dir_all(&root);
    names
}

/// Tables named in CREATE TABLE statements anywhere under src/,
/// with test modules excluded (content truncated at `#[cfg(test)]` /
/// `mod tests {`) so test fixtures don't count.
fn source_created_tables() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let src = repo_root().join("src");
    walk_rs(&src, &mut |content| {
        scan_creates(content, &mut out);
    });
    for t in TRANSIENT_TABLES {
        out.remove(*t);
    }
    out
}

fn walk_rs(dir: &Path, f: &mut dyn FnMut(&str)) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, f);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(mut content) = std::fs::read_to_string(&path) {
                for marker in ["#[cfg(test)]", "\nmod tests {"] {
                    if let Some(idx) = content.find(marker) {
                        content.truncate(idx);
                    }
                }
                f(&content);
            }
        }
    }
}

/// Minimal scanner for `CREATE TABLE [IF NOT EXISTS] <name> (` — the paren
/// requirement keeps prose in comments from matching.
fn scan_creates(content: &str, out: &mut BTreeSet<String>) {
    let upper = content.to_uppercase();
    let mut from = 0usize;
    while let Some(pos) = upper[from..].find("CREATE TABLE") {
        let abs = from + pos + "CREATE TABLE".len();
        from = abs;
        let rest = &content[abs..];
        let rest = rest.trim_start();
        let rest = if rest.len() >= 13 && rest[..13].eq_ignore_ascii_case("IF NOT EXISTS") {
            rest[13..].trim_start()
        } else {
            rest
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        let after = rest[name.len()..].trim_start();
        if after.starts_with('(') {
            out.insert(name);
        }
    }
}

fn assert_catalogued(tables: &BTreeSet<String>, origin: &str) {
    let catalog = load_catalog();
    let mut missing = Vec::new();
    for t in tables {
        if !catalog.contains_key(t) {
            missing.push(t.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "\n\nARCHITECTURE VIOLATION: {origin} table(s) with no docs/db-catalog.toml entry:\n  {}\n\n\
         Every table must be classified before it ships. Read docs/DATA-ARCHITECTURE.md,\n\
         choose a layer (L0 ingest / L1 canonical series / L2 derived / L3 ledger /\n\
         L4 knowledge), and add a [tables.<name>] entry with layer, purpose, writers,\n\
         readers, a named consumer, and the layer's required property.\n",
        missing.join("\n  ")
    );
}

#[test]
fn every_migrated_table_has_a_catalog_entry() {
    assert_catalogued(&migrated_tables(), "freshly migrated");
}

#[test]
fn every_source_created_table_has_a_catalog_entry() {
    let tables = source_created_tables();
    assert!(
        tables.len() >= 100,
        "CREATE TABLE scan found only {} tables — scanner is likely broken",
        tables.len()
    );
    assert_catalogued(&tables, "code-created (CREATE TABLE in src/)");
}

#[test]
fn catalog_entries_are_well_formed() {
    let catalog = load_catalog();
    assert!(
        catalog.len() >= 100,
        "catalog has only {} entries — file truncated?",
        catalog.len()
    );
    for (name, entry) in &catalog {
        let layer = entry
            .get("layer")
            .and_then(|l| l.as_str())
            .unwrap_or_else(|| panic!("[tables.{name}] missing string `layer`"));
        assert!(
            VALID_LAYERS.contains(&layer),
            "[tables.{name}] has invalid layer {layer:?}; must be one of {VALID_LAYERS:?} \
             (see docs/DATA-ARCHITECTURE.md)"
        );
        let purpose = entry
            .get("purpose")
            .and_then(|p| p.as_str())
            .unwrap_or_else(|| panic!("[tables.{name}] missing string `purpose`"));
        assert!(
            !purpose.trim().is_empty(),
            "[tables.{name}] has empty purpose"
        );
        for key in ["writers", "readers"] {
            assert!(
                entry.get(key).map(|v| v.is_array()).unwrap_or(false),
                "[tables.{name}] missing array `{key}`"
            );
        }
        if layer == "L2" {
            assert_eq!(
                entry.get("rebuildable").and_then(|v| v.as_bool()),
                Some(true),
                "[tables.{name}] is L2 — it must declare `rebuildable = true` \
                 (L2 is a cache, not state; see docs/DATA-ARCHITECTURE.md)"
            );
        }
    }
}

#[test]
fn no_catalog_entry_for_unknown_table() {
    // The reverse direction: catalog entries must correspond to a table the
    // code actually creates, so the catalog can't drift into fiction.
    let known: BTreeSet<String> = migrated_tables()
        .union(&source_created_tables())
        .cloned()
        .collect();
    let catalog = load_catalog();
    let stale: Vec<_> = catalog.keys().filter(|k| !known.contains(*k)).collect();
    assert!(
        stale.is_empty(),
        "catalog entries with no CREATE TABLE in src/ (remove or fix): {stale:?}"
    );
}
