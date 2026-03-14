use anyhow::Result;

use crate::data::fedwatch;
use crate::db::backend::BackendConnection;
use crate::db::predictions_cache;

const CONFLICT_THRESHOLD_PCT_POINTS: f64 = 5.0;

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    let snapshot = fedwatch::fetch_snapshot()?;
    let prediction_markets = predictions_cache::get_cached_predictions_backend(backend, 200)?;
    let conflict = fedwatch::detect_no_change_conflict(
        &snapshot,
        &prediction_markets,
        CONFLICT_THRESHOLD_PCT_POINTS,
    );

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "snapshot": snapshot,
                "conflicts": conflict.as_ref().map(|c| vec![c]).unwrap_or_default(),
            }))?
        );
        return Ok(());
    }

    println!("\nFedWatch (CME)\n");
    println!("  Source: {}", snapshot.source_url);
    println!("  Fetched: {}", snapshot.fetched_at);
    println!();
    println!("  Next meeting: {}", snapshot.meeting_info.meeting_date);
    println!(
        "  Contract: {} (expires {})",
        snapshot.meeting_info.contract, snapshot.meeting_info.expires
    );
    println!(
        "  Mid price: {:.4} | Prior vol: {} | Prior OI: {}",
        snapshot.meeting_info.mid_price,
        snapshot.meeting_info.prior_volume,
        snapshot.meeting_info.prior_open_interest
    );
    println!();
    println!(
        "  Summary probs: Ease {:.1}% | No Change {:.1}% | Hike {:.1}%",
        snapshot.summary.ease_pct, snapshot.summary.no_change_pct, snapshot.summary.hike_pct
    );
    println!();
    println!("  Target rate distribution (Now):");
    for row in snapshot.target_probabilities.iter().take(8) {
        println!(
            "    {:<12} {:>6.1}%  (1D {:>5.1}% | 1W {:>5.1}% | 1M {:>5.1}%)",
            row.target_rate_bps, row.now_pct, row.one_day_pct, row.one_week_pct, row.one_month_pct
        );
    }
    println!();
    if !snapshot.meetings.is_empty() {
        println!("  Upcoming meetings:");
        println!("    {}", snapshot.meetings.join(", "));
        println!();
    }
    if let Some(conflict) = conflict {
        println!("  ⚠ Data source conflict detected:");
        println!(
            "    Metric: {} | CME {:.1}% vs alt {:.1}% (Δ {:.1}pp)",
            conflict.metric,
            conflict.cme_value_pct,
            conflict.alt_value_pct,
            conflict.delta_pct_points
        );
        println!("    Alt source: {}", conflict.alt_source_label);
        println!("    Recommended source: {}", conflict.recommended_source);
        println!("    Rationale: {}", conflict.rationale);
        println!();
    }

    Ok(())
}
