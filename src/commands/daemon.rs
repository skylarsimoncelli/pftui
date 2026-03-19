use anyhow::Result;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::commands::refresh;
use crate::config::Config;
use crate::db::backend::open_from_config;

/// Global shutdown flag — set by the signal handler, read by the main loop.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Install POSIX signal handlers for SIGTERM and SIGINT (graceful shutdown).
///
/// Uses raw `unsafe` because the `libc` crate is not a dependency. The handler
/// only touches an `AtomicBool`, which is async-signal-safe.
fn install_signal_handlers() {
    // SAFETY: the handler performs a single atomic store — async-signal-safe.
    unsafe {
        // sigaction approach using raw syscall pointers is heavy; instead we
        // register a plain function pointer via std C `signal()`. Rust's std
        // re-exports libc types on unix targets.
        #[cfg(unix)]
        {
            extern "C" {
                fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
            }
            const SIGINT: i32 = 2;
            const SIGTERM: i32 = 15;
            signal(SIGINT, handle_signal);
            signal(SIGTERM, handle_signal);
        }
    }
}

#[cfg(unix)]
extern "C" fn handle_signal(_sig: i32) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

// ── Lock file ───────────────────────────────────────────────────────────

/// Daemon lock file to prevent multiple instances.
struct DaemonLock {
    path: PathBuf,
}

impl DaemonLock {
    fn acquire() -> Result<Self> {
        let lock_path = daemon_lock_path();
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Check for existing daemon
        if lock_path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&lock_path) {
                if let Ok(pid) = contents.trim().parse::<u32>() {
                    // Check if process is still alive (Linux /proc check)
                    let proc_path = format!("/proc/{}", pid);
                    if std::path::Path::new(&proc_path).exists() {
                        anyhow::bail!(
                            "Daemon already running (PID {}). Kill it first or remove {}",
                            pid,
                            lock_path.display()
                        );
                    }
                }
            }
            // Stale lock — remove it
            let _ = std::fs::remove_file(&lock_path);
        }

        // Write our PID
        let mut f = std::fs::File::create(&lock_path)?;
        write!(f, "{}", std::process::id())?;

        Ok(DaemonLock { path: lock_path })
    }
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn daemon_lock_path() -> PathBuf {
    if let Ok(path) = std::env::var("PFTUI_DATA_DIR") {
        return std::path::Path::new(&path).join("daemon.lock");
    }
    std::path::Path::new(&std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".local/share/pftui/daemon.lock")
}

// ── Heartbeat ───────────────────────────────────────────────────────────

fn heartbeat_path() -> PathBuf {
    if let Ok(path) = std::env::var("PFTUI_DATA_DIR") {
        return std::path::Path::new(&path).join("daemon_heartbeat.json");
    }
    std::path::Path::new(&std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".local/share/pftui/daemon_heartbeat.json")
}

/// Write a heartbeat JSON file so `pftui data status` and external monitors can
/// check daemon health without needing IPC.
fn write_heartbeat(
    cycle: u64,
    status: &str,
    last_refresh_secs: Option<f64>,
    errors: &[String],
    interval_secs: u64,
) {
    let now = chrono::Utc::now().to_rfc3339();
    let error_json: Vec<String> = errors
        .iter()
        .map(|e| format!("\"{}\"", e.replace('"', "\\\"")))
        .collect();
    let heartbeat = format!(
        r#"{{"pid":{},"status":"{}","cycle":{},"last_heartbeat":"{}","last_refresh_duration_secs":{},"errors":[{}],"interval_secs":{}}}"#,
        std::process::id(),
        status,
        cycle,
        now,
        last_refresh_secs
            .map(|s| format!("{:.1}", s))
            .unwrap_or_else(|| "null".to_string()),
        error_json.join(","),
        interval_secs,
    );
    let path = heartbeat_path();
    let _ = std::fs::write(path, heartbeat);
}

// ── Logging ─────────────────────────────────────────────────────────────

