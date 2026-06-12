//! `pftui journal prediction rescore-audit` — legacy-outcome verification.
//!
//! ~354 predictions were scored before the mechanical falsification scorer
//! existed (#883); they were LLM-judged, possibly against since-repaired
//! corrupt prices, and plausibly graded generously. This audit re-derives
//! each legacy outcome mechanically (reusing the EXACT #883 evaluation
//! semantics in `commands::predict`) and reports where the recorded outcome
//! disagrees with the price evidence.
//!
//! Classification per legacy scored prediction (outcome ∈ correct/partial/
//! wrong, `score_notes` NOT starting with `auto-scored:`):
//! - `agree` / `agree-partial` — recorded outcome matches the mechanical
//!   verdict (partial counts as agreement unless the mechanical evidence
//!   contradicts the claim's DIRECTION entirely; see below).
//! - `disagree` — recorded vs mechanical mismatch, with the deciding bar's
//!   date+close as evidence. `recorded=correct, mechanical=wrong` is the
//!   GENEROSITY measure; the reverse is harshness.
//! - `unparseable` — no price-type falsification rule stored and the claim/
//!   resolution_criteria do not parse through the falsify grammar (event-*
//!   and unstructured rules land here). Excluded from agreement stats.
//! - `window-open` — the rule's evaluation window has not expired and no
//!   deciding close exists yet (the LLM scored early). Excluded.
//! - `unevaluable` — rule exists but evaluation failed (missing symbol or
//!   price history). Excluded, counted.
//!
//! PARTIAL judgment (documented per the audit spec): the mechanical scorer
//! is binary. A recorded `partial` is classified `disagree` ONLY when the
//! mechanical result is `wrong` AND the net close-to-close move across the
//! evaluation window ran AGAINST the rule's direction (claimed above/up but
//! the asset fell net, or claimed below/down but it rose net). A partial
//! whose direction was right but threshold unmet — and any range rule — is
//! `agree-partial`.
//!
//! `--apply-high-confidence` flips a disagreeing recorded outcome to the
//! mechanical one ONLY when (a) the rule parsed at HIGH confidence, (b) the
//! deciding close is not within 1% of the threshold, (c) the resolved
//! price series was not affected by the corruption repairs in its window
//! (data-hygiene notes #729/#730/#735: BTC/BTC-USD equity-ticker window
//! 2025-03-20→2026-02-27 and the 2026-06-11 stale stamp; USDJPY=X/JPY=X/
//! CNY=X placeholder closes; KC=F/ZC=F/ZS=F/ZW=F frozen-feed window since
//! 2026-03-13), and (d) the rule carries NO quality-defect flags (see
//! `rule_suspect_flags` — legacy rules frequently encode a negated claim's
//! failure condition, a garbled threshold, or the wrong measurand, making
//! the mechanical verdict a faithful verdict on the RULE but not on the
//! CLAIM). Every flip APPENDS provenance to `score_notes` — the original
//! outcome is preserved in the note, never silently rewritten.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::json;

use crate::commands::predict::{
    evaluate_falsification_rule, load_series_window, parse_falsify_rule,
};
use crate::db::backend::BackendConnection;
use crate::db::prediction_falsification_rules::{self, PredictionFalsificationRule};

/// Relative distance from the deciding close to the rule threshold below
/// which a correction is considered ambiguous and is NOT applied.
const APPLY_PROXIMITY_MIN: f64 = 0.01;

/// BTC equity-ticker corruption window (data-hygiene note #730): rows in the
/// canonical BTC series were Yahoo's EQUITY ticker masquerading as bitcoin.
const BTC_CORRUPTION_START: &str = "2025-03-20";
const BTC_CORRUPTION_END: &str = "2026-02-27";
/// Stale-cache stamp deleted from BTC-USD (note #729).
const BTC_STALE_STAMP_DATE: &str = "2026-06-11";
/// FX series that carried literal placeholder closes (notes #730/#735) —
/// blocked for apply in ANY window (placeholder extent not fully bounded).
const FX_PLACEHOLDER_SERIES: &[&str] = &["USDJPY=X", "JPY=X", "CNY=X"];
/// Frozen agri feeds (note #735): single stamped close repeated since
/// 2026-03-13; repaired 2026-06-12 but the feeds remain dead upstream.
const FROZEN_FEED_SERIES: &[&str] = &["KC=F", "ZC=F", "ZS=F", "ZW=F"];
const FROZEN_FEED_START: &str = "2026-03-13";

/// Tokens stripped before the relaxed re-parse of legacy claim text.
/// Legacy claims read "BTC will close below 50000 by 2026-01-31" — filler
/// words between the grammar tokens are the only normalization performed;
/// the actual parse is still the deterministic #883 grammar.
const RELAXED_FILLER_TOKENS: &[&str] = &["will", "to", "should", "the", "a", "at"];

/// Words that mark a NEGATED claim ("BTC fails to sustain above 72k",
/// "Oil does NOT close above 102"). Legacy LLM-written rules frequently
/// encoded the literal directional phrase of such claims — i.e. the claim's
/// FAILURE condition — instead of the #883 success-condition convention,
/// so the mechanical verdict on the rule can be the inverse of a verdict
/// on the claim. These rows still count in the agreement stats (flagged),
/// but are never auto-corrected.
const NEGATION_MARKERS: &[&str] = &["fails", "fail", "not", "never", "avoid", "avoids", "without"];

/// Multiplicative band within which a number found in the claim text is
/// considered "price-like" relative to the rule threshold (and within which
/// threshold and deciding close must sit relative to each other).
const MAGNITUDE_BAND: f64 = 5.0;

/// Relative tolerance for matching a claim-text level to a rule bound.
const LEVEL_MATCH_TOLERANCE: f64 = 0.02;

