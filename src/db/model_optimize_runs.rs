//! P5b — the persistent **optimization ledger**: the meta-overfitting guardrail.
//!
//! Every `analytics models optimize` run appends ONE immutable row recording the
//! search it performed: a stable `topology_hash` of the research family (model +
//! universe + rules + cadence + objective + cost model + fold scheme + the param
//! NAMES and their RANGES + benchmark), the number of configs tried THIS run, the
//! winner, the verdict, PBO, DSR, and the scored/lockbox windows.
//!
//! The point: a single optimize run already corrects for the multiple testing it
//! did internally (PBO/DSR). What it CANNOT see is the testing you did across
//! REPEATED runs — re-running with a slightly wider grid, a nudged range, a
//! different fold count, day after day, is silent multiple testing that inflates
//! the best observed result. The ledger makes that visible: **cumulative trials
//! for a topology = SUM(n_configs) over every ledger row sharing the hash.** Any
//! change to the topology (a new param range, a different objective, an edited
//! rule) yields a DIFFERENT hash — a NEW research family, reported as a fresh
//! branch, NOT independent confirmation of the old one.
//!
//! Table: `model_optimize_runs` — L3 append-only run provenance (lazily created).
//! NEVER mutated or deleted; the only operations are INSERT and SELECT.

use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::analytics::portfolio_sim::optimize::{Objective, ParamAxis};
use crate::analytics::portfolio_sim::spec::ModelSpec;

/// One appended ledger row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizeRunRecord {
    pub id: i64,
    pub topology_hash: String,
    pub model_name: String,
    pub model_version: i64,
    pub objective: String,
    /// Configs tried in THIS run (the per-run trial count).
    pub n_configs: i64,
    pub winner_params: Option<String>,
    pub verdict: String,
    pub pbo: Option<f64>,
    pub dsr: Option<f64>,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    pub lockbox_start: Option<String>,
    pub lockbox_end: Option<String>,
    pub created_at: String,
}

impl OptimizeRunRecord {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            topology_hash: row.get(1)?,
            model_name: row.get(2)?,
            model_version: row.get(3)?,
            objective: row.get(4)?,
            n_configs: row.get(5)?,
            winner_params: row.get(6)?,
            verdict: row.get(7)?,
            pbo: row.get(8)?,
            dsr: row.get(9)?,
            window_start: row.get(10)?,
            window_end: row.get(11)?,
            lockbox_start: row.get(12)?,
            lockbox_end: row.get(13)?,
            created_at: row.get(14)?,
        })
    }
}

/// What to append (everything except the DB-assigned id + created_at).
#[derive(Debug, Clone)]
pub struct NewOptimizeRun {
    pub topology_hash: String,
    pub model_name: String,
    pub model_version: i64,
    pub objective: String,
    pub n_configs: i64,
    pub winner_params: Option<String>,
    pub verdict: String,
    pub pbo: Option<f64>,
    pub dsr: Option<f64>,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    pub lockbox_start: Option<String>,
    pub lockbox_end: Option<String>,
}

/// Cumulative-trials summary for one topology family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologySummary {
    pub topology_hash: String,
    pub model_name: String,
    pub objective: String,
    pub n_runs: i64,
    /// SUM(n_configs) across every run of this topology — the cumulative trials.
    pub cumulative_trials: i64,
    pub last_verdict: String,
    pub last_run_at: String,
}

fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS model_optimize_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            topology_hash TEXT NOT NULL,
            model_name TEXT NOT NULL,
            model_version INTEGER NOT NULL DEFAULT 1,
            objective TEXT NOT NULL,
            n_configs INTEGER NOT NULL,
            winner_params TEXT,
            verdict TEXT NOT NULL,
            pbo REAL,
            dsr REAL,
            window_start TEXT,
            window_end TEXT,
            lockbox_start TEXT,
            lockbox_end TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_model_optimize_runs_topology
            ON model_optimize_runs(topology_hash);
        CREATE INDEX IF NOT EXISTS idx_model_optimize_runs_created
            ON model_optimize_runs(created_at);",
    )?;
    Ok(())
}

/// Append one run row (the ONLY write path — append-only). Returns the new id.
pub fn append_run(conn: &Connection, run: &NewOptimizeRun) -> Result<i64> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO model_optimize_runs (
            topology_hash, model_name, model_version, objective, n_configs,
            winner_params, verdict, pbo, dsr, window_start, window_end,
            lockbox_start, lockbox_end
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            run.topology_hash,
            run.model_name,
            run.model_version,
            run.objective,
            run.n_configs,
            run.winner_params,
            run.verdict,
            run.pbo,
            run.dsr,
            run.window_start,
            run.window_end,
            run.lockbox_start,
            run.lockbox_end,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Cumulative trials for a topology = SUM(n_configs) over all rows with this
/// hash. 0 when the topology has never been seen.
pub fn cumulative_trials_for_topology(conn: &Connection, topology_hash: &str) -> Result<i64> {
    ensure_table(conn)?;
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(n_configs), 0) FROM model_optimize_runs WHERE topology_hash = ?1",
        params![topology_hash],
        |r| r.get(0),
    )?;
    Ok(total)
}

