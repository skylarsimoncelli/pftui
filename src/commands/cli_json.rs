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

/// Machine-readable detail attached to a command error so the `--json` error
/// envelope can carry structured fields (a stable `reason` slug plus optional
/// numeric context such as `bars_available`) instead of only a prose message.
///
/// Attach with [`anyhow::Error::context`]; [`or_json_error`] downcasts the
/// context chain to find it and merges its fields into `error`.
#[derive(Debug, Clone)]
pub struct ErrorDetail {
    /// Stable, machine-matchable reason slug (e.g. `no_history`).
    pub reason: &'static str,
    /// Bars available for the requested series, when known.
    pub bars_available: Option<usize>,
}

impl ErrorDetail {
    pub fn new(reason: &'static str) -> Self {
        Self {
            reason,
            bars_available: None,
        }
    }

    pub fn with_bars(reason: &'static str, bars_available: usize) -> Self {
        Self {
            reason,
            bars_available: Some(bars_available),
        }
    }
}

impl std::fmt::Display for ErrorDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Rendered as anyhow context — keep it terse; the human-facing message
        // is the underlying error, this is the machine annotation.
        write!(f, "reason={}", self.reason)
    }
}

impl std::error::Error for ErrorDetail {}

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
            // Look for a structured ErrorDetail attached as anyhow context so
            // the envelope can carry a machine-matchable `reason` /
            // `bars_available`. anyhow's own downcast (not std chain-node
            // downcast) is what resolves the context type.
            let detail = e.downcast_ref::<ErrorDetail>();

            // Build the human message. When an ErrorDetail is the outermost
            // context, its own `reason=…` Display line is machine annotation —
            // skip it and use the underlying cause(s); otherwise use the full
            // single-line chain.
            let message = if detail.is_some() {
                e.source()
                    .map(|s| format!("{s:#}"))
                    .unwrap_or_else(|| format!("{e:#}"))
            } else {
                format!("{e:#}")
            };

            let mut error_obj = json!({
                "command": command,
                "message": message,
            });
            if let (Some(d), Some(obj)) = (detail, error_obj.as_object_mut()) {
                obj.insert("reason".into(), json!(d.reason));
                if let Some(bars) = d.bars_available {
                    obj.insert("bars_available".into(), json!(bars));
                }
            }
            let env = json!({ "error": error_obj });
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
