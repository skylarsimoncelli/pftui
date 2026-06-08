use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde_json::Value;

const FIXTURE: &str = "tests/fixtures/db/v0.27.0.sqlite";

#[test]
fn prior_release_db_migrates_and_supports_representative_cli_smoke() {
    let root = TestRoot::new();
    prepare_isolated_home(&root);
    copy_fixture_to_default_db(&root);

    let db_info = run_json(
        &root,
        &["--cached-only", "system", "db-info", "--json"],
        "system db-info",
    );
    assert_current_tables_present(&db_info);
    assert_migration_columns_present(&active_db_path(&root));

    for (label, args) in [
        (
            "portfolio status",
            &["--cached-only", "portfolio", "status", "--json"][..],
        ),
        (
            "data status",
            &["--cached-only", "data", "status", "--json"][..],
        ),
        (
            "analytics situation",
            &["--cached-only", "analytics", "situation", "--json"][..],
        ),
        (
            "analytics calibration --by-layer",
            &[
                "--cached-only",
                "analytics",
                "calibration",
                "--by-layer",
                "--json",
            ][..],
        ),
        (
            "report chart stacked-bar",
            &[
                "--cached-only",
                "report",
                "chart",
                "stacked-bar",
                "--from-db",
                "portfolio",
                "--format",
                "ascii",
                "--json",
            ][..],
        ),
    ] {
        let value = run_json(&root, args, label);
        assert!(
            !value.is_null(),
            "{label} returned empty JSON after prior-release migration"
        );
    }
}

fn run_json(root: &TestRoot, args: &[&str], label: &str) -> Value {
    let output = run_pftui(root, args);
    assert_success(label, &output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "{label} produced empty stdout after prior-release migration"
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

fn assert_current_tables_present(db_info: &Value) {
    let tables = db_info
        .get("tables")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("db-info JSON is missing tables array:\n{db_info:#}"));
    let names: BTreeSet<&str> = tables
        .iter()
        .filter_map(|table| table.get("name").and_then(Value::as_str))
        .collect();

    for table in [
        "transactions",
        "price_cache",
        "news_cache",
        "user_predictions",
        "news_source_tiers",
        "news_source_accuracy",
        "news_source_accuracy_events",
        "rss_feed_health",
        "technical_snapshots",
        "scenario_contract_mappings",
        "narrative_money_history",
        "news_silence_baselines",
        "prediction_lessons",
        "lesson_citations",
    ] {
        assert!(
            names.contains(table),
            "migrated prior-release DB is missing current table {table}; tables={names:?}"
        );
    }
}

fn assert_migration_columns_present(db_path: &Path) {
    let conn = Connection::open(db_path)
        .unwrap_or_else(|err| panic!("failed to reopen migrated DB {}: {err}", db_path.display()));

    assert_columns(
        &conn,
        "news_cache",
        &[
            "source_type",
            "symbol_tag",
            "source_domain",
            "source_tier",
            "source_independence",
            "description",
            "extra_snippets",
            "topic",
        ],
    );
    assert_columns(
        &conn,
        "user_predictions",
        &[
            "topic",
            "source_article_id",
            "resolution_criteria",
            "lessons_applied",
        ],
    );
    assert_columns(
        &conn,
        "prediction_lessons",
        &["status", "last_cited_at"],
    );
    // `calibration_matrix` may pre-date `conviction_band` (and other analytic
    // columns) on legacy DBs; the self-healing migration must add them so
    // `analytics calibration-matrix rebuild` can INSERT.
    assert_columns(
        &conn,
        "calibration_matrix",
        &[
            "layer",
            "topic",
            "conviction_band",
            "n",
            "hit_rate",
            "stated_confidence",
            "recorded_at",
        ],
    );
    assert_indexes(
        &conn,
        &[
            "idx_news_source_domain",
            "idx_news_source_tier",
            "idx_news_source_independence",
            "idx_news_topic",
            "idx_user_predictions_topic",
            "idx_user_predictions_source_article",
            "idx_prediction_lessons_status",
            "idx_prediction_lessons_last_cited_at",
        ],
    );
}

fn assert_columns(conn: &Connection, table: &str, expected: &[&str]) {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap_or_else(|err| panic!("failed to inspect table {table}: {err}"));
    let actual: BTreeSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap_or_else(|err| panic!("failed to read table info for {table}: {err}"))
        .collect::<Result<_, _>>()
        .unwrap_or_else(|err| panic!("failed to collect columns for {table}: {err}"));

    for column in expected {
        assert!(
            actual.contains(*column),
            "migrated table {table} is missing column {column}; columns={actual:?}"
        );
    }
}

fn assert_indexes(conn: &Connection, expected: &[&str]) {
    let actual: BTreeSet<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index'")
        .expect("failed to prepare index query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("failed to read indexes")
        .collect::<Result<_, _>>()
        .expect("failed to collect indexes");

    for index in expected {
        assert!(
            actual.contains(*index),
            "migrated prior-release DB is missing index {index}; indexes={actual:?}"
        );
    }
}

fn prepare_isolated_home(root: &TestRoot) {
    fs::create_dir_all(root.xdg_config().join("pftui")).expect("failed to create XDG config dir");
    fs::write(root.xdg_config().join("pftui/config.toml"), "").expect("failed to write XDG config");

    fs::create_dir_all(root.home_config().join("pftui")).expect("failed to create home config dir");
    fs::write(root.home_config().join("pftui/config.toml"), "")
        .expect("failed to write home config");
}

fn copy_fixture_to_default_db(root: &TestRoot) {
    for path in [root.xdg_db_path(), root.home_db_path()] {
        fs::create_dir_all(path.parent().expect("DB path has a parent"))
            .expect("failed to create DB parent");
        fs::copy(FIXTURE, &path)
            .unwrap_or_else(|err| panic!("failed to copy fixture to {}: {err}", path.display()));
    }
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
            .expect("system time before unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pftui-prior-release-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("failed to create test root");
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
