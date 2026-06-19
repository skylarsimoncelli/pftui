//! Series resolution — turn a symbol/field/timeframe reference into a
//! lookahead-safe series aligned onto the backtest's master daily axis.
//!
//! Design notes:
//!
//! - The **master axis** is the primary `--asset`'s daily bar dates
//!   (oldest-first). Every other series is projected onto it.
//! - A reference resolves first to a **bucket series**: one value per bar at
//!   the requested timeframe (daily = one bucket per bar; weekly/monthly =
//!   the last close in each ISO-week / calendar-month group). Each bucket
//!   carries the date on which it *completes*.
//! - **Projection** walks the master axis and, for each date `d`, takes the
//!   value of the latest bucket whose completion date is `<= d`. This is the
//!   single place lookahead can leak: a weekly value only becomes visible on
//!   or after the week's last bar, never before. Days before a series' first
//!   datapoint resolve to `None` (no fabricated history).
//! - Indicators (`sma`/`ema`/`rsi`) are computed over the **bucket** values
//!   (so "200-week MA" means 200 weekly closes), then projected to daily.
//!
//! All values are `f64`: these are price levels and indicator statistics, not
//! monetary balances (cf. `research::event_study`).

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Datelike, NaiveDate};

use super::parser::{IndicatorKind, PriceField, Timeframe};
use crate::indicators::{compute_rsi, compute_sma};

/// Abstracts where raw `(date, value)` series come from, so the engine is
/// testable without a database. Returned series must be oldest-first with
/// unique ascending `YYYY-MM-DD` dates.
pub trait SeriesLoader {
    fn load(&self, symbol: &str, field: PriceField) -> Result<Vec<(String, f64)>>;
}

/// One value per completed bucket, in chronological order.
struct BucketSeries {
    values: Vec<f64>,
    /// Date each bucket completes (its last bar's date), ascending.
    end_date: Vec<String>,
}

pub struct Resolver<'a> {
    master_dates: Vec<String>,
    primary_symbol: String,
    loader: &'a dyn SeriesLoader,
    raw_cache: HashMap<(String, PriceField), Vec<(String, f64)>>,
    /// Memoized daily-aligned series (field + indicator) keyed by a stable
    /// string — avoids recomputing `sma(close, 200)` each time it appears in an
    /// expression (or across a parameter sweep).
    series_cache: HashMap<String, Vec<Option<f64>>>,
    /// Symbols referenced that resolved to ZERO data (likely typos / no history)
    /// — surfaced so the caller can fail loudly instead of silently producing
    /// an all-`None` mask.
    missing: std::collections::BTreeSet<String>,
}

impl<'a> Resolver<'a> {
    pub fn new(master_dates: Vec<String>, primary_symbol: &str, loader: &'a dyn SeriesLoader) -> Self {
        Resolver {
            master_dates,
            primary_symbol: primary_symbol.to_string(),
            loader,
            raw_cache: HashMap::new(),
            series_cache: HashMap::new(),
            missing: std::collections::BTreeSet::new(),
        }
    }

    pub fn master_len(&self) -> usize {
        self.master_dates.len()
    }

    pub fn master_dates(&self) -> &[String] {
        &self.master_dates
    }

    /// Symbols referenced in resolved expressions that had no price history.
    pub fn missing_symbols(&self) -> Vec<String> {
        self.missing.iter().cloned().collect()
    }

    fn raw(&mut self, symbol: &str, field: PriceField) -> Result<&[(String, f64)]> {
        let resolved = resolve_alias(symbol);
        let key = (resolved.clone(), field);
        if !self.raw_cache.contains_key(&key) {
            let series = self.loader.load(&resolved, field)?;
            if series.is_empty() {
                self.missing.insert(resolved.clone());
            }
            self.raw_cache.insert(key.clone(), series);
        }
        Ok(self.raw_cache.get(&key).map(|v| v.as_slice()).unwrap())
    }

