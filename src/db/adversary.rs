//! `adversary` — write-time adversary composer.
//!
//! Given a draft prediction, classify it into a `cluster_key` (re-using the
//! preflight classifier — `crate::db::clusters::classify_claim`), then
//! compose a deterministic structured "case against" the claim from existing
//! DB substrate:
//!
//! * `anti_pattern_arguments`: each anti-pattern `reasoning_fragment` reachable
//!   from the cluster (via `lesson_fragment_edges` → `prediction_lessons`).
//! * `cofailure_warnings`: top-3 lessons from the highest co-failing cluster
//!   identified by `failure_correlations`.
//! * `falsification_triggers`: derived conditions under which the claim would
//!   clearly fail, composed from anti-pattern derivations + lesson
//!   `why_wrong` snippets.
//!
//! No LLM call is required. The composer is deliberately data-driven so the
//! same claim against the same substrate produces the same view, which
//! makes the write-time adversary easy to test and embed as part of the
//! prediction's permanent record.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db::clusters;
use crate::db::failure_correlations::{self, FailureCorrelation};
use crate::db::reasoning_fragments::{self, ReasoningFragment};

/// Draft prediction inputs the adversary is arguing against. Mirrors
/// `crate::db::preflight::PreflightDraft` so the same draft object flows
/// through both paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdversaryDraft {
    pub claim: String,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub conviction: Option<String>,
    /// Analyst layer (e.g. "low", "medium", "high", "macro"). Used for
    /// future calibration-aware filtering; today it flows through only to
    /// keep the input shape symmetric with preflight.
    pub layer: Option<String>,
}

/// One anti-pattern argument distilled from a `reasoning_fragments` row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdversaryArgument {
    pub fragment_id: String,
    pub fragment_name: String,
    pub summary: String,
    pub confidence: String,
}

/// One co-failure warning: a lesson from the highest-share co-failing cluster.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CofailureWarning {
    pub cluster_key: String,
    pub lesson_id: i64,
    pub miss_type: String,
    pub why_wrong: String,
    pub signal_misread: Option<String>,
}

/// Composed write-time adversary view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdversaryView {
    pub draft: AdversaryDraft,
    pub cluster_key: Option<String>,
    pub anti_pattern_arguments: Vec<AdversaryArgument>,
    pub cofailure_warnings: Vec<CofailureWarning>,
    pub falsification_triggers: Vec<String>,
}

impl AdversaryView {
    /// Compact bullet rendering for pretty-mode CLI output. Deterministic.
    pub fn pretty_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push("Adversary view (case against)".to_string());
        out.push(format!(
            "  cluster_key: {}",
            self.cluster_key
                .clone()
                .unwrap_or_else(|| "<unclassified>".into())
        ));
        if self.anti_pattern_arguments.is_empty() {
            out.push("  anti_pattern_arguments: <none>".to_string());
        } else {
            out.push("  anti_pattern_arguments:".to_string());
            for a in &self.anti_pattern_arguments {
                out.push(format!(
                    "    - {} [{}]: {}",
                    a.fragment_id, a.confidence, a.summary
                ));
            }
        }
        if self.cofailure_warnings.is_empty() {
            out.push("  cofailure_warnings: <none>".to_string());
        } else {
            out.push("  cofailure_warnings:".to_string());
            for w in &self.cofailure_warnings {
                out.push(format!(
                    "    - cluster={} lesson#{} [{}]: {}",
                    w.cluster_key, w.lesson_id, w.miss_type, w.why_wrong
                ));
            }
        }
        if self.falsification_triggers.is_empty() {
            out.push("  falsification_triggers: <none>".to_string());
        } else {
            out.push("  falsification_triggers:".to_string());
            for t in &self.falsification_triggers {
                out.push(format!("    - {}", t));
            }
        }
        out
    }

    /// Compact one-line summary suitable for embedding into a prediction's
    /// `resolution_criteria` (mirrors `PreflightFindings::inline_summary`).
    pub fn inline_summary(&self) -> String {
        let cluster = self
            .cluster_key
            .clone()
            .unwrap_or_else(|| "<unclassified>".into());
        let frag_ids: Vec<&str> = self
            .anti_pattern_arguments
            .iter()
            .take(3)
            .map(|a| a.fragment_id.as_str())
            .collect();
        let co_cluster = self
            .cofailure_warnings
            .first()
            .map(|w| w.cluster_key.as_str())
            .unwrap_or("<none>");
        format!(
            "[adversary] cluster={}; anti_patterns=[{}]; co_failing={}; n_falsification_triggers={}",
            cluster,
            frag_ids.join(","),
            co_cluster,
            self.falsification_triggers.len(),
        )
    }
}

