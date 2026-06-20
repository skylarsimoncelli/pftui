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

use super::parser::{IndicatorKind, OhlcKind, PriceField, Timeframe};
use crate::indicators::atr::compute_atr;
use crate::indicators::bollinger::compute_bollinger;
use crate::indicators::{
    compute_adx, compute_cci, compute_ema, compute_fisher, compute_macd, compute_mfi, compute_obv,
    compute_roc, compute_rsi, compute_sma, compute_stochastic, compute_supertrend,
    compute_williams_r,
};

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

/// Aligned OHLCV bars at a timeframe (one entry per completed bucket).
#[derive(Default)]
struct OhlcBars {
    end_date: Vec<String>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    vol: Vec<f64>,
    /// Per-bucket: did REAL high AND low exist (vs close-substituted fallback)?
    hl_real: Vec<bool>,
    /// Per-bucket: was there REAL (>0) volume?
    vol_real: Vec<bool>,
}

impl OhlcBars {
    /// Fraction of buckets with real high+low data (1.0 = fully populated).
    fn hl_coverage(&self) -> f64 {
        if self.hl_real.is_empty() {
            return 0.0;
        }
        self.hl_real.iter().filter(|b| **b).count() as f64 / self.hl_real.len() as f64
    }
    /// Fraction of buckets with real (>0) volume.
    fn vol_coverage(&self) -> f64 {
        if self.vol_real.is_empty() {
            return 0.0;
        }
        self.vol_real.iter().filter(|b| **b).count() as f64 / self.vol_real.len() as f64
    }
}

