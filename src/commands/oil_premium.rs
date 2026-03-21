//! `pftui data oil-premium` — Oil futures term structure and contango/backwardation analysis.
//!
//! Fetches front-month and near-term WTI/Brent futures contracts to compute:
//! - Contango/backwardation (front vs. next month spread)
//! - WTI-Brent spread
//! - Annualised roll yield
//! - Term structure shape signal (war-premium / supply-tightness indicator)

use anyhow::Result;
use chrono::{Datelike, Utc};
use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::price_cache::{get_all_cached_prices_backend, upsert_price_backend};
use crate::price::yahoo;

/// CME futures month codes: F=Jan G=Feb H=Mar J=Apr K=May M=Jun
/// N=Jul Q=Aug U=Sep V=Oct X=Nov Z=Dec
const MONTH_CODES: [char; 12] = ['F', 'G', 'H', 'J', 'K', 'M', 'N', 'Q', 'U', 'V', 'X', 'Z'];

#[derive(Debug, Serialize)]
struct OilPremiumReport {
    wti_front: Option<ContractQuote>,
    wti_next: Option<ContractQuote>,
    brent_front: Option<ContractQuote>,
    brent_next: Option<ContractQuote>,
    wti_term_structure: TermStructure,
    brent_term_structure: TermStructure,
    wti_brent_spread: Option<f64>,
    signal: String,
    interpretation: String,
}

#[derive(Debug, Serialize)]
struct ContractQuote {
    symbol: String,
    price: f64,
    label: String,
}

#[derive(Debug, Serialize)]
struct TermStructure {
    spread: Option<f64>,
    spread_pct: Option<f64>,
    annualised_roll_yield_pct: Option<f64>,
    structure: String,
}

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    let now = Utc::now();
    let (front_month, front_year) = next_active_contract(now.month(), now.year() as u32);
    let (next_month, next_year) = following_contract(front_month, front_year);

    let wti_front_sym = format!(
        "CL{}{}.NYM",
        MONTH_CODES[front_month as usize - 1],
        format_year(front_year)
    );
    let wti_next_sym = format!(
        "CL{}{}.NYM",
        MONTH_CODES[next_month as usize - 1],
        format_year(next_year)
    );
    let brent_front_sym = format!(
        "BZ{}{}.NYM",
        MONTH_CODES[front_month as usize - 1],
        format_year(front_year)
    );
    let brent_next_sym = format!(
        "BZ{}{}.NYM",
        MONTH_CODES[next_month as usize - 1],
        format_year(next_year)
    );

    // Also grab continuous contracts as fallback
    let wti_cont = "CL=F";
    let brent_cont = "BZ=F";

    let mut prices = get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|p| (p.symbol, p.price))
        .collect::<std::collections::HashMap<_, _>>();

    // Fetch all symbols
    let symbols = [
        wti_front_sym.as_str(),
        wti_next_sym.as_str(),
        brent_front_sym.as_str(),
        brent_next_sym.as_str(),
        wti_cont,
        brent_cont,
    ];
    for sym in &symbols {
        fetch_if_missing(backend, &mut prices, sym);
    }

    // Build quotes, falling back to continuous contract for front month
    let wti_front_price = prices
        .get(wti_front_sym.as_str())
        .or_else(|| prices.get(wti_cont))
        .copied();
    let wti_next_price = prices.get(wti_next_sym.as_str()).copied();
    let brent_front_price = prices
        .get(brent_front_sym.as_str())
        .or_else(|| prices.get(brent_cont))
        .copied();
    let brent_next_price = prices.get(brent_next_sym.as_str()).copied();

    let wti_front_label = format!("WTI {} {}", month_name(front_month), front_year);
    let wti_next_label = format!("WTI {} {}", month_name(next_month), next_year);
    let brent_front_label = format!("Brent {} {}", month_name(front_month), front_year);
    let brent_next_label = format!("Brent {} {}", month_name(next_month), next_year);

    let wti_ts = compute_term_structure(wti_front_price, wti_next_price);
    let brent_ts = compute_term_structure(brent_front_price, brent_next_price);

    let wti_brent_spread = match (wti_front_price, brent_front_price) {
        (Some(w), Some(b)) => Some(to_f64(w - b)),
        _ => None,
    };

    let (signal, interpretation) = derive_signal(&wti_ts, &brent_ts, wti_brent_spread);

    let report = OilPremiumReport {
        wti_front: wti_front_price.map(|p| ContractQuote {
            symbol: wti_front_sym.clone(),
            price: to_f64(p),
            label: wti_front_label.clone(),
        }),
        wti_next: wti_next_price.map(|p| ContractQuote {
            symbol: wti_next_sym.clone(),
            price: to_f64(p),
            label: wti_next_label.clone(),
        }),
        brent_front: brent_front_price.map(|p| ContractQuote {
            symbol: brent_front_sym.clone(),
            price: to_f64(p),
            label: brent_front_label.clone(),
        }),
        brent_next: brent_next_price.map(|p| ContractQuote {
            symbol: brent_next_sym.clone(),
            price: to_f64(p),
            label: brent_next_label.clone(),
        }),
        wti_term_structure: wti_ts,
        brent_term_structure: brent_ts,
        wti_brent_spread,
        signal,
        interpretation,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(
            &report,
            &wti_front_label,
            &wti_next_label,
            &brent_front_label,
            &brent_next_label,
        );
    }

    Ok(())
}