    fn bucket_series(
        &mut self,
        symbol: Option<&str>,
        field: PriceField,
        tf: Timeframe,
    ) -> Result<BucketSeries> {
        let sym = symbol
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.primary_symbol.clone());
        let raw = self.raw(&sym, field)?.to_vec();
        bucketize(&raw, tf)
    }

    /// Resolve a plain field reference to a daily-aligned series.
    pub fn field_series(
        &mut self,
        symbol: Option<&str>,
        field: PriceField,
        tf: Timeframe,
    ) -> Result<Vec<Option<f64>>> {
        let cache_key = format!("F:{}:{:?}:{:?}", symbol.unwrap_or("@"), field, tf);
        if let Some(v) = self.series_cache.get(&cache_key) {
            return Ok(v.clone());
        }
        let bs = self.bucket_series(symbol, field, tf)?;
        let vals: Vec<Option<f64>> = bs.values.iter().map(|v| Some(*v)).collect();
        let out = project(&bs.end_date, &vals, &self.master_dates);
        self.series_cache.insert(cache_key, out.clone());
        Ok(out)
    }

    /// Resolve a field to the master axis by EXACT date match — `None` where
    /// the symbol has no value on that exact date (NO carry-forward). This is
    /// the correct alignment for intra-bar high/low used by stop/target checks:
    /// carrying a stale prior extreme forward would fabricate phantom stops on
    /// bars whose OHLC is NULL. Daily timeframe only.
    pub fn field_series_exact(
        &mut self,
        symbol: Option<&str>,
        field: PriceField,
    ) -> Result<Vec<Option<f64>>> {
        let sym = symbol
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.primary_symbol.clone());
        let raw = self.raw(&sym, field)?.to_vec();
        let map: HashMap<&str, f64> = raw.iter().map(|(d, v)| (d.as_str(), *v)).collect();
        Ok(self
            .master_dates
            .iter()
            .map(|d| map.get(d.as_str()).copied())
            .collect())
    }

    /// Resolve an indicator over a field reference to a daily-aligned series.
    /// The indicator is computed at the requested timeframe's bucket
    /// granularity, then projected to daily. Memoized.
    pub fn indicator_series(
        &mut self,
        kind: IndicatorKind,
        symbol: Option<&str>,
        field: PriceField,
        period: usize,
        tf: Timeframe,
    ) -> Result<Vec<Option<f64>>> {
        let cache_key = format!(
            "I:{}:{:?}:{:?}:{}:{:?}",
            symbol.unwrap_or("@"),
            field,
            kind,
            period,
            tf
        );
        if let Some(v) = self.series_cache.get(&cache_key) {
            return Ok(v.clone());
        }
        let bs = self.bucket_series(symbol, field, tf)?;
        let computed = compute_indicator(kind, &bs.values, period);
        let out = project(&bs.end_date, &computed, &self.master_dates);
        self.series_cache.insert(cache_key, out.clone());
        Ok(out)
    }
}

fn compute_indicator(kind: IndicatorKind, values: &[f64], period: usize) -> Vec<Option<f64>> {
    match kind {
        IndicatorKind::Sma => compute_sma(values, period),
        IndicatorKind::Rsi => compute_rsi(values, period),
        IndicatorKind::Ema => compute_ema(values, period),
    }
}

/// EMA with an SMA seed at index `period-1` (same convention as the MACD
/// engine's internal EMA). First `period-1` entries are `None`.
fn compute_ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || n < period {
        return out;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let seed: f64 = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(seed);
    let mut prev = seed;
    for i in period..n {
        let cur = (values[i] - prev) * k + prev;
        out[i] = Some(cur);
        prev = cur;
    }
    out
}

/// Group a raw oldest-first series into timeframe buckets, taking the last
/// value in each group as the bucket close.
fn bucketize(raw: &[(String, f64)], tf: Timeframe) -> Result<BucketSeries> {
    if matches!(tf, Timeframe::Daily) {
        return Ok(BucketSeries {
            values: raw.iter().map(|(_, v)| *v).collect(),
            end_date: raw.iter().map(|(d, _)| d.clone()).collect(),
        });
    }
    let mut values = Vec::new();
    let mut end_date = Vec::new();
    let mut cur_key: Option<(i32, u32)> = None;
    for (d, v) in raw {
        let date = NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("bad date in series: {d}"))?;
        let key = bucket_key(date, tf);
        match cur_key {
            Some(k) if k == key => {
                // Same bucket: extend the running close.
                *values.last_mut().unwrap() = *v;
                *end_date.last_mut().unwrap() = d.clone();
            }
            _ => {
                values.push(*v);
                end_date.push(d.clone());
                cur_key = Some(key);
            }
        }
    }
    Ok(BucketSeries { values, end_date })
}

fn bucket_key(date: NaiveDate, tf: Timeframe) -> (i32, u32) {
    match tf {
        Timeframe::Daily => (date.year(), date.ordinal()),
        Timeframe::Weekly => {
            let iso = date.iso_week();
            (iso.year(), iso.week())
        }
        Timeframe::Monthly => (date.year(), date.month()),
    }
}

/// Project a bucket-indexed value series onto the master daily axis. For each
/// master date, carry forward the latest bucket whose end date is `<= d`.
fn project(end_date: &[String], values: &[Option<f64>], master: &[String]) -> Vec<Option<f64>> {
    let mut out = vec![None; master.len()];
    let mut bi = 0;
    let mut cur: Option<f64> = None;
    for (k, d) in master.iter().enumerate() {
        while bi < end_date.len() && end_date[bi].as_str() <= d.as_str() {
            cur = values[bi];
            bi += 1;
        }
        out[k] = cur;
    }
    out
}

