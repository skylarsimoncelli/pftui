//! `pftui research dossier <ta|cycles|macro>` — competence dossiers (R2).
//!
//! A dossier compiles, from EXISTING measured data only (no LLM, no
//! narrative), the evidence that a domain's signals and forecasts actually
//! carry edge:
//!
//!   (a) the domain's `signal_expectancy` rows (ta → `structure_`/`cyber_`
//!       signals; cycles → `cycle_`; macro → scenario-ledger discipline
//!       stats instead, since the macro domain has no registry signals);
//!   (b) the scored-forecast record for the domain's layers
//!       (ta → low+medium, cycles → medium+high, macro → macro);
//!   (c) worked precedents: the domain's 3 highest-|lift| SIGNIFICANT
//!       signals with their dated event lists and forward returns (reuses
//!       the `research events` internals).
//!
//! Auto-honest: empty sections render "no measured evidence yet" — never
//! invented prose. This is the "prove your comprehension with data,
//! examples, parallels" artifact the analyst prompts consume.

use anyhow::{bail, Result};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::forecast_misalignments::{self, MisalignmentRow};
use crate::db::signal_expectancy::{self, ExpectancyRow};
use crate::research::event_study::EventRow;
use crate::research::forecast_scoring::{self, build_report, ForecastReport};

/// How many worked precedents (signals) and dated events per precedent.
const PRECEDENT_SIGNALS: usize = 3;
const PRECEDENT_EVENTS: usize = 5;

/// Window for the scenario-ledger discipline stats (macro domain).
const SCENARIO_STATS_WINDOW_DAYS: i64 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DossierDomain {
    Ta,
    Cycles,
    Macro,
}

impl DossierDomain {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ta" => Ok(Self::Ta),
            "cycles" => Ok(Self::Cycles),
            "macro" => Ok(Self::Macro),
            other => bail!("unknown dossier domain '{other}'. Valid: ta, cycles, macro"),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Ta => "TA (market structure + Cyber)",
            Self::Cycles => "Cycles (cycle-theory engine)",
            Self::Macro => "Macro",
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Self::Ta => "ta",
            Self::Cycles => "cycles",
            Self::Macro => "macro",
        }
    }

    /// Signal-id prefixes owned by the domain (empty for macro).
    fn signal_prefixes(&self) -> &'static [&'static str] {
        match self {
            Self::Ta => &["structure_", "cyber_"],
            Self::Cycles => &["cycle_"],
            Self::Macro => &[],
        }
    }

    /// Forecast layers whose scored record the domain answers for.
    fn layers(&self) -> &'static [&'static str] {
        match self {
            Self::Ta => &["low", "medium"],
            Self::Cycles => &["medium", "high"],
            Self::Macro => &["macro"],
        }
    }
}

/// Scenario-ledger discipline stats (macro domain's section (a) substitute).
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioLedgerStats {
    pub window_days: i64,
    pub scenarios_total: i64,
    pub scenarios_active: i64,
    pub updates_in_window: i64,
    pub cumulative_abs_delta_pp: f64,
    pub distinct_scenarios_updated: i64,
}

/// One worked precedent: a measured-edge signal with its dated instances.
#[derive(Debug, Clone, Serialize)]
pub struct Precedent {
    pub signal_id: String,
    pub asset: String,
    pub horizon_days: i64,
    pub n_nonoverlap: i64,
    pub mean_lift: Option<f64>,
    pub hit_rate: Option<f64>,
    pub baseline_hit_rate: Option<f64>,
    pub p_value: Option<f64>,
    /// Most recent dated events with forward returns.
    pub events: Vec<EventRow>,
    pub n_events_total: usize,
}

/// The compiled dossier payload (JSON shape; markdown renders from this).
#[derive(Debug, Clone, Serialize)]
pub struct Dossier {
    pub domain: String,
    pub asset_filter: Option<String>,
    pub generated_at: String,
    pub layers: Vec<String>,
    /// (a) for ta/cycles — the domain's measured signal expectancy.
    pub expectancy: Vec<ExpectancyRow>,
    /// (a) for macro — scenario-ledger discipline instead of signals.
    pub scenario_stats: Option<ScenarioLedgerStats>,
    /// (b) the scored-forecast record for the domain's layers.
    pub forecast_report: ForecastReport,
    /// Active misalignments on the domain's layers.
    pub misalignments: Vec<MisalignmentRow>,
    /// (c) worked precedents from significant measured edges.
    pub precedents: Vec<Precedent>,
}

