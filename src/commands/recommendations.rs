//! CLI handlers for `pftui analytics recommendations ...`.
//!
//! These commands operate on the local SQLite backend exclusively because the
//! `recommendations` and `recommendation_outcomes` tables are local-only
//! enrichment substrate that closes the Recommendation → action → outcome
//! chain.

use anyhow::{anyhow, Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::recommendations::{
    accuracy_summary, find_open_for_reply, find_open_for_transaction, get, link_operator_reply,
    link_transaction, list as list_recommendations, list_unscored, price_at, record_ledger_entry,
    score_forward_returns, scoreboard as build_scoreboard, set_outcome_score, AccuracyBucket,
    ForwardScoreSummary, Recommendation, RecommendationOutcome, Scoreboard,
};
use crate::db::transactions::list_transactions_backend;

fn require_sqlite(backend: &BackendConnection) -> Result<&Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow!("analytics recommendations commands require the SQLite backend"))
}

/// Parse a `--since` value: accepts `YYYY-MM-DD` or `<N>d` shorthand. Returns
/// the resolved YYYY-MM-DD date.
fn parse_since(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if let Some(stripped) = trimmed.strip_suffix('d') {
        let days: i64 = stripped
            .parse()
            .with_context(|| format!("invalid --since value '{value}'"))?;
        let date = Utc::now().date_naive() - Duration::days(days);
        return Ok(date.format("%Y-%m-%d").to_string());
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .with_context(|| format!("invalid --since value '{value}'"))?;
    Ok(trimmed.to_string())
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// ------------- record (ledger) -------------

pub fn record_cmd(
    backend: &BackendConnection,
    symbol: &str,
    action: &str,
    rationale: Option<&str>,
    date: Option<&str>,
    source: &str,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let run_date = match date {
        Some(d) => {
            NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .with_context(|| format!("invalid --date '{d}': expected YYYY-MM-DD"))?;
            d.to_string()
        }
        None => Utc::now().date_naive().format("%Y-%m-%d").to_string(),
    };
    let rec = record_ledger_entry(conn, &run_date, symbol, action, rationale, source)?;
    if json {
        return print_json(&rec);
    }
    let priced = match (&rec.entry_price, &rec.price_series) {
        (Some(price), Some(series)) => format!("@ {price} (series {series})"),
        _ => "(no price history — recorded unpriced, will not accrue forward returns)".to_string(),
    };
    println!(
        "Recorded {} {} {} on {} (id {}, source {}).",
        rec.recommendation_type.to_uppercase(),
        rec.asset.as_deref().unwrap_or("-"),
        priced,
        rec.report_date,
        rec.id,
        rec.source,
    );
    if let Some(r) = &rec.rationale_summary {
        println!("  rationale: {r}");
    }
    Ok(())
}

// ------------- list -------------

#[allow(clippy::too_many_arguments)]
pub fn list_cmd(
    backend: &BackendConnection,
    date: Option<&str>,
    asset: Option<&str>,
    recommendation_type: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let resolved_since = match since {
        Some(s) => Some(parse_since(s)?),
        None => None,
    };
    let mut rows = list_recommendations(
        conn,
        date,
        asset,
        recommendation_type,
        resolved_since.as_deref(),
    )?;
    if let Some(limit) = limit {
        rows.truncate(limit);
    }
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No recommendations match.");
        return Ok(());
    }
    for r in &rows {
        let priced = match (&r.entry_price, &r.price_series) {
            (Some(p), Some(s)) => format!(" entry={p} ({s})"),
            _ => String::new(),
        };
        let fwd = [
            ("30d", r.fwd_30d_pct),
            ("90d", r.fwd_90d_pct),
            ("180d", r.fwd_180d_pct),
        ]
        .iter()
        .filter_map(|(label, v)| v.map(|v| format!("{label}={v:+.1}%")))
        .collect::<Vec<_>>()
        .join(" ");
        println!(
            "{} [{}] {} {} src={}{}{} {}",
            r.id,
            r.report_date,
            r.asset.as_deref().unwrap_or("-"),
            r.recommendation_type,
            r.source,
            priced,
            if fwd.is_empty() {
                String::new()
            } else {
                format!(" fwd[{fwd}]")
            },
            r.rationale_summary.as_deref().unwrap_or(""),
        );
    }
    Ok(())
}

// ------------- score -------------

#[derive(Debug, Clone, Serialize)]
struct ScoredOutcomeRow {
    recommendation_id: i64,
    asset: Option<String>,
    recommendation_type: String,
    report_date: String,
    horizon_days: i64,
    evaluated_at: String,
    start_price: Option<String>,
    end_price: Option<String>,
    outcome_score: Option<f64>,
    notes: Option<String>,
}

pub fn score_cmd(
    backend: &BackendConnection,
    all: bool,
    id: Option<i64>,
    horizon: i64,
    since: Option<&str>,
    json: bool,
) -> Result<()> {
    // Default mode (no --all/--id): the forward-return scorer — fill
    // fwd_{30,90,180}d_pct for any priced ledger row whose horizon has
    // elapsed. Idempotent; this is the pass `data refresh` runs in its tail.
    if !all && id.is_none() {
        let conn = require_sqlite(backend)?;
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        let summary = score_forward_returns(conn, &today)?;
        if json {
            return print_json(&summary);
        }
        println!(
            "Forward-return scoring: {} candidate row(s), {} horizon cell(s) filled across {} row(s).",
            summary.candidates, summary.horizons_filled, summary.rows_updated
        );
        if summary.candidates > 0 && summary.horizons_filled == 0 {
            println!("  (horizons not yet elapsed or price history missing — accruing)");
        }
        return Ok(());
    }
    let conn = require_sqlite(backend)?;
    let resolved_since = match since {
        Some(s) => Some(parse_since(s)?),
        None => None,
    };

    let recs: Vec<Recommendation> = if let Some(rid) = id {
        let row = get(conn, rid)?
            .ok_or_else(|| anyhow!("recommendation id={} not found", rid))?;
        vec![row]
    } else {
        list_unscored(conn, resolved_since.as_deref())?
    };

    let mut scored: Vec<ScoredOutcomeRow> = Vec::new();
    for rec in &recs {
        let asset = match rec.asset.as_deref() {
            Some(a) => a,
            None => continue,
        };
        let report_date = match NaiveDate::parse_from_str(&rec.report_date, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };
        let end_date = (report_date + Duration::days(horizon))
            .format("%Y-%m-%d")
            .to_string();
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        // Cap end-date at today — can't evaluate the future.
        let effective_end_date = if end_date.as_str() > today.as_str() {
            today.clone()
        } else {
            end_date.clone()
        };
        let start_price = price_at(conn, asset, &rec.report_date)?;
        let end_price = price_at(conn, asset, &effective_end_date)?;
        let score = crate::db::recommendations::compute_outcome_score(
            &rec.recommendation_type,
            start_price,
            end_price,
        );
        let notes = format!(
            "horizon_days={horizon} effective_end={effective_end_date} type={}",
            rec.recommendation_type
        );
        if let Some(s) = score {
            set_outcome_score(conn, rec.id, s, &today, Some(&notes))?;
        }
        scored.push(ScoredOutcomeRow {
            recommendation_id: rec.id,
            asset: rec.asset.clone(),
            recommendation_type: rec.recommendation_type.clone(),
            report_date: rec.report_date.clone(),
            horizon_days: horizon,
            evaluated_at: today.clone(),
            start_price: start_price.map(|p| p.to_string()),
            end_price: end_price.map(|p| p.to_string()),
            outcome_score: score,
            notes: Some(notes),
        });
    }

    if json {
        return print_json(&scored);
    }
    if scored.is_empty() {
        println!("No recommendations were scored (none eligible).");
        return Ok(());
    }
    for row in &scored {
        println!(
            "rec={} {} {} report_date={} horizon={}d score={}",
            row.recommendation_id,
            row.asset.as_deref().unwrap_or("-"),
            row.recommendation_type,
            row.report_date,
            row.horizon_days,
            row.outcome_score
                .map(|s| format!("{s:.2}"))
                .unwrap_or_else(|| "n/a".to_string()),
        );
    }
    Ok(())
}

/// Refresh-tail hook: run the forward-return scorer best-effort. Returns a
/// zeroed summary on non-SQLite backends (the ledger is local-only substrate,
/// mirroring the prediction auto-score precedent).
pub fn auto_score_for_refresh(backend: &BackendConnection) -> Result<ForwardScoreSummary> {
    let Some(conn) = backend.sqlite_native() else {
        return Ok(ForwardScoreSummary::default());
    };
    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    score_forward_returns(conn, &today)
}

// ------------- scoreboard -------------

fn fmt_cell(cell: &Option<crate::db::recommendations::ScoreboardCell>) -> String {
    match cell {
        Some(c) => format!("n={} {:>3.0}%+ {:+.1}%", c.n, c.pct_positive, c.mean_pct),
        None => "—".to_string(),
    }
}

pub fn scoreboard_cmd(backend: &BackendConnection, symbol: Option<&str>, json: bool) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let board: Scoreboard = build_scoreboard(conn, symbol)?;
    if json {
        return print_json(&board);
    }

    if board.rows.is_empty() {
        println!(
            "No ledger recommendations recorded yet{}. Record with `pftui analytics recommendations record`.",
            symbol.map(|s| format!(" for {s}")).unwrap_or_default()
        );
        return Ok(());
    }

    let any_scored = board
        .rows
        .iter()
        .any(|r| r.h30.is_some() || r.h90.is_some() || r.h180.is_some());
    if !any_scored {
        println!(
            "scoreboard accruing — {} unscored recommendation(s), no forward horizon has both elapsed and priced yet.",
            board.unscored
        );
        println!("\nRecorded so far:");
        for r in &board.rows {
            println!("  {:<8} {:<6} n={}", r.symbol, r.action, r.n_total);
        }
        return Ok(());
    }

    println!("Recommendation scoreboard — forward returns vs entry close\n");
    println!(
        "{:<8} {:<6} {:>4}  {:<20} {:<20} {:<20}",
        "symbol", "action", "n", "30d (n, %pos, mean)", "90d (n, %pos, mean)", "180d (n, %pos, mean)"
    );
    println!("{}", "-".repeat(86));
    for r in &board.rows {
        println!(
            "{:<8} {:<6} {:>4}  {:<20} {:<20} {:<20}",
            r.symbol,
            r.action,
            r.n_total,
            fmt_cell(&r.h30),
            fmt_cell(&r.h90),
            fmt_cell(&r.h180),
        );
    }

    if !board.window_quality.is_empty() {
        println!("\nWindow quality (mean 90d fwd return after ADD − after WAIT):");
        for wq in &board.window_quality {
            match wq.delta_pct {
                Some(delta) => {
                    let verdict = if delta >= 0.0 {
                        "ADD timing added value over waiting"
                    } else {
                        "ADD calls were WORSE than the system's own WAIT calls"
                    };
                    println!(
                        "  {:<8} Δ = {:+.1}pp (add n={} mean {:+.1}% vs wait n={} mean {:+.1}%) — {}",
                        wq.symbol,
                        delta,
                        wq.add_n,
                        wq.add_mean_90d_pct.unwrap_or(0.0),
                        wq.wait_n,
                        wq.wait_mean_90d_pct.unwrap_or(0.0),
                        verdict,
                    );
                }
                None => println!(
                    "  {:<8} accruing — needs a scored 90d return on both ADD (have {}) and WAIT (have {})",
                    wq.symbol, wq.add_n, wq.wait_n
                ),
            }
        }
    }
    if board.unscored > 0 {
        println!("\n{} recommendation(s) still accruing (no scored horizon yet).", board.unscored);
    }
    Ok(())
}

