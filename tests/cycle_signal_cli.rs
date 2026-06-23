//! End-to-end CLI coverage for the cycle-bottom signal surface fixes:
//! - `alerts add` validates cycle conditions (rejects impossible ones non-zero)
//! - `alerts add --json` emits the documented envelope incl. the alert id
//! - non-cycle Technical conditions still arm
//! - `bottom-signals --json` classifies errors with a machine-readable reason
//! - backtest `--window 0` is rejected
//!
//! All runs use an isolated temp HOME so the real local portfolio DB is never
//! touched (on macOS `dirs::data_local_dir()` derives from $HOME; on Linux it
//! derives from $XDG_DATA_HOME / $HOME — both are overridden here).

use std::path::PathBuf;
use std::process::{Command, Output};

/// A throwaway HOME dir, auto-removed on drop.
struct IsolatedHome {
    dir: PathBuf,
}

impl IsolatedHome {
    fn new(tag: &str) -> Self {
        let mut dir = std::env::temp_dir();
        let unique = format!(
            "pftui-cyc-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        dir.push(unique);
        std::fs::create_dir_all(&dir).expect("create isolated HOME");

        // Seed a config.toml so the binary's first-launch interactive prompt
        // (which would otherwise read stdin and pollute stdout) never fires.
        // `dirs::config_dir()` resolves to $HOME/Library/Application Support on
        // macOS and $XDG_CONFIG_HOME on Linux — cover both.
        for cfg_root in [
            dir.join("Library/Application Support/pftui"),
            dir.join(".config/pftui"),
        ] {
            let _ = std::fs::create_dir_all(&cfg_root);
            let _ = std::fs::write(
                cfg_root.join("config.toml"),
                "home_tab = \"portfolio\"\n",
            );
        }
        IsolatedHome { dir }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_pftui"))
            .args(args)
            .env("HOME", &self.dir)
            .env("XDG_DATA_HOME", self.dir.join(".local/share"))
            .env("XDG_CONFIG_HOME", self.dir.join(".config"))
            .env("NO_COLOR", "1")
            .output()
            .expect("run pftui")
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn invalid_cycle_condition_is_rejected_nonzero() {
    let home = IsolatedHome::new("invalid-cond");
    for bad in [
        "cycle_bottom_yearly_4",     // invalid timeframe
        "cycle_bottom_monthly_8",    // N > 7
        "cycle_bottom_monthly_0",    // N = 0
        "cycle_criterion_weekly_bogus_key", // unknown criterion
    ] {
        let out = home.run(&[
            "analytics", "alerts", "add", "--kind", "technical", "--symbol", "BTC-USD",
            "--condition", bad,
        ]);
        assert!(
            !out.status.success(),
            "impossible condition `{bad}` was armed with exit 0 (false green). stdout={}",
            stdout(&out)
        );
        // The error must guide the operator with the valid set.
        let combined = format!("{}{}", stdout(&out), stderr(&out));
        assert!(
            combined.contains("monthly") && combined.contains("1..=7"),
            "rejection for `{bad}` did not list the valid set: {combined}"
        );
    }
}

#[test]
fn valid_cycle_condition_arms_with_json_id() {
    let home = IsolatedHome::new("valid-json");
    let out = home.run(&[
        "analytics", "alerts", "add", "--kind", "technical", "--symbol", "BTC-USD",
        "--condition", "cycle_bottom_monthly_4", "--json",
    ]);
    assert!(out.status.success(), "valid condition failed: {}", stderr(&out));
    let v: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("alerts add --json must emit valid JSON");
    assert_eq!(v["command"], "analytics alerts add");
    assert!(v["id"].as_i64().is_some(), "missing load-bearing id: {v}");
    assert_eq!(v["symbol"], "BTC-USD");
    assert_eq!(v["kind"], "technical");
    assert_eq!(v["condition"], "cycle_bottom_monthly_4");
    assert!(v["label"].as_str().is_some());
    assert_eq!(v["recurring"], false);
}

#[test]
fn non_cycle_technical_condition_still_arms() {
    let home = IsolatedHome::new("noncycle");
    let out = home.run(&[
        "analytics", "alerts", "add", "--kind", "technical", "--symbol", "BTC-USD",
        "--condition", "price_below_sma200", "--json",
    ]);
    assert!(
        out.status.success(),
        "non-cycle technical condition was wrongly rejected: {}",
        stderr(&out)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    assert_eq!(v["condition"], "price_below_sma200");
    assert!(v["id"].as_i64().is_some());
}

#[test]
fn bottom_signals_unknown_symbol_emits_reason() {
    let home = IsolatedHome::new("unknown-sym");
    let out = home.run(&[
        "analytics", "cycles", "bottom-signals", "--asset", "ZZZNOTREAL", "--json",
    ]);
    assert!(!out.status.success(), "unknown symbol should fail nonzero");
    let v: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("error envelope must be JSON under --json");
    // unknown_symbol and no_history are collapsed to `no_history` (see report):
    // with an empty cache we cannot distinguish a typo'd ticker from an
    // uncached-but-valid one without a network call.
    assert_eq!(v["error"]["reason"], "no_history", "envelope: {v}");
    assert!(v["error"]["message"].as_str().is_some());
}

#[test]
fn backtest_window_zero_rejected() {
    let home = IsolatedHome::new("window-zero");
    let out = home.run(&[
        "analytics", "cycles", "bottom-signals", "backtest", "--asset", "BTC",
        "--window", "0", "--json",
    ]);
    assert!(!out.status.success(), "--window 0 should be rejected");
    let combined = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        combined.contains("window 0") || combined.contains("not meaningful"),
        "rejection message unclear: {combined}"
    );
}
