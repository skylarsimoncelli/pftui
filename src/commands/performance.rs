use anyhow::{bail, Result};
use chrono::{Datelike, NaiveDate, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::config::Config;
use crate::db::backend::BackendConnection;
use crate::db::snapshots::{get_all_portfolio_snapshots_backend, PortfolioSnapshot};

/// Compute the return percentage between two values.
fn return_pct(start: Decimal, end: Decimal) -> Option<Decimal> {
    if start == dec!(0) {
        return None;
    }
    Some(((end - start) / start) * dec!(100))
}

/// Find the snapshot closest to (but not after) a target date.
fn snapshot_at_or_before<'a>(snapshots: &'a [PortfolioSnapshot], target: &str) -> Option<&'a PortfolioSnapshot> {
    // Snapshots are in ascending order. Find the last one <= target.
    snapshots.iter().rev().find(|s| s.date.as_str() <= target)
}

/// Find the snapshot for period-based returns (MTD, QTD, YTD).
/// Prefer the first snapshot on/after period start (true period start anchor).
/// If none exists yet, fall back to the latest snapshot before period start.
fn snapshot_for_period<'a>(snapshots: &'a [PortfolioSnapshot], period_start: &str) -> Option<&'a PortfolioSnapshot> {
    // Prefer first snapshot inside the period.
    if let Some(snap) = snapshots.iter().find(|s| s.date.as_str() >= period_start) {
        return Some(snap);
    }
    // No in-period snapshot yet — use latest pre-period snapshot.
    snapshot_at_or_before(snapshots, period_start)
}

/// Format a return percentage for display.
fn fmt_return(pct: Option<Decimal>) -> String {
    match pct {
        Some(p) => format!("{:+.2}%", p),
        None => "N/A".to_string(),
    }
}

/// Format a dollar change for display.
fn fmt_dollar(change: Option<Decimal>, currency: &str) -> String {
    match change {
        Some(c) => format!("{:+.2} {}", c, currency),
        None => "N/A".to_string(),
    }
}

pub fn run(
    backend: &BackendConnection,
    config: &Config,
    since: Option<&str>,
    period: Option<&str>,
    vs_benchmark: Option<&str>,
    json: bool,
) -> Result<()> {
    let today = Utc::now().date_naive();
    // Get all snapshots for standard periods
    let all_snapshots = get_all_portfolio_snapshots_backend(backend)?;

    if all_snapshots.is_empty() {
        println!("No portfolio snapshots found.");
        println!("Run `pftui refresh` to start recording daily snapshots.");
        return Ok(());
    }

    let latest = all_snapshots.last().unwrap();
    let earliest = all_snapshots.first().unwrap();

    if json {
        return print_json(config, &all_snapshots, since, period, vs_benchmark, &today);
    }

    // If --since is provided, show custom period return
    if let Some(since_date) = since {
        validate_date(since_date)?;
        return print_since(config, &all_snapshots, since_date, latest, vs_benchmark);
    }

    // If --period is provided, show return series
    if let Some(period_str) = period {
        return print_period_series(config, &all_snapshots, period_str, &today);
    }

    // Default: show standard period returns (1D, 1W, 1M, MTD, QTD, YTD, since inception)
    print_standard_returns(&all_snapshots, latest, earliest, &today, config, vs_benchmark)
}

fn validate_date(date: &str) -> Result<()> {
    if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        bail!(
            "Invalid date '{}': expected YYYY-MM-DD format (e.g. 2026-02-24)",
            date
        );
    }
    Ok(())
}