// ------------- accuracy -------------

#[derive(Debug, Clone, Serialize)]
struct AccuracyOutput {
    since: String,
    threshold: f64,
    by_asset: bool,
    buckets: Vec<AccuracyBucket>,
}

pub fn accuracy_cmd(
    backend: &BackendConnection,
    recommendation_type: Option<&str>,
    asset: Option<&str>,
    since: &str,
    threshold: f64,
    by_asset: bool,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let resolved_since = parse_since(since)?;
    let buckets = accuracy_summary(
        conn,
        recommendation_type,
        asset,
        Some(&resolved_since),
        threshold,
        by_asset,
    )?;
    let payload = AccuracyOutput {
        since: resolved_since,
        threshold,
        by_asset,
        buckets,
    };
    if json {
        return print_json(&payload);
    }
    if payload.buckets.is_empty() {
        println!(
            "No recommendations found since {} (threshold {})",
            payload.since, payload.threshold
        );
        return Ok(());
    }
    println!(
        "Recommendation accuracy since {} (threshold {:.1})",
        payload.since, payload.threshold
    );
    for b in &payload.buckets {
        let asset_part = if by_asset {
            format!(" asset={}", b.asset.as_deref().unwrap_or("-"))
        } else {
            String::new()
        };
        println!(
            "  type={}{} scored={}/{} hits={} hit_rate={:.1}% avg_score={:+.2}",
            b.recommendation_type, asset_part, b.scored, b.total, b.hits, b.hit_rate_pct, b.avg_score
        );
    }
    Ok(())
}