fn require_sqlite(backend: &BackendConnection) -> Result<&Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow::anyhow!("research dossier requires the SQLite backend"))
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        rusqlite::params![name],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

fn scenario_ledger_stats(conn: &Connection) -> Result<Option<ScenarioLedgerStats>> {
    if !table_exists(conn, "scenarios") {
        return Ok(None);
    }
    let (scenarios_total, scenarios_active): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(status = 'active'), 0) FROM scenarios",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let (updates, cum_delta, distinct): (i64, f64, i64) = if table_exists(conn, "scenario_updates")
    {
        let cutoff = (chrono::Utc::now().date_naive()
            - chrono::Duration::days(SCENARIO_STATS_WINDOW_DAYS))
        .format("%Y-%m-%d")
        .to_string();
        conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(ABS(new_probability - old_probability)), 0.0),
                    COUNT(DISTINCT scenario_id)
             FROM scenario_updates
             WHERE date(created_at) >= ?1
               AND old_probability IS NOT NULL AND new_probability IS NOT NULL",
            rusqlite::params![cutoff],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?
    } else {
        (0, 0.0, 0)
    };
    if scenarios_total == 0 && updates == 0 {
        return Ok(None);
    }
    Ok(Some(ScenarioLedgerStats {
        window_days: SCENARIO_STATS_WINDOW_DAYS,
        scenarios_total,
        scenarios_active,
        updates_in_window: updates,
        cumulative_abs_delta_pp: cum_delta,
        distinct_scenarios_updated: distinct,
    }))
}

