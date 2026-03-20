use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceQuote {
    pub symbol: String,
    pub price: Decimal,
    pub currency: String,
    pub source: String,
    pub fetched_at: String,
    /// Pre-market price (if available, US equities only)
    #[serde(default)]
    pub pre_market_price: Option<Decimal>,
    /// Post-market price (if available, US equities only)
    #[serde(default)]
    pub post_market_price: Option<Decimal>,
    /// Post-market change percentage (if available)
    #[serde(default)]
    pub post_market_change_percent: Option<Decimal>,
    /// Previous trading session close (from Yahoo `regularMarketPreviousClose`)
    #[serde(default)]
    pub previous_close: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub date: String,
    pub close: Decimal,
    /// Daily trading volume (None if unavailable, e.g. ratio charts)
    #[serde(default)]
    pub volume: Option<u64>,
    /// Open price (None if unavailable)
    #[serde(default)]
    pub open: Option<Decimal>,
    /// High price (None if unavailable)
    #[serde(default)]
    pub high: Option<Decimal>,
    /// Low price (None if unavailable)
    #[serde(default)]
    pub low: Option<Decimal>,
}

/// Maximum plausible single-day price change percentage.
///
/// Any daily change exceeding this absolute value is treated as a data anomaly
/// (e.g., stale/corrupt previous close, data source glitch) rather than a real
/// market move. Set at 500% to accommodate legitimate extreme moves (small-cap
/// halts, crypto flash crashes, post-split adjustments) while catching obvious
/// garbage like 224,000% changes from corrupt history data.
///
/// Used by movers, brief, and header ticker to filter anomalous data points.
pub const MAX_PLAUSIBLE_DAILY_CHANGE_PCT: Decimal = dec!(500);

/// Returns `true` if a daily change percentage is plausible (within ±500%).
///
/// Returns `false` for values that almost certainly indicate a data quality
/// issue rather than a real market move.
pub fn is_plausible_daily_change(pct: Decimal) -> bool {
    let abs_pct = if pct < dec!(0) { -pct } else { pct };
    abs_pct <= MAX_PLAUSIBLE_DAILY_CHANGE_PCT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausible_normal_change() {
        assert!(is_plausible_daily_change(dec!(5)));
        assert!(is_plausible_daily_change(dec!(-12)));
        assert!(is_plausible_daily_change(dec!(0)));
    }

    #[test]
    fn plausible_extreme_but_real() {
        // Crypto flash crash, small-cap halt — up to 500% is plausible
        assert!(is_plausible_daily_change(dec!(100)));
        assert!(is_plausible_daily_change(dec!(-90)));
        assert!(is_plausible_daily_change(dec!(499)));
        assert!(is_plausible_daily_change(dec!(500)));
    }

    #[test]
    fn implausible_data_anomaly() {
        assert!(!is_plausible_daily_change(dec!(501)));
        assert!(!is_plausible_daily_change(dec!(224632)));
        assert!(!is_plausible_daily_change(dec!(-1000)));
        assert!(!is_plausible_daily_change(dec!(99999)));
    }
}