// ------------- link (manual) -------------

#[derive(Debug, Clone, Serialize)]
struct LinkOutput {
    recommendation_id: i64,
    operator_reply_id: Option<i64>,
    transaction_id: Option<i64>,
    action_status: Option<String>,
    outcome: Option<RecommendationOutcome>,
}

pub fn link_cmd(
    backend: &BackendConnection,
    id: i64,
    reply_id: Option<i64>,
    transaction_id: Option<i64>,
    action_status: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rec = get(conn, id)?.ok_or_else(|| anyhow!("recommendation id={} not found", id))?;
    if reply_id.is_none() && transaction_id.is_none() {
        return Err(anyhow!(
            "link requires at least one of --reply or --transaction"
        ));
    }
    if let Some(rid) = reply_id {
        link_operator_reply(conn, rec.id, rid, action_status)?;
    }
    if let Some(tid) = transaction_id {
        link_transaction(conn, rec.id, tid)?;
    }
    let outcome = crate::db::recommendations::get_outcome(conn, rec.id)?;
    let payload = LinkOutput {
        recommendation_id: rec.id,
        operator_reply_id: reply_id,
        transaction_id,
        action_status: action_status.map(|s| s.to_string()),
        outcome,
    };
    if json {
        return print_json(&payload);
    }
    println!("linked rec={}", payload.recommendation_id);
    if let Some(o) = payload.outcome {
        println!(
            "  reply_id={:?} tx_id={:?} status={:?} score={:?}",
            o.operator_reply_id, o.transaction_id, o.action_status, o.outcome_score
        );
    }
    Ok(())
}