/// Compile the dossier from the local ledgers. Pure read.
pub fn compile(
    backend: &BackendConnection,
    domain: DossierDomain,
    asset: Option<&str>,
) -> Result<Dossier> {
    let conn = require_sqlite(backend)?;
    let asset_upper = asset.map(|a| a.to_uppercase());

    // (a) Signal expectancy: every persisted row whose signal id carries one
    // of the domain's prefixes. The persisted asset is the deep series
    // (BTC-USD), so the filter tolerates the held-alias spelling.
    let prefixes = domain.signal_prefixes();
    let expectancy: Vec<ExpectancyRow> = if prefixes.is_empty() {
        Vec::new()
    } else {
        signal_expectancy::latest_rows(conn, None, None)?
            .into_iter()
            .filter(|r| prefixes.iter().any(|p| r.signal_id.starts_with(p)))
            .filter(|r| match &asset_upper {
                None => true,
                Some(a) => {
                    let row_asset = r.asset.to_uppercase();
                    row_asset == *a || row_asset == format!("{a}-USD")
                }
            })
            .collect()
    };

    // (a, macro) scenario-ledger discipline.
    let scenario_stats = if domain == DossierDomain::Macro {
        scenario_ledger_stats(conn)?
    } else {
        None
    };

    // (b) Scored-forecast record for the domain's layers.
    let layers: Vec<String> = domain.layers().iter().map(|l| l.to_string()).collect();
    let scored_rows: Vec<forecast_scoring::ScoredRow> =
        forecast_scoring::load_rows(conn, None, asset, None)?
            .into_iter()
            .filter(|r| layers.contains(&r.analyst))
            .collect();
    let forecast_report = build_report(&scored_rows);

    // Active misalignments on the domain's layers (asset-filtered).
    let misalignments: Vec<MisalignmentRow> =
        forecast_misalignments::active_misalignments(conn)?
            .into_iter()
            .filter(|m| layers.contains(&m.layer))
            .filter(|m| match &asset_upper {
                None => true,
                Some(a) => m.asset.to_uppercase() == *a,
            })
            .collect();

    // (c) Worked precedents: the 3 highest-|mean_lift| SIGNIFICANT
    // (signal, asset) cells, each with its dated event list. One precedent
    // per (signal, asset) pair — the best horizon represents it.
    let mut significant: Vec<&ExpectancyRow> = expectancy
        .iter()
        .filter(|r| r.significant && r.mean_lift.is_some())
        .collect();
    significant.sort_by(|a, b| {
        b.mean_lift
            .unwrap_or(0.0)
            .abs()
            .partial_cmp(&a.mean_lift.unwrap_or(0.0).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut precedents: Vec<Precedent> = Vec::new();
    for row in significant {
        if precedents.len() >= PRECEDENT_SIGNALS {
            break;
        }
        if precedents
            .iter()
            .any(|p| p.signal_id == row.signal_id && p.asset == row.asset)
        {
            continue;
        }
        let Some((_, study)) =
            crate::commands::research_harness::event_study_for(backend, &row.signal_id, &row.asset)?
        else {
            continue;
        };
        let n_events_total = study.events.len();
        let start = n_events_total.saturating_sub(PRECEDENT_EVENTS);
        precedents.push(Precedent {
            signal_id: row.signal_id.clone(),
            asset: row.asset.clone(),
            horizon_days: row.horizon_days,
            n_nonoverlap: row.n_nonoverlap,
            mean_lift: row.mean_lift,
            hit_rate: row.hit_rate,
            baseline_hit_rate: row.baseline_hit_rate,
            p_value: row.p_value,
            events: study.events[start..].to_vec(),
            n_events_total,
        });
    }

    Ok(Dossier {
        domain: domain.key().to_string(),
        asset_filter: asset_upper,
        generated_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        layers,
        expectancy,
        scenario_stats,
        forecast_report,
        misalignments,
        precedents,
    })
}

const NO_EVIDENCE: &str = "_no measured evidence yet_";

fn fmt_pct(v: Option<f64>) -> String {
    v.map(|x| format!("{:.0}%", x * 100.0))
        .unwrap_or_else(|| "—".to_string())
}

fn fmt_signed(v: Option<f64>, suffix: &str) -> String {
    v.map(|x| format!("{x:+.2}{suffix}"))
        .unwrap_or_else(|| "—".to_string())
}

/// Render the compiled dossier as a compact markdown-ish report.
pub fn render_markdown(d: &Dossier, domain: DossierDomain) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Competence dossier — {}{}\n\nGenerated {}. Every number below is MEASURED from the local ledgers \
         (signal_expectancy, forecast_scores, forecast_misalignments{}); empty sections say so.\n\n",
        domain.label(),
        d.asset_filter
            .as_deref()
            .map(|a| format!(" — {a}"))
            .unwrap_or_default(),
        d.generated_at,
        if domain == DossierDomain::Macro {
            ", scenario ledger"
        } else {
            ""
        }
    ));

    // (a) expectancy / scenario discipline
    if domain == DossierDomain::Macro {
        out.push_str("## Scenario-ledger discipline\n\n");
        match &d.scenario_stats {
            Some(s) => {
                out.push_str(&format!(
                    "- scenarios tracked: {} ({} active)\n\
                     - probability updates (last {}d): {} across {} scenario(s)\n\
                     - cumulative |Δprobability| (last {}d): {:.1}pp\n\n",
                    s.scenarios_total,
                    s.scenarios_active,
                    s.window_days,
                    s.updates_in_window,
                    s.distinct_scenarios_updated,
                    s.window_days,
                    s.cumulative_abs_delta_pp * 100.0
                ));
            }
            None => out.push_str(&format!("{NO_EVIDENCE}\n\n")),
        }
    } else {
        out.push_str("## Signal expectancy (measured, latest as_of)\n\n");
        if d.expectancy.is_empty() {
            out.push_str(&format!(
                "{NO_EVIDENCE} — run `pftui research backtest` to measure this domain's signals\n\n"
            ));
        } else {
            for r in &d.expectancy {
                let flag = if r.significant {
                    " *sig"
                } else if r.n_nonoverlap < crate::research::event_study::ANECDOTAL_N as i64 {
                    " ~anecdotal"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "- `{}` on {} {}d: n={} hit {} (base {}) mean {} (base {}) lift {} mae {} p={}{}\n",
                    r.signal_id,
                    r.asset,
                    r.horizon_days,
                    r.n_nonoverlap,
                    fmt_pct(r.hit_rate),
                    fmt_pct(r.baseline_hit_rate),
                    fmt_signed(r.mean_pct, "%"),
                    fmt_signed(r.baseline_mean_pct, "%"),
                    fmt_signed(r.mean_lift, "pp"),
                    fmt_signed(r.mae_mean, "%"),
                    r.p_value
                        .map(|p| format!("{p:.3}"))
                        .unwrap_or_else(|| "—".to_string()),
                    flag
                ));
            }
            out.push('\n');
        }
    }

    // (b) scored-forecast record
    out.push_str(&format!(
        "## Scored forecast record (layers: {})\n\n",
        d.layers.join(", ")
    ));
    if d.forecast_report.rows.is_empty() {
        out.push_str(&format!(
            "{NO_EVIDENCE} — run `pftui research forecasts score` first\n\n"
        ));
    } else {
        for r in &d.forecast_report.rows {
            let streak = if r.current_miss_streak > 0 {
                format!(
                    " — current streak: {} {} miss(es)",
                    r.current_miss_streak,
                    r.streak_call.as_deref().unwrap_or("?")
                )
            } else {
                String::new()
            };
            out.push_str(&format!(
                "- {} / {} ({}d): n={} hit {} mean-weighted {} bull→{} bear→{}{}\n",
                r.layer,
                r.asset,
                r.horizon_days,
                r.n_scored,
                r.hit_rate_pct
                    .map(|v| format!("{v:.0}%"))
                    .unwrap_or_else(|| "—".to_string()),
                r.mean_weighted_score
                    .map(|v| format!("{v:+.2}"))
                    .unwrap_or_else(|| "—".to_string()),
                fmt_signed(r.mean_realized_bull_pct, "%"),
                fmt_signed(r.mean_realized_bear_pct, "%"),
                streak
            ));
        }
        out.push('\n');
    }

    // Misalignments
    out.push_str("## Active misalignments in this domain\n\n");
    if d.misalignments.is_empty() {
        out.push_str("_none_\n\n");
    } else {
        for m in &d.misalignments {
            out.push_str(&format!(
                "- ⚠ {}/{} — {} consecutive wrong-sign {} calls ({} → {}, {:+.1}% cumulative against). On probation: not voting in convergence; prediction confidence capped 0.25.\n",
                m.layer,
                m.asset,
                m.streak_len,
                m.call,
                m.span_start,
                m.span_end,
                m.cum_realized_against_pct
            ));
        }
        out.push('\n');
    }

    // (c) worked precedents
    out.push_str("## Worked precedents — highest-|lift| significant signals\n\n");
    if d.precedents.is_empty() {
        out.push_str(&format!(
            "{NO_EVIDENCE} — no signal in this domain has a SIGNIFICANT measured edge; no precedent list will be invented\n"
        ));
    } else {
        for p in &d.precedents {
            out.push_str(&format!(
                "### `{}` on {} — {}d lift {} (hit {} vs base {}, n={}, p={})\n\n",
                p.signal_id,
                p.asset,
                p.horizon_days,
                fmt_signed(p.mean_lift, "pp"),
                fmt_pct(p.hit_rate),
                fmt_pct(p.baseline_hit_rate),
                p.n_nonoverlap,
                p.p_value
                    .map(|v| format!("{v:.3}"))
                    .unwrap_or_else(|| "—".to_string()),
            ));
            if p.events.is_empty() {
                out.push_str(&format!("{NO_EVIDENCE}\n\n"));
            } else {
                out.push_str(&format!(
                    "Last {} of {} dated instances (forward returns; `(overlap)` = excluded from stats):\n\n",
                    p.events.len(),
                    p.n_events_total
                ));
                for e in &p.events {
                    let outcomes: Vec<String> = e
                        .outcomes
                        .iter()
                        .map(|o| {
                            let tag = format!("{}d", o.horizon_days);
                            match o.return_pct {
                                Some(r) if o.kept => format!("{tag} {r:+.1}%"),
                                Some(r) => format!("{tag} {r:+.1}% (overlap)"),
                                None => format!("{tag} —"),
                            }
                        })
                        .collect();
                    out.push_str(&format!("- {}  {} — {}\n", e.date, outcomes.join("  "), e.detail));
                }
                out.push('\n');
            }
        }
    }

    out.trim_end().to_string()
}

