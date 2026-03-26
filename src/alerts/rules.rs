use anyhow::{bail, Result};
use rust_decimal::Decimal;
use std::str::FromStr;

use super::{AlertDirection, AlertKind};

/// Parsed result from a natural-language alert rule string.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRule {
    pub kind: AlertKind,
    pub symbol: String,
    pub direction: AlertDirection,
    pub threshold: Decimal,
    /// Reconstructed canonical rule text.
    pub rule_text: String,
}

/// Parse a natural-language alert rule string into structured components.
///
/// Supported formats:
///   Price:       "GC=F above 5500", "BTC below 55000", "TSLA below 300"
///   Allocation:  "gold allocation above 30%", "cash allocation below 30%"
///   Indicator:   "VIX above 25", "DXY above 100", "GC=F RSI below 30",
///                "BTC below SMA50", "AAPL MACD cross bullish", "ETH change above 5%"
///
/// Indicator detection: if the second token is a known indicator name (RSI, MACD, SMA, etc.),
/// it's treated as an indicator alert. Otherwise it's a price alert.
pub fn parse_rule(input: &str) -> Result<ParsedRule> {
    let input = input.trim();
    if input.is_empty() {
        bail!("Empty alert rule");
    }

    let tokens: Vec<&str> = input.split_whitespace().collect();
    if tokens.len() < 3 {
        bail!(
            "Invalid rule format: expected '<symbol> <above|below> <value>', got: {}",
            input
        );
    }

    // Check for ratio rule: "GC=F/CL=F above 30" or "GC=F / CL=F above 30"
    if let Some(rule) = try_parse_ratio_rule(&tokens, input) {
        return Ok(rule);
    }

    // Check for allocation rule: "<category> allocation above/below <pct>%"
    if tokens.len() >= 4 && tokens[1].eq_ignore_ascii_case("allocation") {
        return parse_allocation_rule(&tokens, input);
    }

    // Alternate SMA syntax: "<symbol> above|below SMA50"
    if tokens.len() == 3 && is_sma_period(tokens[2]) {
        return parse_sma_cross_rule(&tokens, input);
    }

    // MACD cross syntax: "<symbol> MACD cross bullish|bearish"
    if tokens.len() == 4
        && tokens[1].eq_ignore_ascii_case("MACD")
        && tokens[2].eq_ignore_ascii_case("cross")
    {
        return parse_macd_cross_rule(&tokens, input);
    }

    // Daily change syntax: "<symbol> change above|below 5%"
    if tokens.len() == 4 && tokens[1].eq_ignore_ascii_case("change") {
        return parse_change_rule(&tokens, input);
    }

    // Check for indicator rule: "<symbol> <INDICATOR> above/below <value>"
    if tokens.len() >= 4 && is_indicator_name(tokens[1]) {
        return parse_indicator_rule(&tokens, input);
    }

    // Default: price rule "<symbol> above/below <value>"
    parse_price_rule(&tokens, input)
}

fn parse_price_rule(tokens: &[&str], original: &str) -> Result<ParsedRule> {
    if tokens.len() < 3 {
        bail!(
            "Price rule needs at least 3 tokens: '<symbol> <above|below> <value>', got: {}",
            original
        );
    }

    let symbol = tokens[0].to_uppercase();
    let direction: AlertDirection = tokens[1].parse().map_err(|_| {
        anyhow::anyhow!(
            "Expected 'above' or 'below' after symbol, got '{}' in: {}",
            tokens[1],
            original
        )
    })?;
    let threshold = parse_threshold(tokens[2])?;
    let rule_text = format!("{} {} {}", symbol, direction, threshold);

    Ok(ParsedRule {
        kind: AlertKind::Price,
        symbol,
        direction,
        threshold,
        rule_text,
    })
}

fn parse_allocation_rule(tokens: &[&str], original: &str) -> Result<ParsedRule> {
    // Format: "<category> allocation above/below <pct>%"
    if tokens.len() < 4 {
        bail!(
            "Allocation rule needs: '<category> allocation <above|below> <pct>%', got: {}",
            original
        );
    }

    let category = tokens[0].to_lowercase();
    let direction: AlertDirection = tokens[2].parse().map_err(|_| {
        anyhow::anyhow!(
            "Expected 'above' or 'below' after 'allocation', got '{}' in: {}",
            tokens[2],
            original
        )
    })?;
    let threshold = parse_threshold(tokens[3])?;
    let rule_text = format!("{} allocation {} {}%", category, direction, threshold);

    Ok(ParsedRule {
        kind: AlertKind::Allocation,
        symbol: category,
        direction,
        threshold,
        rule_text,
    })
}

