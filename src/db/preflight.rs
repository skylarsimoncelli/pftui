//! `preflight` — pre-flight check at prediction write time.
//!
//! Given a draft prediction (claim text + optional symbol/timeframe/
//! conviction/layer), classify it into a `cluster_key` via the existing
//! `crate::db::clusters::classify_claim` keyword classifier and assemble a
//! cross-table briefing of everything the substrate already knows about
//! similar predictions, so the analyst sees calibration / fragments /
//! co-failures BEFORE saving the prediction.
//!
//! Returned struct includes:
//! - The classified `cluster_key` (or None when no rule matches).
//! - Matched `reasoning_fragments` via `lesson_fragment_edges` for that cluster.
//! - The single `calibration_adjustments` row for (layer, topic, conviction)
//!   when all three are provided.
//! - Top-3 similar past `user_predictions` in the same cluster with scored
//!   outcomes (cluster membership detected via `lessons_applied` JSON →
//!   `prediction_lessons.cluster_key`).
//! - Highest-share co-failing cluster from `failure_correlations`.
//! - `scenario_prediction_links` distribution for matching scenarios
//!   (scenarios linked to past predictions in the same cluster).
//! - Most-similar `prediction_falsification_rules` claim — surfaced via the
//!   linked `user_predictions.claim` so the analyst can copy the rule shape.
//! - `preflight_score` (0..=100). Higher = riskier. Predictions whose score
//!   meets the abort threshold are blocked from `journal prediction add`
//!   unless `--accept-preflight` is also passed.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::calibration_adjustments::{self, CalibrationAdjustment};
use crate::db::clusters;
use crate::db::failure_correlations::{self, FailureCorrelation};
use crate::db::reasoning_fragments::{self, ReasoningFragment};
use crate::db::thesis_dependencies::{self, ThesisDependency};

