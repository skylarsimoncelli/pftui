//! Handlers for the live-DB enrichment CLI surface added under
//! `pftui analytics {sources,events,fragments,calibration-adjustments,
//! failures,clusters,falsifications}` and `pftui journal replies`.
//!
//! Each handler is sqlite-backed because the enrichment tables are local-only
//! lookup substrate populated by the analyst routines.

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::{
    calibration_adjustments, clusters, event_annotations, failure_correlations, operator_replies,
    prediction_falsification_rules, reasoning_fragments, sources_registry,
};

fn require_sqlite(backend: &BackendConnection) -> Result<&rusqlite::Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow!("analytics enrichment commands require the SQLite backend"))
}

fn split_csv(value: Option<&str>) -> Vec<String> {
    value
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn split_csv_i64(value: Option<&str>) -> Result<Vec<i64>> {
    let Some(v) = value else {
        return Ok(Vec::new());
    };
    v.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<i64>()
                .map_err(|e| anyhow!("invalid id '{}': {}", s, e))
        })
        .collect()
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// ------- sources -------

pub fn sources_list(
    backend: &BackendConnection,
    source_type: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = sources_registry::list(conn, source_type)?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!(
            "No sources found{}.",
            source_type
                .map(|t| format!(" with type={}", t))
                .unwrap_or_default()
        );
        return Ok(());
    }
    for s in &rows {
        println!(
            "{:<28} {:<11} {}  aliases={} topics={}  acc={}",
            s.canonical_id,
            s.source_type,
            s.display_name,
            s.aliases.len(),
            s.topics.len(),
            s.accuracy_rating.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn sources_set(
    backend: &BackendConnection,
    canonical_id: &str,
    display_name: &str,
    source_type: &str,
    aliases: Option<&str>,
    topics: Option<&str>,
    accuracy_rating: Option<&str>,
    framework_summary: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let aliases_vec = split_csv(aliases);
    let topics_vec = split_csv(topics);
    sources_registry::upsert(
        conn,
        canonical_id,
        display_name,
        source_type,
        &aliases_vec,
        &topics_vec,
        accuracy_rating,
        framework_summary,
    )?;
    let after = sources_registry::list(conn, None)?;
    let updated = after
        .into_iter()
        .find(|s| s.canonical_id == canonical_id)
        .ok_or_else(|| anyhow!("upsert succeeded but row not found"))?;
    if json {
        print_json(&updated)
    } else {
        println!("set: {} ({})", updated.canonical_id, updated.source_type);
        Ok(())
    }
}

pub fn sources_remove(backend: &BackendConnection, canonical_id: &str, json: bool) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let removed = sources_registry::remove(conn, canonical_id)?;
    if json {
        print_json(&serde_json::json!({
            "canonical_id": canonical_id,
            "removed": removed,
        }))
    } else {
        if removed {
            println!("removed: {}", canonical_id);
        } else {
            println!("not found: {}", canonical_id);
        }
        Ok(())
    }
}

// ------- events -------

pub fn events_list(
    backend: &BackendConnection,
    category: Option<&str>,
    since: Option<&str>,
    asset: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = event_annotations::list(conn, category, since, asset)?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No event annotations match.");
        return Ok(());
    }
    for e in &rows {
        println!(
            "{} [{}] mag={} {}",
            e.event_date, e.category, e.magnitude, e.headline
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn events_add(
    backend: &BackendConnection,
    event_date: &str,
    event_time: Option<&str>,
    category: &str,
    headline: &str,
    detail: Option<&str>,
    source: Option<&str>,
    magnitude: i64,
    persistence: Option<&str>,
    asset_impact: Option<&str>,
    related_scenario: Option<&str>,
    related_prediction: Option<&str>,
    notes: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let assets = split_csv(asset_impact);
    let scenarios = split_csv(related_scenario);
    let preds = split_csv_i64(related_prediction)?;
    let id = event_annotations::insert(
        conn,
        &event_annotations::EventAnnotationInsert {
            event_date,
            event_time,
            category,
            headline,
            detail,
            source,
            magnitude,
            persistence,
            asset_impact: &assets,
            related_predictions: &preds,
            related_scenarios: &scenarios,
            notes,
        },
    )?;
    if json {
        print_json(&serde_json::json!({"id": id, "event_date": event_date}))
    } else {
        println!("added event_annotation id={}", id);
        Ok(())
    }
}

// ------- fragments -------

#[derive(Debug, Serialize)]
struct FragmentsResult {
    classified_cluster: Option<String>,
    classification_query: Option<String>,
    fragments: Vec<reasoning_fragments::ReasoningFragment>,
}

#[allow(clippy::too_many_arguments)]
pub fn fragments_list(
    backend: &BackendConnection,
    fragment_type: Option<&str>,
    topic: Option<&str>,
    cluster: Option<&str>,
    for_claim: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    // Decide which cluster (if any) to filter by:
    //   --for-claim takes priority; if it cannot classify, fall back to
    //   `--cluster` (when provided) or no cluster filter.
    let (classified_cluster, claim_query) = match for_claim {
        Some(text) => (
            clusters::classify_claim(text).map(|s| s.to_string()),
            Some(text.to_string()),
        ),
        None => (cluster.map(|s| s.to_string()), None),
    };
    let effective_cluster = classified_cluster
        .clone()
        .or_else(|| cluster.map(|s| s.to_string()));

    let mut rows = if let Some(c) = effective_cluster.as_deref() {
        reasoning_fragments::fragments_for_cluster(conn, c)?
    } else {
        reasoning_fragments::list(conn, fragment_type, topic)?
    };
    // Apply remaining client-side filters when we went through the cluster path.
    if effective_cluster.is_some() {
        if let Some(t) = fragment_type {
            rows.retain(|f| f.fragment_type == t);
        }
        if let Some(t) = topic {
            rows.retain(|f| f.topic == t);
        }
    }

    if json {
        return print_json(&FragmentsResult {
            classified_cluster,
            classification_query: claim_query,
            fragments: rows,
        });
    }
    if let Some(c) = &classified_cluster {
        println!("Classified claim → cluster: {}", c);
    }
    if rows.is_empty() {
        println!("No fragments match the filters.");
        return Ok(());
    }
    for f in &rows {
        println!(
            "{:<32} [{}] topic={} cites={} conf={}{}",
            f.canonical_id,
            f.fragment_type,
            f.topic,
            f.cited_count,
            f.confidence,
            if f.operator_endorsed { " *endorsed" } else { "" }
        );
    }
    Ok(())
}

pub fn fragments_show(backend: &BackendConnection, canonical_id: &str, json: bool) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let frag = match reasoning_fragments::get(conn, canonical_id)? {
        Some(f) => f,
        None => return Err(anyhow!("fragment '{}' not found", canonical_id)),
    };
    let edges = reasoning_fragments::edges_for_fragment(conn, canonical_id)?;
    let payload = reasoning_fragments::FragmentWithEdges {
        fragment: frag.clone(),
        edges: edges.clone(),
    };
    if json {
        return print_json(&payload);
    }
    println!(
        "{} [{}] topic={} confidence={}\n  {}",
        frag.canonical_id, frag.fragment_type, frag.topic, frag.confidence, frag.fragment
    );
    if let Some(d) = &frag.derivation {
        println!("  derivation: {}", d);
    }
    println!("  cited_count: {}", frag.cited_count);
    println!("  edges: {}", edges.len());
    for e in &edges {
        println!("    lesson #{} — {}", e.lesson_id, e.edge_strength);
    }
    Ok(())
}

// ------- calibration adjustments -------

pub fn calibration_adjustments_list(
    backend: &BackendConnection,
    layer: Option<&str>,
    topic: Option<&str>,
    conviction: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = calibration_adjustments::list(conn, layer, topic, conviction)?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No calibration adjustments recorded.");
        return Ok(());
    }
    for r in &rows {
        println!(
            "{:<8} {:<14} {:<8} n={:<4} hit={:.0}% conf={:.0}% adj={:+.1}pp [{}]  {}",
            r.layer,
            r.topic,
            r.conviction,
            r.n_scored,
            r.raw_hit_rate * 100.0,
            r.avg_confidence * 100.0,
            r.adjustment_pp,
            r.adjustment_direction,
            r.apply_note
        );
    }
    Ok(())
}

// ------- failure correlations -------

pub fn failures_correlations(
    backend: &BackendConnection,
    cluster: Option<&str>,
    min_share: Option<f64>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = failure_correlations::list(conn, cluster, min_share)?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No failure correlations match.");
        return Ok(());
    }
    for r in &rows {
        println!(
            "{:<24} ↔ {:<24}  co={}  share={:.2}  (a={}, b={}, win={}d)",
            r.cluster_a,
            r.cluster_b,
            r.co_wrong_count,
            r.co_wrong_share,
            r.a_total_wrong,
            r.b_total_wrong,
            r.window_days
        );
    }
    Ok(())
}

