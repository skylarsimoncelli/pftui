#![allow(dead_code)]

use super::palette::{ReportPalette, DARK, LIGHT};

pub const FONT_SANS: &str = "'Inter', system-ui, sans-serif";
pub const FONT_MONO: &str = "'JetBrains Mono', 'SF Mono', monospace";
pub const DEFAULT_CHART_WIDTH: u32 = 580;
pub const STACKED_BAR_HEIGHT: u32 = 46;
pub const PROB_BAR_HEIGHT: u32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportThemeKind {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportTheme {
    pub kind: ReportThemeKind,
    pub palette: ReportPalette,
    pub font_sans: &'static str,
    pub font_mono: &'static str,
}

pub const DARK_THEME: ReportTheme = ReportTheme {
    kind: ReportThemeKind::Dark,
    palette: DARK,
    font_sans: FONT_SANS,
    font_mono: FONT_MONO,
};

pub const LIGHT_THEME: ReportTheme = ReportTheme {
    kind: ReportThemeKind::Light,
    palette: LIGHT,
    font_sans: FONT_SANS,
    font_mono: FONT_MONO,
};
