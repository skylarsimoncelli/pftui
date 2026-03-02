use ratatui::style::Color;

use crate::models::asset::AssetCategory;

// ---- Animation constants ----
pub const PULSE_PERIOD: u64 = 90; // ~1.4s at 60fps
pub const FLASH_DURATION: u64 = 45; // ~0.7s at 60fps
pub const PULSE_PERIOD_BORDER: u64 = 120; // 2s at 60fps — subtle breathing for active panel borders
pub const SELECTION_FLASH_DURATION: u64 = 15; // ~0.25s at 60fps — row highlight on selection change

// ---- Theme struct ----


#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub name: &'static str,

    // Surface depth levels (darkest → lightest)
    pub surface_0: Color,     // deepest bg, app canvas
    pub surface_1: Color,     // panel interiors
    pub surface_1_alt: Color, // odd row stripe
    pub surface_2: Color,     // header, status bar, popups
    pub surface_3: Color,     // selected row

    // Border hierarchy
    pub border_active: Color,   // focused panel
    pub border_inactive: Color, // unfocused panels
    pub border_accent: Color,   // help popup, flashes
    pub border_subtle: Color,   // header/status separators

    // Text hierarchy
    pub text_primary: Color,   // main text
    pub text_secondary: Color, // labels, hints
    pub text_muted: Color,     // timestamps, disabled
    pub text_accent: Color,    // highlighted labels

    // Gain/loss
    pub gain_green: Color,
    pub loss_red: Color,
    pub neutral: Color,

    // Indicators
    pub live_green: Color,
    pub stale_yellow: Color,
    pub key_hint: Color, // [key] brackets

    // Charts
    pub chart_line: Color,
    pub chart_grad_low: Color,  // bottom of chart (red)
    pub chart_grad_mid: Color,  // middle of chart (gold)
    pub chart_grad_high: Color, // top of chart (green)

    // Category colors
    pub cat_equity: Color,
    pub cat_crypto: Color,
    pub cat_forex: Color,
    pub cat_commodity: Color,
    pub cat_fund: Color,
    pub cat_cash: Color,
}

impl Theme {
    pub fn category_color(&self, cat: AssetCategory) -> Color {
        match cat {
            AssetCategory::Equity => self.cat_equity,
            AssetCategory::Crypto => self.cat_crypto,
            AssetCategory::Forex => self.cat_forex,
            AssetCategory::Commodity => self.cat_commodity,
            AssetCategory::Fund => self.cat_fund,
            AssetCategory::Cash => self.cat_cash,
        }
    }
}

// ---- Predefined themes ----

pub const THEME_NAMES: &[&str] = &[
    "midnight",
    "catppuccin",
    "nord",
    "dracula",
    "solarized",
    "gruvbox",
];

pub fn theme_by_name(name: &str) -> Theme {
    match name {
        "catppuccin" => catppuccin(),
        "nord" => nord(),
        "dracula" => dracula(),
        "solarized" => solarized(),
        "gruvbox" => gruvbox(),
        _ => midnight(),
    }
}

pub fn next_theme_name(current: &str) -> &'static str {
    let idx = THEME_NAMES.iter().position(|&n| n == current).unwrap_or(0);
    THEME_NAMES[(idx + 1) % THEME_NAMES.len()]
}

/// Midnight — deep navy/charcoal with jewel-tone accents (default)
pub fn midnight() -> Theme {
    Theme {
        name: "midnight",
        surface_0: Color::Rgb(12, 13, 22),
        surface_1: Color::Rgb(18, 20, 34),
        surface_1_alt: Color::Rgb(22, 24, 38),
        surface_2: Color::Rgb(28, 31, 50),
        surface_3: Color::Rgb(40, 44, 68),
        border_active: Color::Rgb(110, 140, 200),
        border_inactive: Color::Rgb(55, 58, 85),
        border_accent: Color::Rgb(160, 180, 255),
        border_subtle: Color::Rgb(38, 40, 60),
        text_primary: Color::Rgb(230, 233, 245),
        text_secondary: Color::Rgb(140, 145, 170),
        text_muted: Color::Rgb(80, 85, 110),
        text_accent: Color::Rgb(180, 195, 255),
        gain_green: Color::Rgb(50, 210, 120),
        loss_red: Color::Rgb(235, 85, 85),
        neutral: Color::Rgb(140, 145, 170),
        live_green: Color::Rgb(50, 210, 120),
        stale_yellow: Color::Rgb(240, 190, 60),
        key_hint: Color::Rgb(240, 190, 60),
        chart_line: Color::Rgb(100, 160, 240),
        chart_grad_low: Color::Rgb(235, 85, 85),
        chart_grad_mid: Color::Rgb(240, 190, 60),
        chart_grad_high: Color::Rgb(50, 210, 120),
        cat_equity: Color::Rgb(90, 140, 230),
        cat_crypto: Color::Rgb(245, 158, 50),
        cat_forex: Color::Rgb(80, 210, 135),
        cat_commodity: Color::Rgb(220, 175, 55),
        cat_fund: Color::Rgb(175, 100, 220),
        cat_cash: Color::Rgb(170, 175, 195),
    }
}