// ------- clusters -------

pub fn clusters_list(backend: &BackendConnection, json: bool) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = clusters::list_clusters(conn)?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No clusters present on prediction_lessons (cluster_key NULL on all rows).");
        return Ok(());
    }
    for c in &rows {
        println!("{:<40} {}", c.cluster_key, c.lesson_count);
    }
    Ok(())
}

pub fn clusters_stats(backend: &BackendConnection, json: bool) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = clusters::cluster_stats(conn)?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No cluster stats: prediction_lessons has no cluster_key rows yet.");
        return Ok(());
    }
    for c in &rows {
        println!(
            "{:<40} lessons={:<4} predictions_applying={}",
            c.cluster_key, c.lesson_count, c.predictions_applying
        );
    }
    Ok(())
}

// ------- falsifications -------

pub fn falsifications_list(
    backend: &BackendConnection,
    rule_type: Option<&str>,
    auto_eligible: bool,
    for_prediction: Option<i64>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = prediction_falsification_rules::list(conn, rule_type, auto_eligible, for_prediction)?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No falsification rules match.");
        return Ok(());
    }
    for r in &rows {
        println!(
            "#{:<4} pred={:<6} type={:<16} auto={}  {}",
            r.id,
            r.prediction_id
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".to_string()),
            r.rule_type,
            r.auto_eligible,
            r.description
        );
    }
    Ok(())
}