// ------------- relink historical -------------

#[derive(Debug, Clone, Serialize)]
pub struct RelinkOutput {
    pub replies_linked: u32,
    pub transactions_linked: u32,
    pub window_days: i64,
}

pub fn relink_historical_cmd(
    backend: &BackendConnection,
    window_days: i64,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let payload = relink_historical(conn, backend, window_days)?;
    if json {
        return print_json(&payload);
    }
    println!(
        "relinked {} replies and {} transactions (window={}d)",
        payload.replies_linked, payload.transactions_linked, payload.window_days
    );
    Ok(())
}

/// Worker: scan operator_replies and transactions, attaching them to any open
/// recommendation that matches on `(report_date, asset)` or `(asset,
/// direction, within window)` respectively. Idempotent — calling repeatedly
/// is a no-op once everything is linked.
pub fn relink_historical(
    conn: &Connection,
    backend: &BackendConnection,
    window_days: i64,
) -> Result<RelinkOutput> {
    let mut replies_linked = 0u32;
    let mut transactions_linked = 0u32;

    // ----- replies -----
    let replies = crate::db::operator_replies::list(conn, None, None, None)?;
    for reply in &replies {
        // Skip replies that already have an outcome row pointing at them.
        let already: Option<i64> = conn
            .prepare(
                "SELECT recommendation_id FROM recommendation_outcomes WHERE operator_reply_id = ?1 LIMIT 1",
            )?
            .query_row(rusqlite::params![reply.id], |r| r.get(0))
            .ok();
        if already.is_some() {
            continue;
        }
        let found = find_open_for_reply(
            conn,
            &reply.report_date,
            reply.asset.as_deref(),
            Some(&reply.decision_type),
        )?;
        if let Some(rec) = found {
            let status = response_to_action_status(&reply.response_class);
            link_operator_reply(conn, rec.id, reply.id, Some(status))?;
            replies_linked += 1;
        }
    }

    // ----- transactions -----
    let txs = list_transactions_backend(backend).unwrap_or_default();
    for tx in &txs {
        let already: Option<i64> = conn
            .prepare(
                "SELECT recommendation_id FROM recommendation_outcomes WHERE transaction_id = ?1 LIMIT 1",
            )?
            .query_row(rusqlite::params![tx.id], |r| r.get(0))
            .ok();
        if already.is_some() {
            continue;
        }
        let direction = match tx.tx_type {
            crate::models::transaction::TxType::Buy => "buy",
            crate::models::transaction::TxType::Sell => "sell",
        };
        let found = find_open_for_transaction(conn, &tx.symbol, direction, &tx.date, window_days)?;
        if let Some(rec) = found {
            link_transaction(conn, rec.id, tx.id)?;
            transactions_linked += 1;
        }
    }

    Ok(RelinkOutput {
        replies_linked,
        transactions_linked,
        window_days,
    })
}