/// Compose the write-time adversary view for the supplied draft prediction.
pub fn compose(conn: &Connection, draft: &AdversaryDraft) -> Result<AdversaryView> {
    let cluster_key = clusters::classify_claim(&draft.claim).map(|s| s.to_string());

    let anti_pattern_fragments = match cluster_key.as_deref() {
        Some(c) if table_exists(conn, "reasoning_fragments")? => {
            // Reuse the preflight-side reachability helper for parity.
            let frags = reasoning_fragments::fragments_for_cluster(conn, c)?;
            frags
                .into_iter()
                .filter(|f| f.fragment_type == "anti-pattern")
                .collect::<Vec<_>>()
        }
        _ => Vec::new(),
    };

    let anti_pattern_arguments: Vec<AdversaryArgument> = anti_pattern_fragments
        .iter()
        .map(argument_from_fragment)
        .collect();

    let top_co_failing_cluster = match cluster_key.as_deref() {
        Some(c) if table_exists(conn, "failure_correlations")? => {
            let rows = failure_correlations::list(conn, Some(c), None)?;
            rows.into_iter().next()
        }
        _ => None,
    };

    let cofailure_warnings = match (cluster_key.as_deref(), top_co_failing_cluster.as_ref()) {
        (Some(self_cluster), Some(corr)) => {
            let other = other_cluster_of(corr, self_cluster);
            collect_top_lessons_for_cluster(conn, other, 3)?
                .into_iter()
                .map(|(lesson_id, miss_type, why_wrong, signal_misread)| CofailureWarning {
                    cluster_key: other.to_string(),
                    lesson_id,
                    miss_type,
                    why_wrong,
                    signal_misread,
                })
                .collect::<Vec<_>>()
        }
        _ => Vec::new(),
    };

    let falsification_triggers = derive_falsification_triggers(
        &anti_pattern_fragments,
        &cofailure_warnings,
        cluster_key.as_deref(),
    );

    Ok(AdversaryView {
        draft: draft.clone(),
        cluster_key,
        anti_pattern_arguments,
        cofailure_warnings,
        falsification_triggers,
    })
}

fn argument_from_fragment(frag: &ReasoningFragment) -> AdversaryArgument {
    // Truncate fragment text into a short blurb so the JSON payload stays
    // tight when embedded in `resolution_criteria` or rendered inline.
    let summary: String = frag.fragment.chars().take(240).collect();
    AdversaryArgument {
        fragment_id: frag.canonical_id.clone(),
        fragment_name: frag.canonical_id.replace('-', " "),
        summary,
        confidence: frag.confidence.clone(),
    }
}

fn other_cluster_of<'a>(corr: &'a FailureCorrelation, self_cluster: &str) -> &'a str {
    if corr.cluster_a == self_cluster {
        &corr.cluster_b
    } else {
        &corr.cluster_a
    }
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let exists: i64 = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1")?
        .query_row(params![name], |row| row.get(0))
        .unwrap_or(0);
    Ok(exists > 0)
}