// ------- operator replies -------

pub fn replies_list(
    backend: &BackendConnection,
    report_date: Option<&str>,
    asset: Option<&str>,
    decision_type: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = operator_replies::list(conn, report_date, asset, decision_type)?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No operator replies match.");
        return Ok(());
    }
    for r in &rows {
        println!(
            "{} {} {} asset={} decision={} response={} : {}",
            r.id,
            r.report_date,
            r.reply_date,
            r.asset.as_deref().unwrap_or("-"),
            r.decision_type,
            r.response_class,
            r.reasoning_summary.as_deref().unwrap_or(&r.raw_content),
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn replies_add(
    backend: &BackendConnection,
    report_date: &str,
    reply_date: Option<&str>,
    asset: Option<&str>,
    decision_type: &str,
    response_class: &str,
    conviction_implied: Option<&str>,
    horizon: Option<&str>,
    reasoning: Option<&str>,
    raw_content: &str,
    journal_id: Option<i64>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let reply_date_value: String = reply_date.map(|s| s.to_string()).unwrap_or(today);
    let id = operator_replies::insert(
        conn,
        &operator_replies::OperatorReplyInsert {
            journal_id,
            report_date,
            reply_date: &reply_date_value,
            asset,
            decision_type,
            response_class,
            conviction_implied,
            timeframe_horizon: horizon,
            reasoning_summary: reasoning,
            raw_content,
        },
    )?;
    if json {
        print_json(&serde_json::json!({"id": id}))
    } else {
        println!("added operator_reply id={}", id);
        Ok(())
    }
}
