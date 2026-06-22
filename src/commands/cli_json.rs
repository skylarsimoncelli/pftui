//! Reliability shim for `--json` CLI commands.
//!
//! When an analytics command is invoked with `--json` and then fails, the
//! default path prints a plain-text `Error: …` to STDERR and leaves STDOUT
//! empty — so a `--json` consumer (an agent or script) gets nothing parseable
//! and exit code 1 with no machine-readable reason. [`or_json_error`] fixes
//! that: on failure it emits a structured error envelope on STDOUT, then
//! re-returns the error unchanged so the exit code stays non-zero and a human
//! still sees the stderr line.

use anyhow::Result;
use serde_json::{json, Value};

/// Merge the standard top-level envelope keys into a `--json` payload object,
/// ADDITIVELY: existing keys are NEVER overwritten, so a command that already
/// emits `command`/`as_of`/`resolved_symbol` keeps its own values. This gives
/// every cycles/backtest/TA `--json` shape a uniform spine (`command`, `as_of`,
/// and — where an asset applies — `resolved_symbol`) without restructuring any
/// existing payload field.
///
/// `resolved_symbol` is only inserted when `resolved` is `Some` (commands with
/// no single asset, e.g. the BTC+gold cycle clock, pass `None`).
pub fn envelope(
    mut payload: Value,
    command: &str,
    as_of: &str,
    resolved: Option<&str>,
) -> Value {
    if let Some(obj) = payload.as_object_mut() {
        obj.entry("command").or_insert_with(|| json!(command));
        obj.entry("as_of").or_insert_with(|| json!(as_of));
        if let Some(sym) = resolved {
            obj.entry("resolved_symbol").or_insert_with(|| json!(sym));
        }
    }
    payload
}

/// Wrap a command's `Result` so that, under `--json`, a failure also emits a
/// `{"error": {"command", "message"}}` envelope on STDOUT. No-op on success or
/// when `json` is false. The error is propagated unchanged (exit code + stderr
/// preserved).
///
/// Safe because the wrapped commands bail on their error paths BEFORE writing
/// any stdout, so the envelope is the sole stdout content (never appended to a
/// half-written JSON document).
pub fn or_json_error(command: &str, json: bool, result: Result<()>) -> Result<()> {
    if json {
        if let Err(e) = &result {
            let env = json!({
                "error": {
                    "command": command,
                    // `{:#}` includes the anyhow context chain on one line.
                    "message": format!("{e:#}"),
                }
            });
            if let Ok(s) = serde_json::to_string_pretty(&env) {
                println!("{s}");
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_adds_missing_keys_only() {
        let out = envelope(json!({"foo": 1}), "cmd", "2026-06-22", Some("BTC-USD"));
        assert_eq!(out["command"], json!("cmd"));
        assert_eq!(out["as_of"], json!("2026-06-22"));
        assert_eq!(out["resolved_symbol"], json!("BTC-USD"));
        assert_eq!(out["foo"], json!(1));
    }

    #[test]
    fn envelope_never_overwrites_existing() {
        let out = envelope(
            json!({"command": "mine", "as_of": "1999-01-01", "resolved_symbol": "GC=F"}),
            "other",
            "2026-06-22",
            Some("BTC-USD"),
        );
        // Existing values win — additive, non-destructive.
        assert_eq!(out["command"], json!("mine"));
        assert_eq!(out["as_of"], json!("1999-01-01"));
        assert_eq!(out["resolved_symbol"], json!("GC=F"));
    }

    #[test]
    fn envelope_omits_resolved_when_none() {
        let out = envelope(json!({"btc": null, "gold": null}), "cmd", "2026-06-22", None);
        assert_eq!(out["command"], json!("cmd"));
        assert_eq!(out["as_of"], json!("2026-06-22"));
        assert!(out.get("resolved_symbol").is_none());
    }

    #[test]
    fn passes_ok_through_untouched() {
        assert!(or_json_error("x", true, Ok(())).is_ok());
        assert!(or_json_error("x", false, Ok(())).is_ok());
    }

    #[test]
    fn propagates_error_in_both_modes() {
        // The error is always re-returned (so exit code stays 1); the only
        // difference is whether the envelope is also printed to stdout.
        let mk = || -> Result<()> { anyhow::bail!("boom") };
        assert!(or_json_error("x", true, mk()).is_err());
        assert!(or_json_error("x", false, mk()).is_err());
    }
}