/// Catppuccin Mocha — warm browns/purples with pastel accents
pub fn catppuccin() -> Theme {
    Theme {
        name: "catppuccin",
        surface_0: Color::Rgb(30, 30, 46),
        surface_1: Color::Rgb(36, 36, 54),
        surface_1_alt: Color::Rgb(39, 39, 58),
        surface_2: Color::Rgb(49, 50, 68),
        surface_3: Color::Rgb(69, 71, 90),
        border_active: Color::Rgb(137, 180, 250),
        border_inactive: Color::Rgb(88, 91, 112),
        border_accent: Color::Rgb(203, 166, 247),
        border_subtle: Color::Rgb(49, 50, 68),
        text_primary: Color::Rgb(205, 214, 244),
        text_secondary: Color::Rgb(166, 173, 200),
        text_muted: Color::Rgb(108, 112, 134),
        text_accent: Color::Rgb(137, 180, 250),
        gain_green: Color::Rgb(166, 227, 161),
        loss_red: Color::Rgb(243, 139, 168),
        neutral: Color::Rgb(166, 173, 200),
        live_green: Color::Rgb(166, 227, 161),
        stale_yellow: Color::Rgb(249, 226, 175),
        key_hint: Color::Rgb(249, 226, 175),
        chart_line: Color::Rgb(137, 180, 250),
        chart_grad_low: Color::Rgb(243, 139, 168),
        chart_grad_mid: Color::Rgb(249, 226, 175),
        chart_grad_high: Color::Rgb(166, 227, 161),
        cat_equity: Color::Rgb(137, 180, 250),
        cat_crypto: Color::Rgb(250, 179, 135),
        cat_forex: Color::Rgb(166, 227, 161),
        cat_commodity: Color::Rgb(249, 226, 175),
        cat_fund: Color::Rgb(203, 166, 247),
        cat_cash: Color::Rgb(186, 194, 222),
    }
}

/// Nord — cool arctic blue-gray
pub fn nord() -> Theme {
    Theme {
        name: "nord",
        surface_0: Color::Rgb(46, 52, 64),
        surface_1: Color::Rgb(59, 66, 82),
        surface_1_alt: Color::Rgb(55, 62, 78),
        surface_2: Color::Rgb(67, 76, 94),
        surface_3: Color::Rgb(76, 86, 106),
        border_active: Color::Rgb(136, 192, 208),
        border_inactive: Color::Rgb(76, 86, 106),
        border_accent: Color::Rgb(129, 161, 193),
        border_subtle: Color::Rgb(59, 66, 82),
        text_primary: Color::Rgb(236, 239, 244),
        text_secondary: Color::Rgb(216, 222, 233),
        text_muted: Color::Rgb(127, 140, 160),
        text_accent: Color::Rgb(136, 192, 208),
        gain_green: Color::Rgb(163, 190, 140),
        loss_red: Color::Rgb(191, 97, 106),
        neutral: Color::Rgb(216, 222, 233),
        live_green: Color::Rgb(163, 190, 140),
        stale_yellow: Color::Rgb(235, 203, 139),
        key_hint: Color::Rgb(235, 203, 139),
        chart_line: Color::Rgb(136, 192, 208),
        chart_grad_low: Color::Rgb(191, 97, 106),
        chart_grad_mid: Color::Rgb(235, 203, 139),
        chart_grad_high: Color::Rgb(163, 190, 140),
        cat_equity: Color::Rgb(129, 161, 193),
        cat_crypto: Color::Rgb(208, 135, 112),
        cat_forex: Color::Rgb(163, 190, 140),
        cat_commodity: Color::Rgb(235, 203, 139),
        cat_fund: Color::Rgb(180, 142, 173),
        cat_cash: Color::Rgb(216, 222, 233),
    }
}