/// Draft prediction inputs the analyst is composing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightDraft {
    pub claim: String,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub conviction: Option<String>,
    /// Analyst layer (e.g. "low", "medium", "high", "macro"). When None
    /// the calibration_adjustments lookup is skipped.
    pub layer: Option<String>,
    /// News topic ("fed","inflation","geopolitics","commodities","crypto",
    /// "equities","other"). When None the calibration lookup falls back to
    /// the cluster topic if available.
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarPrediction {
    pub id: i64,
    pub claim: String,
    pub symbol: Option<String>,
    pub conviction: String,
    pub timeframe: Option<String>,
    pub outcome: String,
    pub target_date: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioLinkRow {
    pub scenario_id: i64,
    pub scenario_name: Option<String>,
    pub link_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarFalsificationRule {
    pub id: i64,
    pub prediction_id: i64,
    pub rule_type: String,
    pub claim_excerpt: String,
    pub eval_date_end: Option<String>,
    pub similarity_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterHitStats {
    pub cluster_key: String,
    pub n_total: i64,
    pub n_scored: i64,
    pub n_correct: i64,
    pub n_partial: i64,
    pub n_wrong: i64,
    pub hit_rate_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightFindings {
    pub draft: PreflightDraft,
    pub cluster_key: Option<String>,
    pub cluster_hit_stats: Option<ClusterHitStats>,
    pub reasoning_fragments: Vec<ReasoningFragment>,
    pub calibration_adjustment: Option<CalibrationAdjustment>,
    pub similar_predictions: Vec<SimilarPrediction>,
    pub top_co_failing_cluster: Option<FailureCorrelation>,
    pub scenario_link_distribution: Vec<ScenarioLinkRow>,
    pub similar_falsification_rule: Option<SimilarFalsificationRule>,
    /// Thesis-dependency chains whose antecedent or consequent references
    /// the draft prediction's symbol. Populated only when a symbol is
    /// supplied and the `thesis_dependencies` table is present.
    pub thesis_chains: Vec<ThesisDependency>,
    pub preflight_score: u32,
    pub risk_factors: Vec<String>,
}

impl PreflightFindings {
    /// Returns true when the preflight_score meets or exceeds the abort
    /// threshold. Callers use this to gate `journal prediction add`.
    pub fn is_blocking(&self, threshold: u32) -> bool {
        self.preflight_score >= threshold
    }

    /// Compact one-paragraph string for `--inline` embedding into a
    /// prediction's `reasoning_summary` / journal entry. Deterministic.
    pub fn inline_summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("preflight_score={}/100", self.preflight_score));
        if let Some(c) = &self.cluster_key {
            parts.push(format!("cluster={}", c));
        }
        if let Some(stats) = &self.cluster_hit_stats {
            if stats.n_scored > 0 {
                parts.push(format!(
                    "cluster_hit_rate={:.0}% ({} of {} scored)",
                    stats.hit_rate_pct, stats.n_correct, stats.n_scored
                ));
            }
        }
        if let Some(adj) = &self.calibration_adjustment {
            parts.push(format!(
                "calibration={} {:+.0}pp",
                adj.adjustment_direction, adj.adjustment_pp
            ));
        }
        if !self.reasoning_fragments.is_empty() {
            let frag_ids: Vec<&str> = self
                .reasoning_fragments
                .iter()
                .take(3)
                .map(|f| f.canonical_id.as_str())
                .collect();
            parts.push(format!("fragments=[{}]", frag_ids.join(",")));
        }
        if let Some(c) = &self.top_co_failing_cluster {
            parts.push(format!(
                "co_failing={} ({:.0}%)",
                if c.cluster_a == self.cluster_key.clone().unwrap_or_default() {
                    &c.cluster_b
                } else {
                    &c.cluster_a
                },
                c.co_wrong_share * 100.0,
            ));
        }
        if !self.thesis_chains.is_empty() {
            let summary: Vec<String> = self
                .thesis_chains
                .iter()
                .take(3)
                .map(|c| format!("#{}:{}", c.id, c.current_state))
                .collect();
            parts.push(format!("thesis_chains=[{}]", summary.join(",")));
        }
        format!("[preflight] {}", parts.join("; "))
    }
}

/// Run a preflight check against the substrate tables for the supplied
/// draft prediction. Tolerant of fresh installs: every join is wrapped in
/// a table-exists check so missing enrichment tables degrade gracefully
/// to empty vectors / None values.
pub fn compute_preflight(conn: &Connection, draft: &PreflightDraft) -> Result<PreflightFindings> {
    let cluster_key = clusters::classify_claim(&draft.claim).map(|s| s.to_string());

    // Reasoning fragments reachable from this cluster via
    // lesson_fragment_edges -> prediction_lessons.cluster_key.
    let reasoning_fragments = match cluster_key.as_deref() {
        Some(c) if table_exists(conn, "reasoning_fragments")? => {
            reasoning_fragments::fragments_for_cluster(conn, c)?
        }
        _ => Vec::new(),
    };

    // calibration_adjustments lookup: only when layer is provided.
    let calibration_topic = draft
        .topic
        .clone()
        .or_else(|| topic_from_cluster(cluster_key.as_deref()));
    let calibration_adjustment =
        match (draft.layer.as_deref(), calibration_topic.as_deref(), draft.conviction.as_deref()) {
            (Some(layer), Some(topic), Some(conv))
                if table_exists(conn, "calibration_adjustments")? =>
            {
                let rows = calibration_adjustments::list(
                    conn,
                    Some(layer),
                    Some(topic),
                    Some(conv),
                )?;
                rows.into_iter().next()
            }
            _ => None,
        };

    // Top-3 similar past predictions in the same cluster (via lessons_applied
    // → prediction_lessons.cluster_key). Falls back to symbol when cluster
    // is unknown to still surface comparable past calls.
    let similar_predictions = collect_similar_predictions(
        conn,
        cluster_key.as_deref(),
        draft.symbol.as_deref(),
        3,
    )?;

    // Cluster-wide hit stats: read once for cluster, returns the (n_total,
    // n_scored, n_correct, n_partial, n_wrong, hit_rate_pct) tuple.
    let cluster_hit_stats = match cluster_key.as_deref() {
        Some(c) => collect_cluster_hit_stats(conn, c)?,
        None => None,
    };

    let top_co_failing_cluster = match cluster_key.as_deref() {
        Some(c) if table_exists(conn, "failure_correlations")? => {
            let rows = failure_correlations::list(conn, Some(c), None)?;
            rows.into_iter().next()
        }
        _ => None,
    };

    let scenario_link_distribution = match cluster_key.as_deref() {
        Some(c) => collect_scenario_links(conn, c)?,
        None => Vec::new(),
    };

    let similar_falsification_rule =
        collect_similar_falsification_rule(conn, &draft.claim, cluster_key.as_deref())?;

    // Thesis-dependency chains touching the draft's symbol. Tolerant of
    // missing schema (fresh installs).
    let thesis_chains = match draft.symbol.as_deref() {
        Some(sym) if table_exists(conn, "thesis_dependencies")? => {
            thesis_dependencies::find_chains_for_symbol(conn, sym)?
        }
        _ => Vec::new(),
    };

    let (preflight_score, mut risk_factors) = score_preflight(
        &reasoning_fragments,
        calibration_adjustment.as_ref(),
        cluster_hit_stats.as_ref(),
        top_co_failing_cluster.as_ref(),
    );

    // Surface chain-state warnings as ancillary risk factors so the analyst
    // sees them inline. We do NOT inflate `preflight_score` here — the chain
    // graph is advisory, not blocking.
    for chain in &thesis_chains {
        if chain.current_state == "disconfirmed" {
            risk_factors.push(format!(
                "thesis_chain_disconfirmed:{}",
                chain.id
            ));
        }
    }

    // Gamma-neutral zone warning: when a draft references a numeric
    // target for a symbol whose latest GEX flip strike sits within
    // 5% of that target, mark the cluster's pinning risk explicitly.
    // Advisory only — does not bump preflight_score (analyst can
    // still write the prediction; they just see the gamma-pin context).
    if let Some(sym) = draft.symbol.as_deref() {
        if table_exists(conn, "gex_snapshots")? {
            if let Some(target) = extract_numeric_target(&draft.claim) {
                if let Some(gex) =
                    crate::db::gex_snapshots::latest(conn, sym).unwrap_or(None)
                {
                    if gex.strike_in_zone(target) {
                        let flip = gex
                            .gex_flip_strike
                            .map(|v| format!("{:.2}", v))
                            .unwrap_or_else(|| "n/a".to_string());
                        risk_factors.push(format!(
                            "gamma_neutral_zone:target_{:.2}_flip_{}",
                            target, flip
                        ));
                    }
                }
            }
        }
    }

    Ok(PreflightFindings {
        draft: draft.clone(),
        cluster_key,
        cluster_hit_stats,
        reasoning_fragments,
        calibration_adjustment,
        similar_predictions,
        top_co_failing_cluster,
        scenario_link_distribution,
        similar_falsification_rule,
        thesis_chains,
        preflight_score,
        risk_factors,
    })
}

/// Risk-scoring rubric. Documented inline so the rubric can be reviewed and
/// adjusted independently of the data-collection layer.
///
/// Score starts at 0; each rule below adds to it. Final value is clamped to
/// the 0..=100 range. Higher = riskier.
///
/// * +25 when calibration_adjustments says discount the layer by >=10pp
///   (most-leverage signal: the layer is known to overstate this topic).
/// * +15 when cluster hit rate over the substrate is <= 50% (5+ samples).
/// * +20 when the top co-failing cluster's share is >= 0.5 (frequent joint
///   failure with another cluster — synthesis blind-spot warning).
/// * +15 when an anti-pattern fragment applies to the cluster (the
///   substrate has an explicit argument against this class of claim).
/// * +5 per applicable fragment of confidence='high' (capped at +15 total).
fn score_preflight(
    fragments: &[ReasoningFragment],
    calibration: Option<&CalibrationAdjustment>,
    cluster_stats: Option<&ClusterHitStats>,
    co_failing: Option<&FailureCorrelation>,
) -> (u32, Vec<String>) {
    let mut score: i64 = 0;
    let mut factors: Vec<String> = Vec::new();

    if let Some(adj) = calibration {
        if adj.adjustment_direction == "discount" && adj.adjustment_pp.abs() >= 10.0 {
            score += 25;
            factors.push(format!(
                "calibration_discount_{:.0}pp",
                adj.adjustment_pp.abs()
            ));
        }
    }
    if let Some(stats) = cluster_stats {
        if stats.n_scored >= 5 && stats.hit_rate_pct <= 50.0 {
            score += 15;
            factors.push(format!(
                "cluster_hit_rate_{:.0}pct_n{}",
                stats.hit_rate_pct, stats.n_scored
            ));
        }
    }
    if let Some(c) = co_failing {
        if c.co_wrong_share >= 0.5 {
            score += 20;
            factors.push(format!(
                "co_failing_cluster_share_{:.0}pct",
                c.co_wrong_share * 100.0
            ));
        }
    }
    if fragments.iter().any(|f| f.fragment_type == "anti-pattern") {
        score += 15;
        factors.push("anti_pattern_fragment_applies".to_string());
    }
    let high_conf_frags = fragments.iter().filter(|f| f.confidence == "high").count();
    if high_conf_frags > 0 {
        let bonus = std::cmp::min(high_conf_frags as i64 * 5, 15);
        score += bonus;
        factors.push(format!("{}_high_confidence_fragments", high_conf_frags));
    }

    let clamped = score.clamp(0, 100) as u32;
    (clamped, factors)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let exists: i64 = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1")?
        .query_row(params![name], |row| row.get(0))
        .unwrap_or(0);
    Ok(exists > 0)
}

fn topic_from_cluster(cluster: Option<&str>) -> Option<String> {
    // Crude mapping from a cluster_key to the news_source_accuracy topic
    // taxonomy. Mirrors the dominant topic of each cluster so the
    // calibration_adjustments lookup degrades gracefully when the caller
    // does not supply an explicit --topic.
    let c = cluster?;
    let topic = match c {
        "iran_oil_managed_theater" => "geopolitics",
        "iran_gold_war_fatigue" => "geopolitics",
        "tight_threshold_close_miss" => "other",
        "dxy_two_driver" => "fed",
        "options_gamma_pinning" => "equities",
        "realrates_dominates_gold" => "commodities",
        "fed_dot_repricing" => "fed",
        "fourth_turning_crisis" => "other",
        _ => return None,
    };
    Some(topic.to_string())
}

fn collect_similar_predictions(
    conn: &Connection,
    cluster_key: Option<&str>,
    symbol: Option<&str>,
    limit: usize,
) -> Result<Vec<SimilarPrediction>> {
    if !table_exists(conn, "user_predictions")? {
        return Ok(Vec::new());
    }
    let mut out: Vec<SimilarPrediction> = Vec::new();

    if let Some(c) = cluster_key {
        if table_exists(conn, "prediction_lessons")? {
            let sql = "SELECT DISTINCT up.id, up.claim, up.symbol, up.conviction, up.timeframe,
                              up.outcome, up.target_date, up.created_at
                       FROM user_predictions up, json_each(up.lessons_applied) je
                       JOIN prediction_lessons pl ON pl.id = je.value
                       WHERE pl.cluster_key = ?1
                       ORDER BY up.created_at DESC
                       LIMIT ?2";
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt
                .query_map(params![c, limit as i64], |row| {
                    Ok(SimilarPrediction {
                        id: row.get(0)?,
                        claim: row.get(1)?,
                        symbol: row.get(2)?,
                        conviction: row.get(3)?,
                        timeframe: row.get(4)?,
                        outcome: row.get(5)?,
                        target_date: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.extend(rows);
        }
    }

    // Fallback / supplement: symbol-matched recent predictions to keep the
    // similar list useful even when no lessons are attached yet.
    if out.len() < limit {
        if let Some(sym) = symbol {
            let mut stmt = conn.prepare(
                "SELECT id, claim, symbol, conviction, timeframe, outcome, target_date, created_at
                 FROM user_predictions
                 WHERE symbol = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )?;
            let want = (limit - out.len()) as i64;
            let extra = stmt
                .query_map(params![sym, want], |row| {
                    Ok(SimilarPrediction {
                        id: row.get(0)?,
                        claim: row.get(1)?,
                        symbol: row.get(2)?,
                        conviction: row.get(3)?,
                        timeframe: row.get(4)?,
                        outcome: row.get(5)?,
                        target_date: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for row in extra {
                if !out.iter().any(|existing| existing.id == row.id) {
                    out.push(row);
                    if out.len() >= limit {
                        break;
                    }
                }
            }
        }
    }

    Ok(out)
}

fn collect_cluster_hit_stats(
    conn: &Connection,
    cluster_key: &str,
) -> Result<Option<ClusterHitStats>> {
    if !table_exists(conn, "user_predictions")? || !table_exists(conn, "prediction_lessons")? {
        return Ok(None);
    }
    let row = conn
        .query_row(
            "SELECT
                COUNT(DISTINCT up.id) AS n_total,
                SUM(CASE WHEN up.outcome = 'correct' THEN 1 ELSE 0 END) AS n_correct,
                SUM(CASE WHEN up.outcome = 'partial' THEN 1 ELSE 0 END) AS n_partial,
                SUM(CASE WHEN up.outcome = 'wrong' THEN 1 ELSE 0 END) AS n_wrong
             FROM user_predictions up, json_each(up.lessons_applied) je
             JOIN prediction_lessons pl ON pl.id = je.value
             WHERE pl.cluster_key = ?1",
            params![cluster_key],
            |r| {
                Ok((
                    r.get::<_, i64>(0).unwrap_or(0),
                    r.get::<_, Option<i64>>(1).unwrap_or(Some(0)).unwrap_or(0),
                    r.get::<_, Option<i64>>(2).unwrap_or(Some(0)).unwrap_or(0),
                    r.get::<_, Option<i64>>(3).unwrap_or(Some(0)).unwrap_or(0),
                ))
            },
        )
        .optional()?;
    let Some((n_total, n_correct, n_partial, n_wrong)) = row else {
        return Ok(None);
    };
    if n_total == 0 {
        return Ok(None);
    }
    let n_scored = n_correct + n_partial + n_wrong;
    let hit_rate_pct = if n_scored > 0 {
        (n_correct as f64 + 0.5 * n_partial as f64) / n_scored as f64 * 100.0
    } else {
        0.0
    };
    Ok(Some(ClusterHitStats {
        cluster_key: cluster_key.to_string(),
        n_total,
        n_scored,
        n_correct,
        n_partial,
        n_wrong,
        hit_rate_pct,
    }))
}

fn collect_scenario_links(conn: &Connection, cluster_key: &str) -> Result<Vec<ScenarioLinkRow>> {
    if !table_exists(conn, "scenario_prediction_links")?
        || !table_exists(conn, "user_predictions")?
        || !table_exists(conn, "prediction_lessons")?
    {
        return Ok(Vec::new());
    }
    let scenarios_table = table_exists(conn, "scenarios")?;
    let sql = if scenarios_table {
        "SELECT spl.scenario_id, s.name, COUNT(*) AS n
         FROM scenario_prediction_links spl
         JOIN user_predictions up ON up.id = spl.prediction_id
         JOIN json_each(up.lessons_applied) je ON 1=1
         JOIN prediction_lessons pl ON pl.id = je.value
         LEFT JOIN scenarios s ON s.id = spl.scenario_id
         WHERE pl.cluster_key = ?1
         GROUP BY spl.scenario_id, s.name
         ORDER BY n DESC, spl.scenario_id ASC
         LIMIT 10"
    } else {
        "SELECT spl.scenario_id, NULL, COUNT(*) AS n
         FROM scenario_prediction_links spl
         JOIN user_predictions up ON up.id = spl.prediction_id
         JOIN json_each(up.lessons_applied) je ON 1=1
         JOIN prediction_lessons pl ON pl.id = je.value
         WHERE pl.cluster_key = ?1
         GROUP BY spl.scenario_id
         ORDER BY n DESC, spl.scenario_id ASC
         LIMIT 10"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params![cluster_key], |row| {
            Ok(ScenarioLinkRow {
                scenario_id: row.get(0)?,
                scenario_name: row.get(1).ok(),
                link_count: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn collect_similar_falsification_rule(
    conn: &Connection,
    claim: &str,
    cluster_key: Option<&str>,
) -> Result<Option<SimilarFalsificationRule>> {
    if !table_exists(conn, "prediction_falsification_rules")?
        || !table_exists(conn, "user_predictions")?
    {
        return Ok(None);
    }
    // Resolve actual column names so the query handles legacy schema drift
    // (some live DBs use `user_prediction_id` instead of `prediction_id`,
    // or are missing `eval_date_end`).
    let cols = detect_falsification_columns(conn)?;
    let Some(cols) = cols else { return Ok(None) };

    let where_cluster_clause = cluster_key.is_some() && table_exists(conn, "prediction_lessons")?;
    let sql = if where_cluster_clause {
        format!(
            "SELECT r.{id}, r.{pid}, r.{rt}, up.claim, {eval_end_expr}
             FROM prediction_falsification_rules r
             JOIN user_predictions up ON up.id = r.{pid}
             JOIN json_each(up.lessons_applied) je ON 1=1
             JOIN prediction_lessons pl ON pl.id = je.value
             WHERE pl.cluster_key = ?1
             ORDER BY r.{id} DESC
             LIMIT 50",
            id = cols.id,
            pid = cols.prediction_id,
            rt = cols.rule_type,
            eval_end_expr = cols
                .eval_date_end
                .as_ref()
                .map(|c| format!("r.{c}"))
                .unwrap_or_else(|| "NULL".to_string()),
        )
    } else {
        format!(
            "SELECT r.{id}, r.{pid}, r.{rt}, up.claim, {eval_end_expr}
             FROM prediction_falsification_rules r
             JOIN user_predictions up ON up.id = r.{pid}
             ORDER BY r.{id} DESC
             LIMIT 50",
            id = cols.id,
            pid = cols.prediction_id,
            rt = cols.rule_type,
            eval_end_expr = cols
                .eval_date_end
                .as_ref()
                .map(|c| format!("r.{c}"))
                .unwrap_or_else(|| "NULL".to_string()),
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<(i64, i64, String, String, Option<String>)> = if where_cluster_clause {
        let c = cluster_key.unwrap_or_default();
        stmt.query_map(params![c], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4).ok(),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4).ok(),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    let draft_tokens = tokenize(claim);
    if draft_tokens.is_empty() {
        return Ok(None);
    }
    let mut best: Option<(f64, i64, i64, String, String, Option<String>)> = None;
    for (id, pred_id, rule_type, candidate_claim, eval_end) in rows {
        let score = jaccard(&draft_tokens, &tokenize(&candidate_claim));
        if score > 0.0 {
            match &best {
                Some((s, ..)) if *s >= score => {}
                _ => {
                    best = Some((score, id, pred_id, rule_type, candidate_claim, eval_end));
                }
            }
        }
    }
    Ok(best.map(|(score, id, pred_id, rule_type, claim_text, eval_end)| {
        let excerpt: String = claim_text.chars().take(160).collect();
        SimilarFalsificationRule {
            id,
            prediction_id: pred_id,
            rule_type,
            claim_excerpt: excerpt,
            eval_date_end: eval_end,
            similarity_score: score,
        }
    }))
}

#[derive(Debug, Clone)]
struct FalsificationCols {
    id: String,
    prediction_id: String,
    rule_type: String,
    eval_date_end: Option<String>,
}

fn detect_falsification_columns(conn: &Connection) -> Result<Option<FalsificationCols>> {
    let mut stmt = conn.prepare("PRAGMA table_info('prediction_falsification_rules')")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut names = Vec::new();
    for r in rows {
        names.push(r?);
    }
    let pick = |candidates: &[&str]| -> Option<String> {
        candidates
            .iter()
            .find(|cand| names.iter().any(|n| n == **cand))
            .map(|s| (*s).to_string())
    };
    let Some(id) = pick(&["id"]) else {
        return Ok(None);
    };
    let Some(prediction_id) = pick(&["prediction_id", "user_prediction_id"]) else {
        return Ok(None);
    };
    let Some(rule_type) = pick(&["rule_type"]) else {
        return Ok(None);
    };
    let eval_date_end = pick(&["eval_date_end", "end_date", "window_end", "target_date"]);
    Ok(Some(FalsificationCols {
        id,
        prediction_id,
        rule_type,
        eval_date_end,
    }))
}

/// Extract the first plausible price-target from a claim.
///
/// Looks for patterns like `$745`, `$5,000`, `5000`, `4.5k`, `4.5K`,
/// `75k`. Returns the largest such value (predictions almost always
/// embed exactly one target; the largest one is a robust default).
pub(crate) fn extract_numeric_target(claim: &str) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut chars = claim.chars().peekable();
    while let Some(c) = chars.next() {
        let take_number = c == '$' || c.is_ascii_digit();
        if !take_number {
            continue;
        }
        let mut buf = String::new();
        if c.is_ascii_digit() {
            buf.push(c);
        }
        while let Some(&n) = chars.peek() {
            if n.is_ascii_digit() || n == ',' || n == '.' {
                buf.push(n);
                chars.next();
            } else {
                break;
            }
        }
        if buf.is_empty() {
            continue;
        }
        let multiplier = match chars.peek() {
            Some('k') | Some('K') => {
                chars.next();
                1_000.0
            }
            Some('m') | Some('M') => {
                chars.next();
                1_000_000.0
            }
            _ => 1.0,
        };
        let cleaned: String = buf.chars().filter(|c| *c != ',').collect();
        if let Ok(val) = cleaned.parse::<f64>() {
            let scaled = val * multiplier;
            best = match best {
                Some(b) if b >= scaled => best,
                _ => Some(scaled),
            };
        }
    }
    best
}

fn tokenize(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_string())
        .collect()
}

fn jaccard(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Default abort threshold for `journal prediction add` — predictions whose
/// `preflight_score` meets or exceeds this value are blocked unless
/// `--accept-preflight` is also passed.
pub const DEFAULT_PREFLIGHT_ABORT_THRESHOLD: u32 = 50;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::options::GexSummary;
    use crate::db::gex_snapshots;
    use crate::db::reasoning_fragments::upsert_edge;
    use crate::db::{
        calibration_adjustments, failure_correlations, reasoning_fragments, schema,
    };

    #[test]
    fn extract_numeric_target_dollar() {
        assert_eq!(extract_numeric_target("SPY through $745 in 2 weeks"), Some(745.0));
    }

    #[test]
    fn extract_numeric_target_k_suffix() {
        assert_eq!(extract_numeric_target("BTC to 75k"), Some(75_000.0));
    }

    #[test]
    fn extract_numeric_target_commas() {
        assert_eq!(extract_numeric_target("gold $5,000 by EOY"), Some(5_000.0));
    }

    #[test]
    fn extract_numeric_target_none() {
        assert_eq!(extract_numeric_target("no number here"), None);
    }

    #[test]
    fn preflight_surfaces_gamma_neutral_zone_warning() {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        let gex = GexSummary {
            symbol: "SPY".into(),
            gex_flip_strike: Some(745.0),
            total_gamma_call: 1.0,
            total_gamma_put: 1.0,
            max_pain: Some(745.0),
            fetched_at: "2026-06-02T00:00:00Z".into(),
        };
        gex_snapshots::insert(&conn, &gex).unwrap();
        let draft = PreflightDraft {
            claim: "SPY through $745 in 2 weeks".into(),
            symbol: Some("SPY".into()),
            timeframe: None,
            conviction: None,
            layer: None,
            topic: None,
        };
        let findings = compute_preflight(&conn, &draft).unwrap();
        assert!(
            findings
                .risk_factors
                .iter()
                .any(|r| r.starts_with("gamma_neutral_zone:")),
            "risk_factors did not include gamma_neutral_zone warning: {:?}",
            findings.risk_factors
        );
    }

    #[test]
    fn preflight_no_warning_when_target_outside_zone() {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        let gex = GexSummary {
            symbol: "SPY".into(),
            gex_flip_strike: Some(745.0),
            total_gamma_call: 1.0,
            total_gamma_put: 1.0,
            max_pain: Some(745.0),
            fetched_at: "2026-06-02T00:00:00Z".into(),
        };
        gex_snapshots::insert(&conn, &gex).unwrap();
        let draft = PreflightDraft {
            claim: "SPY to $900".into(),
            symbol: Some("SPY".into()),
            timeframe: None,
            conviction: None,
            layer: None,
            topic: None,
        };
        let findings = compute_preflight(&conn, &draft).unwrap();
        assert!(
            !findings
                .risk_factors
                .iter()
                .any(|r| r.starts_with("gamma_neutral_zone:")),
            "unexpected gamma_neutral_zone warning: {:?}",
            findings.risk_factors
        );
    }

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    fn seed_lesson(conn: &Connection, prediction_id: i64, cluster_key: &str) -> i64 {
        conn.execute(
            "INSERT INTO prediction_lessons
                (prediction_id, miss_type, what_predicted, what_happened, why_wrong,
                 signal_misread, cluster_key)
             VALUES (?1, 'directional', 'a', 'b', 'c', NULL, ?2)",
            params![prediction_id, cluster_key],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn seed_prediction(
        conn: &Connection,
        claim: &str,
        symbol: Option<&str>,
        outcome: &str,
        lessons_applied: &[i64],
    ) -> i64 {
        let lessons_json = serde_json::to_string(lessons_applied).unwrap();
        conn.execute(
            "INSERT INTO user_predictions
                (claim, symbol, conviction, timeframe, topic, outcome, lessons_applied)
             VALUES (?1, ?2, 'high', 'medium', 'commodities', ?3, ?4)",
            params![claim, symbol, outcome, lessons_json],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn classify_then_pull_fragments_calibration_and_similar() {
        let conn = fresh_conn();

        // Fragment + edge for the gold cluster (matched by classify_claim).
        reasoning_fragments::upsert_fragment(
            &conn,
            "realrates-dominates-gold",
            "Real yields drive gold direction",
            "correlation-rule",
            "gold",
            "high",
            None,
            true,
        )
        .unwrap();

        // Seed two scored predictions in the same cluster + link the lesson.
        let pid1 = seed_prediction(
            &conn,
            "Gold above $4500 by July",
            Some("GLD"),
            "wrong",
            &[],
        );
        let lid = seed_lesson(&conn, pid1, "realrates_dominates_gold");
        upsert_edge(&conn, lid, "realrates-dominates-gold", "primary").unwrap();
        // Re-insert the same lesson id onto another prediction by mutating
        // lessons_applied. Simulate it directly.
        conn.execute(
            "UPDATE user_predictions SET lessons_applied = ?1 WHERE id = ?2",
            params![format!("[{}]", lid), pid1],
        )
        .unwrap();

        let pid2 = seed_prediction(
            &conn,
            "Gold breaks 5000 next month",
            Some("GLD"),
            "correct",
            &[],
        );
        conn.execute(
            "UPDATE user_predictions SET lessons_applied = ?1 WHERE id = ?2",
            params![format!("[{}]", lid), pid2],
        )
        .unwrap();

        // Calibration row for (low, commodities, high).
        calibration_adjustments::upsert(
            &conn,
            "low",
            "commodities",
            "high",
            12,
            0.55,
            0.72,
            -17.0,
            "discount",
            "Discount confidence by 17pp before publishing",
        )
        .unwrap();

        // Failure correlation for the cluster.
        failure_correlations::upsert(
            &conn,
            "realrates_dominates_gold",
            "btc_correlation_regime",
            6,
            8,
            10,
            0.75,
            7,
        )
        .unwrap();

        // Use a real-yield + tips claim so the rules-based classifier picks
        // realrates_dominates_gold (iran_gold_war_fatigue would otherwise win
        // on a bare "gold" keyword match because its rule fires earlier).
        let draft = PreflightDraft {
            claim: "Real yield breakdown drives tips and breakeven repricing".into(),
            symbol: Some("GLD".into()),
            timeframe: Some("medium".into()),
            conviction: Some("high".into()),
            layer: Some("low".into()),
            topic: Some("commodities".into()),
        };

        let findings = compute_preflight(&conn, &draft).unwrap();
        assert_eq!(findings.cluster_key.as_deref(), Some("realrates_dominates_gold"));
        assert!(!findings.reasoning_fragments.is_empty());
        let calib = findings
            .calibration_adjustment
            .clone()
            .expect("calibration row");
        assert_eq!(calib.adjustment_direction, "discount");
        assert!(findings.cluster_hit_stats.is_some());
        let stats = findings.cluster_hit_stats.clone().unwrap();
        assert_eq!(stats.n_scored, 2);
        assert_eq!(stats.n_correct, 1);
        let co = findings
            .top_co_failing_cluster
            .clone()
            .expect("co failing");
        assert_eq!(co.cluster_b, "btc_correlation_regime");
        // calibration discount 17pp -> +25; co-failing 75% -> +20; hit rate
        // 75% so no penalty there; high-confidence fragment -> +5; no
        // anti-pattern in this fixture.
        assert!(findings.preflight_score >= 25);
        assert!(findings.preflight_score <= 100);
        assert!(findings.is_blocking(DEFAULT_PREFLIGHT_ABORT_THRESHOLD));
    }

    #[test]
    fn no_cluster_match_returns_low_score_findings() {
        let conn = fresh_conn();
        let draft = PreflightDraft {
            claim: "totally random uncategorized text zzz".into(),
            symbol: None,
            timeframe: None,
            conviction: None,
            layer: None,
            topic: None,
        };
        let findings = compute_preflight(&conn, &draft).unwrap();
        assert!(findings.cluster_key.is_none());
        assert!(findings.reasoning_fragments.is_empty());
        assert!(findings.calibration_adjustment.is_none());
        assert!(findings.top_co_failing_cluster.is_none());
        assert_eq!(findings.preflight_score, 0);
        assert!(!findings.is_blocking(DEFAULT_PREFLIGHT_ABORT_THRESHOLD));
    }

    #[test]
    fn anti_pattern_fragment_alone_does_not_block_by_default() {
        let conn = fresh_conn();
        reasoning_fragments::upsert_fragment(
            &conn,
            "options-gamma-pinning",
            "Round-number strikes pin price intraday",
            "anti-pattern",
            "options",
            "medium",
            None,
            false,
        )
        .unwrap();
        // Wire it to a lesson in the options cluster so fragments_for_cluster
        // can reach it.
        let pid = seed_prediction(&conn, "stub", None, "pending", &[]);
        let lid = seed_lesson(&conn, pid, "options_gamma_pinning");
        upsert_edge(&conn, lid, "options-gamma-pinning", "primary").unwrap();

        let draft = PreflightDraft {
            claim: "SPY gamma pin at 700 by 0dte expiry".into(),
            symbol: Some("SPY".into()),
            timeframe: Some("low".into()),
            conviction: Some("medium".into()),
            layer: Some("low".into()),
            topic: None,
        };
        let findings = compute_preflight(&conn, &draft).unwrap();
        assert_eq!(findings.cluster_key.as_deref(), Some("options_gamma_pinning"));
        assert!(!findings.reasoning_fragments.is_empty());
        // Only the +15 from the anti-pattern fires.
        assert_eq!(findings.preflight_score, 15);
        assert!(!findings.is_blocking(DEFAULT_PREFLIGHT_ABORT_THRESHOLD));
    }

    #[test]
    fn preflight_surfaces_thesis_chains_for_matching_symbol() {
        let conn = fresh_conn();
        // Chain referencing BTC in its consequent.
        crate::db::thesis_dependencies::insert(
            &conn,
            None,
            "XAU > 4500",
            "implies",
            None,
            "BTC > 100000",
            1,
            Some("high"),
            None,
            None,
        )
        .unwrap();
        let draft = PreflightDraft {
            claim: "BTC blast through 120k by July".into(),
            symbol: Some("BTC".into()),
            timeframe: None,
            conviction: None,
            layer: None,
            topic: None,
        };
        let findings = compute_preflight(&conn, &draft).unwrap();
        assert_eq!(findings.thesis_chains.len(), 1);
        assert_eq!(findings.thesis_chains[0].relation, "implies");
        let summary = findings.inline_summary();
        assert!(summary.contains("thesis_chains="));
    }

    #[test]
    fn inline_summary_is_compact_and_deterministic() {
        let conn = fresh_conn();
        let draft = PreflightDraft {
            claim: "Gold real yield breakout".into(),
            symbol: None,
            timeframe: None,
            conviction: None,
            layer: None,
            topic: None,
        };
        let findings = compute_preflight(&conn, &draft).unwrap();
        let s = findings.inline_summary();
        assert!(s.starts_with("[preflight]"));
        assert!(s.contains("preflight_score="));
    }
}