fn print_report(r: &OilPremiumReport, wti_fl: &str, wti_nl: &str, brent_fl: &str, brent_nl: &str) {
    println!("\nOil Futures Term Structure & Premium Analysis");
    println!("══════════════════════════════════════════════\n");

    println!("  WTI Crude");
    println!(
        "    Front ({:>16}): {}",
        wti_fl,
        fmt_price(r.wti_front.as_ref().map(|q| q.price))
    );
    println!(
        "    Next  ({:>16}): {}",
        wti_nl,
        fmt_price(r.wti_next.as_ref().map(|q| q.price))
    );
    println!(
        "    Spread:                   {}",
        fmt_spread(r.wti_term_structure.spread)
    );
    println!(
        "    Structure:                {}",
        r.wti_term_structure.structure
    );
    if let Some(ry) = r.wti_term_structure.annualised_roll_yield_pct {
        println!("    Annualised roll yield:    {:.2}%", ry);
    }

    println!();
    println!("  Brent Crude");
    println!(
        "    Front ({:>16}): {}",
        brent_fl,
        fmt_price(r.brent_front.as_ref().map(|q| q.price))
    );
    println!(
        "    Next  ({:>16}): {}",
        brent_nl,
        fmt_price(r.brent_next.as_ref().map(|q| q.price))
    );
    println!(
        "    Spread:                   {}",
        fmt_spread(r.brent_term_structure.spread)
    );
    println!(
        "    Structure:                {}",
        r.brent_term_structure.structure
    );
    if let Some(ry) = r.brent_term_structure.annualised_roll_yield_pct {
        println!("    Annualised roll yield:    {:.2}%", ry);
    }

    println!();
    if let Some(wbs) = r.wti_brent_spread {
        println!("  WTI-Brent Spread:           ${:.2}", wbs);
    }

    println!();
    println!("  Signal:         {}", r.signal);
    println!("  Interpretation: {}", r.interpretation);
    println!();
}