/// `pftui research dossier <ta|cycles|macro> [--asset X] [--json]`
pub fn run(backend: &BackendConnection, domain: &str, asset: Option<&str>, json: bool) -> Result<()> {
    let domain = DossierDomain::parse(domain)?;
    let dossier = compile(backend, domain, asset)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&dossier)?);
        return Ok(());
    }
    println!("{}", render_markdown(&dossier, domain));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn make_backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: db::open_in_memory(),
        }
    }

    #[test]
    fn domain_parsing_and_mappings() {
        assert!(DossierDomain::parse("TA").is_ok());
        assert!(DossierDomain::parse("cycles").is_ok());
        assert!(DossierDomain::parse("macro").is_ok());
        assert!(DossierDomain::parse("vibes").is_err());
        assert_eq!(DossierDomain::Ta.layers(), &["low", "medium"]);
        assert_eq!(DossierDomain::Cycles.layers(), &["medium", "high"]);
        assert_eq!(DossierDomain::Macro.layers(), &["macro"]);
        assert_eq!(DossierDomain::Macro.signal_prefixes().len(), 0);
    }

    #[test]
    fn empty_database_renders_honest_empty_sections() {
        let backend = make_backend();
        let dossier = compile(&backend, DossierDomain::Ta, Some("GC=F")).unwrap();
        assert!(dossier.expectancy.is_empty());
        assert!(dossier.forecast_report.rows.is_empty());
        assert!(dossier.precedents.is_empty());
        let md = render_markdown(&dossier, DossierDomain::Ta);
        assert!(md.contains("# Competence dossier — TA"));
        assert!(md.contains("— GC=F"));
        // Honesty: empty sections state it, no invented prose.
        assert_eq!(
            md.matches("no measured evidence yet").count(),
            3,
            "expectancy, forecast record, and precedents must all declare emptiness:\n{md}"
        );
        assert!(md.contains("no precedent list will be invented"));
        assert!(md.contains("_none_"), "no misalignments section");
    }

    #[test]
    fn macro_dossier_uses_scenario_ledger_instead_of_signals() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        // Migrations may seed default scenarios — measure the delta.
        let baseline: i64 = conn
            .query_row("SELECT COUNT(*) FROM scenarios", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO scenarios (name, probability, status) VALUES ('Risk-On Rally Test', 0.3, 'active')",
            [],
        )
        .unwrap();
        let dossier = compile(&backend, DossierDomain::Macro, None).unwrap();
        let stats = dossier.scenario_stats.as_ref().expect("scenario stats");
        assert_eq!(stats.scenarios_total, baseline + 1);
        assert!(stats.scenarios_active >= 1);
        let md = render_markdown(&dossier, DossierDomain::Macro);
        assert!(md.contains("## Scenario-ledger discipline"));
        assert!(md.contains(&format!("scenarios tracked: {}", baseline + 1)));
        assert!(!md.contains("## Signal expectancy"));
    }

    #[test]
    fn dossier_compiles_synthetic_expectancy_record_and_misalignment() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();

        // (a) one ta expectancy row + one cycles row that must be filtered out.
        let mk_row = |signal_id: &str, significant: bool| {
            crate::db::signal_expectancy::ExpectancyRow {
                signal_id: signal_id.to_string(),
                signal_version: "1".to_string(),
                asset: "GC=F".to_string(),
                horizon_days: 90,
                as_of: "2026-06-10".to_string(),
                n_total: 14,
                n_evaluable: 13,
                n_nonoverlap: 12,
                hit_rate: Some(0.75),
                baseline_hit_rate: Some(0.55),
                hit_lift: Some(0.20),
                mean_pct: Some(4.2),
                baseline_mean_pct: Some(1.1),
                mean_lift: Some(3.1),
                median_pct: Some(3.5),
                p25: Some(1.0),
                p75: Some(6.0),
                mae_mean: Some(-2.0),
                mae_worst: Some(-7.5),
                mfe_mean: Some(6.0),
                p_value: Some(0.03),
                significant,
                computed_at: None,
            }
        };
        crate::db::signal_expectancy::upsert_rows(
            conn,
            &[
                mk_row("structure_weekly_flip_up", true),
                mk_row("cycle_band_enter_daily", true),
            ],
        )
        .unwrap();

        // (b) a scored forecast row for the medium layer on GC=F.
        crate::research::forecast_scoring::ensure_table(conn).unwrap();
        conn.execute(
            "INSERT INTO forecast_scores
                (view_history_id, analyst, asset, direction, conviction, horizon_days,
                 view_date, realized_pct, direction_hit, weighted_score, status)
             VALUES (1, 'medium', 'GC=F', 'bull', 3, 45, '2026-04-01', -5.0, 0, -0.6, 'scored')",
            [],
        )
        .unwrap();
        // A macro-layer row that must NOT appear in the ta dossier.
        conn.execute(
            "INSERT INTO forecast_scores
                (view_history_id, analyst, asset, direction, conviction, horizon_days,
                 view_date, realized_pct, direction_hit, weighted_score, status)
             VALUES (2, 'macro', 'GC=F', 'bull', 3, 365, '2026-04-01', 4.0, 1, 0.6, 'scored')",
            [],
        )
        .unwrap();

        // Active misalignment on (medium, GC=F).
        crate::db::forecast_misalignments::ensure_table(conn).unwrap();
        conn.execute(
            "INSERT INTO forecast_misalignments
                (layer, asset, detected_at, streak_len, call, span_start, span_end,
                 cum_realized_against_pct, status)
             VALUES ('medium', 'GC=F', '2026-06-01 00:00:00', 7, 'bull',
                     '2026-04-01', '2026-04-22', -40.5, 'active')",
            [],
        )
        .unwrap();

        let dossier = compile(&backend, DossierDomain::Ta, Some("GC=F")).unwrap();
        assert_eq!(dossier.expectancy.len(), 1, "cycle_ row must be filtered out");
        assert_eq!(dossier.expectancy[0].signal_id, "structure_weekly_flip_up");
        assert_eq!(dossier.forecast_report.rows.len(), 1, "macro layer excluded");
        assert_eq!(dossier.forecast_report.rows[0].layer, "medium");
        assert_eq!(dossier.misalignments.len(), 1);
        // No price history → the significant signal has no computable event
        // list, so the precedent section stays honestly empty.
        assert!(dossier.precedents.is_empty());

        let md = render_markdown(&dossier, DossierDomain::Ta);
        assert!(md.contains("`structure_weekly_flip_up` on GC=F 90d"));
        assert!(md.contains("lift +3.10pp"));
        assert!(md.contains("*sig"));
        assert!(md.contains("- medium / GC=F (45d): n=1 hit 0%"));
        assert!(md.contains("⚠ medium/GC=F — 7 consecutive wrong-sign bull calls"));
        assert!(md.contains("no precedent list will be invented"));

        // The cycles dossier picks up the cycle_ row and the medium layer too.
        let cycles = compile(&backend, DossierDomain::Cycles, Some("GC=F")).unwrap();
        assert_eq!(cycles.expectancy.len(), 1);
        assert_eq!(cycles.expectancy[0].signal_id, "cycle_band_enter_daily");
    }

    #[test]
    fn precedents_compile_from_price_history_when_available() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        // 600 daily bars with a long trend so structure events exist.
        let start = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        for i in 0..600i64 {
            let date = (start + chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
            // Rising staircase with periodic pullbacks → pivots + flips.
            let wave = (i as f64 / 17.0).sin() * 8.0;
            let close = 100.0 + i as f64 * 0.15 + wave;
            conn.execute(
                "INSERT INTO price_history (symbol, date, close, source)
                 VALUES ('GC=F', ?1, ?2, 'test')",
                rusqlite::params![date, format!("{close:.2}")],
            )
            .unwrap();
        }
        // Significant expectancy row for a structure signal on this series.
        let row = crate::db::signal_expectancy::ExpectancyRow {
            signal_id: "structure_daily_flip_up".to_string(),
            signal_version: "1".to_string(),
            asset: "GC=F".to_string(),
            horizon_days: 30,
            as_of: "2026-06-10".to_string(),
            n_total: 20,
            n_evaluable: 18,
            n_nonoverlap: 15,
            hit_rate: Some(0.8),
            baseline_hit_rate: Some(0.6),
            hit_lift: Some(0.2),
            mean_pct: Some(3.0),
            baseline_mean_pct: Some(1.0),
            mean_lift: Some(2.0),
            median_pct: None,
            p25: None,
            p75: None,
            mae_mean: None,
            mae_worst: None,
            mfe_mean: None,
            p_value: Some(0.01),
            significant: true,
            computed_at: None,
        };
        crate::db::signal_expectancy::upsert_rows(conn, &[row]).unwrap();

        let dossier = compile(&backend, DossierDomain::Ta, Some("GC=F")).unwrap();
        assert_eq!(dossier.precedents.len(), 1);
        let p = &dossier.precedents[0];
        assert_eq!(p.signal_id, "structure_daily_flip_up");
        assert!(p.n_events_total > 0, "synthetic waves must emit flip events");
        assert!(!p.events.is_empty());
        assert!(p.events.len() <= PRECEDENT_EVENTS);
        let md = render_markdown(&dossier, DossierDomain::Ta);
        assert!(md.contains("### `structure_daily_flip_up` on GC=F — 30d lift +2.00pp"));
        assert!(md.contains("dated instances"));
    }
}