fn parse_indicator_rule(tokens: &[&str], original: &str) -> Result<ParsedRule> {
    // Format: "<symbol> <INDICATOR> above/below <value>"
    if tokens.len() < 4 {
        bail!(
            "Indicator rule needs: '<symbol> <indicator> <above|below> <value>', got: {}",
            original
        );
    }

    let symbol = tokens[0].to_uppercase();
    let indicator = tokens[1].to_uppercase();
    let direction: AlertDirection = tokens[2].parse().map_err(|_| {
        anyhow::anyhow!(
            "Expected 'above' or 'below' after indicator, got '{}' in: {}",
            tokens[2],
            original
        )
    })?;
    let threshold = parse_threshold(tokens[3])?;
    // Store the combined "SYMBOL INDICATOR" as the symbol for indicator alerts
    let composite_symbol = format!("{} {}", symbol, indicator);
    let rule_text = format!("{} {} {} {}", symbol, indicator, direction, threshold);

    Ok(ParsedRule {
        kind: AlertKind::Indicator,
        symbol: composite_symbol,
        direction,
        threshold,
        rule_text,
    })
}

fn parse_sma_cross_rule(tokens: &[&str], original: &str) -> Result<ParsedRule> {
    let symbol = tokens[0].to_uppercase();
    let direction: AlertDirection = tokens[1].parse().map_err(|_| {
        anyhow::anyhow!(
            "Expected 'above' or 'below' before SMA period, got '{}' in: {}",
            tokens[1],
            original
        )
    })?;
    let period = parse_sma_period(tokens[2])?;
    let composite_symbol = format!("{} SMA{}", symbol, period);
    let rule_text = format!("{} {} SMA{}", symbol, direction, period);

    Ok(ParsedRule {
        kind: AlertKind::Indicator,
        symbol: composite_symbol,
        direction,
        threshold: Decimal::from(period),
        rule_text,
    })
}

fn parse_macd_cross_rule(tokens: &[&str], original: &str) -> Result<ParsedRule> {
    let symbol = tokens[0].to_uppercase();
    let direction = match tokens[3].to_ascii_lowercase().as_str() {
        "bullish" => AlertDirection::Above,
        "bearish" => AlertDirection::Below,
        other => {
            bail!(
                "Expected 'bullish' or 'bearish' after 'MACD cross', got '{}' in: {}",
                other,
                original
            )
        }
    };
    let rule_text = format!(
        "{} MACD cross {}",
        symbol,
        if direction == AlertDirection::Above {
            "bullish"
        } else {
            "bearish"
        }
    );

    Ok(ParsedRule {
        kind: AlertKind::Indicator,
        symbol: format!("{} MACD_CROSS", symbol),
        direction,
        threshold: Decimal::ZERO,
        rule_text,
    })
}

fn parse_change_rule(tokens: &[&str], original: &str) -> Result<ParsedRule> {
    let symbol = tokens[0].to_uppercase();
    let direction: AlertDirection = tokens[2].parse().map_err(|_| {
        anyhow::anyhow!(
            "Expected 'above' or 'below' after 'change', got '{}' in: {}",
            tokens[2],
            original
        )
    })?;
    let threshold = parse_threshold(tokens[3])?;
    let rule_text = format!("{} change {} {}%", symbol, direction, threshold);

    Ok(ParsedRule {
        kind: AlertKind::Indicator,
        symbol: format!("{} CHANGE_PCT", symbol),
        direction,
        threshold,
        rule_text,
    })
}