fn print_standard_returns(
    snapshots: &[PortfolioSnapshot],
    latest: &PortfolioSnapshot,
    earliest: &PortfolioSnapshot,
    today: &NaiveDate,
    config: &Config,
    _vs_benchmark: Option<&str>,
) -> Result<()> {
    let current_value = latest.total_value;

    // Compute period start dates
    let d1 = (*today - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
    let w1 = (*today - chrono::Duration::days(7)).format("%Y-%m-%d").to_string();
    let m1 = (*today - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
    let mtd = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
        .unwrap_or(*today)
        .format("%Y-%m-%d")
        .to_string();
    let qtd = {
        let q_month = match today.month() {
            1..=3 => 1,
            4..=6 => 4,
            7..=9 => 7,
            _ => 10,
        };
        NaiveDate::from_ymd_opt(today.year(), q_month, 1)
            .unwrap_or(*today)
            .format("%Y-%m-%d")
            .to_string()
    };
    let ytd = NaiveDate::from_ymd_opt(today.year(), 1, 1)
        .unwrap_or(*today)
        .format("%Y-%m-%d")
        .to_string();

    // Find snapshot values at each period start
    let val_1d = snapshot_at_or_before(snapshots, &d1).map(|s| s.total_value);
    let val_1w = snapshot_at_or_before(snapshots, &w1).map(|s| s.total_value);
    let val_1m = snapshot_at_or_before(snapshots, &m1).map(|s| s.total_value);
    let val_mtd = snapshot_for_period(snapshots, &mtd).map(|s| s.total_value);
    let val_qtd = snapshot_for_period(snapshots, &qtd).map(|s| s.total_value);
    let val_ytd = snapshot_for_period(snapshots, &ytd).map(|s| s.total_value);
    let val_inception = Some(earliest.total_value);

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                  📈  PORTFOLIO PERFORMANCE                 ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!(
        "  Current Value: {:.2} {}    (as of {})",
        current_value, config.base_currency, latest.date
    );
    println!(
        "  Tracking Since: {}    ({} snapshots)",
        earliest.date,
        snapshots.len()
    );
    println!();

    println!(
        "  {:<16} {:>12} {:>18}",
        "Period", "Return", "Change"
    );
    println!("  {}", "─".repeat(48));

    let periods: Vec<(&str, Option<Decimal>)> = vec![
        ("1 Day", val_1d),
        ("1 Week", val_1w),
        ("1 Month", val_1m),
        ("MTD", val_mtd),
        ("QTD", val_qtd),
        ("YTD", val_ytd),
        ("Since Inception", val_inception),
    ];

    for (label, start_val) in &periods {
        let ret = start_val.and_then(|sv| return_pct(sv, current_value));
        let change = start_val.map(|sv| current_value - sv);
        println!(
            "  {:<16} {:>12} {:>18}",
            label,
            fmt_return(ret),
            fmt_dollar(change, &config.base_currency)
        );
    }

    println!();

    // Value composition
    println!("  Invested: {:.2}    Cash: {:.2}", latest.invested_value, latest.cash_value);

    Ok(())
}

fn print_since(
    config: &Config,
    snapshots: &[PortfolioSnapshot],
    since_date: &str,
    latest: &PortfolioSnapshot,
    _vs_benchmark: Option<&str>,
) -> Result<()> {
    let start_snap = snapshot_at_or_before(snapshots, since_date);
    let current_value = latest.total_value;

    match start_snap {
        Some(snap) => {
            let ret = return_pct(snap.total_value, current_value);
            let change = current_value - snap.total_value;

            println!("Performance since {} → {}", snap.date, latest.date);
            println!();
            println!("  Start Value:   {:.2} {}", snap.total_value, config.base_currency);
            println!("  Current Value: {:.2} {}", current_value, config.base_currency);
            println!("  Change:        {}", fmt_dollar(Some(change), &config.base_currency));
            println!("  Return:        {}", fmt_return(ret));

            // Show intermediate snapshots if available
            let period_snaps: Vec<&PortfolioSnapshot> = snapshots
                .iter()
                .filter(|s| s.date.as_str() >= snap.date.as_str())
                .collect();

            if period_snaps.len() > 2 {
                println!();
                println!("  Daily snapshots in period: {}", period_snaps.len());

                // Find best and worst day
                let mut best_day: Option<(&str, Decimal)> = None;
                let mut worst_day: Option<(&str, Decimal)> = None;

                for window in period_snaps.windows(2) {
                    let prev = window[0];
                    let curr = window[1];
                    if let Some(pct) = return_pct(prev.total_value, curr.total_value) {
                        match &best_day {
                            None => best_day = Some((&curr.date, pct)),
                            Some((_, best_pct)) => {
                                if pct > *best_pct {
                                    best_day = Some((&curr.date, pct));
                                }
                            }
                        }
                        match &worst_day {
                            None => worst_day = Some((&curr.date, pct)),
                            Some((_, worst_pct)) => {
                                if pct < *worst_pct {
                                    worst_day = Some((&curr.date, pct));
                                }
                            }
                        }
                    }
                }

                if let Some((date, pct)) = best_day {
                    println!("  Best Day:  {} ({:+.2}%)", date, pct);
                }
                if let Some((date, pct)) = worst_day {
                    println!("  Worst Day: {} ({:+.2}%)", date, pct);
                }
            }
        }
        None => {
            println!("No snapshots found on or before {}.", since_date);
            println!("Earliest snapshot: {}", snapshots.first().map(|s| s.date.as_str()).unwrap_or("none"));
        }
    }

    Ok(())
}

fn print_period_series(
    config: &Config,
    snapshots: &[PortfolioSnapshot],
    period: &str,
    _today: &NaiveDate,
) -> Result<()> {
    // Group snapshots by period
    let grouped = match period {
        "daily" => group_daily(snapshots),
        "weekly" => group_weekly(snapshots),
        "monthly" => group_monthly(snapshots),
        other => bail!("Unknown period '{}'. Use: daily, weekly, monthly", other),
    };

    println!(
        "  {:<12} {:>14} {:>10}",
        "Period", format!("Value ({})", config.base_currency), "Return"
    );
    println!("  {}", "─".repeat(40));

    for (label, snap) in &grouped {
        println!(
            "  {:<12} {:>14.2} {:>10}",
            label, snap.total_value, "—"
        );
    }

    // Show period-over-period returns
    if grouped.len() > 1 {
        println!();
        println!("  Period Returns:");
        println!("  {}", "─".repeat(40));
        for window in grouped.windows(2) {
            let (_, prev) = &window[0];
            let (label, curr) = &window[1];
            let ret = return_pct(prev.total_value, curr.total_value);
            let change = curr.total_value - prev.total_value;
            println!(
                "  {:<12} {:>10} {:>18}",
                label,
                fmt_return(ret),
                fmt_dollar(Some(change), &config.base_currency)
            );
        }
    }

    Ok(())
}

fn group_daily(snapshots: &[PortfolioSnapshot]) -> Vec<(String, PortfolioSnapshot)> {
    snapshots.iter().map(|s| (s.date.clone(), s.clone())).collect()
}

fn group_weekly(snapshots: &[PortfolioSnapshot]) -> Vec<(String, PortfolioSnapshot)> {
    // Take the last snapshot of each ISO week
    let mut weeks: Vec<(String, PortfolioSnapshot)> = Vec::new();

    for snap in snapshots {
        if let Ok(date) = NaiveDate::parse_from_str(&snap.date, "%Y-%m-%d") {
            let week_label = format!("{}-W{:02}", date.iso_week().year(), date.iso_week().week());
            match weeks.last_mut() {
                Some((label, existing)) if *label == week_label => {
                    *existing = snap.clone();
                }
                _ => {
                    weeks.push((week_label, snap.clone()));
                }
            }
        }
    }

    weeks
}

fn group_monthly(snapshots: &[PortfolioSnapshot]) -> Vec<(String, PortfolioSnapshot)> {
    let mut months: Vec<(String, PortfolioSnapshot)> = Vec::new();

    for snap in snapshots {
        if let Ok(date) = NaiveDate::parse_from_str(&snap.date, "%Y-%m-%d") {
            let month_label = format!("{}-{:02}", date.year(), date.month());
            match months.last_mut() {
                Some((label, existing)) if *label == month_label => {
                    *existing = snap.clone();
                }
                _ => {
                    months.push((month_label, snap.clone()));
                }
            }
        }
    }

    months
}

fn print_json(
    config: &Config,
    snapshots: &[PortfolioSnapshot],
    since: Option<&str>,
    _period: Option<&str>,
    _vs_benchmark: Option<&str>,
    today: &NaiveDate,
) -> Result<()> {
    let latest = snapshots.last().unwrap();
    let earliest = snapshots.first().unwrap();
    let current_value = latest.total_value;

    // Build standard period returns
    let d1 = (*today - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
    let w1 = (*today - chrono::Duration::days(7)).format("%Y-%m-%d").to_string();
    let m1 = (*today - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
    let ytd = NaiveDate::from_ymd_opt(today.year(), 1, 1)
        .unwrap_or(*today)
        .format("%Y-%m-%d")
        .to_string();

    let val_1d = snapshot_at_or_before(snapshots, &d1).map(|s| s.total_value);
    let val_1w = snapshot_at_or_before(snapshots, &w1).map(|s| s.total_value);
    let val_1m = snapshot_at_or_before(snapshots, &m1).map(|s| s.total_value);
    let val_ytd = snapshot_at_or_before(snapshots, &ytd).map(|s| s.total_value);
    let val_inception = Some(earliest.total_value);

    let ret_1d = val_1d.and_then(|v| return_pct(v, current_value));
    let ret_1w = val_1w.and_then(|v| return_pct(v, current_value));
    let ret_1m = val_1m.and_then(|v| return_pct(v, current_value));
    let ret_ytd = val_ytd.and_then(|v| return_pct(v, current_value));
    let ret_inception = val_inception.and_then(|v| return_pct(v, current_value));

    let fmt_dec = |d: Option<Decimal>| -> String {
        match d {
            Some(v) => format!("{:.2}", v),
            None => "null".to_string(),
        }
    };

    let since_block = if let Some(since_date) = since {
        let snap = snapshot_at_or_before(snapshots, since_date);
        match snap {
            Some(s) => {
                let ret = return_pct(s.total_value, current_value);
                let change = current_value - s.total_value;
                format!(
                    r#","since":{{"from":"{}","start_value":{},"return_pct":{},"change":{:.2}}}"#,
                    s.date,
                    fmt_dec(Some(s.total_value)),
                    fmt_dec(ret),
                    change
                )
            }
            None => r#","since":null"#.to_string(),
        }
    } else {
        String::new()
    };

    println!(
        r#"{{"current_value":{:.2},"currency":"{}","as_of":"{}","tracking_since":"{}","snapshots":{},"returns":{{"1d":{},"1w":{},"1m":{},"ytd":{},"inception":{}}}{}}}"#,
        current_value,
        config.base_currency,
        latest.date,
        earliest.date,
        snapshots.len(),
        fmt_dec(ret_1d),
        fmt_dec(ret_1w),
        fmt_dec(ret_1m),
        fmt_dec(ret_ytd),
        fmt_dec(ret_inception),
        since_block,
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_backend(conn: rusqlite::Connection) -> crate::db::backend::BackendConnection {
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn test_return_pct_basic() {
        assert_eq!(return_pct(dec!(100), dec!(110)), Some(dec!(10)));
        assert_eq!(return_pct(dec!(100), dec!(90)), Some(dec!(-10)));
        assert_eq!(return_pct(dec!(100), dec!(100)), Some(dec!(0)));
    }

    #[test]
    fn test_return_pct_zero_start() {
        assert_eq!(return_pct(dec!(0), dec!(100)), None);
    }

    #[test]
    fn test_snapshot_at_or_before() {
        let snaps = vec![
            PortfolioSnapshot {
                date: "2026-02-01".into(),
                total_value: dec!(100000),
                cash_value: dec!(20000),
                invested_value: dec!(80000),
                snapshot_at: "2026-02-01T12:00:00".into(),
            },
            PortfolioSnapshot {
                date: "2026-02-15".into(),
                total_value: dec!(105000),
                cash_value: dec!(20000),
                invested_value: dec!(85000),
                snapshot_at: "2026-02-15T12:00:00".into(),
            },
            PortfolioSnapshot {
                date: "2026-03-01".into(),
                total_value: dec!(110000),
                cash_value: dec!(20000),
                invested_value: dec!(90000),
                snapshot_at: "2026-03-01T12:00:00".into(),
            },
        ];

        // Exact match
        let snap = snapshot_at_or_before(&snaps, "2026-02-15").unwrap();
        assert_eq!(snap.date, "2026-02-15");

        // Between dates — should get the earlier one
        let snap = snapshot_at_or_before(&snaps, "2026-02-20").unwrap();
        assert_eq!(snap.date, "2026-02-15");

        // Before all snapshots — should return None
        assert!(snapshot_at_or_before(&snaps, "2026-01-01").is_none());

        // After all snapshots — should get the last one
        let snap = snapshot_at_or_before(&snaps, "2026-04-01").unwrap();
        assert_eq!(snap.date, "2026-03-01");
    }

    #[test]
    fn test_group_weekly() {
        let snaps = vec![
            PortfolioSnapshot {
                date: "2026-02-23".into(), // Mon
                total_value: dec!(100000),
                cash_value: dec!(20000),
                invested_value: dec!(80000),
                snapshot_at: "".into(),
            },
            PortfolioSnapshot {
                date: "2026-02-24".into(), // Tue
                total_value: dec!(101000),
                cash_value: dec!(20000),
                invested_value: dec!(81000),
                snapshot_at: "".into(),
            },
            PortfolioSnapshot {
                date: "2026-03-02".into(), // Mon next week
                total_value: dec!(102000),
                cash_value: dec!(20000),
                invested_value: dec!(82000),
                snapshot_at: "".into(),
            },
        ];

        let weeks = group_weekly(&snaps);
        assert_eq!(weeks.len(), 2);
        // First week should have the latest value from that week
        assert_eq!(weeks[0].1.total_value, dec!(101000));
        assert_eq!(weeks[1].1.total_value, dec!(102000));
    }

    #[test]
    fn test_group_monthly() {
        let snaps = vec![
            PortfolioSnapshot {
                date: "2026-02-01".into(),
                total_value: dec!(100000),
                cash_value: dec!(20000),
                invested_value: dec!(80000),
                snapshot_at: "".into(),
            },
            PortfolioSnapshot {
                date: "2026-02-28".into(),
                total_value: dec!(105000),
                cash_value: dec!(20000),
                invested_value: dec!(85000),
                snapshot_at: "".into(),
            },
            PortfolioSnapshot {
                date: "2026-03-01".into(),
                total_value: dec!(106000),
                cash_value: dec!(20000),
                invested_value: dec!(86000),
                snapshot_at: "".into(),
            },
        ];

        let months = group_monthly(&snaps);
        assert_eq!(months.len(), 2);
        assert_eq!(months[0].0, "2026-02");
        assert_eq!(months[0].1.total_value, dec!(105000)); // last of Feb
        assert_eq!(months[1].0, "2026-03");
    }

    #[test]
    fn test_fmt_return() {
        assert_eq!(fmt_return(Some(dec!(10.50))), "+10.50%");
        assert_eq!(fmt_return(Some(dec!(-5.25))), "-5.25%");
        assert_eq!(fmt_return(None), "N/A");
    }

    #[test]
    fn test_performance_no_snapshots() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        let backend = to_backend(conn);
        let result = run(&backend, &config, None, None, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_performance_with_snapshots() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::snapshots::upsert_portfolio_snapshot;

        // Add some snapshots
        upsert_portfolio_snapshot(&conn, "2026-02-01", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-02-15", dec!(105000), dec!(20000), dec!(85000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-01", dec!(110000), dec!(20000), dec!(90000))
            .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, None, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_performance_since() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::snapshots::upsert_portfolio_snapshot;

        upsert_portfolio_snapshot(&conn, "2026-02-01", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-01", dec!(110000), dec!(20000), dec!(90000))
            .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, Some("2026-02-01"), None, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_performance_period_weekly() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::snapshots::upsert_portfolio_snapshot;

        upsert_portfolio_snapshot(&conn, "2026-02-23", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-02-24", dec!(101000), dec!(20000), dec!(81000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-02", dec!(102000), dec!(20000), dec!(82000))
            .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, Some("weekly"), None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_performance_json() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::snapshots::upsert_portfolio_snapshot;

        upsert_portfolio_snapshot(&conn, "2026-02-01", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-01", dec!(110000), dec!(20000), dec!(90000))
            .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, None, None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_performance_invalid_since_date() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::snapshots::upsert_portfolio_snapshot;
        upsert_portfolio_snapshot(&conn, "2026-02-01", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, Some("not-a-date"), None, None, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_performance_invalid_period() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::snapshots::upsert_portfolio_snapshot;
        upsert_portfolio_snapshot(&conn, "2026-02-01", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, Some("yearly"), None, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_snapshot_db_functions() {
        let conn = crate::db::open_in_memory();

        use crate::db::snapshots::{upsert_portfolio_snapshot, get_portfolio_snapshots_since};

        upsert_portfolio_snapshot(&conn, "2026-02-01", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-02-15", dec!(105000), dec!(20000), dec!(85000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-01", dec!(110000), dec!(20000), dec!(90000))
            .unwrap();

        // Test get_portfolio_snapshots_since
        let since = get_portfolio_snapshots_since(&conn, "2026-02-15").unwrap();
        assert_eq!(since.len(), 2);
        assert_eq!(since[0].date, "2026-02-15");
        assert_eq!(since[1].date, "2026-03-01");

        // Test get_all_portfolio_snapshots
        let all = crate::db::snapshots::get_all_portfolio_snapshots(&conn).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].date, "2026-02-01");
        assert_eq!(all[2].date, "2026-03-01");
    }

    #[test]
    fn test_snapshot_for_period_prefers_in_period_start() {
        let snapshots = vec![
            PortfolioSnapshot {
                date: "2026-02-28".into(),
                total_value: dec!(100000),
                cash_value: dec!(20000),
                invested_value: dec!(80000),
                snapshot_at: "".into(),
            },
            PortfolioSnapshot {
                date: "2026-03-06".into(),
                total_value: dec!(101000),
                cash_value: dec!(20000),
                invested_value: dec!(81000),
                snapshot_at: "".into(),
            },
        ];

        let start = snapshot_for_period(&snapshots, "2026-03-01").unwrap();
        assert_eq!(start.date, "2026-03-06");
    }
}