/// Minimum data coverage before an OHLC-family indicator is trusted in the DSL.
/// Mirrors the indicator-panel guard: below this, range indicators are computed
/// on close-collapsed bars (or money-flow on a near-empty volume series) and
/// would print confident garbage — so the DSL resolves them to all-`None`
/// instead of feeding false signals into a backtest.
const OHLC_COVERAGE_MIN: f64 = 0.8;

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

    /// The primary asset's FULL close history (oldest→newest), independent of
    /// the `--from`/`--to` master-axis window. Used so vol-targeting can warm
    /// up its realized-vol estimate on all available history rather than
    /// silently neutralizing leverage on trades near a window start.
    pub fn primary_close_history(&mut self) -> Result<Vec<(String, f64)>> {
        let sym = self.primary_symbol.clone();
        Ok(self.raw(&sym, PriceField::Close)?.to_vec())
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

    /// Like [`raw`](Self::raw) but does NOT flag an empty result as a missing
    /// symbol. Used for the secondary OHLC fields (high/low/volume), which are
    /// legitimately absent for many series and fall back to close / zero — only
    /// a missing CLOSE means a bad symbol.
    fn raw_untracked(&mut self, symbol: &str, field: PriceField) -> Result<Vec<(String, f64)>> {
        let resolved = resolve_alias(symbol);
        let key = (resolved.clone(), field);
        if let Some(v) = self.raw_cache.get(&key) {
            return Ok(v.clone());
        }
        let series = self.loader.load(&resolved, field)?;
        self.raw_cache.insert(key, series.clone());
        Ok(series)
    }

    /// Assemble a symbol's OHLCV bars at the requested timeframe. Missing
    /// high/low fall back to that bar's close; missing volume is zero. Weekly /
    /// monthly buckets aggregate correctly (high=max, low=min, close=last,
    /// volume=sum) — unlike the last-value bucketing used for plain fields.
    fn ohlc_bars(&mut self, symbol: Option<&str>, tf: Timeframe) -> Result<OhlcBars> {
        let sym = symbol
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.primary_symbol.clone());
        // Close is the existence proxy and the bar axis (tracked).
        let close_raw = self.raw(&sym, PriceField::Close)?.to_vec();
        let high_raw = self.raw_untracked(&sym, PriceField::High)?;
        let low_raw = self.raw_untracked(&sym, PriceField::Low)?;
        let vol_raw = self.raw_untracked(&sym, PriceField::Volume)?;
        let hmap: HashMap<&str, f64> = high_raw.iter().map(|(d, v)| (d.as_str(), *v)).collect();
        let lmap: HashMap<&str, f64> = low_raw.iter().map(|(d, v)| (d.as_str(), *v)).collect();
        let vmap: HashMap<&str, f64> = vol_raw.iter().map(|(d, v)| (d.as_str(), *v)).collect();

        let mut bars = OhlcBars::default();
        let mut cur_key: Option<(i32, u32)> = None;
        for (d, c) in &close_raw {
            let real_h = hmap.get(d.as_str()).copied();
            let real_l = lmap.get(d.as_str()).copied();
            let h = real_h.unwrap_or(*c);
            let l = real_l.unwrap_or(*c);
            let v = vmap.get(d.as_str()).copied().unwrap_or(0.0);
            let hl_real = real_h.is_some() && real_l.is_some();
            let vol_real = v > 0.0;
            let date = NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("bad date in series: {d}"))?;
            let key = bucket_key(date, tf);
            match cur_key {
                Some(k) if k == key => {
                    // Extend the open bucket.
                    let i = bars.close.len() - 1;
                    bars.high[i] = bars.high[i].max(h);
                    bars.low[i] = bars.low[i].min(l);
                    bars.close[i] = *c;
                    bars.vol[i] += v;
                    bars.end_date[i] = d.clone();
                    // A bucket counts as real if ANY contributing day was real.
                    bars.hl_real[i] |= hl_real;
                    bars.vol_real[i] |= vol_real;
                }
                _ => {
                    bars.end_date.push(d.clone());
                    bars.high.push(h);
                    bars.low.push(l);
                    bars.close.push(*c);
                    bars.vol.push(v);
                    bars.hl_real.push(hl_real);
                    bars.vol_real.push(vol_real);
                    cur_key = Some(key);
                }
            }
        }
        Ok(bars)
    }

    /// Resolve an OHLC-family indicator to a daily-aligned series. Computed at
    /// the requested timeframe's bucket granularity then projected to daily.
    /// Memoized like the other indicator paths.
    pub fn ohlc_indicator_series(
        &mut self,
        kind: OhlcKind,
        symbol: Option<&str>,
        params: &[f64],
        tf: Timeframe,
    ) -> Result<Vec<Option<f64>>> {
        let cache_key = format!("O:{}:{:?}:{:?}:{:?}", symbol.unwrap_or("@"), kind, params, tf);
        if let Some(v) = self.series_cache.get(&cache_key) {
            return Ok(v.clone());
        }
        let bars = self.ohlc_bars(symbol, tf)?;
        // Coverage gate: don't compute indicators on degenerate inputs (a
        // close-collapsed series for range indicators, or a near-empty volume
        // series for money-flow) — they'd feed false signals into a backtest.
        // Below threshold the whole series resolves to None so no condition
        // fires on it. Close-only indicators (roc/macd/bb_*) are exempt.
        let needs_hl = matches!(
            kind,
            OhlcKind::Atr
                | OhlcKind::Cci
                | OhlcKind::WilliamsR
                | OhlcKind::Fisher
                | OhlcKind::Mfi
                | OhlcKind::StochK
                | OhlcKind::StochD
                | OhlcKind::Adx
                | OhlcKind::PlusDi
                | OhlcKind::MinusDi
                | OhlcKind::Supertrend
                | OhlcKind::SupertrendDir
        );
        let needs_vol = matches!(kind, OhlcKind::Obv | OhlcKind::Mfi);
        if (needs_hl && bars.hl_coverage() < OHLC_COVERAGE_MIN)
            || (needs_vol && bars.vol_coverage() < OHLC_COVERAGE_MIN)
        {
            let out = vec![None; self.master_dates.len()];
            self.series_cache.insert(cache_key, out.clone());
            return Ok(out);
        }
        let p0 = params.first().map(|p| *p as usize).unwrap_or(0);
        let h_opt: Vec<Option<f64>> = bars.high.iter().map(|v| Some(*v)).collect();
        let l_opt: Vec<Option<f64>> = bars.low.iter().map(|v| Some(*v)).collect();
        let computed: Vec<Option<f64>> = match kind {
            OhlcKind::Atr => compute_atr(&h_opt, &l_opt, &bars.close, p0),
            OhlcKind::Cci => compute_cci(&bars.high, &bars.low, &bars.close, p0),
            OhlcKind::WilliamsR => compute_williams_r(&bars.high, &bars.low, &bars.close, p0),
            OhlcKind::Roc => compute_roc(&bars.close, p0),
            OhlcKind::Fisher => compute_fisher(&bars.high, &bars.low, p0),
            OhlcKind::Supertrend | OhlcKind::SupertrendDir => {
                let mult = params.get(1).copied().unwrap_or(3.0);
                let st = compute_supertrend(&bars.high, &bars.low, &bars.close, p0, mult);
                st.iter()
                    .map(|o| {
                        o.map(|r| {
                            if matches!(kind, OhlcKind::SupertrendDir) {
                                r.dir as f64
                            } else {
                                r.line
                            }
                        })
                    })
                    .collect()
            }
            OhlcKind::Mfi => compute_mfi(&bars.high, &bars.low, &bars.close, &bars.vol, p0),
            OhlcKind::Obv => compute_obv(&bars.close, &bars.vol),
            OhlcKind::StochK | OhlcKind::StochD => {
                let k = params.first().map(|p| *p as usize).unwrap_or(0);
                let d = params.get(1).map(|p| *p as usize).unwrap_or(0);
                let s = compute_stochastic(&bars.high, &bars.low, &bars.close, k, d);
                s.iter()
                    .map(|o| o.map(|r| if matches!(kind, OhlcKind::StochK) { r.k } else { r.d }))
                    .collect()
            }
            OhlcKind::Adx | OhlcKind::PlusDi | OhlcKind::MinusDi => {
                let a = compute_adx(&bars.high, &bars.low, &bars.close, p0);
                a.iter()
                    .map(|o| {
                        o.map(|r| match kind {
                            OhlcKind::PlusDi => r.plus_di,
                            OhlcKind::MinusDi => r.minus_di,
                            _ => r.adx,
                        })
                    })
                    .collect()
            }
            OhlcKind::MacdLine | OhlcKind::MacdSignal | OhlcKind::MacdHist => {
                let f = params.first().map(|p| *p as usize).unwrap_or(0);
                let s = params.get(1).map(|p| *p as usize).unwrap_or(0);
                let sig = params.get(2).map(|p| *p as usize).unwrap_or(0);
                let m = compute_macd(&bars.close, f, s, sig);
                m.iter()
                    .map(|o| {
                        o.map(|r| match kind {
                            OhlcKind::MacdLine => r.macd,
                            OhlcKind::MacdSignal => r.signal,
                            _ => r.histogram,
                        })
                    })
                    .collect()
            }
            OhlcKind::BbUpper | OhlcKind::BbLower | OhlcKind::BbMid | OhlcKind::BbPct => {
                let mult = params.get(1).copied().unwrap_or(2.0);
                let b = compute_bollinger(&bars.close, p0, mult);
                b.iter()
                    .enumerate()
                    .map(|(i, o)| {
                        o.map(|bb| match kind {
                            OhlcKind::BbUpper => bb.upper,
                            OhlcKind::BbLower => bb.lower,
                            OhlcKind::BbMid => bb.middle,
                            // %b = (close − lower) / (upper − lower).
                            _ => {
                                let range = bb.upper - bb.lower;
                                if range > 0.0 {
                                    (bars.close[i] - bb.lower) / range
                                } else {
                                    0.5
                                }
                            }
                        })
                    })
                    .collect()
            }
        };
        let out = project(&bars.end_date, &computed, &self.master_dates);
        self.series_cache.insert(cache_key, out.clone());
        Ok(out)
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

    /// A loader where high/low/volume can be independently absent, to exercise
    /// the OHLC coverage gate.
    struct FieldLoader {
        close: Vec<(String, f64)>,
        has_hl: bool,
        has_vol: bool,
    }
    impl SeriesLoader for FieldLoader {
        fn load(&self, _symbol: &str, field: PriceField) -> Result<Vec<(String, f64)>> {
            Ok(match field {
                PriceField::Close => self.close.clone(),
                PriceField::High | PriceField::Low if self.has_hl => self.close.clone(),
                PriceField::Volume if self.has_vol => {
                    self.close.iter().map(|(d, _)| (d.clone(), 1000.0)).collect()
                }
                _ => Vec::new(),
            })
        }
    }

    #[test]
    fn ohlc_coverage_gate_suppresses_degenerate_inputs() {
        // 40 sequential valid calendar dates (spanning month boundaries).
        let base = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let close: Vec<(String, f64)> = (0..40)
            .map(|i| {
                let d = base + chrono::Duration::days(i as i64);
                (d.format("%Y-%m-%d").to_string(), 100.0 + i as f64)
            })
            .collect();
        let dates: Vec<String> = close.iter().map(|(d, _)| d.clone()).collect();

        // Volume-less series: mfi/obv suppressed to all-None; a close-only
        // indicator (roc) still resolves; adx (needs H/L, which IS present) too.
        let loader = FieldLoader { close: close.clone(), has_hl: true, has_vol: false };
        let mut r = Resolver::new(dates.clone(), "X", &loader);
        let mfi = r.ohlc_indicator_series(OhlcKind::Mfi, None, &[14.0], Timeframe::Daily).unwrap();
        assert!(mfi.iter().all(|x| x.is_none()), "mfi suppressed on no-volume series");
        let obv = r.ohlc_indicator_series(OhlcKind::Obv, None, &[], Timeframe::Daily).unwrap();
        assert!(obv.iter().all(|x| x.is_none()), "obv suppressed on no-volume series");
        let adx = r.ohlc_indicator_series(OhlcKind::Adx, None, &[14.0], Timeframe::Daily).unwrap();
        assert!(adx.iter().any(|x| x.is_some()), "adx computes when H/L present");

        // Close-only series (no high/low): range indicators suppressed; roc (close-only) survives.
        let loader2 = FieldLoader { close: close.clone(), has_hl: false, has_vol: false };
        let mut r2 = Resolver::new(dates, "X", &loader2);
        let adx2 = r2.ohlc_indicator_series(OhlcKind::Adx, None, &[14.0], Timeframe::Daily).unwrap();
        assert!(adx2.iter().all(|x| x.is_none()), "adx suppressed on close-collapsed series");
        let roc = r2.ohlc_indicator_series(OhlcKind::Roc, None, &[10.0], Timeframe::Daily).unwrap();
        assert!(roc.iter().any(|x| x.is_some()), "roc (close-only) survives without H/L");
    }

    #[test]
    fn alias_resolves_rate_symbols() {
        assert_eq!(resolve_alias("us10y"), "^TNX");
        assert_eq!(resolve_alias("fedfunds"), "^IRX");
        assert_eq!(resolve_alias("BTC"), "BTC-USD");
        assert_eq!(resolve_alias("UNKNOWN"), "UNKNOWN");
    }
}
