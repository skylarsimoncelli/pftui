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
use serde_json::json;

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
