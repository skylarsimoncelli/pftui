use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::commands::refresh::{self, RefreshPlan};
use crate::config::{Config, DaemonCadenceConfig};
use crate::db::backend::open_from_config;

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn install_signal_handlers() {
    unsafe {
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

struct DaemonLock {
    path: PathBuf,
}

impl DaemonLock {
    fn acquire() -> Result<Self> {
        let lock_path = daemon_lock_path();
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if lock_path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&lock_path) {
                if let Ok(pid) = contents.trim().parse::<u32>() {
                    if process_is_alive(pid) {
                        anyhow::bail!(
                            "Daemon already running (PID {}). Kill it first or remove {}",
                            pid,
                            lock_path.display()
                        );
                    }
                }
            }
            let _ = std::fs::remove_file(&lock_path);
        }

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

fn heartbeat_path() -> PathBuf {
    if let Ok(path) = std::env::var("PFTUI_DATA_DIR") {
        return std::path::Path::new(&path).join("daemon_heartbeat.json");
    }
    std::path::Path::new(&std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".local/share/pftui/daemon_heartbeat.json")
}

fn process_is_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub status: String,
    pub cycle: u64,
    pub last_heartbeat: Option<String>,
    pub last_refresh_duration_secs: Option<f64>,
    pub errors: Vec<String>,
    pub interval_secs: u64,
    #[serde(default)]
    pub tasks: Vec<String>,
    pub message: Option<String>,
}

impl DaemonStatus {
    fn missing(message: &str) -> Self {
        Self {
            running: false,
            pid: None,
            status: "stopped".to_string(),
            cycle: 0,
            last_heartbeat: None,
            last_refresh_duration_secs: None,
            errors: Vec::new(),
            interval_secs: 0,
            tasks: Vec::new(),
            message: Some(message.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HeartbeatPayload {
    pid: u32,
    status: String,
    cycle: u64,
    last_heartbeat: String,
    last_refresh_duration_secs: Option<f64>,
    errors: Vec<String>,
    interval_secs: u64,
    #[serde(default)]
    tasks: Vec<String>,
}

fn write_heartbeat(
    cycle: u64,
    status: &str,
    last_refresh_secs: Option<f64>,
    errors: &[String],
    interval_secs: u64,
    tasks: &[&str],
) {
    let payload = HeartbeatPayload {
        pid: std::process::id(),
        status: status.to_string(),
        cycle,
        last_heartbeat: chrono::Utc::now().to_rfc3339(),
        last_refresh_duration_secs: last_refresh_secs,
        errors: errors.to_vec(),
        interval_secs,
        tasks: tasks.iter().map(|task| task.to_string()).collect(),
    };
    if let Ok(body) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(heartbeat_path(), body);
    }
}

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

struct CadenceTracker {
    last_run: HashMap<&'static str, Instant>,
}

impl CadenceTracker {
    fn new() -> Self {
        Self {
            last_run: HashMap::new(),
        }
    }

    fn due(&self, key: &'static str, cadence_secs: u64) -> bool {
        match self.last_run.get(key) {
            None => true,
            Some(last) => last.elapsed().as_secs() >= cadence_secs.max(1),
        }
    }

    fn build_plan(&self, cadence: &DaemonCadenceConfig) -> RefreshPlan {
        RefreshPlan {
            prices: self.due("prices", cadence.prices_interval_secs),
            predictions: self.due("predictions", cadence.predictions_interval_secs),
            fedwatch: self.due("fedwatch", cadence.fedwatch_interval_secs),
            news_rss: self.due("news_rss", cadence.news_interval_secs),
            news_brave: self.due("news_brave", cadence.brave_news_interval_secs),
            cot: self.due("cot", cadence.cot_interval_secs),
            sentiment: self.due("sentiment", cadence.sentiment_interval_secs),
            calendar: self.due("calendar", cadence.calendar_interval_secs),
            economy: self.due("economy", cadence.economy_interval_secs),
            fred: self.due("fred", cadence.fred_interval_secs),
            bls: self.due("bls", cadence.bls_interval_secs),
            worldbank: self.due("worldbank", cadence.worldbank_interval_secs),
            comex: self.due("comex", cadence.comex_interval_secs),
            onchain: self.due("onchain", cadence.onchain_interval_secs),
            analytics: self.due("analytics", cadence.analytics_interval_secs),
            alerts: self.due("alerts", cadence.alerts_interval_secs),
            cleanup: self.due("cleanup", cadence.cleanup_interval_secs),
        }
    }

    fn mark_executed(&mut self, plan: &RefreshPlan) {
        let now = Instant::now();
        for task in plan.selected_task_names() {
            self.last_run.insert(task, now);
        }
    }
}

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
            "pftui daemon starting (PID {}, wake interval {}s)",
            std::process::id(),
            interval_secs
        ),
    );
    write_heartbeat(0, "starting", None, &[], interval_secs, &[]);

    let cadence = &config.daemon.cadence;
    let mut tracker = CadenceTracker::new();
    let mut cycle: u64 = 0;

    while !SHUTDOWN.load(Ordering::SeqCst) {
        cycle += 1;
        let plan = tracker.build_plan(cadence);
        let task_names = plan.selected_task_names();

        if task_names.is_empty() {
            write_heartbeat(cycle, "idle", None, &[], interval_secs, &[]);
            if !sleep_interruptible(interval_secs) {
                break;
            }
            continue;
        }

        log_event(
            json,
            "refresh_start",
            &format!("cycle {} beginning ({})", cycle, task_names.join(",")),
        );

        let start = Instant::now();
        let mut errors: Vec<String> = Vec::new();

        let backend = match open_from_config(config, db_path) {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("Failed to open database: {}", e);
                log_event(json, "refresh_error", &msg);
                errors.push(msg);
                write_heartbeat(cycle, "error", None, &errors, interval_secs, &task_names);
                if !sleep_interruptible(interval_secs) {
                    break;
                }
                continue;
            }
        };

        match refresh::run_quiet_with_plan(&backend, config, false, &plan) {
            Ok(()) => {
                let elapsed = start.elapsed().as_secs_f64();
                tracker.mark_executed(&plan);
                log_event(
                    json,
                    "refresh_complete",
                    &format!(
                        "cycle {} completed in {:.1}s ({})",
                        cycle,
                        elapsed,
                        task_names.join(",")
                    ),
                );
                write_heartbeat(
                    cycle,
                    "healthy",
                    Some(elapsed),
                    &[],
                    interval_secs,
                    &task_names,
                );
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                let msg = format!("cycle {} failed after {:.1}s: {}", cycle, elapsed, e);
                log_event(json, "refresh_error", &msg);
                errors.push(e.to_string());
                write_heartbeat(
                    cycle,
                    "degraded",
                    Some(elapsed),
                    &errors,
                    interval_secs,
                    &task_names,
                );
            }
        }

        if !sleep_interruptible(interval_secs) {
            break;
        }
    }

    log_event(json, "daemon_stop", "shutting down gracefully");
    write_heartbeat(cycle, "stopped", None, &[], interval_secs, &[]);

    Ok(())
}

