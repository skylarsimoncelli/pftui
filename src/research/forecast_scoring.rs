//! Retroactive forecast scoring — the system's own judgment stream
//! (`analyst_view_history`) converted into a scored corpus
//! (`forecast_scores`), immediately, so self-evaluation doesn't wait on the
//! calendar.
//!
//! # Horizon conventions (CANONICAL — the keystone)
//!
//! Each analyst layer's view implies a forecast horizon. These are fixed
//! conventions, encoded ONLY here (`layer_horizons`). Do not re-derive or
//! introduce alternates elsewhere.
//!
//! | Layer        | Horizon                | Kind          |
//! |--------------|------------------------|---------------|
//! | `low`        | 7 trading days         | trading rows of the priced series |
//! | `medium`     | 45 calendar days       | calendar      |
//! | `high`       | 135 calendar days      | calendar      |
//! | `macro`      | 365 calendar days      | calendar      |
//! | `blind`      | ALL FOUR horizons      | measurement layer — multi-horizon scoring shows where it is informative |
//! | `antithesis` | ALL FOUR horizons      | measurement layer — same rationale |
//!
//! "7 trading days" means 7 rows forward in the priced series' own daily
//! history (so crypto, which prints 7 days/week, uses its own calendar).
//! Calendar horizons are counted from the ENTRY close's date (the first
//! close ON/AFTER the view's recorded date); the exit is the first close
//! ON/AFTER `entry_date + N days`.
//!
//! # Scoring model
//!
//! For a view row (analyst, asset, direction, conviction −5..+5,
//! recorded_at) at one horizon:
//!
//! - `realized_pct` — forward return of the asset over the horizon from the
//!   first close ON/AFTER recorded_at's date. The series resolves with the
//!   `SYM` → `SYM-USD` deep fallback (first candidate that yields an entry
//!   close wins; `series_used` records which).
//! - conviction is direction-authoritative (`effective_conviction`):
//!   pre-#882 rows with `direction='bear' AND conviction>0` are treated as
//!   negative conviction.
//! - `direction_hit` — sign(conviction) == sign(realized) for non-neutral
//!   views (|conviction| ≥ 1). A realized move of exactly 0 matches neither
//!   sign. Neutral views (conviction 0 or direction='neutral') are recorded
//!   with realized returns but excluded from hit stats (`n_neutral`).
//! - `weighted_score` — sign-match (±1) × |conviction|/5, bounded [−1, +1].
//! - `status` — `scored` once entry+exit closes exist; `pending` while the
//!   horizon hasn't elapsed (or closes haven't arrived); `unscorable` when
//!   no price series exists for the asset under either symbol.
//!
//! Idempotent: a rescoring pass fills `pending`/`unscorable` rows and NEVER
//! mutates a `scored` row (L3 ledger contract: scoring fills outcome
//! fields, nothing else is rewritten).

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Duration, NaiveDate};
use rusqlite::{params, Connection};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;
use std::str::FromStr;

use crate::db::analyst_views::effective_conviction;

// ---------------------------------------------------------------------------
// Horizon conventions
// ---------------------------------------------------------------------------

/// How a horizon's day count is walked forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizonKind {
    /// N rows forward in the priced series' own daily history.
    Trading,
    /// First close on/after `entry_date + N calendar days`.
    Calendar,
}

/// One forecast horizon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Horizon {
    pub days: i64,
    pub kind: HorizonKind,
}

/// low — 7 trading days.
pub const HORIZON_LOW: Horizon = Horizon {
    days: 7,
    kind: HorizonKind::Trading,
};
/// medium — 45 calendar days.
pub const HORIZON_MEDIUM: Horizon = Horizon {
    days: 45,
    kind: HorizonKind::Calendar,
};
/// high — 135 calendar days.
pub const HORIZON_HIGH: Horizon = Horizon {
    days: 135,
    kind: HorizonKind::Calendar,
};
/// macro — 365 calendar days.
pub const HORIZON_MACRO: Horizon = Horizon {
    days: 365,
    kind: HorizonKind::Calendar,
};
/// All four canonical horizons (measurement layers score at every one).
pub const ALL_HORIZONS: [Horizon; 4] =
    [HORIZON_LOW, HORIZON_MEDIUM, HORIZON_HIGH, HORIZON_MACRO];

/// The canonical layer → horizon mapping. `None` for analysts outside the
/// six accepted layers (such rows are skipped, counted in the summary).
pub fn layer_horizons(analyst: &str) -> Option<&'static [Horizon]> {
    match analyst {
        "low" => Some(std::slice::from_ref(&HORIZON_LOW)),
        "medium" => Some(std::slice::from_ref(&HORIZON_MEDIUM)),
        "high" => Some(std::slice::from_ref(&HORIZON_HIGH)),
        "macro" => Some(std::slice::from_ref(&HORIZON_MACRO)),
        // Measurement layers: score at ALL FOUR horizons.
        "blind" | "antithesis" => Some(&ALL_HORIZONS),
        _ => None,
    }
}