/// Try to parse a ratio alert rule. Returns None if the input doesn't look like a ratio rule.
///
/// Supported formats:
///   "GC=F/CL=F above 30"       — compact slash
///   "GC=F / CL=F above 30"     — spaced slash
///   "ITA/SPY below 1.2"        — decimal threshold
fn try_parse_ratio_rule(tokens: &[&str], _original: &str) -> Option<ParsedRule> {
    // Pattern 1: compact "A/B above N" (3 tokens, first contains '/')
    if tokens.len() >= 3 && tokens[0].contains('/') && !tokens[0].starts_with('/') && !tokens[0].ends_with('/') {
        let (numerator, denominator) = tokens[0].split_once('/')?;
        if numerator.is_empty() || denominator.is_empty() {
            return None;
        }
        let direction: AlertDirection = tokens[1].parse().ok()?;
        let threshold = parse_threshold(tokens[2]).ok()?;
        let symbol = format!("{}/{}", numerator.to_uppercase(), denominator.to_uppercase());
        let rule_text = format!("{} {} {}", symbol, direction, threshold);
        return Some(ParsedRule {
            kind: AlertKind::Ratio,
            symbol,
            direction,
            threshold,
            rule_text,
        });
    }

    // Pattern 2: spaced "A / B above N" (4+ tokens, token[1] is "/")
    if tokens.len() >= 4 && tokens[1] == "/" {
        let numerator = tokens[0];
        let denominator = tokens[2];
        if numerator.is_empty() || denominator.is_empty() {
            return None;
        }
        if tokens.len() < 5 {
            return None;
        }
        let direction: AlertDirection = tokens[3].parse().ok()?;
        let threshold = parse_threshold(tokens[4]).ok()?;
        let symbol = format!("{}/{}", numerator.to_uppercase(), denominator.to_uppercase());
        let rule_text = format!("{} {} {}", symbol, direction, threshold);
        return Some(ParsedRule {
            kind: AlertKind::Ratio,
            symbol,
            direction,
            threshold,
            rule_text,
        });
    }

    None
}

/// Parse a threshold value, stripping optional trailing '%', '$', or ','.
fn parse_threshold(s: &str) -> Result<Decimal> {
    let cleaned = s.replace(['%', '$', ','], "");
    Decimal::from_str(&cleaned)
        .map_err(|e| anyhow::anyhow!("Invalid threshold value '{}': {}", s, e))
}

/// Known indicator names for detecting indicator-type alerts.
fn is_indicator_name(s: &str) -> bool {
    matches!(
        s.to_uppercase().as_str(),
        "RSI" | "MACD" | "SMA" | "EMA" | "BB" | "BOLLINGER" | "ATR" | "ADX"
    )
}

fn is_sma_period(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    upper.starts_with("SMA") && parse_sma_period(&upper).is_ok()
}