/// Structured log line for daemon events.
fn log_event(json: bool, event: &str, message: &str) {
    if json {
        let now = chrono::Utc::now().to_rfc3339();
        println!(
            r#"{{"ts":"{}","event":"{}","message":"{}","pid":{}}}"#,
            now,
            event,
            message.replace('"', "\\\""),
            std::process::id()
        );
    } else {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        println!("[{}] {} — {}", now, event, message);
    }
}

// ── Main daemon loop ────────────────────────────────────────────────────

/// Run the daemon: loops forever, refreshing data on the given interval.
///
/// The daemon acquires a PID-based lock file to prevent multiple instances,
/// registers signal handlers for graceful shutdown, and on each cycle:
///   1. Opens a fresh DB connection
///   2. Runs the full data refresh pipeline (`data refresh`)
///   3. Evaluates alert rules against updated data
///   4. Writes a heartbeat JSON for external monitoring
///   5. Sleeps until the next cycle (interruptible on signal)
pub fn run(
    config: &Config,
    db_path: &std::path::Path,
    interval_secs: u64,
    json: bool,
) -> Result<()> {
    let _lock = DaemonLock::acquire()?;
    install_signal_handlers();

    log_event(
        json,
        "daemon_start",
        &format!(
            "pftui daemon starting (PID {}, interval {}s)",
            std::process::id(),
            interval_secs
        ),
    );
    write_heartbeat(0, "starting", None, &[], interval_secs);

    let mut cycle: u64 = 0;

    while !SHUTDOWN.load(Ordering::SeqCst) {
        cycle += 1;

        log_event(json, "refresh_start", &format!("cycle {} beginning", cycle));

        let start = std::time::Instant::now();
        let mut errors: Vec<String> = Vec::new();

        // Open a fresh backend connection each cycle to avoid stale connections
        let backend = match open_from_config(config, db_path) {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("Failed to open database: {}", e);
                log_event(json, "refresh_error", &msg);
                errors.push(msg);
                write_heartbeat(cycle, "error", None, &errors, interval_secs);
                if !sleep_interruptible(interval_secs) {
                    break;
                }
                continue;
            }
        };

        match refresh::run_quiet(&backend, config, false) {
            Ok(()) => {
                let elapsed = start.elapsed().as_secs_f64();
                log_event(
                    json,
                    "refresh_complete",
                    &format!("cycle {} completed in {:.1}s", cycle, elapsed),
                );
                write_heartbeat(cycle, "healthy", Some(elapsed), &[], interval_secs);
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                let msg = format!("cycle {} failed after {:.1}s: {}", cycle, elapsed, e);
                log_event(json, "refresh_error", &msg);
                errors.push(format!("{}", e));
                write_heartbeat(cycle, "degraded", Some(elapsed), &errors, interval_secs);
            }
        }

        // Run alert evaluation after refresh
        run_alert_check(&backend, json);

        // Sleep until next cycle, checking for shutdown every second
        if !sleep_interruptible(interval_secs) {
            break;
        }
    }

    log_event(json, "daemon_stop", "shutting down gracefully");
    write_heartbeat(cycle, "stopped", None, &[], interval_secs);

    Ok(())
}

/// Run alert evaluation after each refresh cycle.
fn run_alert_check(backend: &crate::db::backend::BackendConnection, json_log: bool) {
    use crate::alerts::engine::check_alerts_backend_only;

    match check_alerts_backend_only(backend) {
        Ok(results) => {
            let newly: Vec<_> = results.iter().filter(|r| r.newly_triggered).collect();
            if !newly.is_empty() {
                log_event(
                    json_log,
                    "alerts_triggered",
                    &format!("{} alert(s) fired", newly.len()),
                );
            }
        }
        Err(e) => {
            log_event(
                json_log,
                "alerts_error",
                &format!("alert check failed: {}", e),
            );
        }
    }
}