/// Dracula — purple/dark with vivid accents
pub fn dracula() -> Theme {
    Theme {
        name: "dracula",
        surface_0: Color::Rgb(40, 42, 54),
        surface_1: Color::Rgb(48, 51, 65),
        surface_1_alt: Color::Rgb(44, 47, 60),
        surface_2: Color::Rgb(55, 58, 72),
        surface_3: Color::Rgb(68, 71, 90),
        border_active: Color::Rgb(189, 147, 249),
        border_inactive: Color::Rgb(98, 114, 164),
        border_accent: Color::Rgb(255, 121, 198),
        border_subtle: Color::Rgb(55, 58, 72),
        text_primary: Color::Rgb(248, 248, 242),
        text_secondary: Color::Rgb(189, 194, 210),
        text_muted: Color::Rgb(98, 114, 164),
        text_accent: Color::Rgb(189, 147, 249),
        gain_green: Color::Rgb(80, 250, 123),
        loss_red: Color::Rgb(255, 85, 85),
        neutral: Color::Rgb(189, 194, 210),
        live_green: Color::Rgb(80, 250, 123),
        stale_yellow: Color::Rgb(241, 250, 140),
        key_hint: Color::Rgb(241, 250, 140),
        chart_line: Color::Rgb(139, 233, 253),
        chart_grad_low: Color::Rgb(255, 85, 85),
        chart_grad_mid: Color::Rgb(241, 250, 140),
        chart_grad_high: Color::Rgb(80, 250, 123),
        cat_equity: Color::Rgb(139, 233, 253),
        cat_crypto: Color::Rgb(255, 184, 108),
        cat_forex: Color::Rgb(80, 250, 123),
        cat_commodity: Color::Rgb(241, 250, 140),
        cat_fund: Color::Rgb(189, 147, 249),
        cat_cash: Color::Rgb(189, 194, 210),
    }
}

/// Solarized Dark — teal-tinted dark with warm/cool accents
pub fn solarized() -> Theme {
    Theme {
        name: "solarized",
        surface_0: Color::Rgb(0, 43, 54),
        surface_1: Color::Rgb(7, 54, 66),
        surface_1_alt: Color::Rgb(4, 49, 60),
        surface_2: Color::Rgb(15, 63, 75),
        surface_3: Color::Rgb(30, 80, 92),
        border_active: Color::Rgb(38, 139, 210),
        border_inactive: Color::Rgb(88, 110, 117),
        border_accent: Color::Rgb(108, 113, 196),
        border_subtle: Color::Rgb(7, 54, 66),
        text_primary: Color::Rgb(238, 232, 213),
        text_secondary: Color::Rgb(147, 161, 161),
        text_muted: Color::Rgb(88, 110, 117),
        text_accent: Color::Rgb(38, 139, 210),
        gain_green: Color::Rgb(133, 153, 0),
        loss_red: Color::Rgb(220, 50, 47),
        neutral: Color::Rgb(147, 161, 161),
        live_green: Color::Rgb(133, 153, 0),
        stale_yellow: Color::Rgb(181, 137, 0),
        key_hint: Color::Rgb(181, 137, 0),
        chart_line: Color::Rgb(38, 139, 210),
        chart_grad_low: Color::Rgb(220, 50, 47),
        chart_grad_mid: Color::Rgb(181, 137, 0),
        chart_grad_high: Color::Rgb(133, 153, 0),
        cat_equity: Color::Rgb(38, 139, 210),
        cat_crypto: Color::Rgb(203, 75, 22),
        cat_forex: Color::Rgb(133, 153, 0),
        cat_commodity: Color::Rgb(181, 137, 0),
        cat_fund: Color::Rgb(108, 113, 196),
        cat_cash: Color::Rgb(147, 161, 161),
    }
}

/// Gruvbox Dark — warm retro brown/orange palette
pub fn gruvbox() -> Theme {
    Theme {
        name: "gruvbox",
        surface_0: Color::Rgb(40, 40, 40),
        surface_1: Color::Rgb(50, 48, 47),
        surface_1_alt: Color::Rgb(45, 44, 43),
        surface_2: Color::Rgb(60, 56, 54),
        surface_3: Color::Rgb(80, 73, 69),
        border_active: Color::Rgb(215, 153, 33),
        border_inactive: Color::Rgb(102, 92, 84),
        border_accent: Color::Rgb(250, 189, 47),
        border_subtle: Color::Rgb(60, 56, 54),
        text_primary: Color::Rgb(235, 219, 178),
        text_secondary: Color::Rgb(189, 174, 147),
        text_muted: Color::Rgb(124, 111, 100),
        text_accent: Color::Rgb(250, 189, 47),
        gain_green: Color::Rgb(184, 187, 38),
        loss_red: Color::Rgb(251, 73, 52),
        neutral: Color::Rgb(189, 174, 147),
        live_green: Color::Rgb(184, 187, 38),
        stale_yellow: Color::Rgb(250, 189, 47),
        key_hint: Color::Rgb(250, 189, 47),
        chart_line: Color::Rgb(131, 165, 152),
        chart_grad_low: Color::Rgb(251, 73, 52),
        chart_grad_mid: Color::Rgb(250, 189, 47),
        chart_grad_high: Color::Rgb(184, 187, 38),
        cat_equity: Color::Rgb(131, 165, 152),
        cat_crypto: Color::Rgb(254, 128, 25),
        cat_forex: Color::Rgb(184, 187, 38),
        cat_commodity: Color::Rgb(250, 189, 47),
        cat_fund: Color::Rgb(211, 134, 155),
        cat_cash: Color::Rgb(189, 174, 147),
    }
}