/// Reverse lookup used by report rendering (7 is the only trading-day
/// horizon by convention).
pub fn horizon_kind_for_days(days: i64) -> HorizonKind {
    if days == HORIZON_LOW.days {
        HorizonKind::Trading
    } else {
        HorizonKind::Calendar
    }
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

/// Create the `forecast_scores` table (L3 ledger; see docs/db-catalog.toml).
///
/// One row per (view_history_id, horizon_days). Canonical layers produce one
/// row per view; measurement layers (blind/antithesis) produce four.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS forecast_scores (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            view_history_id INTEGER NOT NULL,
            analyst TEXT NOT NULL,
            asset TEXT NOT NULL,
            direction TEXT NOT NULL,
            conviction INTEGER NOT NULL,
            horizon_days INTEGER NOT NULL,
            view_date TEXT NOT NULL,
            realized_pct REAL,
            series_used TEXT,
            direction_hit INTEGER,
            weighted_score REAL,
            status TEXT NOT NULL DEFAULT 'pending',
            scored_at TEXT,
            UNIQUE (view_history_id, horizon_days)
        );
        CREATE INDEX IF NOT EXISTS idx_forecast_scores_layer_asset
            ON forecast_scores(analyst, asset);
        CREATE INDEX IF NOT EXISTS idx_forecast_scores_status
            ON forecast_scores(status);",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Scoring pass
// ---------------------------------------------------------------------------

/// Summary of one scoring pass.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScorePassSummary {
    /// (view, horizon) cells examined (not already `scored`).
    pub examined: usize,
    /// Cells that became `scored` this pass.
    pub newly_scored: usize,
    /// Subset of `newly_scored` that are neutral views (recorded, excluded
    /// from hit stats).
    pub neutral_scored: usize,
    /// Cells left `pending` (horizon not elapsed / closes not yet present).
    pub pending: usize,
    /// Cells left `unscorable` (no price series under SYM or SYM-USD).
    pub unscorable: usize,
    /// History rows skipped because the analyst is not one of the six
    /// accepted layers.
    pub skipped_unknown_layer: usize,
    /// Total `scored` rows in the corpus after this pass.
    pub corpus_scored_total: usize,
    /// Total rows in the corpus after this pass.
    pub corpus_total: usize,
}

struct HistoryRow {
    id: i64,
    analyst: String,
    asset: String,
    direction: String,
    conviction: i64,
    view_date: String,
}

enum Eval {
    Scored {
        realized_pct: f64,
        series_used: String,
    },
    Pending,
    Unscorable,
}

type SeriesCache = HashMap<String, Vec<(String, Decimal)>>;

fn series<'c>(
    cache: &'c mut SeriesCache,
    conn: &Connection,
    symbol: &str,
) -> Result<&'c [(String, Decimal)]> {
    if !cache.contains_key(symbol) {
        let mut stmt = conn.prepare(
            "SELECT date, close FROM price_history WHERE symbol = ?1 ORDER BY date ASC",
        )?;
        let rows = stmt
            .query_map(params![symbol], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let parsed: Vec<(String, Decimal)> = rows
            .into_iter()
            .filter_map(|(date, close)| {
                Decimal::from_str(&close).ok().map(|c| (date, c))
            })
            .collect();
        cache.insert(symbol.to_string(), parsed);
    }
    Ok(cache
        .get(symbol)
        .map(|v| v.as_slice())
        .unwrap_or_default())
}

/// `SYM` → `SYM-USD` deep-fallback candidate list.
fn series_candidates(asset_uc: &str) -> Vec<String> {
    let mut out = vec![asset_uc.to_string()];
    if !asset_uc.ends_with("-USD") {
        out.push(format!("{asset_uc}-USD"));
    }
    out
}

