//! Performance budget guard for `pftui report build daily --mode both`.
//!
//! The daily report build sits in every operator workflow and every cron-driven
//! autonomous run. Without a stated wall-time budget, it can silently degrade as
//! sections accrete: a 200ms initial implementation becomes a 30s monster after
//! 20 feature additions. The fix is to set a budget early and benchmark in CI.
//!
//! ## Budget
//!
//! - **Target:** `<2s` end-to-end for `pftui report build daily --mode both`
//!   against the standard test fixture
//!   (`tests/fixtures/db/v0.27.0.sqlite` — ~90 days of history, 4 positions,
//!   ~800 predictions).
//! - **Re-baseline policy:** raise this budget only when a major feature
//!   intentionally adds cost AND a reviewer signs off. Never raise it silently.
//!
//! ## Status
//!
//! The `pftui report build daily` CLI command does not yet exist (the assembler
//! is being implemented in a parallel branch). Until it lands, the perf
//! assertion is `#[ignore]`d so the test suite still parses and links the
//! scaffold. Once the command exists, drop the `#[ignore]` attribute.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FIXTURE: &str = "tests/fixtures/db/v0.27.0.sqlite";

/// Wall-time budget for `pftui report build daily --mode both`.
///
/// Keep in sync with the budget block at the top of
/// `src/report/build/daily.rs`. If you raise this, justify the change in the
/// PR description and update the call-site comment too.
const BUDGET: Duration = Duration::from_millis(2_000);

#[test]
#[ignore = "report build daily CLI not yet wired; remove #[ignore] once the assembler lands"]
fn report_build_daily_meets_perf_budget() {
    let root = TestRoot::new();
    prepare_isolated_home(&root);
    copy_fixture_to_default_db(&root);

    let args = &[
        "--cached-only",
        "report",
        "build",
        "daily",
        "--mode",
        "both",
    ];

    let start = Instant::now();
    let output = run_pftui(&root, args);
    let elapsed = start.elapsed();

    assert_success("report build daily --mode both", &output);

    if elapsed > BUDGET {
        let slowest = slowest_section(&output);
        panic!(
            "report build daily --mode both exceeded perf budget: \
             took {:?} (budget {:?}){}\nstdout:\n{}\nstderr:\n{}",
            elapsed,
            BUDGET,
            slowest
                .map(|s| format!("; slowest section: {s}"))
                .unwrap_or_default(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

/// Smoke-level guard that ALWAYS runs: the perf test scaffold itself must
/// keep compiling and linking against the binary so a future PR that wires
/// the CLI command can simply drop the `#[ignore]` above.
#[test]
fn perf_test_scaffold_compiles_and_links() {
    let _budget = BUDGET;
    let bin = env!("CARGO_BIN_EXE_pftui");
    assert!(
        !bin.is_empty(),
        "CARGO_BIN_EXE_pftui must be wired so the perf test can shell out"
    );
}

/// Best-effort parser: when the CLI is run with `--timing`, sections print
/// `[timing] section_name: 123ms` lines on stderr. The perf assertion does
/// not pass `--timing` itself (the budget is on the no-flag wall-time), but
/// once the assembler lands the failure path may re-run with `--timing` to
/// surface the slowest section. Until then this is a no-op.
fn slowest_section(output: &Output) -> Option<String> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut best: Option<(u64, String)> = None;
    for line in stderr.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("[timing] ") else {
            continue;
        };
        let (name, ms) = rest.split_once(':')?;
        let ms_value: u64 = ms.trim().trim_end_matches("ms").trim().parse().ok()?;
        if best.as_ref().is_none_or(|(b, _)| ms_value > *b) {
            best = Some((ms_value, name.trim().to_string()));
        }
    }
    best.map(|(ms, name)| format!("{name} ({ms}ms)"))
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
            "pftui-report-perf-{}-{unique}",
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
