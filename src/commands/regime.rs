use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::price_cache::get_cached_price;
use crate::db::price_history::get_history;
use crate::db::regime_snapshots;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeAssessment {
    pub regime: String,
    pub confidence: f64,
    pub drivers: Vec<String>,
    pub vix: Option<f64>,
    pub dxy: Option<f64>,
    pub yield_10y: Option<f64>,
    pub oil: Option<f64>,
    pub gold: Option<f64>,
    pub btc: Option<f64>,
}

fn latest_price(conn: &Connection, symbol: &str) -> Option<f64> {
    get_cached_price(conn, symbol, "USD")
        .ok()
        .flatten()
        .and_then(|q| q.price.to_string().parse::<f64>().ok())
}

fn trend_up(conn: &Connection, symbol: &str, days: u32) -> Option<bool> {
    let rows = get_history(conn, symbol, days + 2).ok()?;
    if rows.len() < (days as usize + 1) {
        return None;
    }
    let latest = rows.last()?.close.to_string().parse::<f64>().ok()?;
    let prev = rows[rows.len() - 1 - days as usize]
        .close
        .to_string()
        .parse::<f64>()
        .ok()?;
    Some(latest > prev)
}

pub fn classify_regime(conn: &Connection) -> RegimeAssessment {
    let vix = latest_price(conn, "^VIX");
    let dxy = latest_price(conn, "DX-Y.NYB");
    let yield_10y = latest_price(conn, "^TNX");
    let oil = latest_price(conn, "CL=F");
    let gold = latest_price(conn, "GC=F");
    let btc = latest_price(conn, "BTC").or_else(|| latest_price(conn, "BTC-USD"));

    let eq_up = trend_up(conn, "SPY", 7).or_else(|| trend_up(conn, "^GSPC", 7));
    let dxy_up = trend_up(conn, "DX-Y.NYB", 7);
    let gold_up = trend_up(conn, "GC=F", 7);

    let mut drivers = Vec::new();

    let crisis_match = vix.map(|x| x > 30.0).unwrap_or(false) && oil.map(|x| x > 90.0).unwrap_or(false);
    if crisis_match {
        drivers.push("VIX > 30 and oil > 90".to_string());
    }

    let stagflation_match = vix.map(|x| x > 25.0).unwrap_or(false)
        && oil.map(|x| x > 80.0).unwrap_or(false)
        && gold_up.unwrap_or(false)
        && eq_up.map(|v| !v).unwrap_or(false);
    if stagflation_match {
        drivers.push("VIX > 25, oil > 80, gold up, equities down".to_string());
    }

    let risk_off_match = vix.map(|x| x > 25.0).unwrap_or(false)
        || (dxy_up.unwrap_or(false) && gold_up.unwrap_or(false) && eq_up.map(|v| !v).unwrap_or(false));
    if risk_off_match {
        drivers.push("VIX high or DXY/gold up with equities down".to_string());
    }

    let risk_on_match = vix.map(|x| x < 20.0).unwrap_or(false)
        && eq_up.unwrap_or(false)
        && !dxy_up.unwrap_or(false);
    if risk_on_match {
        drivers.push("VIX < 20, equities up, DXY stable/falling".to_string());
    }

    let (regime, matched, total) = if crisis_match {
        ("crisis", 2.0, 2.0)
    } else if stagflation_match {
        ("stagflation", 4.0, 4.0)
    } else if risk_off_match {
        let mut m = 0.0;
        let mut t = 0.0;
        t += 1.0;
        if vix.map(|x| x > 25.0).unwrap_or(false) { m += 1.0; }
        t += 1.0;
        if dxy_up.unwrap_or(false) { m += 1.0; }
        t += 1.0;
        if gold_up.unwrap_or(false) { m += 1.0; }
        t += 1.0;
        if eq_up.map(|v| !v).unwrap_or(false) { m += 1.0; }
        ("risk-off", m, t)
    } else if risk_on_match {
        let mut m = 0.0;
        let mut t = 0.0;
        t += 1.0;
        if vix.map(|x| x < 20.0).unwrap_or(false) { m += 1.0; }
        t += 1.0;
        if eq_up.unwrap_or(false) { m += 1.0; }
        t += 1.0;
        if !dxy_up.unwrap_or(false) { m += 1.0; }
        ("risk-on", m, t)
    } else {
        ("transition", 1.0, 3.0)
    };

    RegimeAssessment {
        regime: regime.to_string(),
        confidence: if total > 0.0 { matched / total } else { 0.0 },
        drivers,
        vix,
        dxy,
        yield_10y,
        oil,
        gold,
        btc,
    }
}

pub fn classify_and_store_if_needed(conn: &Connection) -> Result<bool> {
    let assessment = classify_regime(conn);
    let current = regime_snapshots::get_current(conn)?;

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let should_store = match current {
        None => true,
        Some(ref c) => {
            let last_date = c.recorded_at.get(0..10).unwrap_or("");
            c.regime != assessment.regime || last_date != today
        }
    };

    if should_store {
        let drivers_json = serde_json::to_string(&assessment.drivers)?;
        regime_snapshots::store_regime(
            conn,
            &assessment.regime,
            Some(assessment.confidence),
            Some(&drivers_json),
            assessment.vix,
            assessment.dxy,
            assessment.yield_10y,
            assessment.oil,
            assessment.gold,
            assessment.btc,
        )?;
        return Ok(true);
    }

    Ok(false)
}

pub fn run(conn: &Connection, action: &str, limit: Option<usize>, json_output: bool) -> Result<()> {
    match action {
        "current" => {
            let current = regime_snapshots::get_current(conn)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "current": current }))?);
            } else if let Some(c) = current {
                println!(
                    "Current Regime: {} (confidence: {:.2})",
                    c.regime.to_uppercase(),
                    c.confidence.unwrap_or(0.0)
                );
                if let Some(dr) = c.drivers {
                    println!("  Drivers: {}", dr);
                }
                println!(
                    "  VIX: {:?} | DXY: {:?} | 10Y: {:?} | Oil: {:?} | Gold: {:?} | BTC: {:?}",
                    c.vix, c.dxy, c.yield_10y, c.oil, c.gold, c.btc
                );
                println!("  Recorded: {}", c.recorded_at);
            } else {
                println!("No regime snapshots yet. Run `pftui refresh`.");
            }
        }
        "history" => {
            let rows = regime_snapshots::get_history(conn, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "history": rows }))?);
            } else if rows.is_empty() {
                println!("No regime history.");
            } else {
                println!("Regime history ({}):", rows.len());
                for r in rows {
                    println!("  {}  {}  conf={:.2}", r.recorded_at, r.regime, r.confidence.unwrap_or(0.0));
                }
            }
        }
        "transitions" => {
            let rows = regime_snapshots::get_transitions(conn, limit)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "transitions": rows }))?);
            } else if rows.is_empty() {
                println!("No regime transitions.");
            } else {
                println!("Regime transitions ({}):", rows.len());
                for r in rows {
                    println!("  {}  {}", r.recorded_at, r.regime);
                }
            }
        }
        other => anyhow::bail!("unknown regime action '{}'. Valid: current, history, transitions", other),
    }

    Ok(())
}