/// Sleep for `secs` seconds, waking every second to check for shutdown signal.
/// Returns `true` if the full duration elapsed, `false` if interrupted.
fn sleep_interruptible(secs: u64) -> bool {
    for _ in 0..secs {
        if SHUTDOWN.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    !SHUTDOWN.load(Ordering::SeqCst)
}

// ── Status subcommand ───────────────────────────────────────────────────

/// Read the daemon heartbeat file and print daemon status.
pub fn run_status(json: bool) -> Result<()> {
    let path = heartbeat_path();
    if !path.exists() {
        if json {
            println!(r#"{{"running":false,"message":"No daemon heartbeat found"}}"#);
        } else {
            println!("Daemon is not running (no heartbeat file found).");
        }
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)?;

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
        if let Some(pid) = parsed.get("pid").and_then(|p| p.as_u64()) {
            let proc_path = format!("/proc/{}", pid);
            let alive = std::path::Path::new(&proc_path).exists();

            if json {
                let mut obj = parsed.clone();
                if let Some(map) = obj.as_object_mut() {
                    map.insert("running".to_string(), serde_json::Value::Bool(alive));
                }
                println!("{}", serde_json::to_string(&obj)?);
            } else if alive {
                let status = parsed
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown");
                let cycle = parsed.get("cycle").and_then(|c| c.as_u64()).unwrap_or(0);
                let last = parsed
                    .get("last_heartbeat")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown");
                let interval = parsed
                    .get("interval_secs")
                    .and_then(|s| s.as_u64())
                    .unwrap_or(0);
                let duration = parsed
                    .get("last_refresh_duration_secs")
                    .and_then(|s| s.as_f64());
                println!("✓ Daemon running (PID {})", pid);
                println!("  Status:    {}", status);
                println!("  Cycle:     {}", cycle);
                println!("  Interval:  {}s", interval);
                if let Some(d) = duration {
                    println!("  Last run:  {:.1}s", d);
                }
                println!("  Heartbeat: {}", last);
            } else {
                println!("✗ Daemon not running (PID {} is dead)", pid);
                println!(
                    "  Last heartbeat: {}",
                    parsed
                        .get("last_heartbeat")
                        .and_then(|s| s.as_str())
                        .unwrap_or("unknown")
                );
            }
        } else if json {
            println!(r#"{{"running":false,"message":"Heartbeat file missing PID"}}"#);
        } else {
            println!("Daemon heartbeat file is missing PID field.");
        }
    } else if json {
        println!(r#"{{"running":false,"message":"Heartbeat file is corrupt"}}"#);
    } else {
        println!("Daemon heartbeat file is corrupt.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_lock_path_ends_with_expected_name() {
        let path = daemon_lock_path();
        assert!(path.to_string_lossy().ends_with("daemon.lock"));
    }

    #[test]
    fn heartbeat_path_ends_with_expected_name() {
        let path = heartbeat_path();
        assert!(path.to_string_lossy().ends_with("daemon_heartbeat.json"));
    }

    #[test]
    fn sleep_interruptible_exits_on_shutdown() {
        // Set shutdown flag before calling sleep
        SHUTDOWN.store(true, Ordering::SeqCst);
        let result = sleep_interruptible(60);
        assert!(!result);
        // Reset for other tests
        SHUTDOWN.store(false, Ordering::SeqCst);
    }

    #[test]
    fn log_event_does_not_panic() {
        log_event(true, "test_event", "test message with \"quotes\"");
        log_event(false, "test_event", "plain test message");
    }

    #[test]
    fn run_status_no_heartbeat_file() {
        // When no heartbeat file exists, run_status should not error
        // (it prints "not running" message). We can't easily capture stdout
        // here, so just verify no panic.
        std::env::set_var("PFTUI_DATA_DIR", "/tmp/pftui-daemon-test-nonexistent");
        let result = run_status(false);
        assert!(result.is_ok());
        let result_json = run_status(true);
        assert!(result_json.is_ok());
        std::env::remove_var("PFTUI_DATA_DIR");
    }
}