/// Number of prior runs recorded for a topology.
pub fn run_count_for_topology(conn: &Connection, topology_hash: &str) -> Result<i64> {
    ensure_table(conn)?;
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM model_optimize_runs WHERE topology_hash = ?1",
        params![topology_hash],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// All runs, most-recent first (bounded).
pub fn list_runs(conn: &Connection, limit: usize) -> Result<Vec<OptimizeRunRecord>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, topology_hash, model_name, model_version, objective, n_configs,
                winner_params, verdict, pbo, dsr, window_start, window_end,
                lockbox_start, lockbox_end, created_at
         FROM model_optimize_runs
         ORDER BY id DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit as i64], OptimizeRunRecord::from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// One summary row per topology family, ordered by cumulative trials desc.
pub fn topology_summaries(conn: &Connection) -> Result<Vec<TopologySummary>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT topology_hash,
                MAX(model_name),
                MAX(objective),
                COUNT(*) AS n_runs,
                SUM(n_configs) AS cumulative_trials,
                (SELECT verdict FROM model_optimize_runs r2
                   WHERE r2.topology_hash = r1.topology_hash
                   ORDER BY r2.id DESC LIMIT 1) AS last_verdict,
                MAX(created_at) AS last_run_at
         FROM model_optimize_runs r1
         GROUP BY topology_hash
         ORDER BY cumulative_trials DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TopologySummary {
                topology_hash: row.get(0)?,
                model_name: row.get(1)?,
                objective: row.get(2)?,
                n_runs: row.get(3)?,
                cumulative_trials: row.get(4)?,
                last_verdict: row.get(5)?,
                last_run_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Topology hash.
// ---------------------------------------------------------------------------

/// A stable SHA-256 hash of the research FAMILY: model name + universe + rules +
/// rebalance cadence + objective + cost model + fold scheme + the searched param
/// NAMES and their RANGES + benchmark. Any change to ANY of these is a different
/// family (a new branch), NOT confirmation of the old one.
///
/// `fold_scheme_descriptor` is a short, deterministic string for the fold scheme
/// (warmup/train/test/step/lockbox days) — the caller passes it so the hash
/// reflects how the timeline was partitioned.
pub fn topology_hash(
    spec: &ModelSpec,
    axes: &[ParamAxis],
    objective: Objective,
    fold_scheme_descriptor: &str,
) -> String {
    let mut h = Sha256::new();
    let upd = |h: &mut Sha256, k: &str, v: &str| {
        h.update(k.as_bytes());
        h.update(b"=");
        h.update(v.as_bytes());
        h.update(b"\n");
    };

    upd(&mut h, "model", &spec.model.name);
    upd(&mut h, "base_currency", &spec.model.base_currency);

    // Universe — sorted by symbol for stability.
    let mut assets: Vec<String> = spec
        .universe
        .assets
        .iter()
        .map(|a| format!("{}:{}:{}", a.symbol, a.class, a.price_currency))
        .collect();
    assets.sort();
    upd(&mut h, "cash_class", &spec.universe.cash_class);
    upd(&mut h, "universe", &assets.join(","));

    // Base policy — sorted by class.
    let mut targets: Vec<String> = spec
        .base_policy
        .targets
        .iter()
        .map(|t| format!("{}:{}:{}:{}", t.class, t.target, t.floor, t.ceiling))
        .collect();
    targets.sort();
    upd(&mut h, "within_class", &spec.base_policy.within_class);
    upd(&mut h, "base_policy", &targets.join(","));

    // Cost model + cadence + fill + bands (the constraints that shape outcomes).
    let c = &spec.constraints;
    upd(&mut h, "rebalance_cadence", &c.rebalance_cadence);
    upd(&mut h, "rebalance_band_mode", &c.rebalance_band_mode);
    upd(&mut h, "fill", &c.fill);
    upd(&mut h, "commission_pct", &c.commission_pct.to_string());
    upd(&mut h, "slippage_pct", &c.slippage_pct.to_string());
    upd(
        &mut h,
        "cash_yield_proxy",
        c.cash_yield_proxy.as_deref().unwrap_or("none"),
    );
    upd(&mut h, "no_average_down", &c.no_average_down.to_string());
    upd(
        &mut h,
        "max_position",
        &c.max_position.map(|v| v.to_string()).unwrap_or_default(),
    );

    // Rules — sorted by id, full structural content.
    let mut rules: Vec<String> = spec
        .rules
        .iter()
        .map(|r| {
            format!(
                "{}|when={}|kind={}|class={:?}|symbol={:?}|by={:?}|from={:?}|to={:?}|scope={:?}|prio={}|cadence={:?}",
                r.id,
                r.when,
                r.then.kind,
                r.then.class,
                r.then.symbol,
                r.then.by,
                r.then.from,
                r.then.to,
                r.then.scope,
                r.priority,
                r.cadence,
            )
        })
        .collect();
    rules.sort();
    upd(&mut h, "rules", &rules.join(";;"));

    // Objective + benchmark (the benchmark is fixed: rebalanced base policy).
    upd(&mut h, "objective", objective.label());
    upd(&mut h, "benchmark", "rebalanced_base_policy");

    // Searched param NAMES + RANGES (NOT the param defaults) — sorted by name.
    let mut axis_descs: Vec<String> = axes
        .iter()
        .map(|a| format!("{}={}:{}:{}", a.name, a.min, a.max, a.step))
        .collect();
    axis_descs.sort();
    upd(&mut h, "search_space", &axis_descs.join(","));

    upd(&mut h, "fold_scheme", fold_scheme_descriptor);

    format!("{:x}", h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::portfolio_sim::optimize::parse_axis;
    use crate::analytics::portfolio_sim::spec::parse_str;

    fn spec_toml(commission: f64) -> String {
        format!(
            r#"
[model]
name = "t"
version = 1
base_currency = "USD"
[universe]
assets = [ {{ symbol = "RISK", class = "risk" }} ]
cash_class = "cash"
[base_policy]
targets = [ {{ class = "cash", target = 0.9 }}, {{ class = "risk", target = 0.1 }} ]
[constraints]
rebalance_cadence = "weekly"
commission_pct = {commission}
[[rules]]
id = "r"
when = "always"
then = {{ kind = "tilt", class = "risk", by = "tilt_size", from = "cash" }}
[params]
tilt_size = 0.1
"#
        )
    }

    fn mem() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn cumulative_trials_sum_across_same_topology() {
        let conn = mem();
        let spec = parse_str(&spec_toml(0.0)).unwrap();
        let axes = vec![parse_axis("tilt_size=0.0:0.8:0.1").unwrap()];
        let hash = topology_hash(&spec, &axes, Objective::Cagr, "fs");

        let run = |n: i64| NewOptimizeRun {
            topology_hash: hash.clone(),
            model_name: "t".into(),
            model_version: 1,
            objective: "cagr".into(),
            n_configs: n,
            winner_params: Some("{}".into()),
            verdict: "robust".into(),
            pbo: Some(0.1),
            dsr: Some(0.97),
            window_start: None,
            window_end: None,
            lockbox_start: None,
            lockbox_end: None,
        };
        append_run(&conn, &run(9)).unwrap();
        append_run(&conn, &run(9)).unwrap();
        // Cumulative = 9 + 9 = 18; two runs.
        assert_eq!(cumulative_trials_for_topology(&conn, &hash).unwrap(), 18);
        assert_eq!(run_count_for_topology(&conn, &hash).unwrap(), 2);
    }

    #[test]
    fn changed_topology_is_a_separate_family() {
        let conn = mem();
        let spec = parse_str(&spec_toml(0.0)).unwrap();

        // Baseline family.
        let axes_a = vec![parse_axis("tilt_size=0.0:0.8:0.1").unwrap()];
        let hash_a = topology_hash(&spec, &axes_a, Objective::Cagr, "fs");
        // (1) A widened param RANGE → different hash.
        let axes_b = vec![parse_axis("tilt_size=0.0:0.9:0.1").unwrap()];
        let hash_b = topology_hash(&spec, &axes_b, Objective::Cagr, "fs");
        // (2) A different OBJECTIVE → different hash.
        let hash_c = topology_hash(&spec, &axes_a, Objective::Sharpe, "fs");
        // (3) A different cost model → different hash.
        let spec_cost = parse_str(&spec_toml(0.002)).unwrap();
        let hash_d = topology_hash(&spec_cost, &axes_a, Objective::Cagr, "fs");

        assert_ne!(hash_a, hash_b, "range change → new family");
        assert_ne!(hash_a, hash_c, "objective change → new family");
        assert_ne!(hash_a, hash_d, "cost change → new family");

        // The same inputs reproduce the same hash (stable / deterministic).
        assert_eq!(hash_a, topology_hash(&spec, &axes_a, Objective::Cagr, "fs"));

        // Cumulative trials are tracked PER family — a changed topology does not
        // inherit the old family's trial count.
        let mk = |hash: &str| NewOptimizeRun {
            topology_hash: hash.to_string(),
            model_name: "t".into(),
            model_version: 1,
            objective: "cagr".into(),
            n_configs: 9,
            winner_params: None,
            verdict: "fragile".into(),
            pbo: None,
            dsr: None,
            window_start: None,
            window_end: None,
            lockbox_start: None,
            lockbox_end: None,
        };
        append_run(&conn, &mk(&hash_a)).unwrap();
        append_run(&conn, &mk(&hash_b)).unwrap();
        assert_eq!(cumulative_trials_for_topology(&conn, &hash_a).unwrap(), 9);
        assert_eq!(cumulative_trials_for_topology(&conn, &hash_b).unwrap(), 9);
        // Two distinct families recorded.
        assert_eq!(topology_summaries(&conn).unwrap().len(), 2);
    }

    #[test]
    fn unseen_topology_has_zero_cumulative() {
        let conn = mem();
        assert_eq!(
            cumulative_trials_for_topology(&conn, "deadbeef").unwrap(),
            0
        );
    }
}