/// Evaluate one (view, horizon) cell against price history.
fn evaluate(
    cache: &mut SeriesCache,
    conn: &Connection,
    asset_uc: &str,
    view_date: &str,
    horizon: Horizon,
) -> Result<Eval> {
    let mut any_rows = false;
    for candidate in series_candidates(asset_uc) {
        let s = series(cache, conn, &candidate)?;
        if s.is_empty() {
            continue;
        }
        any_rows = true;
        // Entry: first close ON/AFTER the view date.
        let entry_idx = s.partition_point(|(d, _)| d.as_str() < view_date);
        if entry_idx >= s.len() {
            continue; // series exists but hasn't reached the view date yet
        }
        let (entry_date, entry_close) = &s[entry_idx];
        if *entry_close == Decimal::ZERO {
            continue; // unusable entry print; try the fallback series
        }
        let exit: Option<&(String, Decimal)> = match horizon.kind {
            HorizonKind::Trading => s.get(entry_idx + horizon.days as usize),
            HorizonKind::Calendar => {
                let Ok(entry_d) = NaiveDate::parse_from_str(
                    entry_date.get(..10).unwrap_or(entry_date),
                    "%Y-%m-%d",
                ) else {
                    return Ok(Eval::Pending);
                };
                let target = (entry_d + Duration::days(horizon.days))
                    .format("%Y-%m-%d")
                    .to_string();
                let exit_idx = s.partition_point(|(d, _)| d.as_str() < target.as_str());
                s.get(exit_idx)
            }
        };
        return Ok(match exit {
            Some((_, exit_close)) => {
                let realized = ((exit_close - entry_close) / entry_close
                    * Decimal::from(100))
                .to_f64()
                .unwrap_or(0.0);
                Eval::Scored {
                    realized_pct: realized,
                    series_used: candidate,
                }
            }
            None => Eval::Pending, // horizon not elapsed (or closes missing)
        });
    }
    Ok(if any_rows {
        Eval::Pending
    } else {
        Eval::Unscorable
    })
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        params![name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Score every `analyst_view_history` row not yet in `forecast_scores`
/// (the historical backfill IS this function's first run), plus fill
/// `pending`/`unscorable` cells whose data has since arrived. Idempotent:
/// rows with status `scored` are never touched again.
pub fn score_all(conn: &Connection) -> Result<ScorePassSummary> {
    ensure_table(conn)?;
    let mut summary = ScorePassSummary::default();
    if !table_exists(conn, "analyst_view_history")? {
        return Ok(summary);
    }

    let mut stmt = conn.prepare(
        "SELECT id, analyst, asset, direction, conviction, recorded_at
         FROM analyst_view_history ORDER BY recorded_at ASC, id ASC",
    )?;
    let history = stmt
        .query_map([], |row| {
            Ok(HistoryRow {
                id: row.get(0)?,
                analyst: row.get(1)?,
                asset: row.get(2)?,
                direction: row.get(3)?,
                conviction: row.get(4)?,
                view_date: row
                    .get::<_, String>(5)?
                    .get(..10)
                    .unwrap_or_default()
                    .to_string(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Existing cells: (view_history_id, horizon_days) → status.
    let mut existing: HashMap<(i64, i64), String> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT view_history_id, horizon_days, status FROM forecast_scores",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                (row.get::<_, i64>(0)?, row.get::<_, i64>(1)?),
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (key, status) = row?;
            existing.insert(key, status);
        }
    }

    let mut cache: SeriesCache = HashMap::new();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut upsert = conn.prepare(
        "INSERT INTO forecast_scores
            (view_history_id, analyst, asset, direction, conviction, horizon_days,
             view_date, realized_pct, series_used, direction_hit, weighted_score,
             status, scored_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(view_history_id, horizon_days) DO UPDATE SET
            realized_pct = excluded.realized_pct,
            series_used = excluded.series_used,
            direction_hit = excluded.direction_hit,
            weighted_score = excluded.weighted_score,
            status = excluded.status,
            scored_at = excluded.scored_at
         WHERE forecast_scores.status != 'scored'",
    )?;

    for view in &history {
        let Some(horizons) = layer_horizons(&view.analyst) else {
            summary.skipped_unknown_layer += 1;
            continue;
        };
        let asset_uc = view.asset.to_uppercase();
        // Direction is authoritative: pre-#882 rows may carry a sign that
        // contradicts the direction (bear with +conviction).
        let eff = effective_conviction(&view.direction, view.conviction);
        let neutral = view.direction == "neutral" || eff == 0;

        for horizon in horizons {
            if existing
                .get(&(view.id, horizon.days))
                .map(|s| s == "scored")
                .unwrap_or(false)
            {
                continue; // never mutate a scored row
            }
            summary.examined += 1;
            let eval = evaluate(&mut cache, conn, &asset_uc, &view.view_date, *horizon)?;
            let (realized, series_used, hit, weighted, status, scored_at) = match eval {
                Eval::Scored {
                    realized_pct,
                    series_used,
                } => {
                    let (hit, weighted) = if neutral {
                        (None, None)
                    } else {
                        let r_sign = if realized_pct > 0.0 {
                            1
                        } else if realized_pct < 0.0 {
                            -1
                        } else {
                            0
                        };
                        let c_sign = if eff > 0 { 1 } else { -1 };
                        let hit = r_sign == c_sign;
                        let weighted =
                            (if hit { 1.0 } else { -1.0 }) * (eff.abs() as f64 / 5.0);
                        (Some(hit), Some(weighted))
                    };
                    summary.newly_scored += 1;
                    if neutral {
                        summary.neutral_scored += 1;
                    }
                    (
                        Some(realized_pct),
                        Some(series_used),
                        hit,
                        weighted,
                        "scored",
                        Some(now.clone()),
                    )
                }
                Eval::Pending => {
                    summary.pending += 1;
                    (None, None, None, None, "pending", None)
                }
                Eval::Unscorable => {
                    summary.unscorable += 1;
                    (None, None, None, None, "unscorable", None)
                }
            };
            upsert.execute(params![
                view.id,
                view.analyst,
                asset_uc,
                view.direction,
                eff,
                horizon.days,
                view.view_date,
                realized,
                series_used,
                hit.map(|h| h as i64),
                weighted,
                status,
                scored_at,
            ])?;
        }
    }

    summary.corpus_scored_total = conn.query_row(
        "SELECT COUNT(*) FROM forecast_scores WHERE status = 'scored'",
        [],
        |row| row.get(0),
    )?;
    summary.corpus_total =
        conn.query_row("SELECT COUNT(*) FROM forecast_scores", [], |row| row.get(0))?;
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Loading scored rows
// ---------------------------------------------------------------------------

/// One persisted forecast-score row.
#[derive(Debug, Clone, Serialize)]
pub struct ScoredRow {
    pub view_history_id: i64,
    pub analyst: String,
    pub asset: String,
    pub direction: String,
    /// Effective (direction-authoritative) signed conviction.
    pub conviction: i64,
    pub horizon_days: i64,
    pub view_date: String,
    pub realized_pct: Option<f64>,
    pub series_used: Option<String>,
    pub direction_hit: Option<bool>,
    pub weighted_score: Option<f64>,
    pub status: String,
}

/// Load forecast-score rows, oldest first, with optional filters.
/// `window_days` keeps rows with `view_date >= today - N days`.
pub fn load_rows(
    conn: &Connection,
    layer: Option<&str>,
    asset: Option<&str>,
    window_days: Option<i64>,
) -> Result<Vec<ScoredRow>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT view_history_id, analyst, asset, direction, conviction, horizon_days,
                view_date, realized_pct, series_used, direction_hit, weighted_score, status
         FROM forecast_scores WHERE 1=1",
    );
    let mut args: Vec<String> = Vec::new();
    if let Some(layer) = layer {
        sql.push_str(&format!(" AND analyst = ?{}", args.len() + 1));
        args.push(layer.to_string());
    }
    if let Some(asset) = asset {
        sql.push_str(&format!(" AND UPPER(asset) = UPPER(?{})", args.len() + 1));
        args.push(asset.to_string());
    }
    if let Some(days) = window_days {
        let cutoff = (chrono::Utc::now().date_naive() - Duration::days(days))
            .format("%Y-%m-%d")
            .to_string();
        sql.push_str(&format!(" AND view_date >= ?{}", args.len() + 1));
        args.push(cutoff);
    }
    sql.push_str(" ORDER BY view_date ASC, view_history_id ASC, horizon_days ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok(ScoredRow {
                view_history_id: row.get(0)?,
                analyst: row.get(1)?,
                asset: row.get(2)?,
                direction: row.get(3)?,
                conviction: row.get(4)?,
                horizon_days: row.get(5)?,
                view_date: row.get(6)?,
                realized_pct: row.get(7)?,
                series_used: row.get(8)?,
                direction_hit: row.get::<_, Option<i64>>(9)?.map(|v| v != 0),
                weighted_score: row.get(10)?,
                status: row.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Report aggregation
// ---------------------------------------------------------------------------

/// Layer rendering order (canonical voting layers first, then measurement).
pub const LAYER_ORDER: [&str; 6] = ["low", "medium", "high", "macro", "blind", "antithesis"];

fn layer_rank(layer: &str) -> usize {
    LAYER_ORDER
        .iter()
        .position(|l| *l == layer)
        .unwrap_or(LAYER_ORDER.len())
}

/// Aggregates for one (layer × horizon × asset) cell — or a per-layer
/// TOTALS row (asset = `"TOTAL"`).
#[derive(Debug, Clone, Serialize)]
pub struct ReportRow {
    pub layer: String,
    pub asset: String,
    pub horizon_days: i64,
    /// Non-neutral scored rows.
    pub n_scored: usize,
    pub n_neutral: usize,
    pub n_pending: usize,
    pub hits: usize,
    pub hit_rate_pct: Option<f64>,
    pub mean_weighted_score: Option<f64>,
    /// Mean realized return when the call was bullish.
    pub mean_realized_bull_pct: Option<f64>,
    /// Mean realized return when the call was bearish.
    pub mean_realized_bear_pct: Option<f64>,
    /// Current consecutive same-sign-miss streak (the gold-failure number).
    /// 0 for TOTALS rows (a cross-asset streak isn't meaningful).
    pub current_miss_streak: usize,
    /// `bull`/`bear` sign of the streak's calls, when a streak exists.
    pub streak_call: Option<String>,
}

/// Full report payload.
#[derive(Debug, Clone, Serialize)]
pub struct ForecastReport {
    pub rows: Vec<ReportRow>,
    pub totals: Vec<ReportRow>,
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// Tail streak of one group's rows (oldest-first input): consecutive
/// most-recent non-neutral scored MISSES whose calls share a sign. A hit or
/// a sign flip breaks it; neutral and non-scored rows are skipped.
#[derive(Debug, Clone, Serialize)]
pub struct StreakInfo {
    pub len: usize,
    /// `bull` or `bear` — the sign of the missed calls.
    pub call: String,
    pub first_view_date: String,
    pub last_view_date: String,
    /// Sum of the streak rows' realized horizon returns (overlapping
    /// windows — indicative magnitude, not a portfolio return).
    pub cumulative_realized_pct: f64,
}

pub fn tail_streak(rows: &[&ScoredRow]) -> Option<StreakInfo> {
    let mut info: Option<StreakInfo> = None;
    for row in rows.iter().rev() {
        if row.status != "scored" {
            continue;
        }
        let Some(hit) = row.direction_hit else {
            continue; // neutral — excluded from hit stats, doesn't break
        };
        let sign = if row.conviction > 0 { 1 } else { -1 };
        if hit {
            break;
        }
        match &mut info {
            None => {
                info = Some(StreakInfo {
                    len: 1,
                    call: if sign > 0 { "bull" } else { "bear" }.to_string(),
                    first_view_date: row.view_date.clone(),
                    last_view_date: row.view_date.clone(),
                    cumulative_realized_pct: row.realized_pct.unwrap_or(0.0),
                });
            }
            Some(existing) => {
                let existing_sign = if existing.call == "bull" { 1 } else { -1 };
                if sign != existing_sign {
                    break;
                }
                existing.len += 1;
                existing.first_view_date = row.view_date.clone();
                existing.cumulative_realized_pct += row.realized_pct.unwrap_or(0.0);
            }
        }
    }
    info
}

fn aggregate(layer: &str, asset: &str, horizon_days: i64, rows: &[&ScoredRow]) -> ReportRow {
    let scored: Vec<&&ScoredRow> = rows.iter().filter(|r| r.status == "scored").collect();
    let non_neutral: Vec<&&ScoredRow> = scored
        .iter()
        .filter(|r| r.direction_hit.is_some())
        .copied()
        .collect();
    let n_neutral = scored.len() - non_neutral.len();
    let n_pending = rows.iter().filter(|r| r.status == "pending").count();
    let hits = non_neutral
        .iter()
        .filter(|r| r.direction_hit == Some(true))
        .count();
    let weighted: Vec<f64> = non_neutral
        .iter()
        .filter_map(|r| r.weighted_score)
        .collect();
    let bull: Vec<f64> = non_neutral
        .iter()
        .filter(|r| r.conviction > 0)
        .filter_map(|r| r.realized_pct)
        .collect();
    let bear: Vec<f64> = non_neutral
        .iter()
        .filter(|r| r.conviction < 0)
        .filter_map(|r| r.realized_pct)
        .collect();
    let streak = if asset == "TOTAL" {
        None
    } else {
        tail_streak(rows)
    };
    ReportRow {
        layer: layer.to_string(),
        asset: asset.to_string(),
        horizon_days,
        n_scored: non_neutral.len(),
        n_neutral,
        n_pending,
        hits,
        hit_rate_pct: if non_neutral.is_empty() {
            None
        } else {
            Some(hits as f64 / non_neutral.len() as f64 * 100.0)
        },
        mean_weighted_score: mean(&weighted),
        mean_realized_bull_pct: mean(&bull),
        mean_realized_bear_pct: mean(&bear),
        current_miss_streak: streak.as_ref().map(|s| s.len).unwrap_or(0),
        streak_call: streak.map(|s| s.call),
    }
}

/// Build the per-(layer × horizon × asset) report with per-(layer ×
/// horizon) TOTALS rows. Canonical layers have exactly one horizon, so this
/// reads as layer × asset; measurement layers fan out per horizon.
pub fn build_report(rows: &[ScoredRow]) -> ForecastReport {
    let mut groups: HashMap<(String, i64, String), Vec<&ScoredRow>> = HashMap::new();
    let mut layer_groups: HashMap<(String, i64), Vec<&ScoredRow>> = HashMap::new();
    for row in rows {
        groups
            .entry((row.analyst.clone(), row.horizon_days, row.asset.clone()))
            .or_default()
            .push(row);
        layer_groups
            .entry((row.analyst.clone(), row.horizon_days))
            .or_default()
            .push(row);
    }
    let mut out: Vec<ReportRow> = groups
        .iter()
        .map(|((layer, horizon, asset), rows)| aggregate(layer, asset, *horizon, rows))
        .collect();
    out.sort_by(|a, b| {
        layer_rank(&a.layer)
            .cmp(&layer_rank(&b.layer))
            .then(a.horizon_days.cmp(&b.horizon_days))
            .then(a.asset.cmp(&b.asset))
    });
    let mut totals: Vec<ReportRow> = layer_groups
        .iter()
        .map(|((layer, horizon), rows)| aggregate(layer, "TOTAL", *horizon, rows))
        .collect();
    totals.sort_by(|a, b| {
        layer_rank(&a.layer)
            .cmp(&layer_rank(&b.layer))
            .then(a.horizon_days.cmp(&b.horizon_days))
    });
    ForecastReport { rows: out, totals }
}

// ---------------------------------------------------------------------------
// Streak feed (R2's misalignment-tripwire input)
// ---------------------------------------------------------------------------

/// One (layer, horizon, asset) whose CURRENT consecutive wrong-sign streak
/// meets the threshold.
#[derive(Debug, Clone, Serialize)]
pub struct StreakRow {
    pub layer: String,
    pub asset: String,
    pub horizon_days: i64,
    pub streak_len: usize,
    /// `bull` or `bear` — what the layer kept calling.
    pub call: String,
    pub first_view_date: String,
    pub last_view_date: String,
    /// Sum of the streak rows' realized horizon returns — the cumulative
    /// move against the calls (overlapping windows; indicative).
    pub cumulative_realized_pct: f64,
}

/// Every (layer, horizon, asset) whose current wrong-sign streak ≥
/// `threshold`, longest first. Stable, structured output — R2's tripwire
/// consumes this.
pub fn current_streaks(rows: &[ScoredRow], threshold: usize) -> Vec<StreakRow> {
    let mut groups: HashMap<(String, i64, String), Vec<&ScoredRow>> = HashMap::new();
    for row in rows {
        groups
            .entry((row.analyst.clone(), row.horizon_days, row.asset.clone()))
            .or_default()
            .push(row);
    }
    let mut out: Vec<StreakRow> = groups
        .iter()
        .filter_map(|((layer, horizon, asset), rows)| {
            tail_streak(rows).and_then(|s| {
                if s.len >= threshold {
                    Some(StreakRow {
                        layer: layer.clone(),
                        asset: asset.clone(),
                        horizon_days: *horizon,
                        streak_len: s.len,
                        call: s.call,
                        first_view_date: s.first_view_date,
                        last_view_date: s.last_view_date,
                        cumulative_realized_pct: s.cumulative_realized_pct,
                    })
                } else {
                    None
                }
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.streak_len
            .cmp(&a.streak_len)
            .then(layer_rank(&a.layer).cmp(&layer_rank(&b.layer)))
            .then(a.horizon_days.cmp(&b.horizon_days))
            .then(a.asset.cmp(&b.asset))
    });
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'test',
                PRIMARY KEY (symbol, date)
            );
            CREATE TABLE analyst_view_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                analyst TEXT NOT NULL,
                asset TEXT NOT NULL,
                direction TEXT NOT NULL,
                conviction INTEGER NOT NULL,
                reasoning_summary TEXT NOT NULL DEFAULT '',
                recorded_at TEXT NOT NULL
            );",
        )
        .expect("schema");
        conn
    }

    fn insert_view(
        conn: &Connection,
        analyst: &str,
        asset: &str,
        direction: &str,
        conviction: i64,
        recorded_at: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![analyst, asset, direction, conviction, recorded_at],
        )
        .expect("insert view");
        conn.last_insert_rowid()
    }

    fn insert_close(conn: &Connection, symbol: &str, date: &str, close: &str) {
        conn.execute(
            "INSERT INTO price_history (symbol, date, close) VALUES (?1, ?2, ?3)",
            params![symbol, date, close],
        )
        .expect("insert close");
    }

    /// Insert N daily closes for `symbol` starting at `start`, generated by
    /// `f(i)` for i in 0..n.
    fn insert_daily_closes(
        conn: &Connection,
        symbol: &str,
        start: &str,
        n: i64,
        f: impl Fn(i64) -> f64,
    ) {
        let start = NaiveDate::parse_from_str(start, "%Y-%m-%d").expect("date");
        for i in 0..n {
            let date = (start + Duration::days(i)).format("%Y-%m-%d").to_string();
            insert_close(conn, symbol, &date, &format!("{:.2}", f(i)));
        }
    }

    fn load_one(conn: &Connection, view_id: i64, horizon: i64) -> ScoredRow {
        load_rows(conn, None, None, None)
            .expect("load")
            .into_iter()
            .find(|r| r.view_history_id == view_id && r.horizon_days == horizon)
            .expect("row present")
    }

    #[test]
    fn horizon_mapping_is_canonical() {
        assert_eq!(layer_horizons("low"), Some(&[HORIZON_LOW][..]));
        assert_eq!(layer_horizons("medium"), Some(&[HORIZON_MEDIUM][..]));
        assert_eq!(layer_horizons("high"), Some(&[HORIZON_HIGH][..]));
        assert_eq!(layer_horizons("macro"), Some(&[HORIZON_MACRO][..]));
        assert_eq!(layer_horizons("blind"), Some(&ALL_HORIZONS[..]));
        assert_eq!(layer_horizons("antithesis"), Some(&ALL_HORIZONS[..]));
        assert_eq!(layer_horizons("adversary"), None);
        assert_eq!(HORIZON_LOW.days, 7);
        assert_eq!(HORIZON_LOW.kind, HorizonKind::Trading);
        assert_eq!(HORIZON_MEDIUM.days, 45);
        assert_eq!(HORIZON_MEDIUM.kind, HorizonKind::Calendar);
        assert_eq!(HORIZON_HIGH.days, 135);
        assert_eq!(HORIZON_MACRO.days, 365);
        assert_eq!(horizon_kind_for_days(7), HorizonKind::Trading);
        assert_eq!(horizon_kind_for_days(45), HorizonKind::Calendar);
    }

    #[test]
    fn scoring_math_low_trading_days_exact() {
        let conn = test_conn();
        // 100, 101, ..., entry at first close on/after 2026-01-05 → 100 at
        // index 0 IF series starts there. Use start 2026-01-05.
        insert_daily_closes(&conn, "AAA", "2026-01-05", 20, |i| 100.0 + i as f64);
        let id = insert_view(&conn, "low", "AAA", "bull", 4, "2026-01-05 09:00:00");
        let summary = score_all(&conn).expect("score");
        assert_eq!(summary.newly_scored, 1);
        let row = load_one(&conn, id, 7);
        assert_eq!(row.status, "scored");
        // entry 100 (2026-01-05), exit = 7 trading rows later = 107.
        let realized = row.realized_pct.expect("realized");
        assert!((realized - 7.0).abs() < 1e-9, "realized {realized}");
        assert_eq!(row.direction_hit, Some(true));
        let w = row.weighted_score.expect("weighted");
        assert!((w - 0.8).abs() < 1e-9, "weighted {w}"); // +1 × 4/5
        assert_eq!(row.series_used.as_deref(), Some("AAA"));
        assert_eq!(row.conviction, 4);
    }

    #[test]
    fn scoring_math_medium_calendar_exact_and_miss() {
        let conn = test_conn();
        // Declining series: 200 - i. Entry 2026-01-02 → 199 (i=1, start 01-01).
        insert_daily_closes(&conn, "BBB", "2026-01-01", 60, |i| 200.0 - i as f64);
        let id = insert_view(&conn, "medium", "BBB", "bull", 5, "2026-01-02 12:00:00");
        score_all(&conn).expect("score");
        let row = load_one(&conn, id, 45);
        assert_eq!(row.status, "scored");
        // entry 199 at 01-02 (i=1); exit = first close on/after 02-16 (i=46) = 154.
        let expected = (154.0 - 199.0) / 199.0 * 100.0;
        let realized = row.realized_pct.expect("realized");
        assert!((realized - expected).abs() < 1e-6, "realized {realized}");
        assert_eq!(row.direction_hit, Some(false));
        let w = row.weighted_score.expect("weighted");
        assert!((w + 1.0).abs() < 1e-9, "weighted {w}"); // −1 × 5/5
    }

    #[test]
    fn neutral_views_recorded_but_excluded_from_hit_stats() {
        let conn = test_conn();
        insert_daily_closes(&conn, "CCC", "2026-01-01", 20, |i| 100.0 + i as f64);
        let id_dir = insert_view(&conn, "low", "CCC", "neutral", 0, "2026-01-01 00:00:00");
        // direction neutral with nonzero conviction is still neutral
        let id_conv = insert_view(&conn, "low", "CCC", "neutral", 2, "2026-01-02 00:00:00");
        let summary = score_all(&conn).expect("score");
        assert_eq!(summary.newly_scored, 2);
        assert_eq!(summary.neutral_scored, 2);
        for id in [id_dir, id_conv] {
            let row = load_one(&conn, id, 7);
            assert_eq!(row.status, "scored");
            assert!(row.realized_pct.is_some());
            assert_eq!(row.direction_hit, None);
            assert_eq!(row.weighted_score, None);
        }
        let report = build_report(&load_rows(&conn, None, None, None).expect("rows"));
        let cell = report
            .rows
            .iter()
            .find(|r| r.asset == "CCC")
            .expect("cell");
        assert_eq!(cell.n_scored, 0);
        assert_eq!(cell.n_neutral, 2);
        assert_eq!(cell.hit_rate_pct, None);
    }

    #[test]
    fn bear_with_positive_conviction_is_treated_as_bearish() {
        let conn = test_conn();
        // Falling series: a bear call should HIT.
        insert_daily_closes(&conn, "DDD", "2026-01-01", 20, |i| 100.0 - i as f64);
        // Pre-#882 sign mess: direction bear, conviction +3.
        let id = insert_view(&conn, "low", "DDD", "bear", 3, "2026-01-01 00:00:00");
        score_all(&conn).expect("score");
        let row = load_one(&conn, id, 7);
        assert_eq!(row.conviction, -3, "stored conviction must be effective/negative");
        assert_eq!(row.direction_hit, Some(true));
        let w = row.weighted_score.expect("weighted");
        assert!((w - 0.6).abs() < 1e-9, "weighted {w}"); // +1 × 3/5
    }

    #[test]
    fn pending_then_scored_idempotence() {
        let conn = test_conn();
        // Only 3 closes — a 7-trading-day horizon can't resolve yet.
        insert_daily_closes(&conn, "EEE", "2026-01-01", 3, |i| 100.0 + i as f64);
        let id = insert_view(&conn, "low", "EEE", "bull", 2, "2026-01-01 00:00:00");
        let s1 = score_all(&conn).expect("score 1");
        assert_eq!(s1.pending, 1);
        assert_eq!(s1.newly_scored, 0);
        assert_eq!(load_one(&conn, id, 7).status, "pending");

        // History arrives → pending fills.
        insert_daily_closes(&conn, "EEE", "2026-01-04", 10, |i| 103.0 + i as f64);
        let s2 = score_all(&conn).expect("score 2");
        assert_eq!(s2.newly_scored, 1);
        let row = load_one(&conn, id, 7);
        assert_eq!(row.status, "scored");
        let realized = row.realized_pct.expect("realized");

        // Mutate the underlying prices, rescore: the scored row must NOT move.
        conn.execute("UPDATE price_history SET close = '999'", [])
            .expect("tamper");
        let s3 = score_all(&conn).expect("score 3");
        assert_eq!(s3.examined, 0, "scored cells are never re-examined");
        let row_after = load_one(&conn, id, 7);
        assert_eq!(row_after.status, "scored");
        assert_eq!(row_after.realized_pct, Some(realized));
    }

    #[test]
    fn series_fallback_to_usd_twin_is_recorded() {
        let conn = test_conn();
        insert_daily_closes(&conn, "BTC-USD", "2026-01-01", 20, |i| 50000.0 + i as f64);
        let id = insert_view(&conn, "low", "BTC", "bull", 1, "2026-01-01 00:00:00");
        score_all(&conn).expect("score");
        let row = load_one(&conn, id, 7);
        assert_eq!(row.status, "scored");
        assert_eq!(row.series_used.as_deref(), Some("BTC-USD"));
    }

    #[test]
    fn asset_with_no_series_is_unscorable() {
        let conn = test_conn();
        let id = insert_view(&conn, "low", "ZZZ", "bull", 3, "2026-01-01 00:00:00");
        let summary = score_all(&conn).expect("score");
        assert_eq!(summary.unscorable, 1);
        assert_eq!(load_one(&conn, id, 7).status, "unscorable");
    }

    #[test]
    fn measurement_layers_score_all_four_horizons() {
        let conn = test_conn();
        // 400 daily closes: enough for every horizon including 365d.
        insert_daily_closes(&conn, "FFF", "2025-01-01", 400, |i| 100.0 + i as f64);
        let id = insert_view(&conn, "blind", "FFF", "bull", 2, "2025-01-01 00:00:00");
        let summary = score_all(&conn).expect("score");
        assert_eq!(summary.newly_scored, 4);
        for days in [7, 45, 135, 365] {
            let row = load_one(&conn, id, days);
            assert_eq!(row.status, "scored", "horizon {days}");
            assert_eq!(row.direction_hit, Some(true), "horizon {days}");
        }
    }

    #[test]
    fn unknown_layer_rows_are_skipped() {
        let conn = test_conn();
        insert_daily_closes(&conn, "GGG", "2026-01-01", 20, |i| 100.0 + i as f64);
        insert_view(&conn, "adversary", "GGG", "bull", 3, "2026-01-01 00:00:00");
        let summary = score_all(&conn).expect("score");
        assert_eq!(summary.skipped_unknown_layer, 1);
        assert_eq!(summary.examined, 0);
        assert!(load_rows(&conn, None, None, None).expect("rows").is_empty());
    }

    fn synthetic_row(
        view_date: &str,
        conviction: i64,
        hit: Option<bool>,
        realized: Option<f64>,
    ) -> ScoredRow {
        ScoredRow {
            view_history_id: 0,
            analyst: "low".to_string(),
            asset: "GC=F".to_string(),
            direction: if conviction > 0 { "bull" } else { "bear" }.to_string(),
            conviction,
            horizon_days: 7,
            view_date: view_date.to_string(),
            realized_pct: realized,
            series_used: Some("GC=F".to_string()),
            direction_hit: hit,
            weighted_score: hit.map(|h| {
                (if h { 1.0 } else { -1.0 }) * conviction.abs() as f64 / 5.0
            }),
            status: "scored".to_string(),
        }
    }

    #[test]
    fn streak_at_end_of_history() {
        let rows = [
            synthetic_row("2026-01-01", 3, Some(true), Some(1.0)),
            synthetic_row("2026-01-02", 3, Some(false), Some(-2.0)),
            synthetic_row("2026-01-03", 4, Some(false), Some(-3.0)),
            synthetic_row("2026-01-04", 2, Some(false), Some(-1.5)),
        ];
        let refs: Vec<&ScoredRow> = rows.iter().collect();
        let streak = tail_streak(&refs).expect("streak");
        assert_eq!(streak.len, 3);
        assert_eq!(streak.call, "bull");
        assert_eq!(streak.first_view_date, "2026-01-02");
        assert_eq!(streak.last_view_date, "2026-01-04");
        assert!((streak.cumulative_realized_pct + 6.5).abs() < 1e-9);
    }

    #[test]
    fn streak_broken_by_hit_and_by_sign_flip() {
        // A hit at the end → no streak.
        let rows = [
            synthetic_row("2026-01-01", 3, Some(false), Some(-2.0)),
            synthetic_row("2026-01-02", 3, Some(true), Some(1.0)),
        ];
        let refs: Vec<&ScoredRow> = rows.iter().collect();
        assert!(tail_streak(&refs).is_none());

        // Sign flip breaks the streak: bear miss then bull misses → 2.
        let rows = [
            synthetic_row("2026-01-01", -3, Some(false), Some(2.0)),
            synthetic_row("2026-01-02", 3, Some(false), Some(-2.0)),
            synthetic_row("2026-01-03", 3, Some(false), Some(-1.0)),
        ];
        let refs: Vec<&ScoredRow> = rows.iter().collect();
        let streak = tail_streak(&refs).expect("streak");
        assert_eq!(streak.len, 2);
        assert_eq!(streak.call, "bull");
    }

    #[test]
    fn streak_skips_neutral_rows_without_breaking() {
        let mut neutral = synthetic_row("2026-01-02", 0, None, Some(0.5));
        neutral.direction = "neutral".to_string();
        let rows = [
            synthetic_row("2026-01-01", 3, Some(false), Some(-2.0)),
            neutral,
            synthetic_row("2026-01-03", 3, Some(false), Some(-1.0)),
        ];
        let refs: Vec<&ScoredRow> = rows.iter().collect();
        let streak = tail_streak(&refs).expect("streak");
        assert_eq!(streak.len, 2);
    }

    #[test]
    fn current_streaks_applies_threshold_and_sorts() {
        let mut rows: Vec<ScoredRow> = (0..6)
            .map(|i| synthetic_row(&format!("2026-01-0{}", i + 1), 3, Some(false), Some(-1.0)))
            .collect();
        // A second asset with only a 2-miss streak.
        let mut other = synthetic_row("2026-01-01", -2, Some(false), Some(1.0));
        other.asset = "SI=F".to_string();
        let mut other2 = synthetic_row("2026-01-02", -2, Some(false), Some(1.5));
        other2.asset = "SI=F".to_string();
        rows.push(other);
        rows.push(other2);

        let streaks = current_streaks(&rows, 5);
        assert_eq!(streaks.len(), 1);
        assert_eq!(streaks[0].asset, "GC=F");
        assert_eq!(streaks[0].streak_len, 6);
        assert_eq!(streaks[0].call, "bull");
        assert!((streaks[0].cumulative_realized_pct + 6.0).abs() < 1e-9);

        let streaks = current_streaks(&rows, 2);
        assert_eq!(streaks.len(), 2);
        assert_eq!(streaks[0].asset, "GC=F", "longest first");
        assert_eq!(streaks[1].asset, "SI=F");
    }

    #[test]
    fn report_aggregates_hit_rate_weighted_and_bull_bear_means() {
        let rows = vec![
            synthetic_row("2026-01-01", 5, Some(true), Some(4.0)),
            synthetic_row("2026-01-02", 5, Some(false), Some(-2.0)),
            synthetic_row("2026-01-03", -5, Some(true), Some(-6.0)),
        ];
        let report = build_report(&rows);
        let cell = report
            .rows
            .iter()
            .find(|r| r.asset == "GC=F")
            .expect("cell");
        assert_eq!(cell.n_scored, 3);
        assert_eq!(cell.hits, 2);
        let hr = cell.hit_rate_pct.expect("hit rate");
        assert!((hr - 200.0 / 3.0).abs() < 1e-9);
        // weighted: +1, −1, +1 → mean 1/3
        let mw = cell.mean_weighted_score.expect("mean weighted");
        assert!((mw - 1.0 / 3.0).abs() < 1e-9);
        // bull realized: 4.0, −2.0 → 1.0; bear realized: −6.0
        assert!((cell.mean_realized_bull_pct.expect("bull") - 1.0).abs() < 1e-9);
        assert!((cell.mean_realized_bear_pct.expect("bear") + 6.0).abs() < 1e-9);
        // TOTALS row exists for the layer
        let total = report
            .totals
            .iter()
            .find(|r| r.layer == "low" && r.horizon_days == 7)
            .expect("totals");
        assert_eq!(total.asset, "TOTAL");
        assert_eq!(total.n_scored, 3);
        assert_eq!(total.current_miss_streak, 0);
    }

    #[test]
    fn load_rows_filters_by_layer_and_asset() {
        let conn = test_conn();
        insert_daily_closes(&conn, "AAA", "2026-01-01", 20, |i| 100.0 + i as f64);
        insert_daily_closes(&conn, "BBB", "2026-01-01", 20, |i| 100.0 + i as f64);
        insert_view(&conn, "low", "AAA", "bull", 3, "2026-01-01 00:00:00");
        insert_view(&conn, "low", "BBB", "bull", 3, "2026-01-01 00:00:00");
        score_all(&conn).expect("score");
        assert_eq!(load_rows(&conn, None, None, None).expect("all").len(), 2);
        assert_eq!(
            load_rows(&conn, Some("low"), Some("aaa"), None)
                .expect("filtered")
                .len(),
            1
        );
        assert_eq!(
            load_rows(&conn, Some("macro"), None, None)
                .expect("layer")
                .len(),
            0
        );
    }
}