fn sleep_interruptible(secs: u64) -> bool {
    for _ in 0..secs {
        if SHUTDOWN.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    !SHUTDOWN.load(Ordering::SeqCst)
}

pub fn read_status() -> Result<DaemonStatus> {
    let path = heartbeat_path();
    if !path.exists() {
        return Ok(DaemonStatus::missing("No daemon heartbeat found"));
    }

    let content = std::fs::read_to_string(&path)?;
    let payload: HeartbeatPayload = match serde_json::from_str(&content) {
        Ok(payload) => payload,
        Err(_) => return Ok(DaemonStatus::missing("Heartbeat file is corrupt")),
    };

    let running = process_is_alive(payload.pid);
    let message = if running {
        None
    } else {
        Some(format!(
            "Daemon is not running (PID {} is dead)",
            payload.pid
        ))
    };

    Ok(DaemonStatus {
        running,
        pid: Some(payload.pid),
        status: payload.status,
        cycle: payload.cycle,
        last_heartbeat: Some(payload.last_heartbeat),
        last_refresh_duration_secs: payload.last_refresh_duration_secs,
        errors: payload.errors,
        interval_secs: payload.interval_secs,
        tasks: payload.tasks,
        message,
    })
}

pub fn run_status(json: bool) -> Result<()> {
    let status = read_status()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else if status.running {
        println!(
            "✓ Daemon running (PID {})",
            status
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "?".to_string())
        );
        println!("  Status:    {}", status.status);
        println!("  Cycle:     {}", status.cycle);
        println!("  Interval:  {}s", status.interval_secs);
        if let Some(duration) = status.last_refresh_duration_secs {
            println!("  Last run:  {:.1}s", duration);
        }
        if let Some(last) = &status.last_heartbeat {
            println!("  Heartbeat: {}", last);
        }
        if !status.tasks.is_empty() {
            println!("  Tasks:     {}", status.tasks.join(", "));
        }
    } else if let Some(message) = status.message {
        println!("{}", message);
        if let Some(last) = status.last_heartbeat {
            println!("  Last heartbeat: {}", last);
        }
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
        SHUTDOWN.store(true, Ordering::SeqCst);
        let result = sleep_interruptible(60);
        assert!(!result);
        SHUTDOWN.store(false, Ordering::SeqCst);
    }

    #[test]
    fn cadence_tracker_marks_and_defers_tasks() {
        let cadence = DaemonCadenceConfig::default();
        let mut tracker = CadenceTracker::new();
        let first = tracker.build_plan(&cadence);
        assert!(first.prices);
        assert!(first.news_rss);
        tracker.mark_executed(&first);
        let second = tracker.build_plan(&cadence);
        assert!(!second.prices);
        assert!(!second.news_rss);
    }

    #[test]
    fn run_status_no_heartbeat_file() {
        std::env::set_var("PFTUI_DATA_DIR", "/tmp/pftui-daemon-test-nonexistent");
        let status = read_status().unwrap();
        assert!(!status.running);
        assert_eq!(status.message.as_deref(), Some("No daemon heartbeat found"));
        std::env::remove_var("PFTUI_DATA_DIR");
    }
}
