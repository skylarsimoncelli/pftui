use anyhow::Result;

use crate::data::fedwatch;

pub fn run(json: bool) -> Result<()> {
    let snapshot = fedwatch::fetch_snapshot()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
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

    Ok(())
}