// ---- Utility functions ----

/// Linearly interpolate between two RGB colors. t in [0.0, 1.0].
pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => Color::Rgb(
            (r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8,
            (g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8,
            (b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8,
        ),
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
}

/// 3-stop gradient: low → mid → high. position in [0.0, 1.0].
pub fn gradient_3(low: Color, mid: Color, high: Color, position: f32) -> Color {
    if position <= 0.5 {
        lerp_color(low, mid, position * 2.0)
    } else {
        lerp_color(mid, high, (position - 0.5) * 2.0)
    }
}

/// Sine-wave pulse intensity from tick counter. Returns 0.3..1.0.
pub fn pulse_intensity(tick: u64, period: u64) -> f32 {
    let phase = (tick % period) as f32 / period as f32;
    let wave = (2.0 * std::f32::consts::PI * phase).sin();
    0.65 + 0.35 * wave
}

/// Apply pulse to a color (dims toward surface_0).
pub fn pulse_color(color: Color, surface: Color, tick: u64, period: u64) -> Color {
    let intensity = pulse_intensity(tick, period);
    lerp_color(surface, color, intensity)
}

/// Dynamic gain color with intensity mapping.
pub fn gain_intensity_color(theme: &Theme, gain_pct: f64) -> Color {
    let abs_pct = gain_pct.abs();
    if gain_pct > 0.0 {
        let t = (abs_pct / 20.0).min(1.0) as f32;
        lerp_color(
            Color::Rgb(30, 100, 60),
            theme.gain_green,
            t,
        )
    } else if gain_pct < 0.0 {
        let t = (abs_pct / 20.0).min(1.0) as f32;
        lerp_color(
            Color::Rgb(120, 40, 40),
            theme.loss_red,
            t,
        )
    } else {
        theme.neutral
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_color_at_zero() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(255, 255, 255);
        assert_eq!(lerp_color(a, b, 0.0), Color::Rgb(0, 0, 0));
    }

    #[test]
    fn lerp_color_at_one() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(255, 255, 255);
        assert_eq!(lerp_color(a, b, 1.0), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn lerp_color_at_half() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(200, 100, 50);
        let result = lerp_color(a, b, 0.5);
        assert_eq!(result, Color::Rgb(100, 50, 25));
    }

    #[test]
    fn lerp_color_clamps_above_one() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(255, 255, 255);
        assert_eq!(lerp_color(a, b, 2.0), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn lerp_color_clamps_below_zero() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(255, 255, 255);
        assert_eq!(lerp_color(a, b, -1.0), Color::Rgb(0, 0, 0));
    }

    #[test]
    fn lerp_color_non_rgb_below_half() {
        let a = Color::Red;
        let b = Color::Blue;
        assert_eq!(lerp_color(a, b, 0.3), Color::Red);
    }

    #[test]
    fn lerp_color_non_rgb_above_half() {
        let a = Color::Red;
        let b = Color::Blue;
        assert_eq!(lerp_color(a, b, 0.7), Color::Blue);
    }

    #[test]
    fn gradient_3_at_zero() {
        let low = Color::Rgb(255, 0, 0);
        let mid = Color::Rgb(255, 255, 0);
        let high = Color::Rgb(0, 255, 0);
        assert_eq!(gradient_3(low, mid, high, 0.0), Color::Rgb(255, 0, 0));
    }

    #[test]
    fn gradient_3_at_half() {
        let low = Color::Rgb(255, 0, 0);
        let mid = Color::Rgb(255, 255, 0);
        let high = Color::Rgb(0, 255, 0);
        assert_eq!(gradient_3(low, mid, high, 0.5), Color::Rgb(255, 255, 0));
    }

    #[test]
    fn gradient_3_at_one() {
        let low = Color::Rgb(255, 0, 0);
        let mid = Color::Rgb(255, 255, 0);
        let high = Color::Rgb(0, 255, 0);
        assert_eq!(gradient_3(low, mid, high, 1.0), Color::Rgb(0, 255, 0));
    }

    #[test]
    fn gradient_3_at_quarter() {
        let low = Color::Rgb(0, 0, 0);
        let mid = Color::Rgb(200, 200, 200);
        let high = Color::Rgb(200, 200, 200);
        // at 0.25, we're halfway between low and mid
        let result = gradient_3(low, mid, high, 0.25);
        assert_eq!(result, Color::Rgb(100, 100, 100));
    }

    #[test]
    fn pulse_intensity_range() {
        for tick in 0..PULSE_PERIOD {
            let val = pulse_intensity(tick, PULSE_PERIOD);
            assert!((0.29..=1.01).contains(&val), "pulse_intensity({tick}) = {val}");
        }
    }

    #[test]
    fn pulse_intensity_border_period_range() {
        // PULSE_PERIOD_BORDER (2s at 60fps) should also stay in valid range
        for tick in 0..PULSE_PERIOD_BORDER {
            let val = pulse_intensity(tick, PULSE_PERIOD_BORDER);
            assert!((0.29..=1.01).contains(&val), "pulse_intensity({tick}, BORDER) = {val}");
        }
    }

    #[test]
    fn pulse_border_period_produces_variation() {
        // Border pulse should produce distinct colors at quarter-phase points
        let c1 = pulse_color(Color::Rgb(100, 200, 255), Color::Rgb(50, 50, 50), 0, PULSE_PERIOD_BORDER);
        let c2 = pulse_color(Color::Rgb(100, 200, 255), Color::Rgb(50, 50, 50), PULSE_PERIOD_BORDER / 4, PULSE_PERIOD_BORDER);
        let c3 = pulse_color(Color::Rgb(100, 200, 255), Color::Rgb(50, 50, 50), PULSE_PERIOD_BORDER / 2, PULSE_PERIOD_BORDER);
        // Quarter-phase (tick 30) should be near peak, half-phase (tick 60) should be near trough
        assert_ne!(c1, c2, "pulse should vary at quarter period");
        assert_ne!(c2, c3, "pulse should vary at half period");
    }

    #[test]
    fn gain_intensity_color_positive() {
        let theme = midnight();
        let color = gain_intensity_color(&theme, 10.0);
        // Should be between muted green and theme.gain_green
        match color {
            Color::Rgb(_, g, _) => assert!(g > 100, "green channel should be significant"),
            _ => panic!("expected Rgb color"),
        }
    }

    #[test]
    fn gain_intensity_color_negative() {
        let theme = midnight();
        let color = gain_intensity_color(&theme, -10.0);
        // Should be between muted red and theme.loss_red
        match color {
            Color::Rgb(r, _, _) => assert!(r > 100, "red channel should be significant"),
            _ => panic!("expected Rgb color"),
        }
    }

    #[test]
    fn gain_intensity_color_zero() {
        let theme = midnight();
        let color = gain_intensity_color(&theme, 0.0);
        assert_eq!(color, theme.neutral);
    }

    #[test]
    fn gain_intensity_color_saturates_at_20pct() {
        let theme = midnight();
        let at_20 = gain_intensity_color(&theme, 20.0);
        let at_50 = gain_intensity_color(&theme, 50.0);
        // Both should be the same (saturated at 20%)
        assert_eq!(at_20, at_50);
    }

    #[test]
    fn all_themes_load_by_name() {
        for name in THEME_NAMES {
            let theme = theme_by_name(name);
            assert_eq!(theme.name, *name);
        }
    }

    #[test]
    fn unknown_theme_returns_midnight() {
        let theme = theme_by_name("nonexistent");
        assert_eq!(theme.name, "midnight");
    }

    #[test]
    fn next_theme_cycles() {
        assert_eq!(next_theme_name("midnight"), "catppuccin");
        assert_eq!(next_theme_name("gruvbox"), "midnight"); // wraps around
    }

    #[test]
    fn next_theme_unknown_starts_at_catppuccin() {
        // Unknown name → index 0 → next is index 1
        assert_eq!(next_theme_name("unknown"), "catppuccin");
    }

    #[test]
    fn category_color_covers_all_categories() {
        let theme = midnight();
        // Just verify it returns something for each category without panicking
        let _ = theme.category_color(AssetCategory::Equity);
        let _ = theme.category_color(AssetCategory::Crypto);
        let _ = theme.category_color(AssetCategory::Forex);
        let _ = theme.category_color(AssetCategory::Commodity);
        let _ = theme.category_color(AssetCategory::Fund);
        let _ = theme.category_color(AssetCategory::Cash);
    }
}