#[derive(Debug, Clone, Serialize)]
pub struct AuditRow {
    pub prediction_id: i64,
    pub layer: String,
    pub symbol: Option<String>,
    pub claim_excerpt: String,
    pub recorded_outcome: String,
    pub mechanical_outcome: Option<String>,
    /// agree | agree-partial | disagree | unparseable | window-open | unevaluable
    pub classification: String,
    /// stored-rule | reparsed-resolution | reparsed-claim
    pub rule_source: Option<String>,
    pub rule_type: Option<String>,
    pub parse_confidence: Option<String>,
    pub threshold: Option<String>,
    pub observed: Option<String>,
    pub series: Option<String>,
    pub evidence: Option<String>,
    /// generous | harsh | partial-direction-contradicted (disagree rows only)
    pub disagreement_kind: Option<String>,
    /// Legacy-rule quality defects (negated/conditional claim, magnitude
    /// mismatch, claim level absent from rule). Non-empty → the mechanical
    /// verdict may not be a verdict on the CLAIM; never auto-corrected.
    pub rule_suspect_flags: Vec<String>,
    pub detail: Option<String>,
    pub apply_eligible: bool,
    pub apply_blockers: Vec<String>,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerAgreement {
    pub layer: String,
    pub agree: usize,
    pub disagree: usize,
    pub agreement_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfusionCell {
    pub recorded: String,
    pub mechanical: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RescoreAuditReport {
    pub generated_at: String,
    pub apply_mode: bool,
    pub total_legacy_scored: usize,
    pub agree: usize,
    pub agree_partial: usize,
    pub disagree: usize,
    pub unparseable: usize,
    pub window_open: usize,
    pub unevaluable: usize,
    /// (agree + agree-partial) / (agree + agree-partial + disagree)
    pub agreement_rate: Option<f64>,
    /// recorded=correct scored mechanically wrong — the generosity measure.
    pub generous_count: usize,
    /// generous_count / adjudicated recorded-correct rows.
    pub generosity_rate: Option<f64>,
    /// recorded=wrong scored mechanically correct.
    pub harsh_count: usize,
    pub harshness_rate: Option<f64>,
    /// Adjudicated rows whose legacy rule carries quality-defect flags
    /// (see `AuditRow::rule_suspect_flags`).
    pub rule_suspect_count: usize,
    /// Agreement over the rows with NO rule-quality flags — the defensible
    /// LLM-grading-drift measure (rule-defect noise removed).
    pub agreement_rate_clean: Option<f64>,
    pub generous_count_clean: usize,
    pub harsh_count_clean: usize,
    pub by_layer: Vec<LayerAgreement>,
    pub confusion: Vec<ConfusionCell>,
    pub applied_count: usize,
    pub rows: Vec<AuditRow>,
}

struct LegacyPrediction {
    id: i64,
    claim: String,
    symbol: Option<String>,
    timeframe: Option<String>,
    resolution_criteria: Option<String>,
    outcome: String,
    created_date: String,
}

fn is_price_rule_type(rule_type: &str) -> bool {
    rule_type.starts_with("close-") || rule_type.starts_with("stays-")
        || rule_type.starts_with("prints-")
}

fn rule_has_threshold(rule: &PredictionFalsificationRule) -> bool {
    rule.threshold_value.is_some() || rule.threshold_low.is_some() || rule.threshold_high.is_some()
}

/// Relaxed legacy-claim normalization: strip filler tokens and `$`/commas on
/// numbers, then run the unchanged deterministic grammar. Returns the parsed
/// rule only — the caller decides the confidence downgrade.
fn relaxed_parse(text: &str) -> Option<crate::commands::predict::ParsedFalsifyRule> {
    let cleaned: Vec<String> = text
        .split_whitespace()
        .filter(|t| !RELAXED_FILLER_TOKENS.contains(&t.to_ascii_lowercase().as_str()))
        .map(|t| t.trim_start_matches('$').to_string())
        .filter(|t| !t.is_empty())
        .collect();
    parse_falsify_rule(&cleaned.join(" ")).ok()
}

/// Resolve the falsification rule to audit a legacy prediction against, in
/// preference order: stored price-type rule row → exact grammar parse of
/// resolution_criteria → exact parse of the claim → relaxed (filler-stripped)
/// parse of either. Returns (rule, source) or None (unparseable).
fn resolve_rule(
    prediction: &LegacyPrediction,
    stored: Option<&PredictionFalsificationRule>,
) -> Option<(PredictionFalsificationRule, &'static str)> {
    if let Some(rule) = stored {
        if is_price_rule_type(&rule.rule_type)
            && rule_has_threshold(rule)
            && !rule.eval_date_end.is_empty()
        {
            let mut rule = rule.clone();
            // Legacy rows carry NULL eval_date_start; the #883 write path
            // starts the window at the add date. Mirror that. When the
            // created date is unavailable, leave None — `rule_window` then
            // falls back to a single-day window at the deadline rather
            // than an unbounded all-history scan.
            if rule.eval_date_start.is_none() && !prediction.created_date.is_empty() {
                rule.eval_date_start = Some(prediction.created_date.clone());
            }
            return Some((rule, "stored-rule"));
        }
    }

    let candidates: [(Option<&str>, &'static str); 2] = [
        (
            prediction.resolution_criteria.as_deref(),
            "reparsed-resolution",
        ),
        (Some(prediction.claim.as_str()), "reparsed-claim"),
    ];
    // Exact parses first (high confidence), then relaxed (medium).
    for (text, source) in candidates {
        if let Some(text) = text {
            if let Ok(parsed) = parse_falsify_rule(text) {
                return Some((synthetic_rule(prediction, &parsed, "high"), source));
            }
        }
    }
    for (text, source) in candidates {
        if let Some(text) = text {
            if let Some(parsed) = relaxed_parse(text) {
                return Some((synthetic_rule(prediction, &parsed, "medium"), source));
            }
        }
    }
    None
}

fn synthetic_rule(
    prediction: &LegacyPrediction,
    parsed: &crate::commands::predict::ParsedFalsifyRule,
    parse_confidence: &str,
) -> PredictionFalsificationRule {
    PredictionFalsificationRule {
        id: 0,
        prediction_id: prediction.id,
        claim: prediction.claim.clone(),
        prediction_symbol: prediction.symbol.clone(),
        current_outcome: prediction.outcome.clone(),
        rule_type: parsed.rule_type.clone(),
        symbol: Some(parsed.asset.clone()),
        threshold_value: parsed.threshold_value,
        threshold_low: parsed.threshold_low,
        threshold_high: parsed.threshold_high,
        eval_date_start: if prediction.created_date.is_empty() {
            None
        } else {
            Some(prediction.created_date.clone())
        },
        eval_date_end: parsed.eval_date_end.clone(),
        parse_confidence: parse_confidence.to_string(),
    }
}

/// Direction encoded by a rule type: +1 (above/up), -1 (below/down),
/// 0 (range — no direction).
fn rule_direction(rule_type: &str) -> i8 {
    if rule_type.ends_with("above") {
        1
    } else if rule_type.ends_with("below") {
        -1
    } else {
        0
    }
}

/// Net close-to-close move across the evaluation window, used by the
/// partial-outcome direction check. None when the series has <2 rows.
fn net_window_move(
    backend: &BackendConnection,
    rule: &PredictionFalsificationRule,
    today: NaiveDate,
) -> Result<Option<Decimal>> {
    let symbol = match rule
        .symbol
        .as_deref()
        .or(rule.prediction_symbol.as_deref())
    {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Ok(None),
    };
    let start = rule.eval_date_start.as_deref().unwrap_or(&rule.eval_date_end);
    let today_str = today.format("%Y-%m-%d").to_string();
    let end = if rule.eval_date_end.as_str() < today_str.as_str() {
        rule.eval_date_end.clone()
    } else {
        today_str
    };
    let Some((_, rows)) = load_series_window(backend, &symbol, start, &end)? else {
        return Ok(None);
    };
    if rows.len() < 2 {
        return Ok(None);
    }
    Ok(Some(rows[rows.len() - 1].1 - rows[0].1))
}

/// Relative distance of the deciding close from the rule threshold
/// (nearest bound for range rules). None when no numeric threshold exists.
fn threshold_proximity(rule: &PredictionFalsificationRule, observed: Decimal) -> Option<f64> {
    use rust_decimal::prelude::ToPrimitive;
    let obs = observed.to_f64()?;
    rule_bounds(rule)
        .into_iter()
        .filter(|b| *b != 0.0)
        .map(|b| ((obs - b) / b).abs())
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

fn rule_bounds(rule: &PredictionFalsificationRule) -> Vec<f64> {
    let mut bounds = Vec::new();
    if rule_direction(&rule.rule_type) != 0 {
        bounds.extend(
            rule.threshold_value
                .or(rule.threshold_high)
                .or(rule.threshold_low),
        );
    } else {
        bounds.extend(rule.threshold_low);
        bounds.extend(rule.threshold_high);
    }
    bounds
}

/// Numeric levels mentioned in the claim text. Percent-suffixed tokens
/// (probabilities, CPI prints) are excluded, as are bare years (1900-2100).
fn claim_numeric_levels(claim: &str) -> Vec<f64> {
    claim
        .split_whitespace()
        .filter(|t| !t.contains('%'))
        .filter_map(|t| {
            let cleaned: &str =
                t.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != ',');
            if cleaned.is_empty() {
                return None;
            }
            let value: f64 = cleaned.replace(',', "").parse().ok()?;
            if !value.is_finite() {
                return None;
            }
            // Bare years read as dates, not price levels.
            if value.fract() == 0.0 && (1900.0..=2100.0).contains(&value) {
                return None;
            }
            Some(value)
        })
        .collect()
}

/// Detect legacy-rule quality defects that make the mechanical verdict an
/// unreliable verdict ON THE CLAIM (the rule itself is still evaluated
/// faithfully). Flagged rows keep their classification in the report but
/// are excluded from `--apply-high-confidence` and broken out of the
/// "clean" agreement statistics. Families observed in the live ledger:
/// - negated claims whose rule encodes the failure condition
///   ("Oil does NOT close above 102" stored as `close-above 102`);
/// - conditional claims ("If Polymarket resolves TRUE, oil holds 85-92");
/// - unit-garbled thresholds ("gold above 680" against a ~4,500 series);
/// - wrong measurand (a CPI 3.0-3.5 band evaluated against the DXY series);
/// - rules encoding a different level than the claim states
///   ("Oil trades below 91" stored as `stays-in-range 86 88`).
fn rule_suspect_flags(
    claim: &str,
    rule: &PredictionFalsificationRule,
    observed: Decimal,
) -> Vec<String> {
    use rust_decimal::prelude::ToPrimitive;
    let mut flags = Vec::new();

    let lower = claim.to_lowercase();
    if lower
        .split(|c: char| !c.is_ascii_alphabetic())
        .any(|w| NEGATION_MARKERS.contains(&w))
    {
        flags.push("claim-negation-marker".to_string());
    }
    if lower.starts_with("if ") || lower.contains(" if ") {
        flags.push("claim-conditional".to_string());
    }

    let bounds = rule_bounds(rule);
    if let Some(obs) = observed.to_f64() {
        if obs != 0.0
            && bounds.iter().any(|b| {
                let ratio = (b / obs).abs();
                *b == 0.0 || !(1.0 / MAGNITUDE_BAND..=MAGNITUDE_BAND).contains(&ratio)
            })
        {
            flags.push("threshold-magnitude-mismatch".to_string());
        }
    }

    // Every price-like level the claim states should correspond to a rule
    // bound; a stated level matching NO bound means the rule encodes a
    // different condition than the claim.
    let mismatched = claim_numeric_levels(claim).into_iter().any(|level| {
        let price_like = bounds.iter().any(|b| {
            *b != 0.0 && (1.0 / MAGNITUDE_BAND..=MAGNITUDE_BAND).contains(&(level / b).abs())
        });
        if !price_like {
            return false;
        }
        !bounds
            .iter()
            .any(|b| *b != 0.0 && ((level - b) / b).abs() <= LEVEL_MATCH_TOLERANCE)
    });
    if mismatched {
        flags.push("claim-level-not-in-rule".to_string());
    }

    flags
}

/// Corruption-repair window blockers for the resolved series (notes
/// #729/#730/#735). `start`/`end` bound the rule's evaluation window.
fn corruption_blockers(series: &str, start: &str, end: &str) -> Vec<String> {
    let mut blockers = Vec::new();
    let upper = series.to_ascii_uppercase();
    if upper == "BTC" || upper == "BTC-USD" {
        if start <= BTC_CORRUPTION_END && end >= BTC_CORRUPTION_START {
            blockers.push("btc-equity-corruption-window".to_string());
        }
        if start <= BTC_STALE_STAMP_DATE && end >= BTC_STALE_STAMP_DATE {
            blockers.push("btc-stale-stamp-2026-06-11".to_string());
        }
    }
    if FX_PLACEHOLDER_SERIES.contains(&upper.as_str()) {
        blockers.push("fx-placeholder-series".to_string());
    }
    if FROZEN_FEED_SERIES.contains(&upper.as_str()) && end >= FROZEN_FEED_START {
        blockers.push("frozen-feed-window".to_string());
    }
    blockers
}

fn load_legacy_scored(conn: &rusqlite::Connection) -> Result<Vec<LegacyPrediction>> {
    let mut stmt = conn.prepare(
        // NOTE: `date(created_at)` is NOT used here — live rows carry a
        // `+00` timezone suffix SQLite's date() rejects (returns NULL).
        // Every stored format starts with the YYYY-MM-DD prefix, so take
        // the prefix and validate it in Rust.
        "SELECT id, claim, symbol, timeframe, resolution_criteria, outcome,
                substr(created_at, 1, 10)
         FROM user_predictions
         WHERE outcome IN ('correct','partial','wrong')
           AND (score_notes IS NULL OR score_notes NOT LIKE 'auto-scored:%')
         ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(LegacyPrediction {
                id: row.get(0)?,
                claim: row.get(1)?,
                symbol: row.get(2)?,
                timeframe: row.get(3)?,
                resolution_criteria: row.get(4)?,
                outcome: row.get(5)?,
                created_date: row
                    .get::<_, Option<String>>(6)?
                    .filter(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
                    .unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn excerpt(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Run the rescore audit. `today` is injected for deterministic tests.
pub fn compute_rescore_audit(
    backend: &BackendConnection,
    apply_high_confidence: bool,
    today: NaiveDate,
) -> Result<RescoreAuditReport> {
    let conn = match backend.sqlite_native() {
        Some(c) => c,
        None => anyhow::bail!("rescore-audit requires the sqlite backend"),
    };
    crate::db::schema::run_migrations(conn)?;

    let legacy = load_legacy_scored(conn)?;
    let stored_rules: HashMap<i64, PredictionFalsificationRule> =
        prediction_falsification_rules::list_all_rules(conn)?
            .into_iter()
            .map(|r| (r.prediction_id, r))
            .collect();

    let mut rows: Vec<AuditRow> = Vec::new();
    for prediction in &legacy {
        let layer = prediction
            .timeframe
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "unspecified".to_string());
        let mut row = AuditRow {
            prediction_id: prediction.id,
            layer,
            symbol: prediction.symbol.clone(),
            claim_excerpt: excerpt(&prediction.claim, 80),
            recorded_outcome: prediction.outcome.clone(),
            mechanical_outcome: None,
            classification: String::new(),
            rule_source: None,
            rule_type: None,
            parse_confidence: None,
            threshold: None,
            observed: None,
            series: None,
            evidence: None,
            disagreement_kind: None,
            rule_suspect_flags: Vec::new(),
            detail: None,
            apply_eligible: false,
            apply_blockers: Vec::new(),
            applied: false,
        };

        let stored = stored_rules.get(&prediction.id);
        let Some((rule, source)) = resolve_rule(prediction, stored) else {
            row.classification = "unparseable".to_string();
            row.detail = Some(match stored {
                Some(r) => format!("stored rule_type '{}' is not mechanically scoreable", r.rule_type),
                None => "no stored rule; claim/resolution_criteria do not parse".to_string(),
            });
            rows.push(row);
            continue;
        };
        row.rule_source = Some(source.to_string());
        row.rule_type = Some(rule.rule_type.clone());
        row.parse_confidence = Some(rule.parse_confidence.clone());

        match evaluate_falsification_rule(backend, &rule, today) {
            Err(err) => {
                row.classification = "unevaluable".to_string();
                row.detail = Some(err.to_string());
            }
            Ok(None) => {
                row.classification = "window-open".to_string();
                row.detail = Some(format!(
                    "window {}..{} not yet decided mechanically (LLM scored early)",
                    rule.eval_date_start.as_deref().unwrap_or("?"),
                    rule.eval_date_end
                ));
            }
            Ok(Some(decision)) => {
                row.mechanical_outcome = Some(decision.outcome.to_string());
                row.threshold = Some(decision.threshold.clone());
                row.observed = Some(decision.observed.to_string());
                row.series = Some(decision.series.clone());
                row.evidence = Some(decision.evidence.clone());

                let recorded = prediction.outcome.as_str();
                let mechanical = decision.outcome;
                let (classification, kind) = match (recorded, mechanical) {
                    ("correct", "correct") | ("wrong", "wrong") => ("agree", None),
                    ("correct", "wrong") => ("disagree", Some("generous")),
                    ("wrong", "correct") => ("disagree", Some("harsh")),
                    ("partial", "correct") => ("agree-partial", None),
                    ("partial", "wrong") => {
                        let direction = rule_direction(&rule.rule_type);
                        let net = net_window_move(backend, &rule, today)?;
                        let contradicted = match (direction, net) {
                            (1, Some(net)) => net < Decimal::ZERO,
                            (-1, Some(net)) => net > Decimal::ZERO,
                            _ => false,
                        };
                        if contradicted {
                            row.detail = Some(format!(
                                "net window move {} ran against the claimed direction",
                                net.map(|n| n.round_dp(4).to_string())
                                    .unwrap_or_else(|| "?".to_string())
                            ));
                            ("disagree", Some("partial-direction-contradicted"))
                        } else {
                            row.detail = Some(
                                "mechanical wrong but direction not entirely contradicted"
                                    .to_string(),
                            );
                            ("agree-partial", None)
                        }
                    }
                    _ => ("unevaluable", None),
                };
                row.classification = classification.to_string();
                row.disagreement_kind = kind.map(str::to_string);
                row.rule_suspect_flags =
                    rule_suspect_flags(&prediction.claim, &rule, decision.observed);

                if classification == "disagree" {
                    let mut blockers: Vec<String> = row
                        .rule_suspect_flags
                        .iter()
                        .map(|f| format!("rule-suspect:{f}"))
                        .collect();
                    if rule.parse_confidence != "high" {
                        blockers.push(format!(
                            "parse-confidence-{}-below-high",
                            rule.parse_confidence
                        ));
                    }
                    match threshold_proximity(&rule, decision.observed) {
                        Some(proximity) if proximity <= APPLY_PROXIMITY_MIN => {
                            blockers.push(format!(
                                "deciding-close-within-1pct-of-threshold ({:.3}%)",
                                proximity * 100.0
                            ));
                        }
                        Some(_) => {}
                        None => blockers.push("no-numeric-threshold".to_string()),
                    }
                    let start = rule
                        .eval_date_start
                        .clone()
                        .unwrap_or_else(|| rule.eval_date_end.clone());
                    let today_str = today.format("%Y-%m-%d").to_string();
                    let end = if rule.eval_date_end.as_str() < today_str.as_str() {
                        rule.eval_date_end.clone()
                    } else {
                        today_str
                    };
                    blockers.extend(corruption_blockers(&decision.series, &start, &end));
                    row.apply_eligible = blockers.is_empty();
                    row.apply_blockers = blockers;
                }
            }
        }
        rows.push(row);
    }

    // ── Apply gated corrections ──
    let mut applied_count = 0usize;
    if apply_high_confidence {
        let today_str = today.format("%Y-%m-%d").to_string();
        for row in rows.iter_mut() {
            if row.classification != "disagree" || !row.apply_eligible {
                continue;
            }
            let Some(new_outcome) = row.mechanical_outcome.clone() else {
                continue;
            };
            let note = format!(
                "rescore-audit {}: outcome corrected {}→{}, evidence: {} [rule {} threshold {}, series {}]",
                today_str,
                row.recorded_outcome,
                new_outcome,
                row.evidence.as_deref().unwrap_or("?"),
                row.rule_type.as_deref().unwrap_or("?"),
                row.threshold.as_deref().unwrap_or("?"),
                row.series.as_deref().unwrap_or("?"),
            );
            conn.execute(
                "UPDATE user_predictions
                 SET outcome = ?2,
                     score_notes = CASE
                       WHEN score_notes IS NULL OR score_notes = '' THEN ?3
                       ELSE score_notes || ' | ' || ?3
                     END
                 WHERE id = ?1",
                rusqlite::params![row.prediction_id, new_outcome, note],
            )?;
            row.applied = true;
            applied_count += 1;
        }
    }

    // ── Aggregate ──
    let count = |class: &str| rows.iter().filter(|r| r.classification == class).count();
    let agree = count("agree");
    let agree_partial = count("agree-partial");
    let disagree = count("disagree");
    let adjudicated = agree + agree_partial + disagree;
    let agreement_rate = if adjudicated > 0 {
        Some((agree + agree_partial) as f64 / adjudicated as f64)
    } else {
        None
    };

    let generous_count = rows
        .iter()
        .filter(|r| r.disagreement_kind.as_deref() == Some("generous"))
        .count();
    let harsh_count = rows
        .iter()
        .filter(|r| r.disagreement_kind.as_deref() == Some("harsh"))
        .count();

    let is_clean = |r: &AuditRow| r.rule_suspect_flags.is_empty();
    let rule_suspect_count = rows
        .iter()
        .filter(|r| {
            !r.rule_suspect_flags.is_empty()
                && matches!(
                    r.classification.as_str(),
                    "agree" | "agree-partial" | "disagree"
                )
        })
        .count();
    let clean_agree = rows
        .iter()
        .filter(|r| {
            matches!(r.classification.as_str(), "agree" | "agree-partial") && is_clean(r)
        })
        .count();
    let clean_disagree = rows
        .iter()
        .filter(|r| r.classification == "disagree" && is_clean(r))
        .count();
    let agreement_rate_clean = if clean_agree + clean_disagree > 0 {
        Some(clean_agree as f64 / (clean_agree + clean_disagree) as f64)
    } else {
        None
    };
    let generous_count_clean = rows
        .iter()
        .filter(|r| r.disagreement_kind.as_deref() == Some("generous") && is_clean(r))
        .count();
    let harsh_count_clean = rows
        .iter()
        .filter(|r| r.disagreement_kind.as_deref() == Some("harsh") && is_clean(r))
        .count();
    let adjudicated_correct = rows
        .iter()
        .filter(|r| {
            r.recorded_outcome == "correct"
                && matches!(r.classification.as_str(), "agree" | "disagree")
        })
        .count();
    let adjudicated_wrong = rows
        .iter()
        .filter(|r| {
            r.recorded_outcome == "wrong"
                && matches!(r.classification.as_str(), "agree" | "disagree")
        })
        .count();

    let mut layer_map: HashMap<String, (usize, usize)> = HashMap::new();
    for row in &rows {
        match row.classification.as_str() {
            "agree" | "agree-partial" => layer_map.entry(row.layer.clone()).or_default().0 += 1,
            "disagree" => layer_map.entry(row.layer.clone()).or_default().1 += 1,
            _ => {}
        }
    }
    let mut by_layer: Vec<LayerAgreement> = layer_map
        .into_iter()
        .map(|(layer, (agree, disagree))| LayerAgreement {
            layer,
            agree,
            disagree,
            agreement_rate: if agree + disagree > 0 {
                Some(agree as f64 / (agree + disagree) as f64)
            } else {
                None
            },
        })
        .collect();
    by_layer.sort_by(|a, b| a.layer.cmp(&b.layer));

    let mut confusion_map: HashMap<(String, String), usize> = HashMap::new();
    for row in &rows {
        if let Some(mech) = &row.mechanical_outcome {
            if matches!(
                row.classification.as_str(),
                "agree" | "agree-partial" | "disagree"
            ) {
                *confusion_map
                    .entry((row.recorded_outcome.clone(), mech.clone()))
                    .or_default() += 1;
            }
        }
    }
    let mut confusion: Vec<ConfusionCell> = confusion_map
        .into_iter()
        .map(|((recorded, mechanical), count)| ConfusionCell {
            recorded,
            mechanical,
            count,
        })
        .collect();
    confusion.sort_by_key(|c| (c.recorded.clone(), c.mechanical.clone()));

    Ok(RescoreAuditReport {
        generated_at: Utc::now().to_rfc3339(),
        apply_mode: apply_high_confidence,
        total_legacy_scored: legacy.len(),
        agree,
        agree_partial,
        disagree,
        unparseable: count("unparseable"),
        window_open: count("window-open"),
        unevaluable: count("unevaluable"),
        agreement_rate,
        generous_count,
        generosity_rate: if adjudicated_correct > 0 {
            Some(generous_count as f64 / adjudicated_correct as f64)
        } else {
            None
        },
        harsh_count,
        harshness_rate: if adjudicated_wrong > 0 {
            Some(harsh_count as f64 / adjudicated_wrong as f64)
        } else {
            None
        },
        rule_suspect_count,
        agreement_rate_clean,
        generous_count_clean,
        harsh_count_clean,
        by_layer,
        confusion,
        applied_count,
        rows,
    })
}

fn pct(value: Option<f64>) -> String {
    value
        .map(|v| format!("{:.1}%", v * 100.0))
        .unwrap_or_else(|| "n/a".to_string())
}

/// CLI entry point for `pftui journal prediction rescore-audit`.
pub fn run_rescore_audit(
    backend: &BackendConnection,
    apply_high_confidence: bool,
    json_output: bool,
) -> Result<()> {
    let today = Utc::now().date_naive();
    let report = compute_rescore_audit(backend, apply_high_confidence, today)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!(report))?);
        return Ok(());
    }

    println!("Legacy prediction rescore audit (mechanical re-verification of LLM-scored outcomes)");
    println!(
        "  Legacy scored predictions: {} (outcome set, no auto-score provenance)",
        report.total_legacy_scored
    );
    println!(
        "  Adjudicated: {} agree / {} agree-partial / {} disagree — agreement {}",
        report.agree,
        report.agree_partial,
        report.disagree,
        pct(report.agreement_rate)
    );
    println!(
        "  Excluded: {} unparseable, {} window-open, {} unevaluable",
        report.unparseable, report.window_open, report.unevaluable
    );
    println!(
        "  GENEROSITY: {} recorded-correct were mechanically WRONG ({} of adjudicated corrects)",
        report.generous_count,
        pct(report.generosity_rate)
    );
    println!(
        "  Harshness: {} recorded-wrong were mechanically CORRECT ({} of adjudicated wrongs)",
        report.harsh_count,
        pct(report.harshness_rate)
    );
    println!(
        "  Rule-quality caveat: {} adjudicated rows carry legacy-rule defect flags (negated/conditional claims, garbled thresholds, wrong measurand).",
        report.rule_suspect_count
    );
    println!(
        "  Clean subset (no rule defects): agreement {}, {} generous, {} harsh — the defensible LLM-grading-drift measure",
        pct(report.agreement_rate_clean),
        report.generous_count_clean,
        report.harsh_count_clean
    );

    println!("\n  By layer (agree / disagree / rate):");
    for layer in &report.by_layer {
        println!(
            "    {:<18} {:>3} / {:>3} — {}",
            layer.layer,
            layer.agree,
            layer.disagree,
            pct(layer.agreement_rate)
        );
    }

    println!("\n  Confusion (recorded → mechanical):");
    for cell in &report.confusion {
        println!(
            "    {:<8} → {:<8} {:>4}",
            cell.recorded, cell.mechanical, cell.count
        );
    }

    if !report.rows.iter().any(|r| r.classification == "disagree") {
        println!("\n  No disagreements — clean bill.");
    } else {
        println!("\n  Disagreements:");
        for row in report.rows.iter().filter(|r| r.classification == "disagree") {
            println!(
                "    #{} [{}] {} — recorded {} vs mechanical {} ({})",
                row.prediction_id,
                row.layer,
                row.claim_excerpt,
                row.recorded_outcome,
                row.mechanical_outcome.as_deref().unwrap_or("?"),
                row.disagreement_kind.as_deref().unwrap_or("?"),
            );
            if let Some(evidence) = &row.evidence {
                println!(
                    "       evidence: {} [series {}]",
                    evidence,
                    row.series.as_deref().unwrap_or("?")
                );
            }
            if row.applied {
                println!("       APPLIED: outcome corrected, provenance appended to score_notes");
            } else if !row.apply_blockers.is_empty() {
                println!("       not applied: {}", row.apply_blockers.join(", "));
            }
        }
    }

    if report.apply_mode {
        println!(
            "\n  Applied {} gated correction(s). Rebuild calibration: pftui analytics calibration-matrix rebuild --since 365",
            report.applied_count
        );
    } else {
        println!("\n  Dry audit — no outcomes were changed. Use --apply-high-confidence to flip gated disagreements.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn backend_with_schema() -> BackendConnection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        prediction_falsification_rules::ensure_table(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    fn conn(backend: &BackendConnection) -> &Connection {
        backend.sqlite_native().unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_prediction(
        backend: &BackendConnection,
        claim: &str,
        symbol: Option<&str>,
        timeframe: &str,
        resolution_criteria: Option<&str>,
        outcome: &str,
        score_notes: Option<&str>,
        created_at: &str,
    ) -> i64 {
        conn(backend)
            .execute(
                "INSERT INTO user_predictions
                    (claim, symbol, conviction, timeframe, topic, confidence,
                     outcome, score_notes, resolution_criteria, created_at, scored_at)
                 VALUES (?1, ?2, 'medium', ?3, 'crypto', 0.6, ?4, ?5, ?6, ?7, datetime('now'))",
                rusqlite::params![
                    claim,
                    symbol,
                    timeframe,
                    outcome,
                    score_notes,
                    resolution_criteria,
                    created_at
                ],
            )
            .unwrap();
        conn(backend).last_insert_rowid()
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_rule(
        backend: &BackendConnection,
        prediction_id: i64,
        rule_type: &str,
        symbol: &str,
        threshold_value: Option<f64>,
        threshold_low: Option<f64>,
        threshold_high: Option<f64>,
        eval_date_end: &str,
        parse_confidence: &str,
    ) {
        conn(backend)
            .execute(
                "INSERT INTO prediction_falsification_rules
                    (prediction_id, rule_type, symbol, threshold_value,
                     threshold_low, threshold_high, eval_date_end,
                     parse_confidence, auto_score_eligible)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
                rusqlite::params![
                    prediction_id,
                    rule_type,
                    symbol,
                    threshold_value,
                    threshold_low,
                    threshold_high,
                    eval_date_end,
                    parse_confidence
                ],
            )
            .unwrap();
    }

    fn insert_closes(backend: &BackendConnection, symbol: &str, closes: &[(&str, &str)]) {
        for (date, close) in closes {
            conn(backend)
                .execute(
                    "INSERT INTO price_history (symbol, date, close, source)
                     VALUES (?1, ?2, ?3, 'test')",
                    rusqlite::params![symbol, date, close],
                )
                .unwrap();
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 12).unwrap()
    }

    fn find_row(report: &RescoreAuditReport, id: i64) -> &AuditRow {
        report
            .rows
            .iter()
            .find(|r| r.prediction_id == id)
            .expect("row present")
    }

    // ── Parse reuse on legacy claim shapes ──

    #[test]
    fn legacy_claim_with_filler_words_reparses_through_grammar() {
        let backend = backend_with_schema();
        insert_closes(
            &backend,
            "ETH",
            &[("2026-01-05", "2400"), ("2026-01-20", "1900")],
        );
        // No stored rule; claim carries filler ("will") the strict grammar rejects.
        let id = insert_prediction(
            &backend,
            "ETH will close below 2000 by 2026-01-31",
            Some("ETH"),
            "low",
            None,
            "correct",
            None,
            "2026-01-01 09:00:00",
        );
        let report = compute_rescore_audit(&backend, false, today()).unwrap();
        let row = find_row(&report, id);
        assert_eq!(row.rule_source.as_deref(), Some("reparsed-claim"));
        assert_eq!(row.rule_type.as_deref(), Some("close-below"));
        // Relaxed parses are medium confidence — never apply-eligible.
        assert_eq!(row.parse_confidence.as_deref(), Some("medium"));
        assert_eq!(row.classification, "agree");
    }

    #[test]
    fn resolution_criteria_carrying_canonical_rule_parses_high_confidence() {
        let backend = backend_with_schema();
        insert_closes(
            &backend,
            "GLD",
            &[("2026-02-02", "380"), ("2026-02-20", "315")],
        );
        let id = insert_prediction(
            &backend,
            "Gold pushes to new highs on safe-haven flows",
            Some("GLD"),
            "medium",
            Some("GLD close above 400 by 2026-02-28"),
            "correct",
            None,
            "2026-02-01 09:00:00",
        );
        let report = compute_rescore_audit(&backend, false, today()).unwrap();
        let row = find_row(&report, id);
        assert_eq!(row.rule_source.as_deref(), Some("reparsed-resolution"));
        assert_eq!(row.parse_confidence.as_deref(), Some("high"));
        // Window expired with max close 380 < 400 → mechanically wrong.
        assert_eq!(row.classification, "disagree");
        assert_eq!(row.disagreement_kind.as_deref(), Some("generous"));
    }

    #[test]
    fn event_rules_and_unparseable_claims_are_counted_not_adjudicated() {
        let backend = backend_with_schema();
        let with_event_rule = insert_prediction(
            &backend,
            "Ceasefire announced before the deadline",
            None,
            "medium",
            None,
            "correct",
            None,
            "2026-04-01 09:00:00",
        );
        insert_rule(
            &backend,
            with_event_rule,
            "event-happens",
            "",
            None,
            None,
            None,
            "2026-04-07",
            "low",
        );
        insert_prediction(
            &backend,
            "Breadth deteriorates while megacaps mask the damage",
            None,
            "high",
            None,
            "wrong",
            None,
            "2026-04-01 09:00:00",
        );
        let report = compute_rescore_audit(&backend, false, today()).unwrap();
        assert_eq!(report.unparseable, 2);
        assert_eq!(report.agreement_rate, None);
        let row = find_row(&report, with_event_rule);
        assert!(row
            .detail
            .as_deref()
            .unwrap()
            .contains("event-happens"));
    }

    // ── Agree / disagree / confusion classification ──

    #[test]
    fn classification_and_confusion_cover_generous_and_harsh_cases() {
        let backend = backend_with_schema();
        insert_closes(
            &backend,
            "BTC-USD",
            &[
                ("2026-03-02", "60000"),
                ("2026-03-10", "64000"),
                ("2026-03-20", "59000"),
            ],
        );
        // Agree: recorded correct, close above 62k printed.
        let agree_id = insert_prediction(
            &backend, "c1", Some("BTC-USD"), "low", None, "correct", None,
            "2026-03-01 00:00:00",
        );
        insert_rule(
            &backend, agree_id, "close-above", "BTC-USD",
            Some(62000.0), None, None, "2026-03-31", "high",
        );
        // Generous: recorded correct, but 70k never printed.
        let generous_id = insert_prediction(
            &backend, "c2", Some("BTC-USD"), "medium", None, "correct",
            Some("EOD: judged correct on momentum"),
            "2026-03-01 00:00:00",
        );
        insert_rule(
            &backend, generous_id, "close-above", "BTC-USD",
            Some(70000.0), None, None, "2026-03-31", "high",
        );
        // Harsh: recorded wrong, but a sub-59500 close DID print.
        let harsh_id = insert_prediction(
            &backend, "c3", Some("BTC-USD"), "low", None, "wrong", None,
            "2026-03-01 00:00:00",
        );
        insert_rule(
            &backend, harsh_id, "close-below", "BTC-USD",
            Some(59500.0), None, None, "2026-03-31", "high",
        );
        let report = compute_rescore_audit(&backend, false, today()).unwrap();
        assert_eq!(find_row(&report, agree_id).classification, "agree");
        assert_eq!(
            find_row(&report, generous_id).disagreement_kind.as_deref(),
            Some("generous")
        );
        assert_eq!(
            find_row(&report, harsh_id).disagreement_kind.as_deref(),
            Some("harsh")
        );
        assert_eq!(report.agree, 1);
        assert_eq!(report.disagree, 2);
        assert_eq!(report.generous_count, 1);
        assert_eq!(report.harsh_count, 1);
        assert!((report.agreement_rate.unwrap() - 1.0 / 3.0).abs() < 1e-9);
        // Generosity rate: 1 generous of 2 adjudicated corrects.
        assert!((report.generosity_rate.unwrap() - 0.5).abs() < 1e-9);
        // Confusion cells: correct→correct, correct→wrong, wrong→correct.
        let cell = |rec: &str, mech: &str| {
            report
                .confusion
                .iter()
                .find(|c| c.recorded == rec && c.mechanical == mech)
                .map(|c| c.count)
                .unwrap_or(0)
        };
        assert_eq!(cell("correct", "correct"), 1);
        assert_eq!(cell("correct", "wrong"), 1);
        assert_eq!(cell("wrong", "correct"), 1);
    }

    // ── Partial handling ──

    #[test]
    fn partial_agrees_unless_direction_entirely_contradicted() {
        let backend = backend_with_schema();
        // RIGHT direction, threshold unmet: rose 100 → 110, target 120.
        insert_closes(
            &backend,
            "AAA",
            &[("2026-01-02", "100"), ("2026-01-30", "110")],
        );
        let partial_ok = insert_prediction(
            &backend, "p1", Some("AAA"), "low", None, "partial", None,
            "2026-01-01 00:00:00",
        );
        insert_rule(
            &backend, partial_ok, "close-above", "AAA",
            Some(120.0), None, None, "2026-01-31", "high",
        );
        // Direction contradicted: claimed above, asset FELL net.
        insert_closes(
            &backend,
            "BBB",
            &[("2026-01-02", "100"), ("2026-01-30", "80")],
        );
        let partial_bad = insert_prediction(
            &backend, "p2", Some("BBB"), "low", None, "partial", None,
            "2026-01-01 00:00:00",
        );
        insert_rule(
            &backend, partial_bad, "close-above", "BBB",
            Some(120.0), None, None, "2026-01-31", "high",
        );
        // Range rule (no direction): mechanical wrong → agree-partial.
        insert_closes(
            &backend,
            "CCC",
            &[("2026-01-02", "100"), ("2026-01-30", "70")],
        );
        let partial_range = insert_prediction(
            &backend, "p3", Some("CCC"), "low", None, "partial", None,
            "2026-01-01 00:00:00",
        );
        insert_rule(
            &backend, partial_range, "stays-in-range", "CCC",
            None, Some(90.0), Some(110.0), "2026-01-31", "high",
        );
        let report = compute_rescore_audit(&backend, false, today()).unwrap();
        assert_eq!(find_row(&report, partial_ok).classification, "agree-partial");
        let bad = find_row(&report, partial_bad);
        assert_eq!(bad.classification, "disagree");
        assert_eq!(
            bad.disagreement_kind.as_deref(),
            Some("partial-direction-contradicted")
        );
        assert_eq!(
            find_row(&report, partial_range).classification,
            "agree-partial"
        );
    }

    // ── Apply gating ──

    #[test]
    fn apply_flips_outcome_and_appends_provenance_preserving_original() {
        let backend = backend_with_schema();
        insert_closes(
            &backend,
            "GLD",
            &[("2026-02-02", "300"), ("2026-02-20", "310")],
        );
        let id = insert_prediction(
            &backend, "gold to 400", Some("GLD"), "medium", None, "correct",
            Some("EOD: judged correct generously"),
            "2026-02-01 00:00:00",
        );
        insert_rule(
            &backend, id, "close-above", "GLD",
            Some(400.0), None, None, "2026-02-28", "high",
        );
        let report = compute_rescore_audit(&backend, true, today()).unwrap();
        assert_eq!(report.applied_count, 1);
        assert!(find_row(&report, id).applied);
        let (outcome, notes): (String, String) = conn(&backend)
            .query_row(
                "SELECT outcome, score_notes FROM user_predictions WHERE id = ?1",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(outcome, "wrong");
        // Original note preserved, provenance appended with old→new.
        assert!(notes.starts_with("EOD: judged correct generously"));
        assert!(notes.contains("rescore-audit 2026-06-12: outcome corrected correct→wrong"));
        assert!(notes.contains("evidence:"));
    }

    #[test]
    fn apply_blocks_threshold_proximity_low_confidence_and_corruption_window() {
        let backend = backend_with_schema();
        // (1) Proximity block: nearest close 399 vs threshold 400 (0.25%).
        insert_closes(
            &backend,
            "SLV",
            &[("2026-02-02", "390"), ("2026-02-20", "399")],
        );
        let near_id = insert_prediction(
            &backend, "silver to 400", Some("SLV"), "medium", None, "correct",
            None, "2026-02-01 00:00:00",
        );
        insert_rule(
            &backend, near_id, "close-above", "SLV",
            Some(400.0), None, None, "2026-02-28", "high",
        );
        // (2) Low parse confidence block.
        insert_closes(
            &backend,
            "USO",
            &[("2026-02-02", "60"), ("2026-02-20", "62")],
        );
        let low_conf_id = insert_prediction(
            &backend, "oil to 80", Some("USO"), "medium", None, "correct",
            None, "2026-02-01 00:00:00",
        );
        insert_rule(
            &backend, low_conf_id, "close-above", "USO",
            Some(80.0), None, None, "2026-02-28", "low",
        );
        // (3) Corruption-window block: BTC-USD inside the equity-ticker
        // repair window 2025-03-20→2026-02-27.
        insert_closes(
            &backend,
            "BTC-USD",
            &[("2025-06-02", "60000"), ("2025-06-20", "61000")],
        );
        let btc_id = insert_prediction(
            &backend, "btc to 90k", Some("BTC-USD"), "medium", None, "correct",
            None, "2025-06-01 00:00:00",
        );
        insert_rule(
            &backend, btc_id, "close-above", "BTC-USD",
            Some(90000.0), None, None, "2025-06-30", "high",
        );
        let report = compute_rescore_audit(&backend, true, today()).unwrap();
        assert_eq!(report.applied_count, 0);
        for (id, blocker) in [
            (near_id, "deciding-close-within-1pct-of-threshold"),
            (low_conf_id, "parse-confidence-low-below-high"),
            (btc_id, "btc-equity-corruption-window"),
        ] {
            let row = find_row(&report, id);
            assert_eq!(row.classification, "disagree", "id {id}");
            assert!(!row.applied, "id {id}");
            assert!(
                row.apply_blockers.iter().any(|b| b.contains(blocker)),
                "id {id}: expected blocker {blocker}, got {:?}",
                row.apply_blockers
            );
            let outcome: String = conn(&backend)
                .query_row(
                    "SELECT outcome FROM user_predictions WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(outcome, "correct", "id {id} must stay untouched");
        }
    }

    #[test]
    fn auto_scored_and_pending_predictions_are_excluded() {
        let backend = backend_with_schema();
        insert_prediction(
            &backend, "machine scored", Some("SPY"), "low", None, "wrong",
            Some("auto-scored: close-above SPY > 700 within 2026-01-01..2026-01-31 — ..."),
            "2026-01-01 00:00:00",
        );
        insert_prediction(
            &backend, "still pending", Some("SPY"), "low", None, "pending",
            None, "2026-06-01 00:00:00",
        );
        let report = compute_rescore_audit(&backend, false, today()).unwrap();
        assert_eq!(report.total_legacy_scored, 0);
    }

    #[test]
    fn open_window_predictions_classify_window_open() {
        let backend = backend_with_schema();
        insert_closes(
            &backend,
            "QQQ",
            &[("2026-06-08", "500"), ("2026-06-10", "505")],
        );
        let id = insert_prediction(
            &backend, "qqq to 550", Some("QQQ"), "low", None, "correct", None,
            "2026-06-05 00:00:00",
        );
        insert_rule(
            &backend, id, "close-above", "QQQ",
            Some(550.0), None, None, "2026-12-31", "high",
        );
        let report = compute_rescore_audit(&backend, false, today()).unwrap();
        assert_eq!(find_row(&report, id).classification, "window-open");
        assert_eq!(report.window_open, 1);
    }

    // ── Calibration rebuild integration ──

    #[test]
    fn applied_corrections_change_calibration_rebuild() {
        let backend = backend_with_schema();
        insert_closes(
            &backend,
            "GLD",
            &[("2026-02-02", "300"), ("2026-02-20", "310")],
        );
        // Generously-scored correct that the apply pass flips to wrong.
        let id = insert_prediction(
            &backend, "gold to 400", Some("GLD"), "low", None, "correct", None,
            "2026-02-01 00:00:00",
        );
        insert_rule(
            &backend, id, "close-above", "GLD",
            Some(400.0), None, None, "2026-02-28", "high",
        );
        let before = crate::analytics::calibration_scorer::rebuild_calibration_matrix_backend(
            &backend, 365,
        )
        .unwrap();
        let before_row = before
            .rows
            .iter()
            .find(|r| r.layer == "low")
            .expect("row before");
        assert!((before_row.hit_rate - 1.0).abs() < 1e-9);

        let report = compute_rescore_audit(&backend, true, today()).unwrap();
        assert_eq!(report.applied_count, 1);

        let after = crate::analytics::calibration_scorer::rebuild_calibration_matrix_backend(
            &backend, 365,
        )
        .unwrap();
        let after_row = after
            .rows
            .iter()
            .find(|r| r.layer == "low")
            .expect("row after");
        assert!((after_row.hit_rate - 0.0).abs() < 1e-9);
    }

    /// Rule-quality guards: a legacy rule that encodes a negated claim's
    /// failure condition, a magnitude-garbled threshold, or a level absent
    /// from the claim must never be auto-corrected, and must be broken out
    /// of the clean agreement statistics.
    #[test]
    fn rule_defect_flags_block_apply_and_partition_stats() {
        let backend = backend_with_schema();
        // (1) Negated claim, rule stores the failure condition: VIX did
        // fail to sustain above 24 (recorded correct is RIGHT), but the
        // stays-above rule mechanically scores wrong.
        insert_closes(
            &backend,
            "^VIX",
            &[("2026-03-16", "26"), ("2026-03-17", "22")],
        );
        let negated = insert_prediction(
            &backend,
            "VIX fails to sustain above 24 despite energy attacks",
            Some("^VIX"), "low", None, "correct", None, "2026-03-15 00:00:00",
        );
        insert_rule(
            &backend, negated, "stays-above", "^VIX",
            Some(24.0), None, None, "2026-03-18", "high",
        );
        // (2) Unit-garbled threshold: "gold closes above 680" against a
        // ~4,500 series (the real claim level was scrubbed/garbled at
        // write time) — trivially correct mechanically.
        insert_closes(
            &backend,
            "GC=F",
            &[("2026-03-19", "4500"), ("2026-03-20", "4570")],
        );
        let garbled = insert_prediction(
            &backend,
            "DXY pullback extends metals recovery - gold closes above 680",
            Some("GC=F"), "low", None, "wrong", None, "2026-03-18 00:00:00",
        );
        insert_rule(
            &backend, garbled, "close-above", "GC=F",
            Some(680.0), None, None, "2026-03-20", "high",
        );
        // (3) Claim states a level the rule does not encode: "below 91"
        // stored as stays-in-range 86..88.
        insert_closes(
            &backend,
            "CL=F",
            &[("2026-04-15", "87"), ("2026-04-16", "90.07")],
        );
        let level_mismatch = insert_prediction(
            &backend,
            "Oil trades below 91 as phase-end gravity sets in",
            Some("CL=F"), "low", None, "correct", None, "2026-04-14 00:00:00",
        );
        insert_rule(
            &backend, level_mismatch, "stays-in-range", "CL=F",
            None, Some(86.0), Some(88.0), "2026-04-17", "high",
        );

        let report = compute_rescore_audit(&backend, true, today()).unwrap();
        assert_eq!(report.applied_count, 0);
        for (id, flag) in [
            (negated, "claim-negation-marker"),
            (garbled, "threshold-magnitude-mismatch"),
            (level_mismatch, "claim-level-not-in-rule"),
        ] {
            let row = find_row(&report, id);
            assert_eq!(row.classification, "disagree", "id {id}");
            assert!(
                row.rule_suspect_flags.iter().any(|f| f == flag),
                "id {id}: expected flag {flag}, got {:?}",
                row.rule_suspect_flags
            );
            assert!(!row.applied, "id {id}");
            assert!(
                row.apply_blockers
                    .iter()
                    .any(|b| b == &format!("rule-suspect:{flag}")),
                "id {id}: blocker missing, got {:?}",
                row.apply_blockers
            );
        }
        // All three disagreements are rule-suspect → the clean subset is
        // empty and its agreement rate is None, not a misleading 0%.
        assert_eq!(report.rule_suspect_count, 3);
        assert_eq!(report.agreement_rate_clean, None);
        assert_eq!(report.generous_count_clean, 0);
        assert_eq!(report.harsh_count_clean, 0);
        // Headline stats still count them — flagged, not hidden.
        assert_eq!(report.disagree, 3);
    }

    #[test]
    fn claim_numeric_levels_skip_years_and_percentages() {
        let levels = claim_numeric_levels(
            "If CPI prints 3.2% in March 2026, oil holds $95 and gold 4,500",
        );
        assert_eq!(levels, vec![95.0, 4500.0]);
    }

    /// Regression: live `created_at` values carry a `+00` timezone suffix
    /// (`2026-03-12 01:03:01.092759+00`) that SQLite's `date()` returns NULL
    /// for. The window start must still bind to the creation DATE — an
    /// unbounded start would scan all history and find decades-old closes
    /// "satisfying" the rule (observed live before the substr fix).
    #[test]
    fn timezone_suffixed_created_at_still_bounds_the_window() {
        let backend = backend_with_schema();
        insert_closes(
            &backend,
            "DDD",
            &[
                ("2007-11-02", "200"), // ancient qualifying close — must NOT count
                ("2026-03-12", "90"),
                ("2026-03-18", "95"),
            ],
        );
        let id = insert_prediction(
            &backend, "ddd breaks 150", Some("DDD"), "low", None, "correct",
            None, "2026-03-12 01:03:01.092759+00",
        );
        insert_rule(
            &backend, id, "close-above", "DDD",
            Some(150.0), None, None, "2026-03-20", "high",
        );
        let report = compute_rescore_audit(&backend, false, today()).unwrap();
        let row = find_row(&report, id);
        // Mechanically wrong inside the bounded window (max close 95 < 150);
        // the 2007 close outside the window must not flip it to agree.
        assert_eq!(row.mechanical_outcome.as_deref(), Some("wrong"));
        assert_eq!(row.classification, "disagree");
        assert!(row
            .evidence
            .as_deref()
            .unwrap()
            .contains("2026-03-18"));
    }

    #[test]
    fn corruption_blocker_windows_are_precise() {
        // BTC outside the repair window → no blockers.
        assert!(corruption_blockers("BTC-USD", "2026-03-01", "2026-03-31").is_empty());
        // Overlap on either edge → blocked.
        assert!(!corruption_blockers("BTC-USD", "2026-02-20", "2026-03-31").is_empty());
        assert!(!corruption_blockers("BTC", "2025-01-01", "2025-03-20").is_empty());
        // Stale-stamp date.
        assert!(corruption_blockers("BTC-USD", "2026-06-10", "2026-06-12")
            .iter()
            .any(|b| b.contains("stale-stamp")));
        // FX placeholders blocked in any window.
        assert!(!corruption_blockers("JPY=X", "2024-01-01", "2024-02-01").is_empty());
        // Frozen feeds blocked only when the window touches 2026-03-13+.
        assert!(corruption_blockers("ZC=F", "2026-01-01", "2026-02-01").is_empty());
        assert!(!corruption_blockers("ZC=F", "2026-03-01", "2026-04-01").is_empty());
        // Unrelated series never blocked.
        assert!(corruption_blockers("SPY", "2025-01-01", "2026-06-12").is_empty());
    }
}