fn compute_term_structure(front: Option<Decimal>, next: Option<Decimal>) -> TermStructure {
    match (front, next) {
        (Some(f), Some(n)) => {
            let spread = to_f64(f - n);
            let spread_pct = if n != Decimal::ZERO {
                Some(to_f64((f - n) * Decimal::from(100) / n))
            } else {
                None
            };
            // Annualise: spread is ~1 month, so multiply by 12
            let annualised = spread_pct.map(|pct| pct * 12.0);
            let structure = if spread > 0.05 {
                "BACKWARDATION".to_string()
            } else if spread < -0.05 {
                "CONTANGO".to_string()
            } else {
                "FLAT".to_string()
            };
            TermStructure {
                spread: Some(round2(spread)),
                spread_pct: spread_pct.map(round2),
                annualised_roll_yield_pct: annualised.map(round2),
                structure,
            }
        }
        _ => TermStructure {
            spread: None,
            spread_pct: None,
            annualised_roll_yield_pct: None,
            structure: "UNAVAILABLE".to_string(),
        },
    }
}

fn derive_signal(
    wti: &TermStructure,
    brent: &TermStructure,
    wti_brent_spread: Option<f64>,
) -> (String, String) {
    let wti_back = wti.structure == "BACKWARDATION";
    let brent_back = brent.structure == "BACKWARDATION";
    let deep_back = wti.spread_pct.map(|p| p > 2.0).unwrap_or(false)
        || brent.spread_pct.map(|p| p > 2.0).unwrap_or(false);
    let wide_wb = wti_brent_spread.map(|s| s.abs() > 5.0).unwrap_or(false);

    if deep_back && wide_wb {
        (
            "🔴 SEVERE SUPPLY STRESS".to_string(),
            "Deep backwardation + wide WTI-Brent spread signals acute physical supply tightness. \
             War premium or major disruption likely priced in. Monitor Hormuz, OPEC+ emergency actions."
                .to_string(),
        )
    } else if wti_back && brent_back {
        (
            "🟠 SUPPLY TIGHTNESS".to_string(),
            "Both WTI and Brent in backwardation — physical demand exceeds near-term supply. \
             Consistent with geopolitical risk premium or strong demand. Watch inventory draws."
                .to_string(),
        )
    } else if wti_back || brent_back {
        (
            "🟡 MIXED STRUCTURE".to_string(),
            "Split term structure — one benchmark in backwardation, other not. \
             May indicate regional supply imbalance rather than global tightness."
                .to_string(),
        )
    } else if wti.structure == "CONTANGO" && brent.structure == "CONTANGO" {
        (
            "🟢 CONTANGO — AMPLE SUPPLY".to_string(),
            "Both benchmarks in contango — future months priced higher than spot. \
             Storage economics favorable. No immediate supply stress signal."
                .to_string(),
        )
    } else {
        (
            "⚪ NEUTRAL".to_string(),
            "Term structure near flat. No strong supply/demand signal from futures curve."
                .to_string(),
        )
    }
}

/// Determine front-month contract. WTI contracts expire ~20th of month before delivery.
/// If we're past the 15th, front month is likely 2 months out; otherwise 1 month out.
fn next_active_contract(current_month: u32, current_year: u32) -> (u32, u32) {
    // Front-month is typically the next calendar month
    if current_month == 12 {
        (1, current_year + 1)
    } else {
        (current_month + 1, current_year)
    }
}

fn following_contract(month: u32, year: u32) -> (u32, u32) {
    if month == 12 {
        (1, year + 1)
    } else {
        (month + 1, year)
    }
}

fn format_year(year: u32) -> String {
    format!("{}", year % 100)
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}

fn fetch_if_missing(
    backend: &BackendConnection,
    prices: &mut std::collections::HashMap<String, Decimal>,
    symbol: &str,
) {
    if prices.contains_key(symbol) {
        return;
    }
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(_) => return,
    };
    if let Ok(quote) = rt.block_on(yahoo::fetch_price(symbol)) {
        let _ = upsert_price_backend(backend, &quote);
        prices.insert(symbol.to_string(), quote.price);
    }
}