fn parse_sma_period(s: &str) -> Result<u32> {
    let upper = s.to_ascii_uppercase();
    let period = upper
        .strip_prefix("SMA")
        .ok_or_else(|| anyhow::anyhow!("Expected SMA period like SMA50, got '{}'", s))?;
    let parsed: u32 = period
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid SMA period '{}'", s))?;
    if parsed == 0 {
        bail!("SMA period must be positive");
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_parse_price_above() {
        let rule = parse_rule("GC=F above 5500").unwrap();
        assert_eq!(rule.kind, AlertKind::Price);
        assert_eq!(rule.symbol, "GC=F");
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.threshold, dec!(5500));
    }

    #[test]
    fn test_parse_price_below() {
        let rule = parse_rule("BTC below 55000").unwrap();
        assert_eq!(rule.kind, AlertKind::Price);
        assert_eq!(rule.symbol, "BTC");
        assert_eq!(rule.direction, AlertDirection::Below);
        assert_eq!(rule.threshold, dec!(55000));
    }

    #[test]
    fn test_parse_price_with_decimal() {
        let rule = parse_rule("TSLA below 300.50").unwrap();
        assert_eq!(rule.kind, AlertKind::Price);
        assert_eq!(rule.symbol, "TSLA");
        assert_eq!(rule.threshold, dec!(300.50));
    }

    #[test]
    fn test_parse_allocation_above() {
        let rule = parse_rule("gold allocation above 30%").unwrap();
        assert_eq!(rule.kind, AlertKind::Allocation);
        assert_eq!(rule.symbol, "gold");
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.threshold, dec!(30));
    }

    #[test]
    fn test_parse_allocation_below() {
        let rule = parse_rule("cash allocation below 10%").unwrap();
        assert_eq!(rule.kind, AlertKind::Allocation);
        assert_eq!(rule.symbol, "cash");
        assert_eq!(rule.direction, AlertDirection::Below);
        assert_eq!(rule.threshold, dec!(10));
    }

    #[test]
    fn test_parse_indicator_rsi() {
        let rule = parse_rule("GC=F RSI below 30").unwrap();
        assert_eq!(rule.kind, AlertKind::Indicator);
        assert_eq!(rule.symbol, "GC=F RSI");
        assert_eq!(rule.direction, AlertDirection::Below);
        assert_eq!(rule.threshold, dec!(30));
    }

    #[test]
    fn test_parse_indicator_vix() {
        // VIX is not an indicator name, so "VIX above 25" is a price alert
        let rule = parse_rule("VIX above 25").unwrap();
        assert_eq!(rule.kind, AlertKind::Price);
        assert_eq!(rule.symbol, "VIX");
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.threshold, dec!(25));
    }

    #[test]
    fn test_parse_indicator_macd() {
        let rule = parse_rule("AAPL MACD above 0").unwrap();
        assert_eq!(rule.kind, AlertKind::Indicator);
        assert_eq!(rule.symbol, "AAPL MACD");
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.threshold, dec!(0));
    }

    #[test]
    fn test_parse_sma_cross_rule() {
        let rule = parse_rule("BTC below SMA50").unwrap();
        assert_eq!(rule.kind, AlertKind::Indicator);
        assert_eq!(rule.symbol, "BTC SMA50");
        assert_eq!(rule.direction, AlertDirection::Below);
        assert_eq!(rule.threshold, dec!(50));
    }

    #[test]
    fn test_parse_macd_cross_rule() {
        let rule = parse_rule("AAPL MACD cross bullish").unwrap();
        assert_eq!(rule.kind, AlertKind::Indicator);
        assert_eq!(rule.symbol, "AAPL MACD_CROSS");
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.threshold, dec!(0));
    }

    #[test]
    fn test_parse_change_rule() {
        let rule = parse_rule("ETH change above 5%").unwrap();
        assert_eq!(rule.kind, AlertKind::Indicator);
        assert_eq!(rule.symbol, "ETH CHANGE_PCT");
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.threshold, dec!(5));
    }

    #[test]
    fn test_parse_with_comma_in_value() {
        let rule = parse_rule("BTC above 100,000").unwrap();
        assert_eq!(rule.threshold, dec!(100000));
    }

    #[test]
    fn test_parse_with_dollar_sign() {
        let rule = parse_rule("AAPL above $200").unwrap();
        assert_eq!(rule.threshold, dec!(200));
    }

    #[test]
    fn test_parse_empty_input() {
        assert!(parse_rule("").is_err());
    }

    #[test]
    fn test_parse_too_few_tokens() {
        assert!(parse_rule("GC=F").is_err());
        assert!(parse_rule("GC=F above").is_err());
    }

    #[test]
    fn test_parse_invalid_direction() {
        assert!(parse_rule("GC=F sideways 5500").is_err());
    }

    #[test]
    fn test_parse_invalid_threshold() {
        assert!(parse_rule("GC=F above notanumber").is_err());
    }

    #[test]
    fn test_parse_case_insensitive_direction() {
        let rule = parse_rule("GC=F ABOVE 5500").unwrap();
        assert_eq!(rule.direction, AlertDirection::Above);
        let rule = parse_rule("BTC Below 50000").unwrap();
        assert_eq!(rule.direction, AlertDirection::Below);
    }

    #[test]
    fn test_parse_whitespace_handling() {
        let rule = parse_rule("  GC=F   above   5500  ").unwrap();
        assert_eq!(rule.symbol, "GC=F");
        assert_eq!(rule.threshold, dec!(5500));
    }

    #[test]
    fn test_parse_ratio_compact() {
        let rule = parse_rule("GC=F/CL=F above 30").unwrap();
        assert_eq!(rule.kind, AlertKind::Ratio);
        assert_eq!(rule.symbol, "GC=F/CL=F");
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.threshold, dec!(30));
    }

    #[test]
    fn test_parse_ratio_below() {
        let rule = parse_rule("ITA/SPY below 1.2").unwrap();
        assert_eq!(rule.kind, AlertKind::Ratio);
        assert_eq!(rule.symbol, "ITA/SPY");
        assert_eq!(rule.direction, AlertDirection::Below);
        assert_eq!(rule.threshold, dec!(1.2));
    }

    #[test]
    fn test_parse_ratio_spaced() {
        let rule = parse_rule("GC=F / CL=F above 30").unwrap();
        assert_eq!(rule.kind, AlertKind::Ratio);
        assert_eq!(rule.symbol, "GC=F/CL=F");
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.threshold, dec!(30));
    }

    #[test]
    fn test_parse_ratio_decimal_threshold() {
        let rule = parse_rule("BTC/ETH above 15.5").unwrap();
        assert_eq!(rule.kind, AlertKind::Ratio);
        assert_eq!(rule.symbol, "BTC/ETH");
        assert_eq!(rule.threshold, dec!(15.5));
    }
}
