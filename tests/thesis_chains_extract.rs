//! Integration coverage for `pftui analytics thesis-chains extract`.
//!
//! Uses a fully isolated XDG_DATA_HOME so no read or write touches the real
//! local pftui DB. The fixture inserts synthetic thesis / lesson rows whose
//! free-form text matches the heuristic extractor's regex patterns, then
//! asserts:
//!   1. `extract --dry-run --json` returns the expected count + chains.
//!   2. `extract --apply --json` writes the chains.
//!   3. A second `extract --apply` run is idempotent (all chains dedupe).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde_json::Value;

#[test]
fn extract_dry_run_then_apply_is_idempotent() {
    let root = TestRoot::new();
    prepare_isolated_home(&root);
    seed_synthetic_db(&root);

    // 1. Dry-run: no writes, but proposes chains.
    let dry = run_json(
        &root,
        &[
            "--cached-only",
            "analytics",
            "thesis-chains",
            "extract",
            "--from-thesis",
            "--from-lessons",
            "--dry-run",
            "--json",
        ],
        "extract dry-run",
    );
    let proposed = dry["proposed"].as_u64().expect("proposed count present");
    assert!(
        proposed >= 2,
        "expected at least 2 proposed chains, got {proposed}: {dry}"
    );
    assert_eq!(
        dry["applied"].as_u64(),
        Some(0),
        "dry-run must not apply: {dry}"
    );

    // 2. Apply: writes the chains.
    let applied = run_json(
        &root,
        &[
            "--cached-only",
            "analytics",
            "thesis-chains",
            "extract",
            "--from-thesis",
            "--from-lessons",
            "--apply",
            "--json",
        ],
        "extract apply (1st)",
    );
    let applied_count = applied["applied"].as_u64().expect("applied count");
    assert_eq!(
        applied_count, proposed,
        "first apply should write every proposed chain: {applied}"
    );

    // 3. Re-apply: should dedupe everything.
    let reapplied = run_json(
        &root,
        &[
            "--cached-only",
            "analytics",
            "thesis-chains",
            "extract",
            "--from-thesis",
            "--from-lessons",
            "--apply",
            "--json",
        ],
        "extract apply (2nd)",
    );
    assert_eq!(
        reapplied["applied"].as_u64(),
        Some(0),
        "second apply should write nothing: {reapplied}"
    );
    assert_eq!(
        reapplied["deduped"].as_u64(),
        Some(proposed),
        "second apply should dedupe every proposed chain: {reapplied}"
    );
}

fn seed_synthetic_db(root: &TestRoot) {
    // Touch the DB once via a real CLI invocation so migrations run, then
    // open it and inject synthetic source rows.
    let _ = run_pftui(root, &["--cached-only", "system", "db-info", "--json"]);

    let db_path = active_db_path(root);
    let conn = Connection::open(&db_path)
        .unwrap_or_else(|e| panic!("failed to open synthetic db {}: {e}", db_path.display()));

    // Synthetic thesis section (no portfolio data — pure heuristic fixture).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS thesis (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            section TEXT NOT NULL UNIQUE,
            content TEXT NOT NULL,
            conviction TEXT NOT NULL DEFAULT 'medium',
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO thesis (section, content, conviction)
         VALUES ('synthetic-macro-2026', ?1, 'medium')",
        params!["If real yields back off, gold should grind higher. A weaker dollar implies stronger gold."],
    )
    .unwrap();

    // Synthetic prediction_lessons row. The FK on prediction_id points at
    // user_predictions(id); seed a synthetic parent prediction first.
    conn.execute(
        "INSERT INTO user_predictions
            (claim, conviction, created_at, outcome)
         VALUES ('synthetic claim', 'medium', datetime('now'), 'wrong')",
        [],
    )
    .ok();
    let pid: i64 = conn
        .query_row("SELECT id FROM user_predictions ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
        .unwrap_or(1);
    conn.execute(
        "INSERT INTO prediction_lessons
         (prediction_id, miss_type, what_predicted, what_happened, why_wrong)
         VALUES (?1, 'directional', 'fake', 'fake',
                 'Higher real yields dampens gold demand.')",
        params![pid],
    )
    .unwrap();
}

fn run_json(root: &TestRoot, args: &[&str], label: &str) -> Value {
    let output = run_pftui(root, args);
    assert_success(label, &output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "{label} produced empty stdout"
    );
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|err| panic!("{label} did not emit valid JSON: {err}\nstdout:\n{stdout}"))
}

fn run_pftui(root: &TestRoot, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pftui"))
        .args(args)
        .env("HOME", root.home())
        .env("XDG_CONFIG_HOME", root.xdg_config())
        .env("XDG_DATA_HOME", root.xdg_data())
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|err| panic!("failed to run pftui {args:?}: {err}"))
}

fn assert_success(label: &str, output: &Output) {
    if output.status.success() {
        return;
    }
    panic!(
        "{label} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn prepare_isolated_home(root: &TestRoot) {
    fs::create_dir_all(root.xdg_config().join("pftui")).expect("create XDG config");
    fs::write(root.xdg_config().join("pftui/config.toml"), "").expect("write XDG config");
    fs::create_dir_all(root.home_config().join("pftui")).expect("create home config");
    fs::write(root.home_config().join("pftui/config.toml"), "").expect("write home config");
}

fn active_db_path(root: &TestRoot) -> PathBuf {
    if cfg!(target_os = "macos") {
        root.home_db_path()
    } else {
        root.xdg_db_path()
    }
}

struct TestRoot {
    root: PathBuf,
}

impl TestRoot {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pftui-thesis-chains-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create test root");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn xdg_config(&self) -> PathBuf {
        self.root.join("xdg_config")
    }

    fn xdg_data(&self) -> PathBuf {
        self.root.join("xdg_data")
    }

    fn home_config(&self) -> PathBuf {
        self.home().join("Library/Application Support")
    }

    fn xdg_db_path(&self) -> PathBuf {
        self.xdg_data().join("pftui/portfolios/default.db")
    }

    fn home_db_path(&self) -> PathBuf {
        self.home()
            .join("Library/Application Support/pftui/portfolios/default.db")
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