fn to_f64(v: Decimal) -> f64 {
    v.to_string().parse::<f64>().unwrap_or(0.0)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn fmt_price(v: Option<f64>) -> String {
    v.map(|p| format!("${:.2}", p))
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_spread(v: Option<f64>) -> String {
    v.map(|s| {
        let sign = if s >= 0.0 { "+" } else { "" };
        format!("{}${:.2}", sign, s)
    })
    .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn next_active_contract_jan() {
        assert_eq!(next_active_contract(1, 2026), (2, 2026));
    }

    #[test]
    fn next_active_contract_dec() {
        assert_eq!(next_active_contract(12, 2026), (1, 2027));
    }

    #[test]
    fn following_contract_normal() {
        assert_eq!(following_contract(4, 2026), (5, 2026));
    }

    #[test]
    fn following_contract_dec() {
        assert_eq!(following_contract(12, 2026), (1, 2027));
    }

    #[test]
    fn format_year_works() {
        assert_eq!(format_year(2026), "26");
        assert_eq!(format_year(2030), "30");
    }

    #[test]
    fn term_structure_backwardation() {
        let ts = compute_term_structure(
            Some(Decimal::from_str("100.00").unwrap()),
            Some(Decimal::from_str("98.00").unwrap()),
        );
        assert_eq!(ts.structure, "BACKWARDATION");
        assert!(ts.spread.unwrap() > 0.0);
    }

    #[test]
    fn term_structure_contango() {
        let ts = compute_term_structure(
            Some(Decimal::from_str("95.00").unwrap()),
            Some(Decimal::from_str("98.00").unwrap()),
        );
        assert_eq!(ts.structure, "CONTANGO");
        assert!(ts.spread.unwrap() < 0.0);
    }

    #[test]
    fn term_structure_flat() {
        let ts = compute_term_structure(
            Some(Decimal::from_str("100.00").unwrap()),
            Some(Decimal::from_str("100.03").unwrap()),
        );
        assert_eq!(ts.structure, "FLAT");
    }

    #[test]
    fn term_structure_unavailable() {
        let ts = compute_term_structure(None, Some(Decimal::from_str("98.00").unwrap()));
        assert_eq!(ts.structure, "UNAVAILABLE");
    }

    #[test]
    fn signal_severe() {
        let wti = TermStructure {
            spread: Some(5.0),
            spread_pct: Some(5.0),
            annualised_roll_yield_pct: Some(60.0),
            structure: "BACKWARDATION".to_string(),
        };
        let brent = TermStructure {
            spread: Some(3.0),
            spread_pct: Some(3.0),
            annualised_roll_yield_pct: Some(36.0),
            structure: "BACKWARDATION".to_string(),
        };
        let (sig, _) = derive_signal(&wti, &brent, Some(-8.0));
        assert!(sig.contains("SEVERE"));
    }

    #[test]
    fn signal_contango() {
        let ts = TermStructure {
            spread: Some(-2.0),
            spread_pct: Some(-2.0),
            annualised_roll_yield_pct: Some(-24.0),
            structure: "CONTANGO".to_string(),
        };
        let (sig, _) = derive_signal(&ts, &ts, Some(-1.0));
        assert!(sig.contains("CONTANGO"));
    }

    #[test]
    fn signal_mixed() {
        let back = TermStructure {
            spread: Some(1.0),
            spread_pct: Some(1.0),
            annualised_roll_yield_pct: Some(12.0),
            structure: "BACKWARDATION".to_string(),
        };
        let contango = TermStructure {
            spread: Some(-1.0),
            spread_pct: Some(-1.0),
            annualised_roll_yield_pct: Some(-12.0),
            structure: "CONTANGO".to_string(),
        };
        let (sig, _) = derive_signal(&back, &contango, Some(-2.0));
        assert!(sig.contains("MIXED"));
    }

    #[test]
    fn month_codes_complete() {
        assert_eq!(MONTH_CODES.len(), 12);
        // Jan=F, Apr=J, Jun=M, Dec=Z
        assert_eq!(MONTH_CODES[0], 'F');
        assert_eq!(MONTH_CODES[3], 'J');
        assert_eq!(MONTH_CODES[5], 'M');
        assert_eq!(MONTH_CODES[11], 'Z');
    }
}