#[allow(clippy::type_complexity)]
fn collect_top_lessons_for_cluster(
    conn: &Connection,
    cluster_key: &str,
    limit: usize,
) -> Result<Vec<(i64, String, String, Option<String>)>> {
    if !table_exists(conn, "prediction_lessons")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, miss_type, why_wrong, signal_misread
         FROM prediction_lessons
         WHERE cluster_key = ?1
         ORDER BY id DESC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![cluster_key, limit as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Compose falsification triggers from the available fragments + lessons.
/// Deterministic: same inputs always produce the same trigger list.
fn derive_falsification_triggers(
    anti_pattern_fragments: &[ReasoningFragment],
    cofailure_warnings: &[CofailureWarning],
    cluster_key: Option<&str>,
) -> Vec<String> {
    let mut triggers: Vec<String> = Vec::new();

    for frag in anti_pattern_fragments {
        // Fragment derivation strings often look like
        // "round-strike OI > X" or "real yields > 2.5%". Use the derivation
        // when present (precise condition), else paraphrase from name.
        let condition = match frag.derivation.as_ref() {
            Some(d) if !d.trim().is_empty() => d.trim().to_string(),
            _ => format!("anti-pattern '{}' observed", frag.canonical_id),
        };
        triggers.push(format!(
            "If {}, the claim is invalidated (anti-pattern: {}).",
            condition, frag.canonical_id
        ));
    }

    for warn in cofailure_warnings {
        // Truncate why_wrong into a single-clause condition.
        let snippet: String = warn.why_wrong.chars().take(160).collect();
        triggers.push(format!(
            "If the {} regime that broke lesson#{} re-emerges (\"{}\"), the claim is at high risk.",
            warn.cluster_key, warn.lesson_id, snippet,
        ));
    }

    if triggers.is_empty() {
        if let Some(c) = cluster_key {
            triggers.push(format!(
                "If the dominant mechanism for cluster {} reverses inside the prediction horizon, the claim is invalidated.",
                c
            ));
        }
    }

    triggers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::reasoning_fragments::upsert_edge;
    use crate::db::{failure_correlations, reasoning_fragments, schema};

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    fn seed_lesson(
        conn: &Connection,
        prediction_id: i64,
        cluster_key: &str,
        why_wrong: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO prediction_lessons
                (prediction_id, miss_type, what_predicted, what_happened, why_wrong,
                 signal_misread, cluster_key)
             VALUES (?1, 'directional', 'a', 'b', ?2, NULL, ?3)",
            params![prediction_id, why_wrong, cluster_key],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn seed_prediction(conn: &Connection, claim: &str, symbol: Option<&str>) -> i64 {
        conn.execute(
            "INSERT INTO user_predictions
                (claim, symbol, conviction, timeframe, topic, outcome, lessons_applied)
             VALUES (?1, ?2, 'high', 'medium', 'commodities', 'pending', '[]')",
            params![claim, symbol],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn compose_returns_expected_struct_from_fixture_substrate() {
        let conn = fresh_conn();

        // Anti-pattern fragment for the options cluster.
        reasoning_fragments::upsert_fragment(
            &conn,
            "options-gamma-pinning",
            "Round-number strikes pin price intraday when OI > 50k",
            "anti-pattern",
            "options",
            "high",
            Some("call+put OI at round strike > 50_000"),
            false,
        )
        .unwrap();

        // Wire it to a lesson in the options cluster.
        let pid = seed_prediction(&conn, "stub", None);
        let lid = seed_lesson(
            &conn,
            pid,
            "options_gamma_pinning",
            "Pin held into expiry — call OI at the strike dominated",
        );
        upsert_edge(&conn, lid, "options-gamma-pinning", "primary").unwrap();

        // Co-failing cluster + seed three lessons on the OTHER side.
        failure_correlations::upsert(
            &conn,
            "options_gamma_pinning",
            "tight_threshold_close_miss",
            5,
            7,
            9,
            0.71,
            7,
        )
        .unwrap();
        let pid2 = seed_prediction(&conn, "stub2", None);
        let _l_a = seed_lesson(
            &conn,
            pid2,
            "tight_threshold_close_miss",
            "Threshold within 0.5xATR coin-flips",
        );
        let pid3 = seed_prediction(&conn, "stub3", None);
        let _l_b = seed_lesson(
            &conn,
            pid3,
            "tight_threshold_close_miss",
            "Round-number magnet pulled close to level",
        );
        let pid4 = seed_prediction(&conn, "stub4", None);
        let _l_c = seed_lesson(
            &conn,
            pid4,
            "tight_threshold_close_miss",
            "Auction close pinned to nearest 5pt strike",
        );
        let pid5 = seed_prediction(&conn, "stub5", None);
        let _l_d = seed_lesson(
            &conn,
            pid5,
            "tight_threshold_close_miss",
            "FOURTH lesson — should be excluded by top-3 cap",
        );

        let draft = AdversaryDraft {
            claim: "SPY gamma pin at 700 by 0dte expiry".into(),
            symbol: Some("SPY".into()),
            timeframe: Some("low".into()),
            conviction: Some("medium".into()),
            layer: Some("low".into()),
        };
        let view = compose(&conn, &draft).unwrap();
        assert_eq!(view.cluster_key.as_deref(), Some("options_gamma_pinning"));
        assert_eq!(view.anti_pattern_arguments.len(), 1);
        assert_eq!(
            view.anti_pattern_arguments[0].fragment_id,
            "options-gamma-pinning"
        );
        assert_eq!(view.anti_pattern_arguments[0].confidence, "high");
        // Top-3 cap on co-failing lessons.
        assert_eq!(view.cofailure_warnings.len(), 3);
        for w in &view.cofailure_warnings {
            assert_eq!(w.cluster_key, "tight_threshold_close_miss");
        }
        // Falsification triggers compose from BOTH fragments and warnings.
        assert!(view.falsification_triggers.len() >= 4);
        assert!(view
            .falsification_triggers
            .iter()
            .any(|t| t.contains("anti-pattern: options-gamma-pinning")));
        assert!(view
            .falsification_triggers
            .iter()
            .any(|t| t.contains("tight_threshold_close_miss")));
    }

    #[test]
    fn unclassified_claim_returns_empty_arrays() {
        let conn = fresh_conn();
        let draft = AdversaryDraft {
            claim: "totally random uncategorized zzz".into(),
            symbol: None,
            timeframe: None,
            conviction: None,
            layer: None,
        };
        let view = compose(&conn, &draft).unwrap();
        assert!(view.cluster_key.is_none());
        assert!(view.anti_pattern_arguments.is_empty());
        assert!(view.cofailure_warnings.is_empty());
        assert!(view.falsification_triggers.is_empty());
    }

    #[test]
    fn cluster_without_anti_pattern_still_returns_warnings_and_default_trigger() {
        let conn = fresh_conn();

        // No anti-pattern fragment at all. Just a co-failing cluster.
        failure_correlations::upsert(
            &conn,
            "realrates_dominates_gold",
            "btc_correlation_regime",
            4,
            6,
            8,
            0.66,
            7,
        )
        .unwrap();
        let pid = seed_prediction(&conn, "stub", None);
        seed_lesson(
            &conn,
            pid,
            "btc_correlation_regime",
            "BTC decoupled from real yields during the late-March risk-on burst",
        );

        // Use the same claim shape the preflight test relies on so the
        // rules-based classifier picks `realrates_dominates_gold` rather
        // than `iran_gold_war_fatigue` (which would otherwise win on a
        // bare "gold" keyword).
        let draft = AdversaryDraft {
            claim: "Real yield breakdown drives tips and breakeven repricing".into(),
            symbol: Some("GLD".into()),
            timeframe: Some("medium".into()),
            conviction: Some("high".into()),
            layer: Some("medium".into()),
        };
        let view = compose(&conn, &draft).unwrap();
        assert_eq!(
            view.cluster_key.as_deref(),
            Some("realrates_dominates_gold")
        );
        assert!(view.anti_pattern_arguments.is_empty());
        assert_eq!(view.cofailure_warnings.len(), 1);
        assert_eq!(
            view.cofailure_warnings[0].cluster_key,
            "btc_correlation_regime"
        );
        // Falsification triggers should include the warning-derived one.
        assert!(!view.falsification_triggers.is_empty());
        assert!(view
            .falsification_triggers
            .iter()
            .any(|t| t.contains("btc_correlation_regime")));
    }

    #[test]
    fn pretty_lines_render_compact_bullet_list() {
        let conn = fresh_conn();
        let draft = AdversaryDraft {
            claim: "Gold real yield breakout".into(),
            symbol: None,
            timeframe: None,
            conviction: None,
            layer: None,
        };
        let view = compose(&conn, &draft).unwrap();
        let lines = view.pretty_lines();
        assert!(lines[0].starts_with("Adversary view"));
        assert!(lines.iter().any(|l| l.contains("cluster_key")));
    }

    #[test]
    fn inline_summary_is_compact_and_deterministic() {
        let conn = fresh_conn();
        let draft = AdversaryDraft {
            claim: "Gold real yield breakout".into(),
            symbol: None,
            timeframe: None,
            conviction: None,
            layer: None,
        };
        let view = compose(&conn, &draft).unwrap();
        let s = view.inline_summary();
        assert!(s.starts_with("[adversary]"));
        assert!(s.contains("cluster="));
        assert!(s.contains("n_falsification_triggers="));
    }
}
