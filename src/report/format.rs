//! Shared display formatting for report output.
//!
//! Centralises money/percentage formatting so every section renders numbers
//! the same way. The headline behaviour callers want is **thousands
//! separators** — `$346,838` rather than `$346837.6` — which historically was
//! missing because each renderer formatted ad hoc.

use rust_decimal::Decimal;

/// Insert thousands separators into the integer part of an already-rendered
/// numeric string. Preserves a leading `-`, any fractional part, and any
/// non-numeric suffix is left untouched (callers pass plain decimal strings).
pub fn group_thousands(s: &str) -> String {
    let neg = s.starts_with('-');
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    // Only group if the integer part is purely digits (defensive).
    if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return s.to_string();
    }
    let len = int_part.len();
    let mut grouped = String::with_capacity(len + len / 3);
    for (idx, ch) in int_part.chars().enumerate() {
        if idx > 0 && (len - idx) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&grouped);
    if let Some(f) = frac_part {
        out.push('.');
        out.push_str(f);
    }
    out
}

/// Money with a leading `$`, grouped thousands, trailing zeros normalised.
pub fn fmt_money(d: Decimal) -> String {
    format!("${}", group_thousands(&d.round_dp(2).normalize().to_string()))
}

/// Signed money delta: leading `+` for non-negative, `-` for negative, with a
/// `$` and grouped thousands.
pub fn fmt_signed_money(d: Decimal) -> String {
    let rounded = d.round_dp(2);
    if rounded.is_sign_negative() {
        format!("-${}", group_thousands(&rounded.abs().normalize().to_string()))
    } else {
        format!("+${}", group_thousands(&rounded.normalize().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn groups_thousands() {
        assert_eq!(group_thousands("346837.6"), "346,837.6");
        assert_eq!(group_thousands("4365.3"), "4,365.3");
        assert_eq!(group_thousands("999"), "999");
        assert_eq!(group_thousands("1000"), "1,000");
        assert_eq!(group_thousands("1234567"), "1,234,567");
        assert_eq!(group_thousands("-1234567.89"), "-1,234,567.89");
        assert_eq!(group_thousands("0"), "0");
    }

    #[test]
    fn fmt_money_adds_separators_and_dollar() {
        assert_eq!(fmt_money(dec!(346837.6)), "$346,837.6");
        assert_eq!(fmt_money(dec!(61667)), "$61,667");
        assert_eq!(fmt_money(dec!(4365.30)), "$4,365.3");
        assert_eq!(fmt_money(dec!(1)), "$1");
    }

    #[test]
    fn fmt_signed_money_signs_and_groups() {
        assert_eq!(fmt_signed_money(dec!(12500)), "+$12,500");
        assert_eq!(fmt_signed_money(dec!(-8915.67)), "-$8,915.67");
        assert_eq!(fmt_signed_money(dec!(0)), "+$0");
    }

    #[test]
    fn non_numeric_passthrough_is_safe() {
        assert_eq!(group_thousands("n/a"), "n/a");
        assert_eq!(group_thousands(""), "");
    }
}
