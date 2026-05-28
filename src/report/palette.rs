#![allow(dead_code)]

use crate::models::asset::AssetCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportPalette {
    pub bg: &'static str,
    pub bg_alt: &'static str,
    pub panel: &'static str,
    pub border: &'static str,
    pub text: &'static str,
    pub muted: &'static str,
    pub muted2: &'static str,
    pub bull: &'static str,
    pub bear: &'static str,
    pub amber: &'static str,
    pub neutral: &'static str,
    pub cyan: &'static str,
    pub blue: &'static str,
    pub mauve: &'static str,
    pub yellow: &'static str,
    pub cash: &'static str,
    pub crypto: &'static str,
    pub gold: &'static str,
    pub silver: &'static str,
    pub equity: &'static str,
}

pub const DARK: ReportPalette = ReportPalette {
    bg: "#0d1117",
    bg_alt: "#161b22",
    panel: "#1c2128",
    border: "#30363d",
    text: "#c9d1d9",
    muted: "#6e7681",
    muted2: "#484f58",
    bull: "#a6e3a1",
    bear: "#f38ba8",
    amber: "#fab387",
    neutral: "#a6adc8",
    cyan: "#89dceb",
    blue: "#89b4fa",
    mauve: "#cba6f7",
    yellow: "#f9e2af",
    cash: "#6e7681",
    crypto: "#fab387",
    gold: "#f9e2af",
    silver: "#bac2de",
    equity: "#89b4fa",
};

pub const LIGHT: ReportPalette = ReportPalette {
    bg: "#ffffff",
    bg_alt: "#f6f8fa",
    panel: "#eef2f6",
    border: "#d0d7de",
    text: "#24292f",
    muted: "#57606a",
    muted2: "#8c959f",
    bull: "#1a7f37",
    bear: "#cf222e",
    amber: "#bf8700",
    neutral: "#57606a",
    cyan: "#0969da",
    blue: "#0969da",
    mauve: "#8250df",
    yellow: "#9a6700",
    cash: "#57606a",
    crypto: "#bc4c00",
    gold: "#9a6700",
    silver: "#6e7781",
    equity: "#0969da",
};

pub fn token(name: &str) -> Option<&'static str> {
    match normalize_token(name).as_str() {
        "bg" => Some(DARK.bg),
        "bg_alt" | "bg-alt" => Some(DARK.bg_alt),
        "panel" => Some(DARK.panel),
        "border" => Some(DARK.border),
        "text" => Some(DARK.text),
        "muted" => Some(DARK.muted),
        "muted2" | "muted-2" => Some(DARK.muted2),
        "bull" | "green" => Some(DARK.bull),
        "bear" | "red" => Some(DARK.bear),
        "amber" | "orange" => Some(DARK.amber),
        "neutral" | "gray" | "grey" => Some(DARK.neutral),
        "cyan" => Some(DARK.cyan),
        "blue" => Some(DARK.blue),
        "mauve" | "purple" => Some(DARK.mauve),
        "yellow" => Some(DARK.yellow),
        "cash" | "slate" => Some(DARK.cash),
        "crypto" => Some(DARK.crypto),
        "gold" => Some(DARK.gold),
        "silver" => Some(DARK.silver),
        "equity" | "stocks" => Some(DARK.equity),
        _ => None,
    }
}

pub fn color_or_raw(value: &str) -> String {
    token(value).unwrap_or(value).to_string()
}

pub fn asset_color(symbol: &str, category: AssetCategory) -> &'static str {
    let symbol = symbol.trim().to_ascii_uppercase();
    match symbol.as_str() {
        "USD" | "GBP" | "EUR" | "CAD" | "AUD" | "JPY" => DARK.cash,
        "BTC" | "BTC-USD" | "ETH" | "ETH-USD" => DARK.crypto,
        "GC=F" | "XAU" | "XAUUSD" | "GOLD" => DARK.gold,
        "SI=F" | "XAG" | "XAGUSD" | "SILVER" => DARK.silver,
        _ => match category {
            AssetCategory::Cash => DARK.cash,
            AssetCategory::Crypto => DARK.crypto,
            AssetCategory::Commodity => DARK.amber,
            AssetCategory::Equity | AssetCategory::Fund => DARK.equity,
            AssetCategory::Forex => DARK.cyan,
        },
    }
}

fn normalize_token(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace(' ', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_tokens_match_python_source() {
        assert_eq!(token("bg"), Some("#0d1117"));
        assert_eq!(token("panel"), Some("#1c2128"));
        assert_eq!(token("border"), Some("#30363d"));
        assert_eq!(token("bull"), Some("#a6e3a1"));
        assert_eq!(token("bear"), Some("#f38ba8"));
        assert_eq!(token("cash"), Some("#6e7681"));
        assert_eq!(token("crypto"), Some("#fab387"));
        assert_eq!(token("gold"), Some("#f9e2af"));
        assert_eq!(token("silver"), Some("#bac2de"));
    }

    #[test]
    fn palette_accepts_report_aliases() {
        assert_eq!(token("slate"), Some(DARK.cash));
        assert_eq!(token("red"), Some(DARK.bear));
        assert_eq!(token("green"), Some(DARK.bull));
        assert_eq!(color_or_raw("#123456"), "#123456");
    }
}