fn response_to_action_status(response: &str) -> &'static str {
    match response {
        "yes" | "executed" => "accepted",
        "no" | "remove" => "rejected",
        "refine" => "partial",
        "wait" => "deferred",
        "ignore" => "ignored",
        _ => "accepted",
    }
}

/// Hook for `pftui journal entry add --author skylar`: parse a DECISION REPLY
/// payload from the journal content and, if it matches, insert an
/// `operator_replies` row + link to the matching recommendation. Returns the
/// newly inserted reply id (if any).
pub fn try_link_decision_reply_from_journal(
    backend: &BackendConnection,
    journal_id: i64,
    content: &str,
    journal_date_ymd: &str,
) -> Result<Option<i64>> {
    let conn = match backend.sqlite_native() {
        Some(c) => c,
        None => return Ok(None),
    };
    let parsed = match crate::db::recommendations::parse_decision_reply(content) {
        Some(m) => m,
        None => return Ok(None),
    };
    if !crate::db::operator_replies::VALID_DECISION_TYPES.contains(&parsed.decision_type.as_str()) {
        // Type out of vocabulary — skip linking quietly rather than error out
        // and block the user's journal entry.
        return Ok(None);
    }
    if !crate::db::operator_replies::VALID_RESPONSE_CLASSES.contains(&parsed.response_class.as_str()) {
        return Ok(None);
    }
    let report_date = parsed
        .report_date
        .clone()
        .unwrap_or_else(|| journal_date_ymd.to_string());
    let reply_id = crate::db::operator_replies::insert(
        conn,
        &crate::db::operator_replies::OperatorReplyInsert {
            journal_id: Some(journal_id),
            report_date: &report_date,
            reply_date: journal_date_ymd,
            asset: Some(&parsed.asset),
            decision_type: &parsed.decision_type,
            response_class: &parsed.response_class,
            conviction_implied: None,
            timeframe_horizon: None,
            reasoning_summary: parsed.reasoning_summary.as_deref(),
            raw_content: content,
        },
    )?;
    if let Some(rec) = find_open_for_reply(
        conn,
        &report_date,
        Some(&parsed.asset),
        Some(&parsed.decision_type),
    )? {
        let status = response_to_action_status(&parsed.response_class);
        link_operator_reply(conn, rec.id, reply_id, Some(status))?;
    }
    Ok(Some(reply_id))
}