/// Map a user-facing symbol/alias to the canonical price_history ticker.
/// Interest-rate aliases resolve to Yahoo yield series already present in
/// `price_history`, so "rate hiking/cutting cycles" become moving-average
/// crossings on a level series like any other asset.
pub fn resolve_alias(token: &str) -> String {
    let upper = token.to_uppercase();
    let mapped = match upper.as_str() {
        "BTC" | "BITCOIN" | "BTC-USD" => "BTC-USD",
        "ETH" | "ETHEREUM" | "ETH-USD" => "ETH-USD",
        "SOL" | "SOLANA" | "SOL-USD" => "SOL-USD",
        "GOLD" | "XAUUSD" | "GC=F" => "GC=F",
        "SILVER" | "XAGUSD" | "SI=F" => "SI=F",
        "DXY" | "DOLLAR" | "DX-Y.NYB" => "DX-Y.NYB",
        "SPY" | "S&P" | "SP500" | "S&P500" => "SPY",
        "QQQ" | "NASDAQ" => "QQQ",
        "OIL" | "CRUDE" | "WTI" | "CL=F" => "CL=F",
        "VIX" | "^VIX" => "^VIX",
        // Interest-rate proxies (Yahoo yield tickers, ×10 in the raw series).
        "FEDFUNDS" | "RATE3M" | "T13W" | "US3M" | "IRX" | "^IRX" => "^IRX",
        "US5Y" | "RATE5Y" | "FVX" | "^FVX" => "^FVX",
        "US10Y" | "RATE10Y" | "TNX" | "^TNX" | "RATES" => "^TNX",
        "US30Y" | "RATE30Y" | "TYX" | "^TYX" => "^TYX",
        _ => return token.to_string(),
    };
    mapped.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MapLoader(HashMap<String, Vec<(String, f64)>>);
    impl SeriesLoader for MapLoader {
        fn load(&self, symbol: &str, _field: PriceField) -> Result<Vec<(String, f64)>> {
            Ok(self.0.get(symbol).cloned().unwrap_or_default())
        }
    }

    fn daily(start_day: u32, vals: &[f64]) -> Vec<(String, f64)> {
        vals.iter()
            .enumerate()
            .map(|(i, v)| (format!("2020-01-{:02}", start_day + i as u32), *v))
            .collect()
    }

    #[test]
    fn daily_field_aligns_one_to_one() {
        let raw = daily(1, &[10.0, 11.0, 12.0]);
        let dates: Vec<String> = raw.iter().map(|(d, _)| d.clone()).collect();
        let mut map = HashMap::new();
        map.insert("X".to_string(), raw);
        let loader = MapLoader(map);
        let mut r = Resolver::new(dates, "X", &loader);
        let s = r.field_series(None, PriceField::Close, Timeframe::Daily).unwrap();
        assert_eq!(s, vec![Some(10.0), Some(11.0), Some(12.0)]);
    }

    #[test]
    fn secondary_symbol_carries_forward_without_lookahead() {
        // master axis has 4 days; secondary only prints on day 2.
        let master: Vec<String> = (1..=4).map(|d| format!("2020-01-{:02}", d)).collect();
        let mut map = HashMap::new();
        map.insert("X".to_string(), daily(1, &[1.0, 2.0, 3.0, 4.0]));
        map.insert(
            "Y".to_string(),
            vec![("2020-01-02".to_string(), 99.0), ("2020-01-04".to_string(), 88.0)],
        );
        let loader = MapLoader(map);
        let mut r = Resolver::new(master, "X", &loader);
        let s = r.field_series(Some("Y"), PriceField::Close, Timeframe::Daily).unwrap();
        // day1: no Y datapoint yet -> None; day2: 99; day3: carry 99; day4: 88.
        assert_eq!(s, vec![None, Some(99.0), Some(99.0), Some(88.0)]);
    }

    #[test]
    fn weekly_bucket_uses_last_close_and_is_visible_after_week_end() {
        // Two ISO weeks of daily data. Master = same daily axis.
        let raw = vec![
            ("2020-01-06".to_string(), 1.0), // Mon wk2
            ("2020-01-10".to_string(), 5.0), // Fri wk2 (week close)
            ("2020-01-13".to_string(), 6.0), // Mon wk3
            ("2020-01-17".to_string(), 9.0), // Fri wk3 (week close)
        ];
        let dates: Vec<String> = raw.iter().map(|(d, _)| d.clone()).collect();
        let mut map = HashMap::new();
        map.insert("X".to_string(), raw);
        let loader = MapLoader(map);
        let mut r = Resolver::new(dates, "X", &loader);
        let s = r.field_series(None, PriceField::Close, Timeframe::Weekly).unwrap();
        // wk2 close (5.0) becomes visible on its end date 01-10 and carries
        // into wk3 until wk3 closes (9.0) on 01-17.
        assert_eq!(s, vec![None, Some(5.0), Some(5.0), Some(9.0)]);
    }

    #[test]
    fn alias_resolves_rate_symbols() {
        assert_eq!(resolve_alias("us10y"), "^TNX");
        assert_eq!(resolve_alias("fedfunds"), "^IRX");
        assert_eq!(resolve_alias("BTC"), "BTC-USD");
        assert_eq!(resolve_alias("UNKNOWN"), "UNKNOWN");
    }
}