/// Hook for `pftui portfolio transaction add`: attach the just-inserted
/// transaction to the most recent open recommendation for the same asset and
/// direction (within `window_days`).
pub fn try_link_transaction_to_recommendation(
    backend: &BackendConnection,
    transaction_id: i64,
    asset: &str,
    direction: &str,
    tx_date: &str,
    window_days: i64,
) -> Result<Option<i64>> {
    let conn = match backend.sqlite_native() {
        Some(c) => c,
        None => return Ok(None),
    };
    let found = find_open_for_transaction(conn, asset, direction, tx_date, window_days)?;
    if let Some(rec) = found {
        link_transaction(conn, rec.id, transaction_id)?;
        return Ok(Some(rec.id));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::recommendations::{
        ensure_table, insert_recommendation, RecommendationInsert,
    };

    fn make_backend() -> BackendConnection {
        // Build an in-memory SQLite backend with the required parent tables.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                category TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per TEXT NOT NULL,
                currency TEXT NOT NULL,
                date TEXT NOT NULL,
                notes TEXT,
                paired_tx_id INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                open TEXT,
                high TEXT,
                low TEXT,
                volume TEXT,
                PRIMARY KEY (symbol, date)
            );",
        )
        .unwrap();
        crate::db::operator_replies::ensure_table(&conn).unwrap();
        ensure_table(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn parse_since_accepts_days_shorthand_and_iso() {
        let iso = parse_since("2026-05-01").unwrap();
        assert_eq!(iso, "2026-05-01");
        let days = parse_since("30d").unwrap();
        assert_eq!(days.len(), 10); // YYYY-MM-DD
    }

    #[test]
    fn score_cmd_persists_outcome_from_price_history() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        let id = insert_recommendation(
            conn,
            &RecommendationInsert {
                report_date: "2026-05-01",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO price_history (symbol, date, close) VALUES
                ('BTC','2026-05-01','100.00'),
                ('BTC','2026-05-31','110.00');",
        )
        .unwrap();
        score_cmd(&backend, false, Some(id), 30, None, true).unwrap();
        let outcome = crate::db::recommendations::get_outcome(conn, id).unwrap().unwrap();
        assert!(outcome.outcome_score.is_some());
    }

    #[test]
    fn relink_historical_links_reply_to_open_recommendation() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        let rec_id = insert_recommendation(
            conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        let reply_id = crate::db::operator_replies::insert(
            conn,
            &crate::db::operator_replies::OperatorReplyInsert {
                journal_id: None,
                report_date: "2026-05-28",
                reply_date: "2026-05-28",
                asset: Some("BTC"),
                decision_type: "add",
                response_class: "yes",
                conviction_implied: None,
                timeframe_horizon: None,
                reasoning_summary: None,
                raw_content: "x",
            },
        )
        .unwrap();
        let out = relink_historical(conn, &backend, 7).unwrap();
        assert_eq!(out.replies_linked, 1);
        let outcome = crate::db::recommendations::get_outcome(conn, rec_id).unwrap().unwrap();
        assert_eq!(outcome.operator_reply_id, Some(reply_id));
        assert_eq!(outcome.action_status.as_deref(), Some("accepted"));
        // Idempotent — second run does not double-link.
        let out2 = relink_historical(conn, &backend, 7).unwrap();
        assert_eq!(out2.replies_linked, 0);
    }

    #[test]
    fn relink_historical_links_transaction_within_window() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        let rec_id = insert_recommendation(
            conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (symbol, category, tx_type, quantity, price_per, currency, date, notes)
             VALUES ('BTC','crypto','buy','1','100','USD','2026-06-02', NULL)",
            [],
        )
        .unwrap();
        let out = relink_historical(conn, &backend, 7).unwrap();
        assert_eq!(out.transactions_linked, 1);
        let outcome = crate::db::recommendations::get_outcome(conn, rec_id).unwrap().unwrap();
        assert!(outcome.transaction_id.is_some());
    }

    #[test]
    fn accuracy_cmd_groups_by_type() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        for score in [10.0, 80.0, -25.0] {
            let id = insert_recommendation(
                conn,
                &RecommendationInsert {
                    report_date: "2026-05-28",
                    asset: Some("BTC"),
                    recommendation_type: "add",
                    urgency: "normal",
                    rationale_summary: None,
                },
            )
            .unwrap();
            crate::db::recommendations::set_outcome_score(conn, id, score, "2026-06-01", None)
                .unwrap();
        }
        accuracy_cmd(&backend, Some("add"), None, "2026-01-01", 0.0, false, true).unwrap();
    }

    #[test]
    fn try_link_decision_reply_from_journal_inserts_and_links() {
        let backend = make_backend();
        let conn = backend.sqlite_native().unwrap();
        let rec_id = insert_recommendation(
            conn,
            &RecommendationInsert {
                report_date: "2026-05-28",
                asset: Some("BTC"),
                recommendation_type: "add",
                urgency: "normal",
                rationale_summary: None,
            },
        )
        .unwrap();
        let reply_id = try_link_decision_reply_from_journal(
            &backend,
            999,
            "DECISION REPLY asset=BTC type=add response=yes report_date=2026-05-28",
            "2026-05-29",
        )
        .unwrap()
        .unwrap();
        let outcome = crate::db::recommendations::get_outcome(conn, rec_id).unwrap().unwrap();
        assert_eq!(outcome.operator_reply_id, Some(reply_id));
    }
}
